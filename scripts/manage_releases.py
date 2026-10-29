#!/usr/bin/env python3
"""MooTool-style release metadata, asset staging and update-feed publication."""
import argparse
import hashlib
import json
import re
import shutil
import subprocess
import zipfile
from datetime import datetime
from pathlib import Path
from zoneinfo import ZoneInfo

REPO = "rememberber/MooMole"
PRODUCT = "io.github.rememberber.moomole"
RELEASES = f"https://github.com/{REPO}/releases"
ROOT = Path(__file__).resolve().parents[1]
SEMVER = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?")


def version_key(value):
    match = SEMVER.fullmatch(value)
    if not match:
        raise ValueError(f"Invalid release version: {value}")
    pre = match[4].split(".") if match[4] else []
    if any(not p or (p.isdigit() and len(p) > 1 and p[0] == "0") for p in pre):
        raise ValueError(f"Invalid prerelease: {value}")
    if match[5] and any(not p for p in match[5].split(".")):
        raise ValueError(f"Invalid build metadata: {value}")
    return (*map(int, match.group(1, 2, 3)), not pre,
            tuple((0, int(p)) if p.isdigit() else (1, p) for p in pre))


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def release_info(root, tag):
    version = read_json(root / "package.json")["version"]
    key = version_key(version)
    if tag != f"v{version}":
        raise ValueError(f"Release tag must be v{version}, got {tag}")
    path = root / "release-notes" / f"{version}.md"
    content = path.read_text(encoding="utf-8").strip()
    if not content.startswith(f"# MooMole {version}\n"):
        raise ValueError(f"Release notes must start with '# MooMole {version}'")
    notes = content.split("\n", 1)[1].strip()
    headings = list(re.finditer(r"^## (English|中文|日本語)\s*$", notes, re.M))
    if [h[1] for h in headings] != ["English", "中文", "日本語"]:
        raise ValueError("Release notes need English, 中文, 日本語 sections in that order")
    for index, heading in enumerate(headings):
        end = headings[index + 1].start() if index + 1 < len(headings) else len(notes)
        if not notes[heading.end():end].strip():
            raise ValueError("Empty release-notes section")
    if len(notes.encode()) > 16 * 1024 or re.search(r"\bTODO\b", notes, re.I):
        raise ValueError("Finish release notes and keep them within the updater's 16 KiB limit")
    return {"version": version, "tag": tag, "title": f"MooMole {version}",
            "notes": notes, "notes_path": f"release-notes/{version}.md",
            "prerelease": not key[3], "releaseUrl": f"{RELEASES}/tag/{tag}"}


def asset_record(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"Expected a regular release file: {path}")
    hashes = {name: hashlib.new(name) for name in ["sha256", "sha512"]}
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            for digest in hashes.values():
                digest.update(chunk)
    return {"fileName": path.name, "size": path.stat().st_size,
            **{name: digest.hexdigest() for name, digest in hashes.items()}}


def stage_assets(candidate, output, version, arch):
    report = read_json(candidate / "validation.json")
    if (arch not in ["arm64", "x64"] or report["version"] != version
            or report["architecture"] != arch or report["source"]["dirty"]):
        raise ValueError("Candidate version, architecture or source does not match")
    output.mkdir(parents=True, exist_ok=False)
    image = f"MooMole-{version}-mac-{arch}.dmg"
    shutil.copyfile(candidate / image, output / image)
    materials = output / f"MooMole-{version}-mac-{arch}-materials.zip"
    # Keep the existing complete candidate layout. Already-compressed payloads
    # need no second compression pass, nor a developer-machine round trip.
    with zipfile.ZipFile(materials, "w", compression=zipfile.ZIP_STORED) as archive:
        for path in sorted(candidate.rglob("*")):
            if path.is_symlink():
                raise ValueError("Do not package candidate links")
            if path.is_file():
                archive.write(path, path.relative_to(candidate).as_posix())
    metadata = {"version": version, "architecture": arch,
                "sourceRevision": report["source"]["revision"],
                "engineVersion": report["engineVersion"], "engineRevision": report["engineRevision"],
                "assets": [asset_record(output / image), asset_record(materials)]}
    write_json(output / f"release-{arch}.json", metadata)
    return metadata


