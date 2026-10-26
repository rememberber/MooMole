"""Portable source-byte fixtures, not actual provider licenses or App signatures."""
import base64
import copy
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("npm_inputs", Path(__file__).with_name("npm-source-inputs.py"))
inputs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inputs)


def record(data):
    return {"size": len(data), "sha256": inputs.digest(data), "sha512": inputs.digest(data, "sha512")}


def fixture():
    original = inputs.make_archive({
        "repo/LICENSE": (b"fixture notice", 0o644),
        "repo/src/space 中文.ts": (b"original source", 0o755),
        "repo/unrelated/cache": (b"excluded", 0o644),
    })
    declaration = inputs.make_archive({"react/index.d.ts": (b"fixture declaration", 0o644)})
    origins = {
        "packages": [{"name": "@types/react", "version": "1.0.0",
                      "integrity": "sha512-" + base64.b64encode(bytes.fromhex(inputs.digest(declaration, "sha512"))).decode()}],
        "repositoryInputs": [{"file": "archives/repo.tar.gz", "repository": "https://github.com/example/repo",
                              "revision": "a" * 40, "origin": "fixture:repo", "entries": 2,
                              "expandedBytes": 29, "sourceSubtree": {
                                  "originalProviderArchive": record(original), "retainedPrefixes": ["src"]}}],
        "publishedDeclarations": {"name": "@types/react", "version": "1.0.0",
                                  "file": "archives/types-react-original.tgz", **record(declaration),
                                  "declarations": [{"path": "react/index.d.ts", "size": 19,
                                                    "sha256": inputs.digest(b"fixture declaration")}]},
    }
    return origins, lambda url, _: original if url == "fixture:repo" else declaration


