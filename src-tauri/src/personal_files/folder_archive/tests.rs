use super::*;
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
};
fn job() -> Job {
    Job::new(TaskKind::PersonalArchive)
}
fn settings() -> db::Settings {
    db::Settings {
        acknowledged: true,
        ..Default::default()
    }
}
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let t = tempfile::Builder::new()
        .prefix("MooMole-folder-owned-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let root = home.join("Source");
    let out = home.join("Archives");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&out).unwrap();
    fs::create_dir(root.join("Empty")).unwrap();
    fs::create_dir(root.join("Nested")).unwrap();
    fs::write(root.join("Nested/data.txt"), b"owned nested data").unwrap();
    fs::write(root.join(".hidden"), b"owned hidden").unwrap();
    (t, root, out)
}
#[test]
fn whole_folder_plan_refuses_partial_scope_links_conflicts_protection_and_changes() {
    let (_t, root, out) = fixture();
    let config = settings();
    let control = job();
    let mut p =
        Plan::prepare(&root, out.to_str().unwrap(), "owned.zip", &config, &control).unwrap();
    assert_eq!(p.draft.files, 2);
    assert_eq!(p.draft.directories, 3);
    assert_eq!(p.draft.hidden, 1);
    p.validate(&config, &control, Instant::now()).unwrap();
    p.issued = Instant::now() - Duration::from_secs(121);
    assert!(p.time().is_err());
    fs::write(root.join("Nested/data.txt"), b"changed nested").unwrap();
    assert!(p.validate(&config, &control, Instant::now()).is_err());
    fs::write(out.join("owned.zip"), b"keep existing").unwrap();
    assert!(Plan::prepare(&root, out.to_str().unwrap(), "owned.zip", &config, &control).is_err());
    assert_eq!(fs::read(out.join("owned.zip")).unwrap(), b"keep existing");
    let mut protected = config.clone();
    protected
        .protected_paths
        .push(root.join("Nested/data.txt").to_string_lossy().into_owned());
    assert!(Plan::prepare(
        &root,
        out.to_str().unwrap(),
        "other.zip",
        &protected,
        &control
    )
    .is_err());
    symlink("Nested/data.txt", root.join("link")).unwrap();
    assert!(Plan::prepare(&root, out.to_str().unwrap(), "other.zip", &config, &control).is_err());
    fs::remove_file(root.join("link")).unwrap();
    fs::hard_link(root.join("Nested/data.txt"), root.join("hard")).unwrap();
    assert!(Plan::prepare(&root, out.to_str().unwrap(), "other.zip", &config, &control).is_err());
    fs::remove_file(root.join("hard")).unwrap();
    fs::create_dir(root.join("Library.photoslibrary")).unwrap();
    assert!(Plan::prepare(&root, out.to_str().unwrap(), "other.zip", &config, &control).is_err());
    assert!(!out.join("other.zip").exists());
    assert!(Plan::prepare(
        &root,
        root.to_str().unwrap(),
        "recursive.zip",
        &config,
        &control
    )
    .is_err());
}
#[test]
fn cancellation_keeps_sources_and_reopened_publication_intent_never_replays() {
    let (t, root, out) = fixture();
    let path = t.path().canonicalize().unwrap().join("state.sqlite");
    let c = db::open_owned(&path).unwrap();
    db::save_settings(&c, &settings()).unwrap();
    let db = Mutex::new(c);
    let p = Plan::prepare(
        &root,
        out.to_str().unwrap(),
        "cancelled.zip",
        &settings(),
        &job(),
    )
    .unwrap();
    let control = job();
    control.cancel();
    let mut r = run(&db, p, uuid::Uuid::new_v4().to_string(), &control).unwrap();
    assert_eq!(r.state, "cancelled");
    assert!(!out.join("cancelled.zip").exists());
    assert!(root.join("Nested/data.txt").exists());
    r.state = "running".into();
    r.phase = "publishing".into();
    r.finished_at = None;
    journal::save(&db.lock().unwrap(), &r).unwrap();
    drop(db);
    let c = db::open_owned(&path).unwrap();
    let mut row = journal::reconcile(&c, &[]).unwrap().remove(0);
    assert_eq!(row.state, "unknown");
    assert!(journal::admission(&c).is_err());
    row.reviewed_at = Some(db::now());
    journal::save(&c, &row).unwrap();
    assert!(journal::admission(&c).is_ok());
    assert!(!out.join("cancelled.zip").exists());
}
fn put(path: &Path, key: &std::ffi::CStr, value: &[u8]) {
    let held = AnchoredPath::open(path).unwrap();
    assert_eq!(
        unsafe {
            libc::fsetxattr(
                held.target().as_raw_fd(),
                key.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        },
        0
    );
}
fn get(path: &Path, key: &std::ffi::CStr) -> Vec<u8> {
    let held = AnchoredPath::open(path).unwrap();
    let len = unsafe {
        libc::fgetxattr(
            held.target().as_raw_fd(),
            key.as_ptr(),
            std::ptr::null_mut(),
            0,
            0,
            0,
        )
    };
    assert!(len >= 0);
    let mut bytes = vec![0u8; len as usize];
    assert_eq!(
        unsafe {
            libc::fgetxattr(
                held.target().as_raw_fd(),
                key.as_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                0,
                0,
            )
        },
        len
    );
    bytes
}

#[test]
fn folder_archive_and_in_app_mac_extraction_roundtrip_preserves_originals_and_root_properties() {
    let (t, root, out) = fixture();
    let cfg = settings();
    put(&root, c"org.moomole.owned", b"root metadata");
    put(
        &root.join("Nested/data.txt"),
        c"com.apple.ResourceFork",
        b"owned resource fork",
    );
    put(
        &root.join("Nested/data.txt"),
        c"com.apple.metadata:_kMDItemUserTags",
        b"<?xml version=\"1.0\"?><plist version=\"1.0\"><array><string>Red\n6</string></array></plist>",
    );
    fs::set_permissions(
        root.join("Nested/data.txt"),
        fs::Permissions::from_mode(0o444),
    )
    .unwrap();
    let c = db::open_owned(&t.path().join("roundtrip.sqlite")).unwrap();
    db::save_settings(&c, &cfg).unwrap();
    let db = Mutex::new(c);
    let p = Plan::prepare(&root, out.to_str().unwrap(), "roundtrip.zip", &cfg, &job()).unwrap();
    let archived = run(&db, p, uuid::Uuid::new_v4().to_string(), &job()).unwrap();
    assert_eq!(archived.state, "completed", "{}", archived.reason);
    let zip = out.join("roundtrip.zip");
    let before = fs::read(&zip).unwrap();
    let p = zip_extraction::Plan::prepare_mode(
        &zip,
        out.to_str().unwrap(),
        "Restored",
        &cfg,
        &job(),
        true,
    )
    .unwrap();
    let r = zip_extraction::run(&db, p, uuid::Uuid::new_v4().to_string(), &job()).unwrap();
    let r = serde_json::to_value(r).unwrap();
    assert_eq!(r["state"], "completed", "{r}");
    let restored = out.join("Restored/Source");
    assert_eq!(get(&restored, c"org.moomole.owned"), b"root metadata");
    for name in ["", "Empty", "Nested", "Nested/data.txt", ".hidden"] {
        let source = root.join(name);
        let target = restored.join(name);
        let m = fs::metadata(&source).unwrap();
        let n = fs::metadata(&target).unwrap();
        assert_eq!(m.mode() & 0o7777, n.mode() & 0o7777);
        assert_eq!(m.mtime(), n.mtime());
        if m.is_file() {
            assert_eq!(fs::read(&source).unwrap(), fs::read(&target).unwrap());
        }
    }
    assert_eq!(
        get(&restored.join("Nested/data.txt"), c"com.apple.ResourceFork"),
        b"owned resource fork"
    );
    assert_eq!(
        get(
            &restored.join("Nested/data.txt"),
            c"com.apple.metadata:_kMDItemUserTags"
        ),
        b"<?xml version=\"1.0\"?><plist version=\"1.0\"><array><string>Red\n6</string></array></plist>"
    );
    assert_eq!(fs::read(&zip).unwrap(), before);
    assert!(!out.join("Restored/__MACOSX").exists());
}
fn timestamp(path: &Path, seconds: i64) {
    let held = AnchoredPath::open(path).unwrap();
    let times = [libc::timespec {
        tv_sec: seconds,
        tv_nsec: 123456789,
    }; 2];
    assert_eq!(
        unsafe { libc::futimens(held.target().as_raw_fd(), times.as_ptr()) },
        0
    );
}
fn native_roundtrip(lose_completion: bool) {
    let (t, root, out) = fixture();
    let home = t.path().canonicalize().unwrap();
    let rich = root.join("Nested/data.txt");
    fs::write(root.join("unicode-照片.txt"), b"owned unicode").unwrap();
    fs::set_permissions(&rich, fs::Permissions::from_mode(0o640)).unwrap();
    fs::set_permissions(root.join("Empty"), fs::Permissions::from_mode(0o750)).unwrap();
    for p in [&root, &root.join("Empty"), &rich] {
        put(p, c"user.moomole.archive", b"owned metadata");
    }
    put(&rich, c"com.apple.ResourceFork", b"owned resource fork");
    put(&rich,c"com.apple.metadata:_kMDItemUserTags",b"<?xml version=\"1.0\"?><plist version=\"1.0\"><array><string>Red\n6</string></array></plist>");
    put(
        &rich,
        c"com.apple.quarantine",
        b"0081;68000000;MooMoleOwnedArchive;00000000-0000-4000-8000-000000000001",
    );
    timestamp(&rich, 1_700_000_001);
    timestamp(&root.join("Empty"), 1_700_000_003);
    timestamp(&root, 1_700_000_005);
    let config = settings();
    let control = job();
    let p = Plan::prepare(&root, out.to_str().unwrap(), "whole.zip", &config, &control).unwrap();
    let before =
        source::inventory(&root, &config.protected_paths, &control, Instant::now()).unwrap();
    let dbpath = home.join("state.sqlite");
    let c = db::open_owned(&dbpath).unwrap();
    db::save_settings(&c, &config).unwrap();
    if lose_completion {
        c.execute_batch(journal::TABLE).unwrap();
        c.execute_batch("CREATE TRIGGER lose_folder_completion BEFORE UPDATE ON folder_archives WHEN json_extract(NEW.body,'$.state')='completed' BEGIN SELECT RAISE(ABORT,'owned lost completion'); END;").unwrap();
    }
    let database = Mutex::new(c);
    let result = run(&database, p, uuid::Uuid::new_v4().to_string(), &control);
    if lose_completion {
        assert!(result.is_err());
    } else {
        assert_eq!(result.unwrap().state, "completed");
    }
    let zip = out.join("whole.zip");
    assert!(zip.is_file());
    assert_eq!(
        source::inventory(&root, &config.protected_paths, &control, Instant::now()).unwrap(),
        before
    );
    let extracted = home.join("Extracted");
    fs::create_dir(&extracted).unwrap();
    assert!(std::process::Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(&zip)
        .arg(&extracted)
        .status()
        .unwrap()
        .success());
    let restored = extracted.join("Source");
    assert!(restored.join("Empty").is_dir());
    assert_eq!(
        fs::read(restored.join("Nested/data.txt")).unwrap(),
        b"owned nested data"
    );
    assert_eq!(fs::read(restored.join(".hidden")).unwrap(), b"owned hidden");
    assert_eq!(
        fs::read(restored.join("unicode-照片.txt")).unwrap(),
        b"owned unicode"
    );
    for relative in ["", "Empty", "Nested/data.txt"] {
        assert_eq!(
            get(&root.join(relative), c"user.moomole.archive"),
            get(&restored.join(relative), c"user.moomole.archive")
        );
    }
    for key in [
        c"com.apple.ResourceFork",
        c"com.apple.metadata:_kMDItemUserTags",
        c"com.apple.quarantine",
    ] {
        assert_eq!(get(&rich, key), get(&restored.join("Nested/data.txt"), key));
    }
    assert_eq!(
        fs::metadata(restored.join("Nested/data.txt"))
            .unwrap()
            .mode()
            & 0o777,
        0o640
    );
    assert_eq!(
        fs::metadata(restored.join("Empty")).unwrap().mode() & 0o777,
        0o750
    );
    assert_eq!(
        fs::metadata(restored.join("Nested/data.txt"))
            .unwrap()
            .mtime(),
        1_700_000_001
    );
    assert_eq!(
        fs::metadata(restored.join("Empty")).unwrap().mtime(),
        1_700_000_003
    );
    assert_eq!(fs::metadata(&restored).unwrap().mtime(), 1_700_000_005);
    drop(database);
    let c = db::open_owned(&dbpath).unwrap();
    let records = journal::reconcile(&c, &[]).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].state,
        if lose_completion {
            "unknown"
        } else {
            "completed"
        }
    );
    assert_eq!(journal::admission(&c).is_err(), lose_completion);
    fs::write(format!("../.cache/folder-archive-native-{}.json",if lose_completion{"lost-completion"}else{"completed"}),serde_json::to_vec_pretty(&serde_json::json!({"scope":"production native archive core, not App window acceptance","records":records,"originalTreeAndMetadataUnchanged":true,"independentDittoExtractionVerified":true,"resourceForkTagsQuarantineModesEmptyDirectoriesHiddenNamesAndSecondPrecisionTimesVerified":true,"completionLossReconciledWithoutReplay":lose_completion})).unwrap()).unwrap();
}
#[test]
#[ignore = "actually builds metadata-preserving folder ZIPs and extracts with native ditto, including lost completion; own disposable fixtures only"]
fn real_folder_zip_preserves_mac_metadata_and_reopens_lost_completion_without_replay() {
    native_roundtrip(false);
    native_roundtrip(true);
}
