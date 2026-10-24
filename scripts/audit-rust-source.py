"""Read-only audit of the registry vendor inventory in an authenticated rust-src archive.

Python 3.9+ is needed only for this release audit, never by the shipped App.
No archive member is extracted to disk or executed.
"""

import argparse
import hashlib
import io
import json
import os
import re
import stat
import sys
import tarfile


MAX_ARCHIVE = 128 * 1024**2
MAX_EXPANDED = 256 * 1024**2
MAX_FILE = 32 * 1024**2


def checked_path(value):
    if (not isinstance(value, str) or not value or "\\" in value
            or any(ord(c) < 32 or ord(c) == 127 for c in value)
            or value.startswith("/") or any(p in ("", ".", "..") for p in value.split("/"))):
        raise ValueError("Unsafe archive path")
    return value


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("Duplicate checksum JSON key")
        result[key] = value
    return result


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def registry_entries(text):
    if len(re.findall(r"^version = [34]$", text, re.M)) != 1:
        raise ValueError("Unsupported Cargo.lock format")
    entries = {}
    for block in re.split(r"(?=^\[\[package\]\])", text, flags=re.M):
        if not block.startswith("[[package]]"):
            continue

        def field(name):
            values = re.findall(r'^' + name + r' = "([^"\r\n]+)"$', block, re.M)
            if len(values) != 1:
                raise ValueError("Ambiguous Cargo.lock package")
            return values[0]

        name, version = field("name"), field("version")
        if not re.fullmatch(r"[A-Za-z0-9_-]+", name) or not re.fullmatch(r"[A-Za-z0-9.+-]+", version):
            raise ValueError("Unsafe Cargo.lock identity")
        if not re.search(r"^source\s*=", block, re.M):
            continue
        if field("source") != "registry+https://github.com/rust-lang/crates.io-index":
            raise ValueError("Unsupported Cargo.lock source")
        checksum = field("checksum")
        key = name + "-" + version
        if key in entries or not re.fullmatch(r"[a-f0-9]{64}", checksum):
            raise ValueError("Duplicate package or invalid registry checksum")
        entries[key] = {"id": "cargo:" + name + "@" + version, "checksum": checksum}
    if not entries:
        raise ValueError("Empty registry inventory")
    return entries


def audit_bytes(data, version, expected_sha256):
    if (not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version)
            or not re.fullmatch(r"[a-f0-9]{64}", expected_sha256)
            or not data or len(data) > MAX_ARCHIVE or sha256(data) != expected_sha256):
        raise ValueError("Rust source version, size or SHA-256 mismatch")
    root = "rust-src-" + version
    prefix = root + "/rust-src/lib/rustlib/src/rust/library/"
    members, seen, total = {}, set(), 0
    with tarfile.open(fileobj=io.BytesIO(data), mode="r|xz") as archive:
        for member in archive:
            name = checked_path(member.name)
            if name in seen or len(seen) >= 20000 or not (name == root or name.startswith(root + "/")):
                raise ValueError("Duplicate, excessive or misplaced archive member")
            seen.add(name)
            if member.isdir():
                if member.size:
                    raise ValueError("Directory has unexpected content")
                continue
            if not member.isfile() or member.mode & 0o7000:
                raise ValueError("Links, special files and privileged modes are refused")
            total += member.size
            if member.size < 0 or member.size > MAX_FILE or total > MAX_EXPANDED:
                raise ValueError("Expanded source budget exceeded")
            body = archive.extractfile(member).read(MAX_FILE + 1)
            if len(body) != member.size:
                raise ValueError("Truncated source member")
            members[name] = body
    lock = prefix + "Cargo.lock"
    if lock not in members or prefix + "Cargo.toml" not in members:
        raise ValueError("Rust library workspace metadata is missing")
    entries = registry_entries(members[lock].decode("utf-8"))
    vendor = prefix + "vendor/"
    actual_packages = {p[len(vendor):].split("/")[0] for p in members if p.startswith(vendor)}
    if actual_packages != set(entries):
        raise ValueError("Vendor inventory differs from locked registry packages")
    records = []
    for key, entry in sorted(entries.items()):
        folder = vendor + key + "/"
        checksum_file = folder + ".cargo-checksum.json"
        if checksum_file not in members:
            raise ValueError("Vendor checksum manifest is missing")
        manifest = json.loads(members[checksum_file].decode("utf-8"), object_pairs_hook=unique_object)
        if (not isinstance(manifest, dict) or manifest.get("package") != entry["checksum"]
                or not isinstance(manifest.get("files"), dict)):
            raise ValueError("Vendor package checksum differs from Cargo.lock")
        listed = manifest["files"]
        actual = {p[len(folder):] for p in members if p.startswith(folder) and p != checksum_file}
        if not listed or actual != set(listed):
            raise ValueError("Vendor file inventory differs from checksum manifest")
        size = 0
        for relative, expected in listed.items():
            checked_path(relative)
            if not isinstance(expected, str) or not re.fullmatch(r"[a-f0-9]{64}", expected):
                raise ValueError("Invalid vendor file checksum")
            body = members[folder + relative]
            if sha256(body) != expected:
                raise ValueError("Vendor file content checksum mismatch")
            size += len(body)
        records.append({**entry, "files": len(listed), "bytes": size,
                        "checksumManifestSha256": sha256(members[checksum_file])})
    return {"schemaVersion": 1, "purpose": "rust-src-registry-vendor-audit", "version": version,
            "archiveSha256": expected_sha256, "archiveBytes": len(data),
            "lockSha256": sha256(members[lock]), "status": "verified",
            "scope": "all registry packages in library/Cargo.lock, including target/test supersets",
            "packages": records, "files": sum(r["files"] for r in records),
            "bytes": sum(r["bytes"] for r in records),
            "targetBuildClosure": "pending", "licenseReview": "pending"}


def audit_file(path, version, expected_sha256):
    before = os.lstat(path)
    if not stat.S_ISREG(before.st_mode) or not 0 < before.st_size <= MAX_ARCHIVE:
        raise ValueError("Archive must be a bounded regular file")
    with open(path, "rb") as stream:
        opened = os.fstat(stream.fileno())
        data = stream.read(MAX_ARCHIVE + 1)
        after = os.fstat(stream.fileno())
    current = os.lstat(path)
    identity = lambda s: (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
    if not all(identity(s) == identity(before) for s in (opened, after, current)) or len(data) != before.st_size:
        raise ValueError("Archive changed while reading")
    return audit_bytes(data, version, expected_sha256)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--sha256", required=True, help="Expected value from authenticated distribution metadata")
    parser.add_argument("--output", required=True, help="New report file; existing files are preserved")
    args = parser.parse_args()
    report = audit_file(args.archive, args.version, args.sha256)
    with open(args.output, "x", encoding="utf-8") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")
    print("Verified %d registry packages / %d files; target build closure and license review pending."
          % (len(report["packages"]), report["files"]))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, UnicodeError, tarfile.TarError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