def collect_assets(info, folder, revision):
    assets = []
    engines = set()
    for arch in ["arm64", "x64"]:
        metadata = read_json(folder / f"release-{arch}.json")
        if (metadata["version"] != info["version"] or metadata["architecture"] != arch
                or metadata["sourceRevision"] != revision):
            raise ValueError("Release assets must share the same tag, source and architecture")
        engines.add((metadata["engineVersion"], metadata["engineRevision"]))
        expected = {f'MooMole-{info["version"]}-mac-{arch}.dmg',
                    f'MooMole-{info["version"]}-mac-{arch}-materials.zip'}
        if {a["fileName"] for a in metadata["assets"]} != expected or len(metadata["assets"]) != 2:
            raise ValueError("Missing or duplicate architecture assets")
        for record in metadata["assets"]:
            if asset_record(folder / record["fileName"]) != record:
                raise ValueError("Release file differs from build metadata")
            assets.append({**record, "architecture": arch,
                           "url": f'{RELEASES}/download/{info["tag"]}/{record["fileName"]}'})
    if len(engines) != 1:
        raise ValueError("Bundled engine versions differ")
    engine_version, engine_revision = engines.pop()
    assets.sort(key=lambda item: item["fileName"])
    sums = folder / "SHA512SUMS"
    sums.write_text("".join(f'{a["sha512"]}  {a["fileName"]}\n' for a in assets), encoding="utf-8")
    return {**info, "schemaVersion": 1, "productId": PRODUCT, "sourceRevision": revision,
            "engineVersion": engine_version, "engineRevision": engine_revision,
            "assets": assets, "checksums": asset_record(sums)}


def release_body(metadata):
    text = f'# {metadata["title"]}\n\n{metadata["notes"]}\n\n---\n\n'
    text += f'Build / 构建 / ビルド: `{metadata["sourceRevision"]}` · Mole CLI {metadata["engineVersion"]}\n\n'
    text += "Assets / 发布资产 / 配布ファイル (bytes / 字节 / バイト, SHA-512):\n\n"
    for item in metadata["assets"]:
        text += f'- `{item["fileName"]}` — {item["size"]:,}\n  SHA-512: `{item["sha512"]}`\n'
    return text


def published_assets_match(metadata, published):
    if published["isDraft"] or published["tagName"] != metadata["tag"]:
        raise ValueError("Release must be published before enabling updates")
    expected = metadata["assets"] + [metadata["checksums"]]
    if not {a["fileName"] for a in expected}.issubset({a["name"] for a in published["assets"]}):
        raise ValueError("Published release assets are incomplete")
    for record in expected:
        actual = next(a for a in published["assets"] if a["name"] == record["fileName"])
        if (actual["state"] != "uploaded" or actual["size"] != record["size"]
                or actual["digest"] != "sha256:" + record["sha256"]):
            raise ValueError("Published asset size/digest differs from the built file")


def replace_block(text, name, content):
    start, end = f"<!-- {name}:start -->", f"<!-- {name}:end -->"
    if text.count(start) != 1 or text.count(end) != 1 or text.index(start) > text.index(end):
        raise ValueError(f"Keep one {name} block in each README")
    begin, finish = text.index(start) + len(start), text.index(end)
    return text[:begin] + "\n\n" + content + "\n\n" + text[finish:]


def readme_blocks(metadata, language, date):
    version, page, engine = metadata["version"], metadata["releaseUrl"], metadata["engineVersion"]
    heading, intro, columns, silicon, intel, verify = {
        "en": (f"Latest published version: **[{version}]({page})** ({date}), bundled with **Mole CLI {engine}**.",
               "This learning and research release provides cleanup, app uninstall and Quick Look maintenance within the limits below. Full permission/fault, installation/upgrade and corresponding-source/license acceptance remains incomplete. Release notes are available in English, Simplified Chinese and Japanese.",
               "Mac | Download", "Apple Silicon", "Intel",
               "Verify file size and SHA-512, open the DMG and copy MooMole to Applications. Matching source and dependency materials are in the architecture's materials ZIP. Packages use ad-hoc signing without Apple Developer ID signing or notarization."),
        "zh": (f"最新公开版本为 **[{version}]({page})**（{date}），内置 **Mole CLI {engine}**。",
               "这是学习研究版本，提供下表限定范围内的真实清理、应用卸载和 Quick Look 维护。完整权限/故障、安装升级及对应源码/许可验收仍未完成。发布说明提供英、中、日三语。",
               "Mac 类型 | 下载", "Apple 芯片", "Intel",
               "核对大小和 SHA-512 后打开 DMG，将 MooMole 复制到“应用程序”。对应架构的 materials ZIP 提供源码与依赖材料。安装包采用免费 ad-hoc 签名，没有 Apple Developer ID 签名或公证。"),
        "ja": (f"最新の公開版は **[{version}]({page})**（{date}）で、**Mole CLI {engine}** を内蔵しています。",
               "学習・研究向けのリリースとして、下表の範囲でクリーンアップ、App のアンインストール、Quick Look メンテナンスを実行できます。権限・障害、インストール・更新、完全な対応ソース・ライセンスの受け入れは未完了です。リリースノートは英語・簡体字中国語・日本語で提供します。",
               "Mac | ダウンロード", "Apple Silicon", "Intel",
               "サイズと SHA-512 を照合し、DMG を開いて MooMole を「アプリケーション」にコピーしてください。各アーキテクチャの materials ZIP は対応ソースと依存資料を含みます。無料の ad-hoc 署名を使用し、Apple Developer ID 署名や公証はありません。"),
    }[language]
    download = f"| {columns} |\n| --- | --- |\n"
    for arch, label in [("arm64", silicon), ("x64", intel)]:
        url = f"{RELEASES}/download/v{version}/MooMole-{version}-mac-{arch}.dmg"
        download += f"| {label} (`{arch}`) | [MooMole {version} DMG]({url}) |\n"
    download += f"\n{verify}\n\n[Release / 发布页 / リリース]({page}) · [SHA512SUMS]({RELEASES}/download/v{version}/SHA512SUMS)"
    return heading + "\n\n" + intro, download


