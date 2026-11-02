use super::*;
#[tokio::test]
#[ignore = "real Music export scan, Mole rules and native Trash; only own disposable media/export"]
async fn real_music_export_trash_preserves_manifest_unselected_media_and_reopens_history() {
    use crate::engine::TaskKind;
    let temporary = tempfile::Builder::new()
        .prefix("moomole-music-native-")
        .tempdir()
        .unwrap();
    let home = temporary.path().canonicalize().unwrap();
    let media = home.join("Music");
    std::fs::create_dir(&media).unwrap();
    let chosen = media.join("owned 中文.wav");
    let keep = media.join("unlisted.wav");
    let mut audio = Vec::from(&b"RIFF"[..]);
    audio.extend(40u32.to_le_bytes());
    audio.extend(b"WAVEfmt ");
    audio.extend(16u32.to_le_bytes());
    audio.extend(1u16.to_le_bytes());
    audio.extend(1u16.to_le_bytes());
    audio.extend(8000u32.to_le_bytes());
    audio.extend(16000u32.to_le_bytes());
    audio.extend(2u16.to_le_bytes());
    audio.extend(16u16.to_le_bytes());
    audio.extend(b"data");
    audio.extend(4u32.to_le_bytes());
    audio.extend([0u8; 4]);
    std::fs::write(&chosen, &audio).unwrap();
    std::fs::write(&keep, &audio).unwrap();
    let manifest = home.join("Library.xml");
    let original=format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>Major Version</key><integer>1</integer><key>Minor Version</key><integer>1</integer><key>Application Version</key><string>1.6</string><key>Tracks</key><dict><key>1</key><dict><key>Track ID</key><integer>1</integer><key>Name</key><string>private title not returned</string><key>Track Type</key><string>File</string><key>Size</key><integer>{}</integer><key>Location</key><string>{}</string></dict></dict></dict></plist>",audio.len(),reqwest::Url::from_file_path(&chosen).unwrap());
    std::fs::write(&manifest, &original).unwrap();
    let library = home.join("Music/Owned.musiclibrary");
    std::fs::create_dir(&library).unwrap();
    std::fs::write(
        library.join("Library.musicdb"),
        b"owned untouched library database",
    )
    .unwrap();
    let scan = software_content::collect_music_library(
        &home,
        &manifest,
        &[],
        Arc::new(Job::new(TaskKind::PersonalScan)),
    )
    .unwrap();
    assert_eq!(scan.items.len(), 1);
    let report = serde_json::to_value(&scan.report).unwrap();
    assert!(!report.to_string().contains("private title"));
    let id = scan.items[0].id.clone();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let confirmation = store.offer(&[id], db::now()).unwrap();
    let confirmed = serde_json::to_value(&confirmation).unwrap();
    let plan = store.take(&confirmation.id, db::now()).unwrap();
    let dbpath = home.join("app.sqlite");
    let c = db::open_owned(&dbpath).unwrap();
    db::save_settings(
        &c,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    let database = Arc::new(std::sync::Mutex::new(c));
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
    let rules_home = home.clone();
    let result = run_with(
        database.clone(),
        home.clone(),
        plan,
        Arc::new(Job::new(TaskKind::PersonalTrash)),
        move |requested, job| {
            let engine = engine.clone();
            let home = rules_home.clone();
            async move { rules::run(&engine, &home, &requested, job).await }
        },
        |_| {},
    )
    .await
    .unwrap();
    // Reconcile only our exact receipt before collecting this disposable fixture.
    for item in &result.items {
        if let Some(r) = &item.receipt {
            let m = std::fs::symlink_metadata(&r.destination).unwrap();
            assert_eq!((m.dev(), m.ino()), (r.device, r.inode));
            assert!(!Path::new(&item.path).exists());
            assert_eq!(std::fs::read(&r.destination).unwrap(), audio);
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
        serde_json::to_string(&result).unwrap()
    );
    assert_eq!(result.origin, software_content::MODULE);
    assert_eq!(std::fs::read(&chosen).unwrap(), audio);
    assert_eq!(std::fs::read(&keep).unwrap(), audio);
    assert_eq!(std::fs::read_to_string(&manifest).unwrap(), original);
    assert_eq!(
        std::fs::read(library.join("Library.musicdb")).unwrap(),
        b"owned untouched library database"
    );
    let expected = serde_json::to_value(&result).unwrap();
    drop(database);
    let mut c = db::open_owned(&dbpath).unwrap();
    assert_eq!(
        serde_json::to_value(execution::detail(&c, &result.summary.id).unwrap()).unwrap(),
        expected
    );
    let preview = crate::execution_archive::prepare(&mut c).unwrap();
    let archive = home.join("archives");
    let header =
        crate::execution_archive::archive(&mut c, &archive, &preview.fingerprint, db::now())
            .unwrap();
    assert_eq!(
        crate::execution_archive::detail(
            &archive,
            &header.id,
            crate::execution_archive::Kind::Cleanup,
            &result.summary.id
        )
        .unwrap(),
        expected
    );
    std::fs::write("../.cache/music-library-native.json",serde_json::to_vec(&serde_json::json!({"report":report,"confirmation":confirmed,"result":expected,"header":header,"manifestPreserved":true,"unselectedMediaAndLibraryDatabasePreserved":true,"exactOwnedReceiptRetrievedForCleanup":true,"sqliteReopenAndArchiveVerified":true,"actualMusicExportWindow":false,"actualAppWindow":false})).unwrap()).unwrap();
}
