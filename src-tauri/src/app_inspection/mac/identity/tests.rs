use super::*;
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::PathBuf,
};

pub(crate) fn fixture(home: &Path, name: &str, id: &str) -> PathBuf {
    let app = home.join("Applications").join(name);
    fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    fs::write(app.join("Contents/Info.plist"), format!(r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>{id}</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleExecutable</key><string>fixture-main</string><key>CFBundleShortVersionString</key><string>1.0</string><key>CFBundleVersion</key><string>1</string></dict></plist>"#)).unwrap();
    fs::write(
        app.join("Contents/MacOS/fixture-main"),
        "#!/bin/sh\nexit 0\n",
    )
    .unwrap();
    fs::set_permissions(
        app.join("Contents/MacOS/fixture-main"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    app
}

#[test]
fn whole_bundle_snapshot_tracks_framework_links_without_following_external_data() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let app = fixture(&home, "中文 ' 应用.app", "io.moomole.fixture");
    let framework = app.join("Contents/Frameworks/Fixture.framework");
    fs::create_dir_all(framework.join("Versions/A/Resources")).unwrap();
    fs::write(
        framework.join("Versions/A/Resources/resource"),
        "bundled resource",
    )
    .unwrap();
    symlink("A", framework.join("Versions/Current")).unwrap();
    symlink("Versions/Current/Resources", framework.join("Resources")).unwrap();
    let outside = home.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("personal"), "retained").unwrap();
    symlink(&outside, app.join("Contents/external-link")).unwrap();
    let before = capture(&app, "io.moomole.fixture").unwrap();
    assert_eq!(before.bundle_id, "io.moomole.fixture");
    assert_eq!(before.app_version.as_deref(), Some("1.0"));
    assert!(before.inventory.entries > 8);
    fs::write(outside.join("personal"), "changed outside; not visited").unwrap();
    assert_eq!(before, capture(&app, "io.moomole.fixture").unwrap());
    let root = before.root.nodes.last().unwrap();
    // Existing cleanup directories STILL reject this layout.
    assert!(tree::inspect(&app, root.dev, root.ino, &mut tree::Budget::new()).is_err());
    fs::remove_file(framework.join("Versions/Current")).unwrap();
    symlink("B", framework.join("Versions/Current")).unwrap();
    assert_ne!(
        before.inventory,
        capture(&app, "io.moomole.fixture").unwrap().inventory
    );
    assert!(outside.join("personal").is_file());
}

#[test]
fn identity_main_program_contents_and_source_must_all_remain_bound() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let app = fixture(&home, "Fixture.app", "io.moomole.fixture");
    let before = capture(&app, "io.moomole.fixture").unwrap();
    assert!(capture(&app, "io.moomole.other").is_err());
    fs::create_dir(app.join("Contents/Resources")).unwrap();
    fs::write(app.join("Contents/Resources/changed"), "new content").unwrap();
    assert_ne!(before, capture(&app, "io.moomole.fixture").unwrap());
    fs::rename(&app, home.join("retained.app")).unwrap();
    let replaced = fixture(&home, "Fixture.app", "io.moomole.fixture");
    assert_ne!(before, capture(&replaced, "io.moomole.fixture").unwrap());
}

#[test]
fn root_identity_executable_links_hardlinks_special_files_and_non_apps_are_refused() {
    for mode in [
        "rootLink",
        "plistLink",
        "exeLink",
        "hardlink",
        "notExecutable",
        "notApp",
        "escapeName",
        "special",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let app = fixture(&home, "Fixture.app", "io.moomole.fixture");
        let executable = app.join("Contents/MacOS/fixture-main");
        match mode {
            "rootLink" => {
                fs::rename(&app, home.join("real.app")).unwrap();
                symlink(home.join("real.app"), &app).unwrap();
            }
            "plistLink" => {
                fs::rename(app.join("Contents/Info.plist"), home.join("original.plist")).unwrap();
                symlink(home.join("original.plist"), app.join("Contents/Info.plist")).unwrap();
            }
            "exeLink" => {
                fs::rename(&executable, home.join("original-executable")).unwrap();
                symlink(home.join("original-executable"), &executable).unwrap();
            }
            "hardlink" => fs::hard_link(&executable, home.join("other-link")).unwrap(),
            "notExecutable" => {
                fs::set_permissions(&executable, fs::Permissions::from_mode(0o644)).unwrap()
            }
            "special" => {
                let path =
                    std::ffi::CString::new(app.join("Contents/pipe").to_str().unwrap()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
            }
            _ => {
                let info = app.join("Contents/Info.plist");
                let text = fs::read_to_string(&info).unwrap().replace(
                    if mode == "notApp" {
                        "<string>APPL</string>"
                    } else {
                        "<string>fixture-main</string>"
                    },
                    if mode == "notApp" {
                        "<string>BNDL</string>"
                    } else {
                        "<string>../outside</string>"
                    },
                );
                fs::write(info, text).unwrap();
            }
        }
        assert!(capture(&app, "io.moomole.fixture").is_err(), "{mode}");
    }
}

#[test]
fn inventory_contents_can_be_compared_after_relocation_but_root_witness_cannot() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let app = fixture(&home, "Fixture.app", "io.moomole.fixture");
    let before = capture(&app, "io.moomole.fixture").unwrap();
    let moved = home.join("owned-relocation.app");
    fs::rename(&app, &moved).unwrap();
    let after = capture(&moved, "io.moomole.fixture").unwrap();
    assert_eq!(before.inventory, after.inventory);
    assert_ne!(before.root, after.root);
    assert_eq!(
        before.root.nodes.last().unwrap().ino,
        after.root.nodes.last().unwrap().ino
    );
    // This is a private directory rename, not a Trash or restore acceptance.
}

#[test]
#[ignore = "explicit installed MooTool bundle metadata and Info.plist only; never executes or moves it"]
fn installed_mootool_whole_bundle_identity_read_only() {
    let path = Path::new("/Applications/MooTool Next Electron.app");
    assert!(path.is_dir(), "explicit read-only target is absent");
    let before = capture(path, "com.rememberber.mootool.next.electron").unwrap();
    let after = capture(path, "com.rememberber.mootool.next.electron").unwrap();
    assert_eq!(before, after);
    eprintln!("Actual installed Electron bundle: inventory entries={}, version={:?}, metadata stable; no running/ownership/execute eligibility inferred",before.inventory.entries,before.app_version);
}