def update_manifest(root, metadata, published):
    published_assets_match(metadata, published)
    if (metadata["productId"] != PRODUCT or metadata["tag"] != "v" + metadata["version"]
            or metadata["releaseUrl"] != f'{RELEASES}/tag/{metadata["tag"]}'):
        raise ValueError("Wrong product or release URL")
    file = root / "update-manifest.json"
    manifest = read_json(file)
    if manifest["schemaVersion"] != 1 or manifest["productId"] != PRODUCT:
        raise ValueError("Wrong update manifest")
    entry = {"version": metadata["version"], "notes": metadata["notes"],
             "releaseUrl": metadata["releaseUrl"], "assets": []}
    for arch in ["arm64", "x64"]:
        name = f'MooMole-{metadata["version"]}-mac-{arch}.dmg'
        item = next(a for a in metadata["assets"] if a["fileName"] == name)
        if item["url"] != f'{RELEASES}/download/{metadata["tag"]}/{name}':
            raise ValueError("Wrong installer URL")
        entry["assets"].append({"platform": "darwin", "architecture": arch, "packageType": "dmg",
                                **{k: item[k] for k in ["fileName", "url", "size", "sha512"]}})
    retained = [r for r in manifest["releases"] if r["version"] != entry["version"]] + [entry]
    retained.sort(key=lambda r: version_key(r["version"]), reverse=True)
    manifest["releases"] = retained[:64]
    stable = next((r for r in retained if version_key(r["version"])[3]), None)
    readmes = {}
    if stable and stable["version"] == entry["version"]:
        date = datetime.fromisoformat(published["publishedAt"].replace("Z", "+00:00")).astimezone(ZoneInfo("Asia/Shanghai")).date().isoformat()
        for name, language in [("README.md", "en"), ("README.zh-CN.md", "zh"), ("README.ja-JP.md", "ja")]:
            content = (root / name).read_text(encoding="utf-8")
            summary, downloads = readme_blocks(metadata, language, date)
            readmes[name] = replace_block(replace_block(content, "release", summary), "downloads", downloads)
    # Prepare all text first, so missing README markers do not partially update the feed.
    write_json(file, manifest)
    for name, content in readmes.items():
        (root / name).write_text(content, encoding="utf-8")
    return entry


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    check = commands.add_parser("validate")
    check.add_argument("--tag", required=True)
    check.add_argument("--github-output", type=Path)
    stage = commands.add_parser("stage")
    stage.add_argument("--candidate-dir", required=True, type=Path)
    stage.add_argument("--output-dir", required=True, type=Path)
    stage.add_argument("--version", required=True)
    stage.add_argument("--arch", required=True, choices=["arm64", "x64"])
    collect = commands.add_parser("validate-assets")
    collect.add_argument("--tag", required=True)
    collect.add_argument("--assets-dir", required=True, type=Path)
    collect.add_argument("--metadata-output", required=True, type=Path)
    update = commands.add_parser("update-manifest")
    update.add_argument("--metadata", required=True, type=Path)
    update.add_argument("--published", required=True, type=Path)
    args = parser.parse_args()
    if args.command == "validate":
        subprocess.run(["node", "scripts/version.mjs", "check", "--tag", args.tag], cwd=ROOT, check=True)
        info = release_info(ROOT, args.tag)
        outputs = {k: info[k] for k in ["version", "title", "notes_path", "prerelease"]}
        outputs["make_latest"] = not info["prerelease"]
        if args.github_output:
            with args.github_output.open("a", encoding="utf-8") as stream:
                for key, value in outputs.items():
                    stream.write(f"{key}={str(value).lower() if isinstance(value, bool) else value}\n")
        print(f'Release metadata ready: {info["tag"]}')
    elif args.command == "stage":
        stage_assets(args.candidate_dir, args.output_dir, args.version, args.arch)
    elif args.command == "validate-assets":
        info = release_info(ROOT, args.tag)
        revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
        metadata = collect_assets(info, args.assets_dir, revision)
        write_json(args.metadata_output, metadata)
        (args.assets_dir / "RELEASE-NOTES.md").write_text(release_body(metadata), encoding="utf-8")
    else:
        update_manifest(ROOT, read_json(args.metadata), read_json(args.published))


if __name__ == "__main__":
    main()
