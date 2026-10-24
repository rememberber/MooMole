import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest


spec = importlib.util.spec_from_file_location("rust_source_audit", Path(__file__).with_name("audit-rust-source.py"))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)
PREFIX = "rust-src-1.99.0/rust-src/lib/rustlib/src/rust/library/"
PACKAGE = PREFIX + "vendor/example-1.0.0/"
CHECKSUM = PACKAGE + ".cargo-checksum.json"
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"


def fixture():
    body = b"// fixture source\n"
    return {
        PREFIX + "Cargo.toml": b"[workspace]\n",
        PREFIX + "Cargo.lock": ("version = 4\n[[package]]\nname = \"example\"\nversion = \"1.0.0\"\n"
                                "source = \"" + REGISTRY + "\"\nchecksum = \"" + "a" * 64 + "\"\n").encode(),
        PACKAGE + "src/lib.rs": body,
        CHECKSUM: json.dumps({"package": "a" * 64, "files": {"src/lib.rs": audit.sha256(body)}}).encode(),
    }


def archive(files, extra=()):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w:xz") as tar:
        for name, body in files.items():
            member = tarfile.TarInfo(name)
            member.size = len(body)
            tar.addfile(member, io.BytesIO(body))
        for member, body in extra:
            tar.addfile(member, io.BytesIO(body) if body else None)
    return stream.getvalue()


def check(data):
    return audit.audit_bytes(data, "1.99.0", audit.sha256(data))


class RustSourceAuditTests(unittest.TestCase):
    def test_success_retains_scope_and_pending_reviews(self):
        data = archive(fixture())
        result = check(data)
        self.assertEqual(result["archiveSha256"], hashlib.sha256(data).hexdigest())
        self.assertEqual(result["packages"][0]["id"], "cargo:example@1.0.0")
        self.assertEqual(result["files"], 1)
        self.assertEqual(result["targetBuildClosure"], "pending")
        self.assertEqual(result["licenseReview"], "pending")

    def test_wrong_archive_or_version_is_refused(self):
        data = archive(fixture())
        with self.assertRaises(ValueError):
            audit.audit_bytes(data, "1.99.0", "b" * 64)
        with self.assertRaises(ValueError):
            audit.audit_bytes(data, "1.98.0", audit.sha256(data))

    def test_vendor_changes_missing_and_unlisted_files_are_refused(self):
        for mutation in ("content", "missing", "extra", "package", "manifest", "extra-package"):
            with self.subTest(mutation=mutation):
                files = fixture()
                if mutation == "content":
                    files[PACKAGE + "src/lib.rs"] = b"changed"
                elif mutation == "missing":
                    del files[PACKAGE + "src/lib.rs"]
                elif mutation == "extra":
                    files[PACKAGE + "unlisted.rs"] = b"extra"
                elif mutation == "package":
                    files[CHECKSUM] = files[CHECKSUM].replace(b"a" * 64, b"b" * 64)
                elif mutation == "manifest":
                    del files[CHECKSUM]
                else:
                    files[PREFIX + "vendor/unlocked-1.0.0/src.rs"] = b"extra"
                with self.assertRaises(ValueError):
                    check(archive(files))

    def test_duplicate_members_traversal_links_and_special_modes_are_refused(self):
        for kind in ("duplicate", "traversal", "absolute", "link", "hardlink", "fifo", "setuid"):
            with self.subTest(kind=kind):
                member = tarfile.TarInfo(PACKAGE + "extra.rs")
                if kind == "duplicate":
                    member.name = PACKAGE + "src/lib.rs"
                elif kind == "traversal":
                    member.name = PACKAGE + "../escape"
                elif kind == "absolute":
                    member.name = "/tmp/escape"
                elif kind == "link":
                    member.type, member.linkname = tarfile.SYMTYPE, "../../escape"
                elif kind == "hardlink":
                    member.type, member.linkname = tarfile.LNKTYPE, PACKAGE + "src/lib.rs"
                elif kind == "fifo":
                    member.type = tarfile.FIFOTYPE
                else:
                    member.mode = 0o4755
                with self.assertRaises(ValueError):
                    check(archive(fixture(), [(member, b"")]))

    def test_ambiguous_json_and_unsupported_lock_sources_are_refused(self):
        files = fixture()
        files[CHECKSUM] = b'{"files":{},"files":{},"package":"' + b"a" * 64 + b'"}'
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            check(archive(files))
        original = fixture()[PREFIX + "Cargo.lock"].decode()
        for lock in (original.replace(REGISTRY, "git+https://example.invalid/repo"),
                     original + original.split("version = 4\n")[1],
                     original.replace("checksum =", "checksum=wrong #")):
            with self.assertRaises(ValueError):
                audit.registry_entries(lock)

    def test_budget_and_input_file_type_are_refused(self):
        old = audit.MAX_FILE
        try:
            audit.MAX_FILE = 4
            with self.assertRaisesRegex(ValueError, "budget"):
                check(archive(fixture()))
        finally:
            audit.MAX_FILE = old
        with tempfile.TemporaryDirectory() as folder:
            source, link = Path(folder) / "source", Path(folder) / "link"
            data = archive(fixture())
            source.write_bytes(data)
            link.symlink_to(source)
            self.assertEqual(audit.audit_file(source, "1.99.0", audit.sha256(data))["files"], 1)
            with self.assertRaises(ValueError):
                audit.audit_file(link, "1.99.0", audit.sha256(data))

    def test_cli_preserves_old_reports_and_writes_nothing_after_a_failed_audit(self):
        with tempfile.TemporaryDirectory() as folder:
            source, report = Path(folder) / "source", Path(folder) / "report.json"
            data = archive(fixture())
            source.write_bytes(data)
            command = [sys.executable, "-B", str(Path(__file__).with_name("audit-rust-source.py")),
                       "--archive", str(source), "--version", "1.99.0", "--sha256", audit.sha256(data),
                       "--output", str(report)]
            report.write_text("preserve existing report", encoding="utf-8")
            result = subprocess.run(command, capture_output=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(report.read_text(encoding="utf-8"), "preserve existing report")
            self.assertNotIn(b"Traceback", result.stderr)
            report.unlink()
            source.write_bytes(b"changed")
            result = subprocess.run(command, capture_output=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(report.exists())


if __name__ == "__main__":
    unittest.main()
