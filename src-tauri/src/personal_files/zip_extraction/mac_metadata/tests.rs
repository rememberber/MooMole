use super::*;
use std::path::PathBuf;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
fn put(path: &Path, key: &str, value: &[u8]) {
    let f = File::open(path).unwrap();
    let k = CString::new(key).unwrap();
    assert_eq!(
        unsafe {
            libc::fsetxattr(
                f.as_raw_fd(),
                k.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        },
        0
    );
}
fn config() -> db::Settings {
    db::Settings {
        acknowledged: true,
        ..Default::default()
    }
}
fn job() -> Job {
    Job::new(TaskKind::PersonalArchive)
}
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let t = tempfile::Builder::new()
        .prefix("moomole-mac-restore-owned-")
        .tempdir()
        .unwrap();
    let root = t.path().canonicalize().unwrap();
    let source = root.join("Original");
    let out = root.join("Output");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&out).unwrap();
    fs::create_dir(source.join("Empty")).unwrap();
    fs::write(source.join(".notes"), b"owned hidden body").unwrap();
    fs::write(source.join("script.sh"), b"#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(source.join("script.sh"), fs::Permissions::from_mode(0o555)).unwrap();
    fs::set_permissions(source.join("Empty"), fs::Permissions::from_mode(0o500)).unwrap();
    put(
        &source.join(".notes"),
        "com.apple.ResourceFork",
        b"owned resource fork",
    );
    put(
        &source.join(".notes"),
        "com.apple.metadata:_kMDItemUserTags",
        b"<?xml version=\"1.0\"?><plist version=\"1.0\"><array><string>Red\n6</string></array></plist>",
    );
    put(
        &source.join(".notes"),
        "org.moomole.owned",
        b"owned custom data",
    );
    let mut info = [0; 32];
    info[8] = 0x40;
    put(&source.join(".notes"), "com.apple.FinderInfo", &info);
    put(
        &source.join(".notes"),
        "com.apple.quarantine",
        b"0081;00000000;OwnedHistorical;00000000-0000-0000-0000-000000000000",
    );
    for path in [
        &source,
        &source.join("Empty"),
        &source.join(".notes"),
        &source.join("script.sh"),
    ] {
        let f = File::open(path).unwrap();
        let times = [libc::timespec {
            tv_sec: 1600000000,
            tv_nsec: 0,
        }; 2];
        assert_eq!(unsafe { libc::futimens(f.as_raw_fd(), times.as_ptr()) }, 0);
    }
    let zip = root.join("archive.zip");
    assert!(Command::new("/usr/bin/ditto")
        .args(["-c", "-k", "--sequesterRsrc", "--keepParent"])
        .arg(&source)
        .arg(&zip)
        .status()
        .unwrap()
        .success());
    (t, source, zip, out)
}
#[test]
fn independent_ditto_zip_restores_data_properties_with_fresh_isolation_and_reopens_without_replay()
{
    let (t, source, zip, out) = fixture();
    let original = fs::read(&zip).unwrap();
    let plan = Plan::prepare_mode(
        &zip,
        out.to_str().unwrap(),
        "Restored",
        &config(),
        &job(),
        true,
    )
    .unwrap();
    let summary = plan.draft.mac_metadata.as_ref().unwrap();
    assert_eq!(summary.items, 4);
    assert_eq!(summary.executable, 1);
    assert_eq!(summary.finder_hidden, 1);
    assert_eq!(summary.skipped_quarantine, 1);
    assert!(summary.attributes >= 4);
    assert!(plan.draft.metadata_bytes > 0);
    let dbpath = t.path().join("state.sqlite");
    let c = db::open_owned(&dbpath).unwrap();
    db::save_settings(&c, &config()).unwrap();
    let db = Mutex::new(c);
    let r = run(&db, plan, uuid::Uuid::new_v4().to_string(), &job()).unwrap();
    assert_eq!(r.state, "completed", "{}", r.reason);
    let restored = out.join("Restored/Original");
    for n in ["", "Empty", ".notes", "script.sh"] {
        let a = source.join(n);
        let b = restored.join(n);
        let am = fs::metadata(&a).unwrap();
        let bm = fs::metadata(&b).unwrap();
        assert_eq!(am.mode() & 0o7777, bm.mode() & 0o7777);
        assert_eq!(am.mtime(), bm.mtime());
        assert_eq!(
            attributes(&File::open(&a).unwrap()).unwrap(),
            attributes(&File::open(&b).unwrap()).unwrap()
        );
        if am.is_file() {
            assert_eq!(fs::read(a).unwrap(), fs::read(b).unwrap());
        }
    }
    assert_eq!(
        fs::metadata(out.join("Restored")).unwrap().mode() & 0o7777,
        0o700
    );
    assert!(!out.join("Restored/__MACOSX").exists());
    let q = quarantine(&File::open(restored.join("script.sh")).unwrap())
        .unwrap()
        .unwrap();
    assert!(String::from_utf8(q.clone()).unwrap().contains("MooMole"));
    assert_ne!(
        q,
        quarantine(&File::open(source.join(".notes")).unwrap())
            .unwrap()
            .unwrap()
    );
    assert_eq!(fs::read(&zip).unwrap(), original);
    assert!(quarantine(&File::open(&zip).unwrap()).unwrap().is_none());
    drop(db);
    let mut c = db::open_owned(&dbpath).unwrap();
    let rows = journal::reconcile(&c, &[]).unwrap();
    assert_eq!(rows[0].state, "completed");
    let preview = crate::execution_archive::prepare(&mut c).unwrap();
    let archive_root = t.path().canonicalize().unwrap().join("Evidence");
    let h =
        crate::execution_archive::archive(&mut c, &archive_root, &preview.fingerprint, db::now())
            .unwrap();
    assert_eq!(serde_json::to_value(&h).unwrap()["version"], 6);
    let view = crate::execution_archive::view(&archive_root, &h.id).unwrap();
    assert_eq!(view.entries[0].summary["macMetadata"], true);
    let raw = crate::execution_archive::detail(
        &archive_root,
        &h.id,
        crate::execution_archive::Kind::Extraction,
        &r.id,
    )
    .unwrap();
    assert!(raw["draft"]["macMetadata"].is_object());
    let exported = t
        .path()
        .canonicalize()
        .unwrap()
        .join("owned.moomole-history");
    crate::execution_archive::export(&archive_root, &h.id, &exported).unwrap();
    let bytes = fs::read(&exported).unwrap();
    let newline = bytes.iter().position(|b| *b == b'\n').unwrap();
    for version in [1, 2, 3, 4, 5] {
        let mut header: serde_json::Value = serde_json::from_slice(&bytes[..newline]).unwrap();
        header["version"] = version.into();
        let mut downgraded = serde_json::to_vec(&header).unwrap();
        downgraded.push(b'\n');
        downgraded.extend_from_slice(&bytes[newline + 1..]);
        fs::write(&exported, &downgraded).unwrap();
        assert!(crate::execution_archive::import(&archive_root, &exported).is_err());
    }
    if let Ok(output) = std::env::var("MOOMOLE_MAC_RESTORE_EVIDENCE") {
        fs::write(output,serde_json::to_vec(&serde_json::json!({"draft":r.draft,"record":r,"header":h,"view":view,"detail":raw,"native":"independent ditto ZIP; body/attributes/mode/time readback; fresh quarantine; source and archive retained; SQLite reopen; v6 archive"})).unwrap()).unwrap();
    }
    // Read-only directories belong solely to this automatically removed fixture.
    for d in [source.join("Empty"), restored.join("Empty")] {
        fs::set_permissions(d, fs::Permissions::from_mode(0o700)).unwrap();
    }
}
#[test]
fn malformed_or_unmatched_sidecar_and_security_attributes_refuse_entire_metadata_mode() {
    let (_t, source, zip, out) = fixture();
    let mut f = File::open(&zip).unwrap();
    let mut entries = inventory::read(&mut f, &job(), Instant::now()).unwrap();
    let mut z = zip::ZipArchive::new(File::open(&zip).unwrap()).unwrap();
    let index = entries
        .iter()
        .find(|e| e.name.ends_with("._.notes"))
        .unwrap()
        .index
        .unwrap();
    let mut raw = Vec::new();
    z.by_index(index).unwrap().read_to_end(&mut raw).unwrap();
    assert!(parse(&raw, false).is_ok());
    for length in 0..raw.len() {
        assert!(parse(&raw[..length], false).is_err());
    }
    for at in [
        0, 4, 24, 26, 30, 34, 38, 42, 46, 84, 92, 96, 100, 118, 120, 124, 130,
    ] {
        let mut bad = raw.clone();
        bad[at] ^= 0xff;
        assert!(parse(&bad, false).is_err(), "offset {at}");
    }
    let mut alias = raw.clone();
    alias[58] |= 0x80;
    assert!(parse(&alias, false).is_err());
    let pos = raw
        .windows(b"com.apple.quarantine".len())
        .position(|w| w == b"com.apple.quarantine")
        .unwrap();
    let mut bad = raw.clone();
    bad[pos..pos + 20].copy_from_slice(b"com.apple.system.Bad");
    assert!(parse(&bad, false).is_err());
    entries.retain(|e| e.name != "Original/.notes");
    assert!(Metadata::read(&mut f, &mut entries, &job(), Instant::now()).is_err());
    put(
        &source.join(".notes"),
        "com.apple.TextEncoding",
        b"UTF-8;134217984",
    );
    let p = Plan::prepare_mode(
        &zip,
        out.to_str().unwrap(),
        "Stopped",
        &config(),
        &job(),
        true,
    )
    .unwrap();
    let c = db::open_owned(&_t.path().join("cancel.sqlite")).unwrap();
    db::save_settings(&c, &config()).unwrap();
    let control = job();
    control.cancel();
    let r = run(
        &Mutex::new(c),
        p,
        uuid::Uuid::new_v4().to_string(),
        &control,
    )
    .unwrap();
    assert_eq!(r.state, "cancelled");
    assert!(!out.join("Stopped").exists());
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&zip)
        .unwrap();
    let mut z = zip::ZipWriter::new_append(f).unwrap();
    z.add_directory(
        "__MACOSX/UserEmpty/",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    z.finish().unwrap();
    // An ordinary empty directory cannot disappear merely because its parent
    // has Apple's metadata-container name. Data-only extraction still keeps it.
    assert!(Plan::prepare_mode(
        &zip,
        out.to_str().unwrap(),
        "NoSilentLoss",
        &config(),
        &job(),
        true
    )
    .is_err());
    assert!(Plan::prepare(&zip, out.to_str().unwrap(), "KeepRaw", &config(), &job()).is_ok());
    fs::set_permissions(source.join("Empty"), fs::Permissions::from_mode(0o700)).unwrap();
}
#[test]
fn restored_read_only_directories_clean_up_on_cancel_but_replacements_are_retained() {
    let (_t, source, zip, out) = fixture();
    let mut p = Plan::prepare_mode(
        &zip,
        out.to_str().unwrap(),
        "Result",
        &config(),
        &job(),
        true,
    )
    .unwrap();
    let parent = p.validate(&config(), &job(), Instant::now()).unwrap();
    let mut s = tree::Stage::create(&parent, &p.draft.id).unwrap();
    s.extract(
        &mut p.snapshot,
        Format::Zip,
        &p.entries,
        p.quarantine.as_deref(),
        &job(),
        Instant::now(),
    )
    .unwrap();
    s.restore(p.metadata.as_ref().unwrap(), &job(), Instant::now())
        .unwrap();
    s.verify_metadata(
        &p.entries,
        p.quarantine.as_deref(),
        p.metadata.as_ref().unwrap(),
        &job(),
        Instant::now(),
    )
    .unwrap();
    assert!(s.cleanup());
    assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
    let mut s = tree::Stage::create(&parent, &uuid::Uuid::new_v4().to_string()).unwrap();
    s.extract(
        &mut p.snapshot,
        Format::Zip,
        &p.entries,
        p.quarantine.as_deref(),
        &job(),
        Instant::now(),
    )
    .unwrap();
    s.restore(p.metadata.as_ref().unwrap(), &job(), Instant::now())
        .unwrap();
    let stage = out.join(s.name.to_str().unwrap());
    fs::rename(
        stage.join("Original/Empty"),
        stage.join("Original/OldEmpty"),
    )
    .unwrap();
    fs::create_dir(stage.join("Original/Empty")).unwrap();
    fs::write(
        stage.join("Original/Empty/user.txt"),
        b"unexpected user content",
    )
    .unwrap();
    assert!(s
        .verify_metadata(
            &p.entries,
            p.quarantine.as_deref(),
            p.metadata.as_ref().unwrap(),
            &job(),
            Instant::now()
        )
        .is_err());
    assert!(!s.cleanup());
    assert_eq!(
        fs::read(stage.join("Original/Empty/user.txt")).unwrap(),
        b"unexpected user content"
    );
    fs::set_permissions(source.join("Empty"), fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn lost_completion_after_metadata_publication_reopens_unknown_and_never_replays() {
    let (t, source, zip, out) = fixture();
    let p = Plan::prepare_mode(
        &zip,
        out.to_str().unwrap(),
        "Published",
        &config(),
        &job(),
        true,
    )
    .unwrap();
    let dbpath = t.path().join("lost.sqlite");
    let c = db::open_owned(&dbpath).unwrap();
    db::save_settings(&c, &config()).unwrap();
    c.execute_batch(journal::TABLE).unwrap();
    c.execute_batch("CREATE TRIGGER lose_mac_completion BEFORE UPDATE ON zip_extractions WHEN json_extract(NEW.body,'$.state')='completed' BEGIN SELECT RAISE(FAIL,'owned completion loss'); END;").unwrap();
    let db = Mutex::new(c);
    assert!(run(&db, p, uuid::Uuid::new_v4().to_string(), &job()).is_err());
    let body = out.join("Published/Original/.notes");
    assert_eq!(fs::read(&body).unwrap(), b"owned hidden body");
    let identity = (
        fs::metadata(&body).unwrap().dev(),
        fs::metadata(&body).unwrap().ino(),
    );
    drop(db);
    let c = db::open_owned(&dbpath).unwrap();
    let rows = journal::reconcile(&c, &[]).unwrap();
    assert_eq!(rows[0].state, "unknown");
    assert!(rows[0].draft.mac_metadata.is_some());
    assert!(journal::admission(&c).is_err());
    assert_eq!(
        (
            fs::metadata(&body).unwrap().dev(),
            fs::metadata(&body).unwrap().ino()
        ),
        identity
    );
    assert_eq!(fs::read_dir(&out).unwrap().count(), 1);
    fs::set_permissions(source.join("Empty"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(
        out.join("Published/Original/Empty"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
}
