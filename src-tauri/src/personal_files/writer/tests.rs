use super::*;

#[tokio::test]
#[ignore = "explicit mounted-volume selection, real Mole analysis/rules and native move inside a unique owned external-volume fixture only"]
async fn real_mounted_volume_analysis_to_personal_move_keeps_unselected_files_and_history() {
    use crate::engine::{self, TaskKind};
    let requested = PathBuf::from(
        std::env::var_os("MOOMOLE_OWNED_VOLUME_ROOT")
            .expect("set the explicit mounted local volume root"),
    );
    let mount = PathBuf::from(crate::mounted_volumes::owned_selected_mount(&requested).unwrap());
    let internal = tempfile::Builder::new()
        .prefix("MooMole-volume-ledger-owned-")
        .tempdir()
        .unwrap();
    let home = internal.path().canonicalize().unwrap();
    let external = tempfile::Builder::new()
        .prefix("MooMole-volume-owned-")
        .tempdir_in(&mount)
        .unwrap();
    let fixture = external.path().canonicalize().unwrap();
    let root = fixture.join("Source");
    let target = fixture.join("Organized");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&target).unwrap();
    let chosen = root.join("chosen.txt");
    let kept = root.join("keep.txt");
    let chosen_bytes = vec![b'A'; 8192];
    let kept_bytes = vec![b'K'; 4096];
    std::fs::write(&chosen, &chosen_bytes).unwrap();
    std::fs::write(&kept, &kept_bytes).unwrap();
    assert_ne!(
        std::fs::metadata(&home).unwrap().dev(),
        std::fs::metadata(&root).unwrap().dev(),
        "fixture must be on a genuinely separate mounted device"
    );
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
    let analysis = engine::run(
        &engine,
        &home,
        Arc::new(Job::new(TaskKind::Analyze)),
        Some(root.clone()),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(analysis["path"].as_str(), root.to_str());
    assert!(analysis["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["path"].as_str() == chosen.to_str()));
    let scan = super::super::scan::collect(&root, &[], Arc::new(Job::new(TaskKind::PersonalScan)))
        .unwrap();
    let id = scan
        .items
        .iter()
        .find(|i| Path::new(&i.path) == chosen)
        .unwrap()
        .id
        .clone();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let settings = db::Settings {
        acknowledged: true,
        ..Default::default()
    };
    let offer = store
        .offer_move(
            &[id],
            target.to_str().unwrap(),
            movement::Conflict::Skip,
            &settings,
            db::now(),
        )
        .unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    let database = db::open_owned(&home.join("app.sqlite")).unwrap();
    db::save_settings(&database, &settings).unwrap();
    let database = Arc::new(std::sync::Mutex::new(database));
    let rule_home = home.clone();
    let rule_engine = engine.clone();
    let result = run_with(
        database.clone(),
        home.clone(),
        plan,
        Arc::new(Job::new(TaskKind::PersonalMove)),
        move |requested, job| {
            let h = rule_home.clone();
            let e = rule_engine.clone();
            async move { rules::run(&e, &h, &requested, job).await }
        },
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(
        result.summary.moved_count,
        1,
        "{}",
        serde_json::to_string_pretty(&result).unwrap()
    );
    assert!(!chosen.exists());
    assert_eq!(
        std::fs::read(target.join("chosen.txt")).unwrap(),
        chosen_bytes
    );
    assert_eq!(std::fs::read(&kept).unwrap(), kept_bytes);
    let expected = serde_json::to_value(&result).unwrap();
    drop(
        Arc::try_unwrap(database)
            .ok()
            .unwrap()
            .into_inner()
            .unwrap(),
    );
    let mut reopened = db::open_owned(&home.join("app.sqlite")).unwrap();
    assert_eq!(
        serde_json::to_value(execution::detail(&reopened, &result.summary.id).unwrap()).unwrap(),
        expected
    );
    let preview = crate::execution_archive::prepare(&mut reopened).unwrap();
    let archive_home = home.join("Archives");
    let archived = crate::execution_archive::archive(
        &mut reopened,
        &archive_home,
        &preview.fingerprint,
        db::now(),
    )
    .unwrap();
    assert_eq!(
        crate::execution_archive::detail(
            &archive_home,
            &archived.id,
            crate::execution_archive::Kind::Cleanup,
            &result.summary.id
        )
        .unwrap(),
        expected
    );
    std::fs::create_dir_all("../.cache").unwrap();
    external.close().unwrap();
    assert!(!fixture.exists());
    std::fs::write("../.cache/mounted-volume-native.json",serde_json::to_vec_pretty(&serde_json::json!({"selectedMountedRoot":mount,"analysis":analysis,"result":expected,"unselectedBytesPreserved":true,"databaseReopened":true,"archiveReadback":true,"ownedFixtureRemoved":true})).unwrap()).unwrap();
}

#[tokio::test]
#[ignore = "real playlist media, Mole rules and native Trash on self-created media/manifest only"]
async fn real_playlist_media_trash_preserves_manifest_and_unlisted_files() {
    use crate::engine::TaskKind;
    let temp = tempfile::Builder::new()
        .prefix("MooMole-playlist-native-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let folder = home.join("Movies");
    std::fs::create_dir(&folder).unwrap();
    let chosen = folder.join("chosen.wav");
    let keep = folder.join("unlisted.wav");
    let mut audio = Vec::from(&b"RIFF"[..]);
    audio.extend(40u32.to_le_bytes());
    audio.extend(b"WAVEfmt ");
    audio.extend(16u32.to_le_bytes());
    audio.extend(1u16.to_le_bytes()); // PCM
    audio.extend(1u16.to_le_bytes()); // mono
    audio.extend(8000u32.to_le_bytes());
    audio.extend(16000u32.to_le_bytes());
    audio.extend(2u16.to_le_bytes());
    audio.extend(16u16.to_le_bytes());
    audio.extend(b"data");
    audio.extend(4u32.to_le_bytes());
    audio.extend([0u8; 4]);
    std::fs::write(&chosen, &audio).unwrap();
    std::fs::write(&keep, "unlisted keep").unwrap();
    let manifest = folder.join("offline.m3u8");
    let original = "#EXTM3U\nchosen.wav\n";
    std::fs::write(&manifest, original).unwrap();
    let scan = software_content::collect_playlist(
        &home,
        &manifest,
        &[],
        Arc::new(Job::new(TaskKind::PersonalScan)),
    )
    .unwrap();
    assert_eq!(scan.items.len(), 1);
    let ids = vec![scan.items[0].id.clone()];
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = store.offer(&ids, db::now()).unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    let database = db::open_owned(&home.join("app.sqlite")).unwrap();
    db::save_settings(
        &database,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    let database = Arc::new(std::sync::Mutex::new(database));
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
    let rule_home = home.clone();
    let result = run_with(
        database.clone(),
        home.clone(),
        plan,
        Arc::new(Job::new(TaskKind::PersonalTrash)),
        move |requested, job| {
            let root = engine.clone();
            let home = rule_home.clone();
            async move { rules::run(&root, &home, &requested, job).await }
        },
        |_| {},
    )
    .await
    .unwrap();
    for item in &result.items {
        if let Some(r) = &item.receipt {
            let m = std::fs::symlink_metadata(&r.destination).unwrap();
            assert_eq!((m.dev(), m.ino()), (r.device, r.inode));
            assert!(!Path::new(&item.path).exists());
            objc2::rc::autoreleasepool(|_| {
                use objc2_foundation::{NSFileManager, NSString, NSURL};
                NSFileManager::defaultManager()
                    .moveItemAtURL_toURL_error(
                        &NSURL::fileURLWithPath(&NSString::from_str(&r.destination)),
                        &NSURL::fileURLWithPath(&NSString::from_str(&item.path)),
                    )
                    .unwrap();
            });
        }
    }
    assert_eq!(
        result.summary.moved_count,
        1,
        "{}",
        serde_json::to_string_pretty(&result).unwrap()
    );
    assert_eq!(result.origin, software_content::MODULE);
    assert_eq!(std::fs::read_to_string(&manifest).unwrap(), original);
    assert_eq!(std::fs::read(&chosen).unwrap(), audio);
    assert_eq!(std::fs::read_to_string(&keep).unwrap(), "unlisted keep");
    let expected = serde_json::to_value(&result).unwrap();
    let mut database = database.lock().unwrap();
    let preview = crate::execution_archive::prepare(&mut database).unwrap();
    let directory = home.join("archives");
    let header = crate::execution_archive::archive(
        &mut database,
        &directory,
        &preview.fingerprint,
        db::now(),
    )
    .unwrap();
    assert_eq!(
        crate::execution_archive::detail(
            &directory,
            &header.id,
            crate::execution_archive::Kind::Cleanup,
            &result.summary.id
        )
        .unwrap(),
        expected
    );
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/playlist-media-native.json",serde_json::to_vec_pretty(&serde_json::json!({"result":expected,"playlistPreserved":true,"unlistedFilePreserved":true,"ownedSampleRetrieved":true,"archiveReadbackVerified":true})).unwrap()).unwrap();
}

#[tokio::test]
#[ignore = "real browser download snapshot, Mole rules and native Trash on self-created files/database only"]
async fn real_browser_download_trash_preserves_history_and_unlisted_files() {
    let evidence = browser_download_roundtrip(None).await;
    std::fs::write(
        "../.cache/browser-downloads-native.json",
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
}
#[tokio::test]
#[ignore = "real multi-browser profile WAL snapshots, Mole and native Trash; own fixtures only"]
async fn real_multi_browser_profiles_trash_preserves_all_source_databases() {
    use software_content::browser_profiles::{Browser, Selection};
    let mut evidence = vec![];
    for browser in [
        Browser::Chrome,
        Browser::Chromium,
        Browser::Edge,
        Browser::Brave,
    ] {
        evidence.push(
            browser_download_roundtrip(Some(Selection {
                browser,
                profile: "Profile 2".into(),
            }))
            .await,
        );
    }
    std::fs::write(
        "../.cache/multi-browser-downloads-native.json",
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
}
async fn browser_download_roundtrip(
    profile: Option<software_content::browser_profiles::Selection>,
) -> serde_json::Value {
    use crate::engine::TaskKind;
    let temp = tempfile::Builder::new()
        .prefix("MooMole-browser-native-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let folder = home.join("Downloads");
    std::fs::create_dir(&folder).unwrap();
    let chosen = folder.join("chosen.txt");
    let keep = folder.join("unlisted.txt");
    std::fs::write(&chosen, "owned completed download").unwrap();
    std::fs::write(&keep, "unlisted keep").unwrap();
    let history = profile.clone().unwrap_or_default().history(&home).unwrap();
    std::fs::create_dir_all(history.parent().unwrap()).unwrap();
    let browser = rusqlite::Connection::open(&history).unwrap();
    browser.pragma_update(None, "journal_mode", "WAL").unwrap();
    browser.execute_batch("CREATE TABLE downloads(id INTEGER PRIMARY KEY,current_path TEXT,target_path TEXT,state INTEGER)").unwrap();
    browser
        .execute(
            "INSERT INTO downloads VALUES(1,?1,?1,1)",
            [chosen.to_str().unwrap()],
        )
        .unwrap();
    let original = std::fs::read(&history).unwrap();
    let wal = history.with_file_name("History-wal");
    let original_wal = std::fs::read(&wal).unwrap();
    let scan = if let Some(selection) = profile.clone() {
        software_content::collect_browser(
            &home,
            selection,
            &[],
            Arc::new(Job::new(TaskKind::PersonalScan)),
        )
    } else {
        software_content::collect(
            &home,
            software_content::Source::ChromeDownloads,
            &[],
            Arc::new(Job::new(TaskKind::PersonalScan)),
        )
    }
    .unwrap();
    assert_eq!(scan.items.len(), 1);
    let ids = vec![scan.items[0].id.clone()];
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = store.offer(&ids, db::now()).unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    let database = db::open_owned(&home.join("app.sqlite")).unwrap();
    db::save_settings(
        &database,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    let database = Arc::new(std::sync::Mutex::new(database));
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
    let rule_home = home.clone();
    let result = run_with(
        database.clone(),
        home.clone(),
        plan,
        Arc::new(Job::new(TaskKind::PersonalTrash)),
        move |requested, job| {
            let root = engine.clone();
            let home = rule_home.clone();
            async move { rules::run(&root, &home, &requested, job).await }
        },
        |_| {},
    )
    .await
    .unwrap();
    for item in &result.items {
        if let Some(r) = &item.receipt {
            let m = std::fs::symlink_metadata(&r.destination).unwrap();
            assert_eq!((m.dev(), m.ino()), (r.device, r.inode));
            assert!(!Path::new(&item.path).exists());
            objc2::rc::autoreleasepool(|_| {
                use objc2_foundation::{NSFileManager, NSString, NSURL};
                NSFileManager::defaultManager()
                    .moveItemAtURL_toURL_error(
                        &NSURL::fileURLWithPath(&NSString::from_str(&r.destination)),
                        &NSURL::fileURLWithPath(&NSString::from_str(&item.path)),
                    )
                    .unwrap();
            });
        }
    }
    assert_eq!(
        result.summary.moved_count,
        1,
        "{}",
        serde_json::to_string_pretty(&result).unwrap()
    );
    assert_eq!(result.origin, software_content::MODULE);
    assert_eq!(std::fs::read(&history).unwrap(), original);
    assert_eq!(std::fs::read(&wal).unwrap(), original_wal);
    assert_eq!(std::fs::read_to_string(&keep).unwrap(), "unlisted keep");
    let expected = serde_json::to_value(&result).unwrap();
    let mut database = database.lock().unwrap();
    let preview = crate::execution_archive::prepare(&mut database).unwrap();
    let directory = home.join("archives");
    let header = crate::execution_archive::archive(
        &mut database,
        &directory,
        &preview.fingerprint,
        db::now(),
    )
    .unwrap();
    assert_eq!(
        crate::execution_archive::detail(
            &directory,
            &header.id,
            crate::execution_archive::Kind::Cleanup,
            &result.summary.id
        )
        .unwrap(),
        expected
    );
    std::fs::create_dir_all("../.cache").unwrap();
    serde_json::json!({"profile":profile,"result":expected,"historyAndWalPreserved":true,"unlistedFilePreserved":true,"ownedSampleRetrieved":true,"archiveReadbackVerified":true})
}

#[tokio::test]
#[ignore = "real Mail-copy scope, fixed Mail process probe, Mole rules and native Trash on owned files only"]
async fn real_mail_attachment_copy_uses_catalog_scope_and_preserves_mail_database() {
    use crate::engine::TaskKind;
    use std::os::unix::ffi::OsStrExt;
    let temp = tempfile::Builder::new()
        .prefix("MooMole-mail-native-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let root = software_content::Source::MailDownloads.path(&home);
    std::fs::create_dir_all(&root).unwrap();
    let old = root.join("owned-attachment.txt");
    std::fs::write(&old, vec![b'x'; 6 * 1024 * 1024]).unwrap();
    let name = std::ffi::CString::new(old.as_os_str().as_bytes()).unwrap();
    let times = [libc::timespec {
        tv_sec: db::now() / 1000 - 40 * 86400,
        tv_nsec: 0,
    }; 2];
    assert_eq!(
        unsafe {
            libc::utimensat(
                libc::AT_FDCWD,
                name.as_ptr(),
                times.as_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        },
        0
    );
    let recent = root.join("recent.txt");
    std::fs::write(&recent, "recent keep").unwrap();
    let maildb = home.join("Library/Mail/Envelope Index");
    std::fs::create_dir_all(maildb.parent().unwrap()).unwrap();
    std::fs::write(&maildb, "owned mail database keep").unwrap();
    let scan = software_content::collect(
        &home,
        software_content::Source::MailDownloads,
        &[],
        Arc::new(Job::new(TaskKind::PersonalScan)),
    )
    .unwrap();
    assert_eq!(scan.items.len(), 1);
    let ids = vec![scan.items[0].id.clone()];
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = store.offer(&ids, db::now()).unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    let database = db::open_owned(&home.join("app.sqlite")).unwrap();
    db::save_settings(
        &database,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    let database = Arc::new(std::sync::Mutex::new(database));
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
    let rule_home = home.clone();
    let result = run_with(
        database.clone(),
        home.clone(),
        plan,
        Arc::new(Job::new(TaskKind::PersonalTrash)),
        move |requested, job| {
            let root = engine.clone();
            let home = rule_home.clone();
            async move { rules::run(&root, &home, &requested, job).await }
        },
        |_| {},
    )
    .await
    .unwrap();
    for item in &result.items {
        if let Some(r) = &item.receipt {
            let m = std::fs::symlink_metadata(&r.destination).unwrap();
            assert_eq!((m.dev(), m.ino()), (r.device, r.inode));
            assert!(!Path::new(&item.path).exists());
            objc2::rc::autoreleasepool(|_| {
                use objc2_foundation::{NSFileManager, NSString, NSURL};
                NSFileManager::defaultManager()
                    .moveItemAtURL_toURL_error(
                        &NSURL::fileURLWithPath(&NSString::from_str(&r.destination)),
                        &NSURL::fileURLWithPath(&NSString::from_str(&item.path)),
                    )
                    .unwrap();
            });
        }
    }
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/software-mail-native.json",serde_json::to_vec_pretty(&serde_json::json!({"result":result,"sampleRetrieved":old.exists(),"mailDatabasePreserved":std::fs::read_to_string(&maildb).unwrap()=="owned mail database keep"})).unwrap()).unwrap();
    assert_eq!(result.origin, software_content::MODULE);
    assert_eq!(
        result.summary.moved_count,
        1,
        "{}",
        serde_json::to_string_pretty(&result).unwrap()
    );
    assert_eq!(std::fs::read_to_string(&recent).unwrap(), "recent keep");
    assert_eq!(
        std::fs::read_to_string(&maildb).unwrap(),
        "owned mail database keep"
    );
    let expected = serde_json::to_value(&result).unwrap();
    let mut database = database.lock().unwrap();
    let preview = crate::execution_archive::prepare(&mut database).unwrap();
    let archives = home.join("archives");
    let header = crate::execution_archive::archive(
        &mut database,
        &archives,
        &preview.fingerprint,
        db::now(),
    )
    .unwrap();
    assert_eq!(
        crate::execution_archive::detail(
            &archives,
            &header.id,
            crate::execution_archive::Kind::Cleanup,
            &result.summary.id
        )
        .unwrap(),
        expected
    );
}

#[tokio::test]
#[ignore = "real Vision, Mole protection and native Trash on locally generated owned photos only"]
async fn real_similar_photos_select_trash_and_archive_without_claiming_identical_content() {
    use crate::engine::TaskKind;
    let temp = tempfile::Builder::new()
        .prefix("MooMole-similar-native-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let root = home.join("Pictures");
    std::fs::create_dir(&root).unwrap();
    for (name, w, h) in [
        ("first.png", 128, 96),
        ("resized.png", 256, 192),
        ("keep.png", 320, 240),
    ] {
        std::fs::write(root.join(name), super::super::media::fixture_png(w, h)).unwrap();
    }
    let job = Arc::new(Job::new(TaskKind::PersonalScan));
    let scan = super::super::scan::collect(&root, &[], job.clone()).unwrap();
    let scan = objc2::rc::autoreleasepool(|_| similar_photos::collect(scan, job, &[])).unwrap();
    assert_eq!(scan.report.similar_photos.as_ref().unwrap().groups.len(), 1);
    let chosen = scan
        .items
        .iter()
        .find(|i| i.path.ends_with("/first.png"))
        .unwrap()
        .id
        .clone();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = store.offer(&[chosen], db::now()).unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    let keeper = plan.similar_photos.as_ref().unwrap()[0].keeper.clone();
    let original = std::fs::read(&keeper.path).unwrap();
    let database = db::open_owned(&home.join("app.sqlite")).unwrap();
    db::save_settings(
        &database,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    let database = Arc::new(std::sync::Mutex::new(database));
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
    let rule_home = home.clone();
    let result = run_with(
        database.clone(),
        home.clone(),
        plan,
        Arc::new(Job::new(TaskKind::PersonalTrash)),
        move |requested, job| {
            let engine = engine.clone();
            let home = rule_home.clone();
            async move { rules::run(&engine, &home, &requested, job).await }
        },
        |_| {},
    )
    .await
    .unwrap();
    // Retrieve only actual returned receipts before assertions, including a
    // partial/uncertain native return. This cleanup is not product restoration.
    for item in &result.items {
        if let Some(r) = &item.receipt {
            let m = std::fs::symlink_metadata(&r.destination).unwrap();
            assert_eq!((m.dev(), m.ino()), (r.device, r.inode));
            assert!(!Path::new(&item.path).exists());
            objc2::rc::autoreleasepool(|_| {
                use objc2_foundation::{NSFileManager, NSString, NSURL};
                NSFileManager::defaultManager()
                    .moveItemAtURL_toURL_error(
                        &NSURL::fileURLWithPath(&NSString::from_str(&r.destination)),
                        &NSURL::fileURLWithPath(&NSString::from_str(&item.path)),
                    )
                    .unwrap();
            });
        }
    }
    assert_eq!(result.origin, similar_photos::MODULE);
    assert_eq!(
        result.summary.moved_count,
        1,
        "{}",
        serde_json::to_string_pretty(&result).unwrap()
    );
    assert_eq!(std::fs::read(&keeper.path).unwrap(), original);
    let expected = serde_json::to_value(&result).unwrap();
    let mut database = database.lock().unwrap();
    let preview = crate::execution_archive::prepare(&mut database).unwrap();
    let directory = home.join("archives");
    let header = crate::execution_archive::archive(
        &mut database,
        &directory,
        &preview.fingerprint,
        db::now(),
    )
    .unwrap();
    assert_eq!(
        crate::execution_archive::detail(
            &directory,
            &header.id,
            crate::execution_archive::Kind::Cleanup,
            &result.summary.id
        )
        .unwrap(),
        expected
    );
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/similar-photos-native-trash.json",serde_json::to_vec_pretty(&serde_json::json!({"result":expected,"keeperContentVerified":true,"ownedSampleRetrieved":true,"archiveReadbackVerified":true})).unwrap()).unwrap();
}

#[tokio::test]
#[ignore = "Real duplicate content checks, Mole protections and native Trash on owned files only; retrieves exact receipts"]
async fn real_duplicates_keep_one_copy_and_record_native_trash() {
    use crate::engine::TaskKind;
    let temp = tempfile::Builder::new()
        .prefix("MooMole-duplicates-native-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let root = home.join("Downloads");
    std::fs::create_dir(&root).unwrap();
    for name in ["one.txt", "two.txt", "keep.txt"] {
        std::fs::write(root.join(name), "owned duplicate content").unwrap();
    }
    let job = Arc::new(Job::new(TaskKind::PersonalScan));
    let scan = super::super::scan::collect(&root, &[], job.clone()).unwrap();
    let scan = duplicates::collect(scan, job).unwrap();
    let ids: Vec<_> = scan
        .items
        .iter()
        .filter(|i| !i.path.ends_with("/keep.txt"))
        .map(|i| i.id.clone())
        .collect();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = store.offer(&ids, db::now()).unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    let database = db::open_owned(&home.join("app.sqlite")).unwrap();
    db::save_settings(
        &database,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    let database = Arc::new(std::sync::Mutex::new(database));
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
    let rule_home = home.clone();
    let result = run_with(
        database.clone(),
        home.clone(),
        plan,
        Arc::new(Job::new(TaskKind::PersonalTrash)),
        move |requested, job| {
            let engine = engine.clone();
            let home = rule_home.clone();
            async move { rules::run(&engine, &home, &requested, job).await }
        },
        |_| {},
    )
    .await
    .unwrap();
    // Retrieve all actual receipts before checking success, never guess Trash.
    for item in &result.items {
        if let Some(r) = &item.receipt {
            let m = std::fs::symlink_metadata(&r.destination).unwrap();
            assert_eq!((m.dev(), m.ino()), (r.device, r.inode));
            assert!(!Path::new(&item.path).exists());
            objc2::rc::autoreleasepool(|_| {
                use objc2_foundation::{NSFileManager, NSString, NSURL};
                NSFileManager::defaultManager()
                    .moveItemAtURL_toURL_error(
                        &NSURL::fileURLWithPath(&NSString::from_str(&r.destination)),
                        &NSURL::fileURLWithPath(&NSString::from_str(&item.path)),
                    )
                    .unwrap();
            });
        }
    }
    assert_eq!(result.origin, duplicates::MODULE);
    assert_eq!(
        result.summary.moved_count,
        2,
        "{}",
        serde_json::to_string_pretty(&result).unwrap()
    );
    for name in ["one.txt", "two.txt", "keep.txt"] {
        assert_eq!(
            std::fs::read_to_string(root.join(name)).unwrap(),
            "owned duplicate content"
        );
    }
    let expected = serde_json::to_value(&result).unwrap();
    let run = result.summary.id.clone();
    drop(database);
    let mut reopened = db::open_owned(&home.join("app.sqlite")).unwrap();
    assert_eq!(
        serde_json::to_value(execution::detail(&reopened, &run).unwrap()).unwrap(),
        expected
    );
    let preview = crate::execution_archive::prepare(&mut reopened).unwrap();
    let archives = home.join("archives");
    let archive = crate::execution_archive::archive(
        &mut reopened,
        &archives,
        &preview.fingerprint,
        db::now(),
    )
    .unwrap();
    assert_eq!(
        crate::execution_archive::detail(
            &archives,
            &archive.id,
            crate::execution_archive::Kind::Cleanup,
            &run
        )
        .unwrap(),
        expected
    );
    std::fs::write(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.cache/duplicates-native.json"),serde_json::to_vec_pretty(&serde_json::json!({"record":expected,"keeperAndRetrievedContentVerified":true,"archiveReadbackVerified":true})).unwrap()).unwrap();
}

#[test]
fn move_intent_reopens_unknown_without_replay_and_rejects_method_tampering() {
    use crate::engine::TaskKind;
    let temp = tempfile::Builder::new()
        .prefix("MooMole-move-interrupted-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let source = home.join("Downloads");
    let destination = home.join("Organized");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(source.join("chosen.txt"), "original content").unwrap();
    let scan =
        super::super::scan::collect(&source, &[], Arc::new(Job::new(TaskKind::PersonalScan)))
            .unwrap();
    let ids = vec![scan.items[0].id.clone()];
    let settings = db::Settings {
        acknowledged: true,
        ..Default::default()
    };
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = store
        .offer_move(
            &ids,
            destination.to_str().unwrap(),
            movement::Conflict::Skip,
            &settings,
            db::now(),
        )
        .unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    let mut database = db::open_owned(&home.join("app.sqlite")).unwrap();
    let run = execution::begin_personal(&mut database, &plan, db::now()).unwrap();
    execution::intent(&mut database, &run, &ids[0], db::now()).unwrap();
    let movement = plan.movement.as_ref().unwrap();
    let target = movement.targets[0].destination.as_ref().unwrap();
    let moved = movement::Prepared::new(&plan.items[0], movement, target, &settings)
        .unwrap()
        .move_checked(|| Ok(()))
        .unwrap();
    assert!(matches!(moved.0, execution::Outcome::Moved(_)));
    // Model worker loss after native return, before result commit. No replay.
    drop(database);
    let mut database = db::open_owned(&home.join("app.sqlite")).unwrap();
    let detail = execution::detail(&database, &run).unwrap();
    assert_eq!(detail.summary.method, "move");
    assert_eq!(detail.summary.state, "interrupted");
    assert_eq!(detail.items[0].state, "uncertain");
    assert!(detail.items[0].receipt.is_none());
    assert_eq!(
        detail.items[0].target_path.as_deref(),
        Some(target.as_str())
    );
    assert_eq!(std::fs::read_to_string(target).unwrap(), "original content");
    assert!(!source.join("chosen.txt").exists());
    assert!(crate::execution_archive::prepare(&mut database).is_err());
    database
        .execute(
            "UPDATE execution_runs SET method='trash' WHERE id=?1",
            [&run],
        )
        .unwrap();
    assert!(execution::detail(&database, &run).is_err());
    database
        .execute(
            "UPDATE execution_runs SET method='move' WHERE id=?1",
            [&run],
        )
        .unwrap();
    crate::execution_review::acknowledge(
        &mut database,
        crate::execution_review::Kind::Cleanup,
        &run,
        &detail.snapshot_sha256,
        db::now(),
    )
    .unwrap();
    let preview = crate::execution_archive::prepare(&mut database).unwrap();
    let archive_root = home.join("archives");
    let archive = crate::execution_archive::archive(
        &mut database,
        &archive_root,
        &preview.fingerprint,
        db::now(),
    )
    .unwrap();
    let archive_path = std::fs::read_dir(&archive_root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let raw = std::fs::read(&archive_path).unwrap();
    let split = raw.iter().position(|b| *b == b'\n').unwrap();
    let mut header: serde_json::Value = serde_json::from_slice(&raw[..split]).unwrap();
    assert_eq!(header["version"], 3);
    header["version"] = 2.into();
    let mut downgraded = serde_json::to_vec(&header).unwrap();
    downgraded.push(b'\n');
    downgraded.extend_from_slice(&raw[split + 1..]);
    std::fs::write(&archive_path, &downgraded).unwrap();
    assert!(crate::execution_archive::detail(
        &archive_root,
        &archive.id,
        crate::execution_archive::Kind::Cleanup,
        &run
    )
    .unwrap_err()
    .contains("v3"));
    assert_eq!(std::fs::read_to_string(target).unwrap(), "original content");
}

#[tokio::test]
#[ignore = "Explicit real Mole checks and native same-volume moves on unique owned files; verifies conflicts, cancel, restart and v3 archive"]
async fn real_personal_move_conflicts_cancel_restart_and_archive() {
    use crate::engine::TaskKind;
    let mut evidence = Vec::new();
    for mode in ["move", "keepBoth", "skip", "cancel", "collision"] {
        let temp = tempfile::Builder::new()
            .prefix("MooMole-move-native-")
            .tempdir()
            .unwrap();
        let home = temp.path().canonicalize().unwrap();
        let root = home.join("Downloads");
        let target = home.join("Organized");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&target).unwrap();
        for name in ["selected-one.txt", "selected-two.txt", "keep.txt"] {
            std::fs::write(root.join(name), name).unwrap();
        }
        let scan =
            super::super::scan::collect(&root, &[], Arc::new(Job::new(TaskKind::PersonalScan)))
                .unwrap();
        let ids: Vec<_> = scan
            .items
            .iter()
            .filter(|i| i.path.contains("selected-"))
            .map(|i| i.id.clone())
            .collect();
        let selected_first = scan
            .items
            .iter()
            .find(|i| i.id == ids[0])
            .unwrap()
            .path
            .clone();
        let first_name = Path::new(&selected_first).file_name().unwrap().to_owned();
        if mode == "keepBoth" || mode == "skip" {
            std::fs::write(target.join(&first_name), "existing destination").unwrap();
        }
        let settings = db::Settings {
            acknowledged: true,
            ..Default::default()
        };
        let mut store = Store {
            scan: Some(scan),
            ..Default::default()
        };
        let offer = store
            .offer_move(
                &ids,
                target.to_str().unwrap(),
                if mode == "keepBoth" {
                    movement::Conflict::KeepBoth
                } else {
                    movement::Conflict::Skip
                },
                &settings,
                db::now(),
            )
            .unwrap();
        let plan = store.take(&offer.id, db::now()).unwrap();
        assert!(store.take(&offer.id, db::now()).is_err());
        let first_target = plan.movement.as_ref().unwrap().targets[0]
            .destination
            .clone();
        let database = db::open_owned(&home.join("app.sqlite")).unwrap();
        db::save_settings(&database, &settings).unwrap();
        let database = Arc::new(std::sync::Mutex::new(database));
        let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        let check_home = home.clone();
        let mut first_id: Option<String> = None;
        let mut injected = false;
        let result = run_with(
            database.clone(),
            home.clone(),
            plan,
            Arc::new(Job::new(TaskKind::PersonalMove)),
            move |requested, job| {
                let stop =
                    mode == "cancel" && first_id.as_ref().is_some_and(|id| id != &requested[0].0);
                first_id.get_or_insert_with(|| requested[0].0.clone());
                if mode == "collision" && !injected {
                    std::fs::write(first_target.as_ref().unwrap(), "late arrival").unwrap();
                    injected = true;
                }
                let home = check_home.clone();
                let engine = engine.clone();
                async move {
                    if stop {
                        job.cancel();
                        return Err("controlled cancellation between items".into());
                    }
                    rules::run(&engine, &home, &requested, job).await
                }
            },
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(result.summary.method, "move");
        assert_eq!(
            result.summary.moved_count,
            if ["skip", "cancel", "collision"].contains(&mode) {
                1
            } else {
                2
            },
            "{mode}: {}",
            serde_json::to_string_pretty(&result).unwrap()
        );
        assert_eq!(
            result.summary.state,
            match mode {
                "skip" | "collision" => "partial",
                "cancel" => "cancelled",
                _ => "completed",
            }
        );
        assert_eq!(
            result.items[0].state,
            match mode {
                "skip" => "skipped",
                "collision" => "failed",
                _ => "moved",
            }
        );
        if mode == "keepBoth" {
            assert!(result.items[0]
                .receipt
                .as_ref()
                .unwrap()
                .destination
                .ends_with(" (1).txt"));
        }
        let run = result.summary.id.clone();
        let expected = serde_json::to_value(&result).unwrap();
        for item in &result.items {
            if let Some(receipt) = &item.receipt {
                let at = AnchoredPath::open(Path::new(&receipt.destination)).unwrap();
                let m = at.target().metadata().unwrap();
                assert_eq!((m.dev(), m.ino()), (receipt.device, receipt.inode));
                assert_eq!(
                    item.target_path.as_deref(),
                    Some(receipt.destination.as_str())
                );
                assert!(!Path::new(&item.path).exists());
                assert_eq!(
                    std::fs::read_to_string(&receipt.destination).unwrap(),
                    Path::new(&item.path).file_name().unwrap().to_str().unwrap()
                );
                // Owned fixture retrieval only; this is not application restore.
                objc2::rc::autoreleasepool(|_| {
                    use objc2_foundation::{NSFileManager, NSString, NSURL};
                    NSFileManager::defaultManager()
                        .moveItemAtURL_toURL_error(
                            &NSURL::fileURLWithPath(&NSString::from_str(&receipt.destination)),
                            &NSURL::fileURLWithPath(&NSString::from_str(&item.path)),
                        )
                        .unwrap();
                });
            }
        }
        for name in ["selected-one.txt", "selected-two.txt", "keep.txt"] {
            assert_eq!(std::fs::read_to_string(root.join(name)).unwrap(), name);
        }
        if mode == "keepBoth" || mode == "skip" {
            assert_eq!(
                std::fs::read_to_string(target.join(&first_name)).unwrap(),
                "existing destination"
            );
        }
        if mode == "collision" {
            assert_eq!(
                std::fs::read_to_string(target.join(&first_name)).unwrap(),
                "late arrival"
            );
        }
        drop(database);
        let mut reopened = db::open_owned(&home.join("app.sqlite")).unwrap();
        assert_eq!(
            serde_json::to_value(execution::detail(&reopened, &run).unwrap()).unwrap(),
            expected
        );
        let archive_root = home.join("archives");
        let preview = crate::execution_archive::prepare(&mut reopened).unwrap();
        let archive = crate::execution_archive::archive(
            &mut reopened,
            &archive_root,
            &preview.fingerprint,
            db::now(),
        )
        .unwrap();
        assert_eq!(serde_json::to_value(&archive).unwrap()["version"], 3);
        assert_eq!(
            crate::execution_archive::detail(
                &archive_root,
                &archive.id,
                crate::execution_archive::Kind::Cleanup,
                &run
            )
            .unwrap(),
            expected
        );
        evidence.push(serde_json::json!({"mode":mode, "originalsRestoredAndUnselectedVerified":true,"record":expected, "archive":archive}));
        std::fs::write(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.cache/personal-move-native.json"),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn personal_intent_reconciles_and_archives_without_replaying_or_changing_the_file() {
    let t = tempfile::Builder::new()
        .prefix("MooMole-personal-recovery-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let root = home.join("Downloads");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("keep.txt"), "owned unique content").unwrap();
    let scan = super::super::scan::collect(
        &root,
        &[],
        Arc::new(Job::new(crate::engine::TaskKind::PersonalScan)),
    )
    .unwrap();
    let ids = vec![scan.items[0].id.clone()];
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = store.offer(&ids, db::now()).unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    let mut database = db::open_owned(&home.join("app.sqlite")).unwrap();
    let run = execution::begin_personal(&mut database, &plan, db::now()).unwrap();
    execution::intent(&mut database, &run, &ids[0], db::now()).unwrap();
    drop(database);
    let mut database = db::open_owned(&home.join("app.sqlite")).unwrap();
    let detail = execution::detail(&database, &run).unwrap();
    assert_eq!(detail.origin, MODULE);
    assert_eq!(detail.summary.state, "interrupted");
    assert_eq!(detail.items[0].state, "uncertain");
    assert!(detail.items[0].receipt.is_none());
    crate::execution_review::acknowledge(
        &mut database,
        crate::execution_review::Kind::Cleanup,
        &run,
        &detail.snapshot_sha256,
        db::now(),
    )
    .unwrap();
    let expected = serde_json::to_value(execution::detail(&database, &run).unwrap()).unwrap();
    let preview = crate::execution_archive::prepare(&mut database).unwrap();
    let archive_root = home.join("archives");
    let archive = crate::execution_archive::archive(
        &mut database,
        &archive_root,
        &preview.fingerprint,
        db::now(),
    )
    .unwrap();
    assert_eq!(
        crate::execution_archive::detail(
            &archive_root,
            &archive.id,
            crate::execution_archive::Kind::Cleanup,
            &run
        )
        .unwrap(),
        expected
    );
    assert_eq!(
        std::fs::read_to_string(root.join("keep.txt")).unwrap(),
        "owned unique content"
    );
    assert!(execution::list(&database).unwrap().is_empty());
}

#[tokio::test]
#[ignore = "Explicit real Mole path checks and system Trash on unique owned personal files; restores only exact receipts"]
async fn real_personal_trash_and_cancel_preserve_durable_results_and_exact_return() {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    use std::os::unix::fs::MetadataExt;
    let mut evidence = Vec::new();
    for cancel in [false, true] {
        let mut temp = tempfile::Builder::new()
            .prefix("MooMole-personal-native-")
            .tempdir()
            .unwrap();
        temp.disable_cleanup(true);
        let home = temp.path().canonicalize().unwrap();
        let root = home.join("Downloads");
        std::fs::create_dir(&root).unwrap();
        for name in ["selected-one.txt", "selected-two.txt", "keep.txt"] {
            std::fs::write(root.join(name), name).unwrap();
        }
        let scan = super::super::scan::collect(
            &root,
            &[],
            Arc::new(Job::new(crate::engine::TaskKind::PersonalScan)),
        )
        .unwrap();
        let ids: Vec<_> = scan
            .items
            .iter()
            .filter(|i| i.path.contains("selected-"))
            .map(|i| i.id.clone())
            .collect();
        let mut store = Store {
            scan: Some(scan),
            ..Default::default()
        };
        let now = db::now();
        let confirmation = store.offer(&ids, now).unwrap();
        let plan = store.take(&confirmation.id, now).unwrap();
        let database = db::open_owned(&home.join("app.sqlite")).unwrap();
        db::save_settings(
            &database,
            &db::Settings {
                acknowledged: true,
                ..Default::default()
            },
        )
        .unwrap();
        let database = Arc::new(std::sync::Mutex::new(database));
        let engine_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        let check_home = home.clone();
        let mut first_id: Option<String> = None;
        let result = run_with(
            database.clone(),
            home.clone(),
            plan,
            Arc::new(Job::new(crate::engine::TaskKind::PersonalTrash)),
            move |requested, job| {
                let stop = cancel && first_id.as_ref().is_some_and(|id| id != &requested[0].0);
                first_id.get_or_insert_with(|| requested[0].0.clone());
                let engine_root = engine_root.clone();
                let home = check_home.clone();
                async move {
                    if stop {
                        job.cancel();
                        return Err("controlled between-item cancellation".into());
                    }
                    rules::run(&engine_root, &home, &requested, job).await
                }
            },
            |_| {},
        )
        .await;
        let records = execution::list(&*database.lock().unwrap()).unwrap();
        let detail = execution::detail(&*database.lock().unwrap(), &records[0].id).unwrap();
        std::fs::write(
            home.join("rescue.json"),
            serde_json::to_vec_pretty(&detail).unwrap(),
        )
        .unwrap();
        // Restore exact receipts before asserting the outcome, including failures.
        for item in &detail.items {
            if let Some(receipt) = &item.receipt {
                let meta = std::fs::symlink_metadata(&receipt.destination).unwrap();
                assert_eq!((meta.dev(), meta.ino()), (receipt.device, receipt.inode));
                assert!(!Path::new(&item.path).exists());
                objc2::rc::autoreleasepool(|_| {
                    let from = NSURL::fileURLWithPath(&NSString::from_str(&receipt.destination));
                    let to = NSURL::fileURLWithPath(&NSString::from_str(&item.path));
                    NSFileManager::defaultManager()
                        .moveItemAtURL_toURL_error(&from, &to)
                        .unwrap();
                });
            }
        }
        let result = result.unwrap();
        assert_eq!(
            result.summary.moved_count,
            if cancel { 1 } else { 2 },
            "{}",
            serde_json::to_string_pretty(&detail).unwrap()
        );
        assert_eq!(result.origin, MODULE);
        assert_eq!(
            result.summary.state,
            if cancel { "cancelled" } else { "completed" }
        );
        assert_eq!(
            std::fs::read_to_string(root.join("keep.txt")).unwrap(),
            "keep.txt"
        );
        for name in ["selected-one.txt", "selected-two.txt"] {
            assert_eq!(std::fs::read_to_string(root.join(name)).unwrap(), name);
        }
        let run = result.summary.id.clone();
        let expected = serde_json::to_value(&result).unwrap();
        evidence.push(serde_json::json!({"cancelAfterFirst": cancel, "restoredAndContentVerified": true, "record": &expected}));
        let evidence_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.cache/personal-files-native-smoke.json");
        std::fs::write(evidence_path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
        drop(database);
        let database = db::open_owned(&home.join("app.sqlite")).unwrap();
        assert_eq!(
            serde_json::to_value(execution::detail(&database, &run).unwrap()).unwrap(),
            expected
        );
        drop(database);
        temp.disable_cleanup(false);
    }
}

#[tokio::test]
async fn new_client_protection_is_persisted_as_retained_without_native_calls() {
    let t = tempfile::Builder::new()
        .prefix("MooMole-personal-writer-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let root = home.join("Downloads");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("selected.zip"), "owned payload").unwrap();
    let scan = super::super::scan::collect(
        &root,
        &[],
        Arc::new(Job::new(crate::engine::TaskKind::PersonalScan)),
    )
    .unwrap();
    let ids: Vec<_> = scan.items.iter().map(|i| i.id.clone()).collect();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let now = db::now();
    let confirmation = store.offer(&ids, now).unwrap();
    let plan = store.take(&confirmation.id, now).unwrap();
    let database = db::open_owned(&home.join("app.sqlite")).unwrap();
    db::save_settings(
        &database,
        &db::Settings {
            acknowledged: true,
            protected_paths: vec![root.to_str().unwrap().into()],
            ..Default::default()
        },
    )
    .unwrap();
    let database = Arc::new(std::sync::Mutex::new(database));
    let result = run_with(
        database.clone(),
        home.clone(),
        plan,
        Arc::new(Job::new(crate::engine::TaskKind::PersonalTrash)),
        |_, _| async { panic!("protected item must not invoke rules or native Trash") },
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(result.origin, MODULE);
    assert_eq!(result.items[0].state, "skipped");
    assert!(result.items[0].receipt.is_none());
    assert_eq!(
        std::fs::read_to_string(root.join("selected.zip")).unwrap(),
        "owned payload"
    );
    let saved = serde_json::to_value(&result).unwrap();
    let run = result.summary.id;
    drop(database);
    let database = db::open_owned(&home.join("app.sqlite")).unwrap();
    assert_eq!(
        serde_json::to_value(execution::detail(&database, &run).unwrap()).unwrap(),
        saved
    );
}

#[tokio::test]
#[ignore = "real exported chat attachment, Mole rules and native Trash on self-created files only"]
async fn real_telegram_export_attachment_trash_preserves_chat_manifest_and_unlisted_files() {
    use crate::engine::TaskKind;
    let temp = tempfile::Builder::new()
        .prefix("MooMole-telegram-export-native-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let folder = home.join("Downloads/ChatExport");
    std::fs::create_dir_all(&folder).unwrap();
    let chosen = folder.join("chosen.wav");
    let keep = folder.join("unlisted.wav");
    let mut audio = Vec::from(&b"RIFF"[..]);
    audio.extend(40u32.to_le_bytes());
    audio.extend(b"WAVEfmt ");
    audio.extend(16u32.to_le_bytes());
    audio.extend(1u16.to_le_bytes()); // PCM
    audio.extend(1u16.to_le_bytes()); // mono
    audio.extend(8000u32.to_le_bytes());
    audio.extend(16000u32.to_le_bytes());
    audio.extend(2u16.to_le_bytes());
    audio.extend(16u16.to_le_bytes());
    audio.extend(b"data");
    audio.extend(4u32.to_le_bytes());
    audio.extend([0u8; 4]);
    std::fs::write(&chosen, &audio).unwrap();
    std::fs::write(&keep, "unlisted keep").unwrap();
    let manifest = folder.join("result.json");
    let original = r#"{"id":1,"type":"personal_chat","messages":[{"id":1,"type":"message","file":"chosen.wav","text":"owned private body never returned"}]}"#;
    std::fs::write(&manifest, original).unwrap();
    let scan = software_content::collect_telegram(
        &home,
        &manifest,
        &[],
        Arc::new(Job::new(TaskKind::PersonalScan)),
    )
    .unwrap();
    assert_eq!(scan.items.len(), 1);
    let ids = vec![scan.items[0].id.clone()];
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = store.offer(&ids, db::now()).unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    let database = db::open_owned(&home.join("app.sqlite")).unwrap();
    db::save_settings(
        &database,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    let database = Arc::new(std::sync::Mutex::new(database));
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
    let rule_home = home.clone();
    let result = run_with(
        database.clone(),
        home.clone(),
        plan,
        Arc::new(Job::new(TaskKind::PersonalTrash)),
        move |requested, job| {
            let root = engine.clone();
            let home = rule_home.clone();
            async move { rules::run(&root, &home, &requested, job).await }
        },
        |_| {},
    )
    .await
    .unwrap();
    for item in &result.items {
        if let Some(r) = &item.receipt {
            let m = std::fs::symlink_metadata(&r.destination).unwrap();
            assert_eq!((m.dev(), m.ino()), (r.device, r.inode));
            assert!(!Path::new(&item.path).exists());
            objc2::rc::autoreleasepool(|_| {
                use objc2_foundation::{NSFileManager, NSString, NSURL};
                NSFileManager::defaultManager()
                    .moveItemAtURL_toURL_error(
                        &NSURL::fileURLWithPath(&NSString::from_str(&r.destination)),
                        &NSURL::fileURLWithPath(&NSString::from_str(&item.path)),
                    )
                    .unwrap();
            });
        }
    }
    assert_eq!(
        result.summary.moved_count,
        1,
        "{}",
        serde_json::to_string_pretty(&result).unwrap()
    );
    assert_eq!(result.origin, software_content::MODULE);
    assert_eq!(std::fs::read_to_string(&manifest).unwrap(), original);
    assert_eq!(std::fs::read(&chosen).unwrap(), audio);
    assert_eq!(std::fs::read_to_string(&keep).unwrap(), "unlisted keep");
    let expected = serde_json::to_value(&result).unwrap();
    assert!(!expected.to_string().contains("owned private body"));
    let mut database = database.lock().unwrap();
    let preview = crate::execution_archive::prepare(&mut database).unwrap();
    let directory = home.join("archives");
    let header = crate::execution_archive::archive(
        &mut database,
        &directory,
        &preview.fingerprint,
        db::now(),
    )
    .unwrap();
    assert_eq!(
        crate::execution_archive::detail(
            &directory,
            &header.id,
            crate::execution_archive::Kind::Cleanup,
            &result.summary.id
        )
        .unwrap(),
        expected
    );
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/telegram-export-native.json",serde_json::to_vec_pretty(&serde_json::json!({"result":expected,"manifestPreserved":true,"unlistedFilePreserved":true,"ownedSampleRetrieved":true,"archiveReadbackVerified":true})).unwrap()).unwrap();
}
