"""Package pinned npm source inputs without executing or extracting provider code."""

import argparse
import base64
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tarfile
import urllib.parse
import urllib.request

LIMIT = 128 * 1024**2
ROOT = Path(__file__).resolve().parent.parent


def digest(data, algorithm="sha256"):
    return hashlib.new(algorithm, data).hexdigest()


def physical(path, directory=False):
    path = Path(os.path.abspath(path))
    for part in (path, *path.parents):
        meta = part.lstat()
        if stat.S_ISLNK(meta.st_mode):
            raise ValueError("Source paths must not contain links")
        if part == path and not directory:
            if not stat.S_ISREG(meta.st_mode) or meta.st_nlink != 1:
                raise ValueError("Source inputs must be private regular files")
        elif not stat.S_ISDIR(meta.st_mode):
            raise ValueError("Source parent must be a physical directory")
    return path


def read(path, limit=LIMIT):
    path = physical(path)
    before = path.stat()
    if not 0 < before.st_size <= limit:
        raise ValueError("Source input exceeds byte budget")
    data = path.read_bytes()
    after = path.stat()
    fields = ("st_dev", "st_ino", "st_size", "st_mtime_ns", "st_ctime_ns")
    if any(getattr(before, k) != getattr(after, k) for k in fields) or len(data) != before.st_size:
        raise ValueError("Source input changed while reading")
    return data


def json_bytes(value):
    return (json.dumps(value, indent=2, ensure_ascii=False) + "\n").encode()


def verify(data, record):
    if (len(data) != record["size"] or digest(data) != record["sha256"]
            or digest(data, "sha512") != record["sha512"]):
        raise ValueError("Pinned source bytes differ")


def archive_files(data, prefixes=None):
    """Read bounded members, reject aliases/types, and never extract provider paths."""
    found, names, roots, total = {}, set(), set(), 0
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        for count, member in enumerate(archive, 1):
            raw = member.name.rstrip("/") if member.isdir() else member.name
            parts = raw.split("/")
            if (count > 100_000 or not raw or any(p in ("", ".", "..") for p in parts)
                    or "\\" in raw or any(ord(c) < 32 or ord(c) == 127 for c in raw)
                    or raw in names or not (member.isfile() or member.isdir())
                    or member.mode & 0o7000):
                raise ValueError("Unsafe or duplicate source archive member")
            names.add(raw)
            roots.add(parts[0])
            if len(roots) != 1:
                raise ValueError("Source archive must have one root")
            if member.isdir():
                continue
            if len(parts) < 2:
                raise ValueError("Source file has no repository-relative path")
            rel = "/".join(parts[1:])
            selected = prefixes is None or "/" not in rel or any(
                rel == p or rel.startswith(p + "/") for p in prefixes)
            # Unselected provider cache files are not read or carried forward.
            total += member.size
            if member.size < 0 or total > 512 * 1024**2:
                raise ValueError("Source archive exceeds expanded budget")
            if not selected:
                continue
            if member.size > 32 * 1024**2:
                raise ValueError("Selected source member exceeds budget")
            body = archive.extractfile(member).read(member.size + 1)
            if len(body) != member.size or body.startswith(b"version https://git-lfs.github.com/spec/v1\n"):
                raise ValueError("Incomplete or unresolved source member")
            found[rel] = (body, member.mode & 0o777)
    if not found:
        raise ValueError("Empty source archive")
    return found