class SourceTests(unittest.TestCase):
    def test_subset_preserves_original_source_modes_and_published_declarations(self):
        origins, fetch = fixture()
        files, records, count = inputs.build_inputs(origins, fetch)
        self.assertEqual(count, 2)
        self.assertEqual(inputs.archive_files(files["archives/repo.tar.gz"][0]), {
            "LICENSE": (b"fixture notice", 0o644),
            "src/space 中文.ts": (b"original source", 0o755),
        })
        self.assertNotIn("unrelated/cache", inputs.archive_files(files["archives/repo.tar.gz"][0]))
        self.assertEqual(records[0]["providerArchive"], origins["repositoryInputs"][0]["sourceSubtree"]["originalProviderArchive"])
        self.assertEqual(inputs.build_inputs(origins, fetch), (files, records, count))

    def test_archive_rejects_unsafe_types_paths_aliases_and_unresolved_sources(self):
        cases = [("r/../escape", tarfile.REGTYPE, 0o644), ("/absolute", tarfile.REGTYPE, 0o644),
                 ("r/link", tarfile.SYMTYPE, 0o644), ("r/hard", tarfile.LNKTYPE, 0o644),
                 ("r/fifo", tarfile.FIFOTYPE, 0o644), ("r/suid", tarfile.REGTYPE, 0o4644)]
        for name, kind, mode in cases:
            stream = io.BytesIO()
            with tarfile.open(fileobj=stream, mode="w:gz") as archive:
                member = tarfile.TarInfo(name)
                member.type, member.mode, member.linkname = kind, mode, "target"
                archive.addfile(member)
            with self.subTest(name=name), self.assertRaises(ValueError):
                inputs.archive_files(stream.getvalue())
        for files in ({"a/file": (b"x", 0o644), "b/file": (b"y", 0o644)},
                      {"r/pointer": (b"version https://git-lfs.github.com/spec/v1\n", 0o644)}):
            with self.assertRaises(ValueError):
                inputs.archive_files(inputs.make_archive(files))
        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode="w:gz") as archive:
            archive.addfile(tarfile.TarInfo("r/same"))
            archive.addfile(tarfile.TarInfo("r/same"))
        with self.assertRaises(ValueError):
            inputs.archive_files(stream.getvalue())

    def test_wrong_archive_inventory_declaration_or_integrity_never_yields_a_packet(self):
        original, fetch = fixture()
        for mutate in (
            lambda o: o["repositoryInputs"][0].update(entries=3),
            lambda o: o["repositoryInputs"][0]["sourceSubtree"]["originalProviderArchive"].update(sha256="0" * 64),
            lambda o: o["publishedDeclarations"]["declarations"][0].update(sha256="0" * 64),
            lambda o: o["publishedDeclarations"].update(declarations=[]),
            lambda o: o["packages"][0].update(integrity="sha512-wrong"),
            lambda o: o["repositoryInputs"][0].update(file="archives/../../escape.tar.gz"),
        ):
            origins = copy.deepcopy(original)
            mutate(origins)
            with self.assertRaises(ValueError):
                inputs.build_inputs(origins, fetch)

    def test_lock_requires_complete_exact_production_inventory(self):
        lock = json.dumps({"packages": {"": {}, "node_modules/one": {"version": "1.0.0", "integrity": "sha512-a"},
                                       "node_modules/dev": {"dev": True}}}).encode()
        origins = {"packageLockSha256": inputs.digest(lock),
                   "packages": [{"name": "one", "version": "1.0.0", "integrity": "sha512-a"}]}
        inputs.validate_lock(origins, lock)
        for change in ({"packages": []}, {"packageLockSha256": "0" * 64},
                       {"packages": origins["packages"] * 2}):
            with self.assertRaises(ValueError):
                inputs.validate_lock({**origins, **change}, lock)

    def test_offline_cache_rechecks_bytes_and_rejects_links_without_network(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder).resolve()
            body = b"provider fixture"
            meta = record(body)
            path = root / (meta["sha256"] + ".archive")
            origin = "https://codeload.github.com/example/repo/tar.gz/" + "a" * 40
            with self.assertRaises(ValueError):
                inputs.obtain(origin, meta, root, True)
            path.write_bytes(body)
            self.assertEqual(inputs.obtain(origin, meta, root, True), body)
            path.write_bytes(b"changed")
            with self.assertRaises(ValueError):
                inputs.obtain(origin, meta, root, True)
            path.unlink()
            (root / "target").write_bytes(body)
            path.symlink_to(root / "target")
            with self.assertRaises(ValueError):
                inputs.obtain(origin, meta, root, True)
            with self.assertRaises(ValueError):
                inputs.obtain("https://unrecognized.example/source", meta, root, True)

    def test_binding_rejects_different_app_dependency_and_changed_asset(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder).resolve()
            def asset(name, body):
                (root / name).write_bytes(body)
                return {"file": name, "size": len(body), "sha512": inputs.digest(body, "sha512")}
            app_zip = asset("MooMole-0.1.0-mac-arm64.app.zip", b"fixture App, no native validity claim")
            dep_zip = asset("MooMole-0.1.0-mac-arm64-dependencies.tar.gz", b"fixture dependencies")
            source = {"revision": "a" * 40, "dirty": False}
            app = {"schemaVersion": 1, "purpose": "local-validation-only", "version": "0.1.0",
                   "profile": "release", "signing": "ad-hoc", "notarized": False,
                   "source": source, "architecture": "arm64", "archive": app_zip,
                   "dependencyMaterials": {"noticesSha256": "b" * 64, "packages": 3}}
            app_bytes = inputs.json_bytes(app)
            bound = {"reportSha512": inputs.digest(app_bytes, "sha512"), "sourceRevision": source["revision"],
                     "architecture": "arm64", "noticesSha256": "b" * 64, "archive": app_zip}
            deps = {"schemaVersion": 1, "status": "verified", "source": source, "binaryBinding": bound,
                    "reviewStatus": "pending", "sha512Encoding": "hex", "records": 1, **dep_zip}
            app_path, dep_path = root / "app.json", root / "deps.json"
            app_path.write_bytes(app_bytes)
            dep_path.write_bytes(inputs.json_bytes(deps))
            self.assertEqual(inputs.binding(app_path, dep_path, "0.1.0")[0], bound)
            dep_path.write_bytes(inputs.json_bytes({**deps, "binaryBinding": {**bound, "architecture": "x64"}}))
            with self.assertRaises(ValueError):
                inputs.binding(app_path, dep_path, "0.1.0")
            dep_path.write_bytes(inputs.json_bytes(deps))
            (root / app_zip["file"]).write_bytes(b"changed")
            with self.assertRaises(ValueError):
                inputs.binding(app_path, dep_path, "0.1.0")


if __name__ == "__main__":
    unittest.main()
