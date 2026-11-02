use super::*;
use crate::{engine::TaskKind, file_restoration};
use std::fs;
fn audio() -> Vec<u8> {
    let mut a = Vec::from(&b"RIFF"[..]);
    a.extend(40u32.to_le_bytes());
    a.extend(b"WAVEfmt ");
    a.extend(16u32.to_le_bytes());
    a.extend(1u16.to_le_bytes());
    a.extend(1u16.to_le_bytes());
    a.extend(8000u32.to_le_bytes());
    a.extend(16000u32.to_le_bytes());
    a.extend(2u16.to_le_bytes());
    a.extend(16u16.to_le_bytes());
    a.extend(b"data");
    a.extend(4u32.to_le_bytes());
    a.extend([0u8; 4]);
    a
}
// Independently reconcile only this test's exact receipts on failure. A rescue
// is fixture cleanup, never counted as successful product restoration.
fn retrieve_owned(source: &execution::Detail, bytes: &[u8]) -> Result<(), String> {
    for item in &source.items {
        let Some(r) = &item.receipt else { continue };
        match fs::symlink_metadata(&item.path) {
            Ok(m)
                if m.dev() == r.device
                    && m.ino() == r.inode
                    && fs::read(&item.path).is_ok_and(|v| v == bytes) =>
            {
                continue
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err("original path cannot be independently reconciled".into()),
        }
        let m = fs::symlink_metadata(&r.destination).map_err(|e| e.to_string())?;
        if m.dev() != r.device
            || m.ino() != r.inode
            || fs::read(&r.destination).map_err(|e| e.to_string())? != bytes
        {
            return Err("receipt changed".into());
        }
        objc2::rc::autoreleasepool(|_| {
            use objc2_foundation::{NSFileManager, NSString, NSURL};
            NSFileManager::defaultManager()
                .moveItemAtURL_toURL_error(
                    &NSURL::fileURLWithPath(&NSString::from_str(&r.destination)),
                    &NSURL::fileURLWithPath(&NSString::from_str(&item.path)),
                )
                .map_err(|e| e.to_string())
        })?;
    }
    Ok(())
}
#[tokio::test]
#[ignore = "formal Mole/native Trash then production single/batch restoration; project-owned disposable fixtures only"]
async fn real_software_sources_restore_exact_trash_receipts_and_keep_original_history() {
    let real_home = PathBuf::from(std::env::var_os("HOME").unwrap())
        .canonicalize()
        .unwrap();
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .canonicalize()
        .unwrap();
    assert!(target.starts_with(&real_home));
    let mut evidence = Vec::new();
    for kind in ["browser", "vlc", "telegram", "music"] {
        // Isolated software account structure inside a disposable project tree:
        // required because restoration originals must be in the real account.
        let temp = tempfile::Builder::new()
            .prefix("moomole-software-restore-owned-")
            .tempdir_in(&target)
            .unwrap();
        let home = temp.path().canonicalize().unwrap();
        let media = home.join("Media");
        fs::create_dir(&media).unwrap();
        let data = audio();
        let paths = [media.join("first.wav"), media.join("second.wav")];
        for p in &paths {
            fs::write(p, &data).unwrap();
        }
        let keep = media.join("unselected.wav");
        fs::write(&keep, &data).unwrap();
        let job = Arc::new(Job::new(TaskKind::PersonalScan));
        let manifest;
        let scan = match kind {
            "browser" => {
                manifest = software_content::browser_profiles::Selection::default()
                    .history(&home)
                    .unwrap();
                fs::create_dir_all(manifest.parent().unwrap()).unwrap();
                let c = rusqlite::Connection::open(&manifest).unwrap();
                c.execute_batch("CREATE TABLE downloads(id INTEGER PRIMARY KEY,current_path TEXT,target_path TEXT,state INTEGER)").unwrap();
                for (i, p) in paths.iter().enumerate() {
                    c.execute(
                        "INSERT INTO downloads VALUES(?1,?2,?2,1)",
                        rusqlite::params![i, p.to_str().unwrap()],
                    )
                    .unwrap();
                }
                drop(c);
                software_content::collect_browser(
                    &home,
                    software_content::browser_profiles::Selection::default(),
                    &[],
                    job,
                )
                .unwrap()
            }
            "vlc" => {
                manifest = home.join("owned.m3u8");
                fs::write(
                    &manifest,
                    format!(
                        "#EXTM3U\n{}\n{}\n",
                        reqwest::Url::from_file_path(&paths[0]).unwrap(),
                        reqwest::Url::from_file_path(&paths[1]).unwrap()
                    ),
                )
                .unwrap();
                software_content::collect_playlist(&home, &manifest, &[], job).unwrap()
            }
            "telegram" => {
                manifest = home.join("result.json");
                fs::write(&manifest,r#"{"id":1,"type":"personal_chat","messages":[{"id":1,"type":"message","file":"Media/first.wav","text":"private text"},{"id":2,"type":"message","file":"Media/second.wav"}]}"#).unwrap();
                software_content::collect_telegram(&home, &manifest, &[], job).unwrap()
            }
            "music" => {
                manifest = home.join("Library.xml");
                let tracks=paths.iter().enumerate().map(|(i,p)|format!("<key>{}</key><dict><key>Track ID</key><integer>{}</integer><key>Track Type</key><string>File</string><key>Size</key><integer>{}</integer><key>Location</key><string>{}</string></dict>",i+1,i+1,data.len(),reqwest::Url::from_file_path(p).unwrap())).collect::<String>();
                fs::write(&manifest,format!("<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>Major Version</key><integer>1</integer><key>Minor Version</key><integer>1</integer><key>Application Version</key><string>1.6</string><key>Tracks</key><dict>{tracks}</dict></dict></plist>")).unwrap();
                software_content::collect_music_library(&home, &manifest, &[], job).unwrap()
            }
            _ => unreachable!(),
        };
        assert_eq!(scan.items.len(), 2, "{kind}");
        let original_manifest = fs::read(&manifest).unwrap();
        let selected = scan.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        let mut store = Store {
            scan: Some(scan),
            ..Default::default()
        };
        let offer = store.offer(&selected, db::now()).unwrap();
        let plan = store.take(&offer.id, db::now()).unwrap();
        let dbpath = home.join("state.sqlite");
        let c = db::open_owned(&dbpath).unwrap();
        let config = db::Settings {
            acknowledged: true,
            ..Default::default()
        };
        db::save_settings(&c, &config).unwrap();
        let database = Arc::new(std::sync::Mutex::new(c));
        let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        let rule_home = home.clone();
        let source = run_with(
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
        let original = serde_json::to_value(&source).unwrap();
        let restored = (|| -> Result<(Vec<serde_json::Value>, Vec<execution::Detail>), String> {
            if source.summary.moved_count != 2 {
                return Err(serde_json::to_string(&source).unwrap());
            }
            let mut drafts = Vec::new();
            let mut restored = Vec::new();
            if kind == "music" {
                for item in &source.items {
                    let p = file_restoration::prepare(&source, &item.id, &real_home, &config)?;
                    drafts.push(serde_json::to_value(p.draft()).unwrap());
                    restored.push(file_restoration::run(
                        database.clone(),
                        real_home.clone(),
                        p,
                        Arc::new(Job::new(TaskKind::FileRestore)),
                    )?);
                }
            } else {
                let ids = source
                    .items
                    .iter()
                    .map(|i| i.id.clone())
                    .collect::<Vec<_>>();
                let p = file_restoration::batch::prepare(&source, &ids, &real_home, &config)?;
                drafts.push(serde_json::to_value(p.draft()).unwrap());
                restored.push(file_restoration::batch::run(
                    database.clone(),
                    real_home.clone(),
                    p,
                    Arc::new(Job::new(TaskKind::FileRestore)),
                    |_| {},
                )?);
            }
            if restored
                .iter()
                .map(|r| r.summary.moved_count)
                .sum::<usize>()
                != 2
            {
                return Err("product restoration did not restore both selected files".into());
            }
            Ok((drafts, restored))
        })();
        if let Err(e) = retrieve_owned(&source, &data) {
            let rescue = temp.keep();
            panic!("exact owned fixture retained at {}: {e}", rescue.display());
        }
        let (drafts, restored) = restored.unwrap();
        for item in &source.items {
            let r = item.receipt.as_ref().unwrap();
            let m = fs::symlink_metadata(&item.path).unwrap();
            assert_eq!((m.dev(), m.ino()), (r.device, r.inode));
            assert!(!Path::new(&r.destination).exists());
            assert_eq!(fs::read(&item.path).unwrap(), data);
        }
        assert_eq!(fs::read(&manifest).unwrap(), original_manifest);
        assert_eq!(fs::read(&keep).unwrap(), data);
        assert_eq!(
            serde_json::to_value(
                execution::detail(&database.lock().unwrap(), &source.summary.id).unwrap()
            )
            .unwrap(),
            original
        );
        drop(database);
        let mut c = db::open_owned(&dbpath).unwrap();
        for r in &restored {
            assert_eq!(
                serde_json::to_value(execution::detail(&c, &r.summary.id).unwrap()).unwrap(),
                serde_json::to_value(r).unwrap()
            );
        }
        let p = crate::execution_archive::prepare(&mut c).unwrap();
        let archive = home.join("archives");
        let h =
            crate::execution_archive::archive(&mut c, &archive, &p.fingerprint, db::now()).unwrap();
        for r in &restored {
            assert_eq!(
                crate::execution_archive::detail(
                    &archive,
                    &h.id,
                    crate::execution_archive::Kind::Cleanup,
                    &r.summary.id
                )
                .unwrap(),
                serde_json::to_value(r).unwrap()
            );
        }
        evidence.push(serde_json::json!({"kind":kind,"source":original,"drafts":drafts,"restored":restored,"header":h,"sourceHistoryUnchanged":true,"mediaManifestAndUnselectedPreserved":true,"sqliteReopenAndArchiveReadback":true,"rescuedByFallback":false,"fixture":home}));
    }
    fs::write("../.cache/software-restoration-native.json",serde_json::to_vec(&serde_json::json!({"cases":evidence,"scope":"isolated software account structures in project TempDirs, formal Mole/native Trash, production single/batch restore using real account home; no actual browser/player/App windows"})).unwrap()).unwrap();
}