def make_archive(files):
    stream = io.BytesIO()
    with gzip.GzipFile(fileobj=stream, mode="wb", filename="", mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as archive:
            for name, (body, mode) in sorted(files.items()):
                parts = name.split("/")
                if any(p in ("", ".", "..") for p in parts) or "\\" in name:
                    raise ValueError("Unsafe output name")
                member = tarfile.TarInfo(name)
                member.size, member.mode, member.mtime = len(body), mode, 0
                archive.addfile(member, io.BytesIO(body))
    return stream.getvalue()


def obtain(origin, record, cache, offline):
    if not re.fullmatch(r"https://(?:codeload\.github\.com/[\w.-]+/[\w.-]+/tar\.gz/[a-f0-9]{40}|registry\.npmjs\.org/@types/react/-/react-[\w.+-]+\.tgz)", origin):
        raise ValueError("Unsupported source origin")
    physical(cache, directory=True)
    destination = cache / (record["sha256"] + ".archive")
    if destination.exists() or destination.is_symlink():
        data = read(destination)
        verify(data, record)
        return data
    if offline:
        raise ValueError("Pinned source not cached for offline packaging")
    request = urllib.request.Request(origin, headers={"User-Agent": "MooMole-source-inputs"})
    # No credentials, provider scripts, or automatic unbounded retry.
    with urllib.request.urlopen(request, timeout=45) as response:
        expected_host = urllib.parse.urlsplit(origin).hostname
        final = urllib.parse.urlsplit(response.url)
        if final.scheme != "https" or final.hostname != expected_host or response.status != 200:
            raise ValueError("Source download changed origin")
        data = response.read(min(LIMIT, record["size"]) + 1)
    verify(data, record)
    with destination.open("xb") as target:
        target.write(data)
    verify(read(destination), record)
    return data


def validate_lock(origins, lock_bytes):
    if digest(lock_bytes) != origins["packageLockSha256"]:
        raise ValueError("npm source pins do not match package-lock")
    actual = {}
    for location, package in json.loads(lock_bytes)["packages"].items():
        if not location or package.get("dev"):
            continue
        if not location.startswith("node_modules/") or "/node_modules/" in location:
            raise ValueError("Unsupported npm production layout")
        actual[location.removeprefix("node_modules/")] = (package["version"], package["integrity"])
    expected = {p["name"]: (p["version"], p["integrity"]) for p in origins["packages"]}
    if len(expected) != len(origins["packages"]) or expected != actual:
        raise ValueError("npm source pins do not cover the production inventory")


def build_inputs(origins, fetch):
    files, records, source_count = {}, [], 0
    for item in origins["repositoryInputs"]:
        name = item["file"]
        if not re.fullmatch(r"archives/[\w.+-]+\.tar\.gz", name) or name in files:
            raise ValueError("Unsafe or duplicate source destination")
        subtree = item["sourceSubtree"]
        provider = subtree["originalProviderArchive"] if subtree else item
        data = fetch(item["origin"], provider)
        verify(data, provider)
        entries = archive_files(data, subtree["retainedPrefixes"] if subtree else None)
        if len(entries) != item["entries"] or sum(len(b) for b, _ in entries.values()) != item["expandedBytes"]:
            raise ValueError("Pinned repository inventory differs")
        if subtree:
            # Original provider archive is pinned; subset gzip bytes are generated,
            # rather than expecting the earlier private packager's gzip headers.
            body = make_archive({"source/" + path: value for path, value in entries.items()})
            if archive_files(body) != entries:
                raise ValueError("Subset archive changed original file bytes or modes")
        else:
            body = data
        files[name] = (body, 0o644)
        source_count += len(entries)
        records.append({"repository": item["repository"], "revision": item["revision"],
                        "origin": item["origin"], "file": name, "size": len(body),
                        "sha256": digest(body), "sha512": digest(body, "sha512"),
                        "files": len(entries), "sourceSubtree": subtree,
                        "providerArchive": {k: provider[k] for k in ("size", "sha256", "sha512")}})
    declaration = origins["publishedDeclarations"]
    package = next(p for p in origins["packages"] if p["name"] == declaration["name"])
    data = fetch("https://registry.npmjs.org/@types/react/-/react-" + declaration["version"] + ".tgz", declaration)
    verify(data, declaration)
    if package["integrity"] != "sha512-" + base64.b64encode(hashlib.sha512(data).digest()).decode():
        raise ValueError("Declaration distribution differs from npm integrity")
    entries = archive_files(data)
    declarations = {"react/" + path: body for path, (body, _) in entries.items() if path.endswith(".d.ts")}
    if set(declarations) != {r["path"] for r in declaration["declarations"]}:
        raise ValueError("Declaration file inventory differs")
    for record in declaration["declarations"]:
        body = declarations[record["path"]]
        if len(body) != record["size"] or digest(body) != record["sha256"]:
            raise ValueError("Declaration bytes differ")
    name = declaration["file"]
    if not re.fullmatch(r"archives/[\w.+-]+\.tgz", name) or name in files:
        raise ValueError("Unsafe declaration destination")
    files[name] = (data, 0o644)
    return files, records, source_count


def binding(app_path, deps_path, version):
    app_bytes, deps_bytes = read(app_path, 4 * 1024**2), read(deps_path, 4 * 1024**2)
    app, deps = json.loads(app_bytes), json.loads(deps_bytes)
    source = app["source"]
    if (app.get("schemaVersion") != 1 or app.get("purpose") != "local-validation-only"
            or app.get("version") != version or app.get("profile") != "release"
            or app.get("signing") != "ad-hoc" or app.get("notarized") is not False
            or source.get("dirty") is not False or not re.fullmatch("[a-f0-9]{40}", source["revision"])
            or app.get("architecture") not in ("arm64", "x64")):
        raise ValueError("Expected a clean release App report")
    archive = app["archive"]
    expected = {"reportSha512": digest(app_bytes, "sha512"), "sourceRevision": source["revision"],
                "architecture": app["architecture"], "noticesSha256": app["dependencyMaterials"]["noticesSha256"],
                "archive": {k: archive[k] for k in ("file", "size", "sha512")}}
    if (deps.get("schemaVersion") != 1 or deps.get("status") != "verified" or deps.get("source") != source
            or deps.get("binaryBinding") != expected or deps.get("reviewStatus") != "pending"
            or deps.get("sha512Encoding") != "hex"
            or not isinstance(deps.get("records"), int) or not 0 < deps["records"] <= 1024
            or deps["records"] + 2 != app["dependencyMaterials"].get("packages")):
        raise ValueError("Dependency packet is bound to a different App")
    for parent, record, name in (
            (app_path.parent, archive, f"MooMole-{version}-mac-{app['architecture']}.app.zip"),
            (deps_path.parent, deps, f"MooMole-{version}-mac-{app['architecture']}-dependencies.tar.gz")):
        if record["file"] != name:
            raise ValueError("Unexpected binding asset name")
        data = read(parent / name)
        if len(data) != record["size"] or digest(data, "sha512") != record["sha512"]:
            raise ValueError("Binding asset bytes differ")
    return expected, {k: deps[k] for k in ("file", "size", "sha512")}, source


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--app-report", required=True, type=Path)
    parser.add_argument("--dependency-report", required=True, type=Path)
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    pins_bytes = read(ROOT / "third-party/npm-source-origins.json", 4 * 1024**2)
    lock_bytes = read(ROOT / "package-lock.json", 4 * 1024**2)
    origins = json.loads(pins_bytes)
    validate_lock(origins, lock_bytes)
    version = json.loads(read(ROOT / "package.json"))["version"]
    if not isinstance(version, str) or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)*", version):
        raise ValueError("Unsupported application version")
    app_binding, dependency, source = binding(args.app_report, args.dependency_report, version)
    # Pins must also belong to the actual binary's source, even when packaging an
    # older verified candidate from the same unchanged dependency inventory.
    for name, expected in (("third-party/npm-source-origins.json", pins_bytes), ("package-lock.json", lock_bytes)):
        git_env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        git_env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        actual = subprocess.run(["git", "-c", "core.fsmonitor=false", "-C", str(ROOT),
                                 "show", source["revision"] + ":" + name],
                                capture_output=True, check=True, timeout=30, env=git_env).stdout
        if actual != expected:
            raise ValueError("Source input pins differ from binary source commit")
    physical(args.output.parent, directory=True)
    if args.output.exists() or args.output.is_symlink():
        raise ValueError("Output must be a new directory")
    cache = ROOT / ".cache/npm-source-inputs"
    cache.mkdir(exist_ok=True)
    files, records, count = build_inputs(origins, lambda url, record: obtain(url, record, cache, args.offline))
    manifest = {"schemaVersion": 1, "purpose": "npm-source-inputs", "version": version,
                "source": source, "binaryBinding": app_binding, "dependencyPacket": dependency,
                "originsSha256": digest(pins_bytes), "packageLockSha256": digest(lock_bytes),
                "packages": origins["packages"], "repositoryInputs": records,
                "publishedDeclarations": origins["publishedDeclarations"],
                "preferredSourceReview": "pending", "buildReproduction": "pending",
                "licenseReview": "pending", "releaseAcceptance": "pending"}
    files["NPM_SOURCES.json"] = (json_bytes(manifest), 0o644)
    files["README.txt"] = (b"Pinned npm source inputs. Original repository archives and a declared Lucide subset; no provider scripts run. Registry/provenance references describe the prior pinned origin review, not a fresh signature audit. @types/react has no verified repository revision. Keep with the bound App, dependency packet, client and Mole sources. Preferred source, generated output/build closure, file-level licensing and release acceptance remain pending.\n", 0o644)
    prefix = f"MooMole-{version}-npm-source-inputs"
    packet = make_archive({prefix + "/" + path: value for path, value in files.items()})
    if len(packet) > LIMIT or archive_files(packet) != files:
        raise ValueError("Source packet round trip differs")
    if binding(args.app_report, args.dependency_report, version) != (app_binding, dependency, source):
        raise ValueError("App binding changed during packaging")
    if read(ROOT / "third-party/npm-source-origins.json") != pins_bytes or read(ROOT / "package-lock.json") != lock_bytes:
        raise ValueError("Source pins changed during packaging")
    args.output.mkdir()
    name = prefix + ".tar.gz"
    with (args.output / name).open("xb") as target:
        target.write(packet)
    if read(args.output / name) != packet:
        raise ValueError("Written source packet differs")
    report = {"schemaVersion": 1, "purpose": "npm-source-inputs-validation", "status": "verified", "file": name,
              "size": len(packet), "sha512": digest(packet, "sha512"), "sha512Encoding": "hex",
              "source": source, "binaryBinding": app_binding, "dependencyPacket": dependency,
              "packages": len(origins["packages"]), "repositorySnapshots": len(records),
              "repositoryFiles": count, "declarationFiles": len(origins["publishedDeclarations"]["declarations"]),
              "originsSha256": digest(pins_bytes), "packageLockSha256": digest(lock_bytes),
              "preferredSourceReview": "pending", "licenseReview": "pending", "releaseAcceptance": "pending"}
    with (args.output / "validation.json").open("xb") as target:
        target.write(json_bytes(report))
    print(f"Pinned npm source inputs verified: {args.output}; release review pending")


if __name__ == "__main__":
    main()
