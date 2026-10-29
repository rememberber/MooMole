"""Fast release-tooling tests; no installer build, upload or real release mutation."""
import copy
import importlib.util
import json
import shutil
import tempfile
import unittest
import zipfile
from pathlib import Path

spec = importlib.util.spec_from_file_location("manage_releases", Path(__file__).with_name("manage_releases.py"))
releases = importlib.util.module_from_spec(spec)
spec.loader.exec_module(releases)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        releases.write_json(self.root / "package.json", {"version": "0.3.0"})
        (self.root / "release-notes").mkdir()
        self.notes = self.root / "release-notes/0.3.0.md"
        self.notes.write_text("# MooMole 0.3.0\n\n## English\nNew.\n\n## 中文\n更新。\n\n## 日本語\n更新。\n", encoding="utf-8")
        self.info = releases.release_info(self.root, "v0.3.0")
        self.folder = self.root / "assets"
        self.folder.mkdir()
        for arch in ["arm64", "x64"]:
            candidate = self.root / f"candidate-{arch}"
            candidate.mkdir()
            releases.write_json(candidate / "validation.json", {
                "version": "0.3.0", "architecture": arch,
                "source": {"revision": "a" * 40, "dirty": False},
                "engineVersion": "1.65.0", "engineRevision": "b" * 40,
            })
            (candidate / f"MooMole-0.3.0-mac-{arch}.dmg").write_bytes(b"fixture dmg " + arch.encode())
            (candidate / "source").mkdir()
            (candidate / "source/BUILD.md").write_text("fixture source\n")
            staged = self.root / f"staged-{arch}"
            releases.stage_assets(candidate, staged, "0.3.0", arch)
            for file in staged.iterdir():
                shutil.copyfile(file, self.folder / file.name)
        self.metadata = releases.collect_assets(self.info, self.folder, "a" * 40)
        self.published = {"isDraft": False, "tagName": "v0.3.0", "publishedAt": "2026-10-29T03:00:00Z",
                          "assets": [{"name": a["fileName"], "state": "uploaded", "size": a["size"],
                                      "digest": "sha256:" + a["sha256"]}
                                     for a in self.metadata["assets"] + [self.metadata["checksums"]]]}
        releases.write_json(self.root / "update-manifest.json", {
            "schemaVersion": 1, "productId": releases.PRODUCT,
            "releases": [{"version": "0.2.0", "notes": "old", "assets": []}],
        })
        self.originals = {}
        for name in ["README.md", "README.zh-CN.md", "README.ja-JP.md"]:
            text = "User introduction\n<!-- release:start -->\nold\n<!-- release:end -->\nUser features\n<!-- downloads:start -->\nold\n<!-- downloads:end -->\nUser license\n"
            (self.root / name).write_text(text)
            self.originals[name] = text

    def test_single_source_notes_and_semver_channels(self):
        body = releases.release_body(self.metadata)
        for text in ["## English", "## 中文", "## 日本語", "SHA-512", "a" * 40]:
            self.assertIn(text, body)
        self.assertEqual(sorted(["1.0.0", "1.0.0-rc.10", "1.0.0-rc.2", "0.9.0"], key=releases.version_key),
                         ["0.9.0", "1.0.0-rc.2", "1.0.0-rc.10", "1.0.0"])
        for version in ["01.0.0", "1.0.0-01", "1.0.0-a..b", "1.0.0+a..b", "v1.0.0"]:
            with self.subTest(version=version), self.assertRaises(ValueError):
                releases.version_key(version)
        with self.assertRaises(ValueError):
            releases.release_info(self.root, "v0.2.0")
        self.notes.write_text("# MooMole 0.3.0\n\n## English\nNew\n\n## 日本語\n更新\n")
        with self.assertRaises(ValueError):
            releases.release_info(self.root, "v0.3.0")

    def test_archive_preserves_source_layout_and_missing_changed_mixed_assets_fail(self):
        with zipfile.ZipFile(self.folder / "MooMole-0.3.0-mac-arm64-materials.zip") as archive:
            self.assertIn("source/BUILD.md", archive.namelist())
            self.assertEqual(archive.read("source/BUILD.md"), b"fixture source\n")
        with self.assertRaises(ValueError):
            releases.collect_assets(self.info, self.folder, "c" * 40)
        file = self.folder / "MooMole-0.3.0-mac-x64.dmg"
        file.write_bytes(b"changed")
        with self.assertRaises(ValueError):
            releases.collect_assets(self.info, self.folder, "a" * 40)
        file.unlink()
        with self.assertRaises(ValueError):
            releases.collect_assets(self.info, self.folder, "a" * 40)

    def test_update_is_idempotent_preserves_old_releases_and_manual_readme_content(self):
        releases.update_manifest(self.root, self.metadata, self.published)
        manifest = releases.read_json(self.root / "update-manifest.json")
        self.assertEqual([r["version"] for r in manifest["releases"]], ["0.3.0", "0.2.0"])
        self.assertEqual(manifest["releases"][0]["notes"], self.info["notes"])
        self.assertEqual({a["architecture"] for a in manifest["releases"][0]["assets"]}, {"arm64", "x64"})
        before = {name: (self.root / name).read_bytes() for name in [*self.originals, "update-manifest.json"]}
        releases.update_manifest(self.root, self.metadata, self.published)
        self.assertEqual(before, {name: (self.root / name).read_bytes() for name in before})
        for name in self.originals:
            text = (self.root / name).read_text()
            self.assertIn("User features", text)
            self.assertIn("User license", text)
            self.assertIn("0.3.0", text)
            self.assertIn("2026-10-29", text)

    def test_incomplete_unpublished_or_wrong_uploaded_assets_cannot_enable_update(self):
        original = (self.root / "update-manifest.json").read_bytes()
        for mutate in [lambda r: r.update(isDraft=True),
                       lambda r: r["assets"].pop(),
                       lambda r: r["assets"][0].update(digest="sha256:wrong")]:
            published = copy.deepcopy(self.published)
            mutate(published)
            with self.assertRaises(ValueError):
                releases.update_manifest(self.root, self.metadata, published)
            self.assertEqual((self.root / "update-manifest.json").read_bytes(), original)
        (self.root / "README.ja-JP.md").write_text("Missing automatic blocks")
        with self.assertRaises(ValueError):
            releases.update_manifest(self.root, self.metadata, self.published)
        self.assertEqual((self.root / "update-manifest.json").read_bytes(), original)

    def test_prerelease_or_older_stable_does_not_replace_readme_latest_stable(self):
        for version in ["0.4.0-beta.1", "0.1.1"]:
            metadata = copy.deepcopy(self.metadata)
            metadata.update(version=version, tag="v" + version, releaseUrl=releases.RELEASES + "/tag/v" + version)
            for item in metadata["assets"]:
                item["fileName"] = item["fileName"].replace("0.3.0", version)
                item["url"] = item["url"].replace("0.3.0", version)
            published = copy.deepcopy(self.published)
            published["tagName"] = "v" + version
            for item in published["assets"]:
                item["name"] = item["name"].replace("0.3.0", version)
            releases.update_manifest(self.root, metadata, published)
            for name, text in self.originals.items():
                self.assertEqual((self.root / name).read_text(), text)


if __name__ == "__main__":
    unittest.main()
