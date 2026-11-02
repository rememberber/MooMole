use super::*;
use crate::{db, engine::TaskKind};
fn fixture() -> (tempfile::TempDir, Scan, Proof) {
    let temp = tempfile::Builder::new()
        .prefix("MooMole-live-unit-")
        .tempdir()
        .unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("Owned.jpg"), b"owned photo").unwrap();
    std::fs::write(root.join("Owned.mov"), b"owned movie").unwrap();
    let scan = super::super::scan::collect(&root, &[], Arc::new(Job::new(TaskKind::PersonalScan)))
        .unwrap();
    let photo = scan
        .items
        .iter()
        .find(|i| is_photo(Path::new(&i.path)))
        .unwrap();
    let movie = scan
        .items
        .iter()
        .find(|i| is_movie(Path::new(&i.path)))
        .unwrap();
    let p = Proof {
        id: uuid::Uuid::new_v4().to_string(),
        photo_id: photo.id.clone(),
        movie_id: movie.id.clone(),
        photo_sha256: format!("{:x}", Sha256::digest(b"owned photo")),
        movie_sha256: format!("{:x}", Sha256::digest(b"owned movie")),
    };
    (temp, scan, p)
}
#[test]
fn pair_selection_requires_both_components_exact_order_content_and_distinct_identity() {
    let (_t, mut scan, p) = fixture();
    scan.items.sort_by_key(|i| i.id != p.photo_id);
    let r = Report {
        pairs: vec![Pair {
            id: p.id.clone(),
            photo_id: p.photo_id.clone(),
            movie_id: p.movie_id.clone(),
            proof: p.clone(),
        }],
        checked_candidates: 1,
        rejected_candidates: 0,
        unpaired_files: 0,
        bytes_read: 20,
    };
    assert!(selection(&r, &scan.items[..1]).is_err());
    assert_eq!(selection(&r, &scan.items).unwrap().len(), 1);
    validate_record(&[p.clone()], &scan.items).unwrap();
    let mut files = scan.items.clone();
    files.reverse();
    assert!(validate_record(&[p.clone()], &files).is_err());
    let mut invalid = p.clone();
    invalid.movie_sha256 = "bad".into();
    assert!(validate_record(&[invalid], &scan.items).is_err());
    scan.report.live_photos = Some(r);
    let ids = scan.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    assert!(store.offer(&ids[..1], db::now()).is_err());
    let offer = store.offer(&ids, db::now()).unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    assert_eq!(plan.revision, REVISION);
    plan.validate_record().unwrap();
    let rebound: Plan = serde_json::from_slice(&serde_json::to_vec(&plan).unwrap()).unwrap();
    rebound.validate_record().unwrap();
    let mut invalid = plan.clone();
    invalid.live_photos = None;
    assert!(invalid.validate_record().is_err());
}
#[test]
fn changed_movie_and_missing_photo_receipt_prevent_pair_processing() {
    let (_t, mut scan, p) = fixture();
    scan.items.sort_by_key(|i| i.id != p.photo_id);
    scan.report.live_photos = Some(Report {
        pairs: vec![Pair {
            id: p.id.clone(),
            photo_id: p.photo_id.clone(),
            movie_id: p.movie_id.clone(),
            proof: p,
        }],
        checked_candidates: 1,
        rejected_candidates: 0,
        unpaired_files: 0,
        bytes_read: 20,
    });
    let ids = scan.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = store.offer(&ids, db::now()).unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    let c = db::open_owned(&_t.path().join("app.sqlite")).unwrap();
    assert!(verify(
        &plan,
        &plan.items[1],
        &Job::new(TaskKind::PersonalTrash),
        &[],
        &c,
        &uuid::Uuid::new_v4().to_string()
    )
    .is_err());
    std::fs::write(&plan.items[1].path, b"changed movie").unwrap();
    assert!(verify(
        &plan,
        &plan.items[0],
        &Job::new(TaskKind::PersonalTrash),
        &[],
        &c,
        ""
    )
    .is_err());
}
#[tokio::test]
#[ignore = "procedural PhotoKit resources + supervised production worker, real Mole and native Trash, exact own receipts returned and automatic fixture cleanup"]
async fn native_pairs_validate_select_trash_cancel_reopen_archive_and_remove_owned_samples() {
    use std::os::unix::fs::MetadataExt;
    let mut temp = tempfile::Builder::new()
        .prefix("MooMole-live-native-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let root = home.join("Media");
    std::fs::create_dir(&root).unwrap();
    let mut generate = tokio::process::Command::new("/usr/bin/swift");
    generate
        .args(["-suppress-warnings", "-module-cache-path"])
        .arg(home.join("swift-modules"))
        .arg(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/fixtures/media/generate-live-photos.swift"),
        )
        .arg(&root);
    crate::process::output_bounded(
        generate,
        Arc::new(Job::new(TaskKind::PersonalScan)),
        Duration::from_secs(90),
        64 * 1024,
    )
    .await
    .unwrap();
    let originals = std::fs::read_dir(&root)
        .unwrap()
        .map(|p| {
            let p = p.unwrap().path();
            let bytes = std::fs::read(&p).unwrap();
            (p, bytes)
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let control = Arc::new(Job::new(TaskKind::PersonalScan));
    let scan = super::super::scan::collect(&root, &[], control.clone()).unwrap();
    let scan = tauri::async_runtime::spawn_blocking(move || collect(scan, control, &[]))
        .await
        .unwrap()
        .unwrap();
    let r = scan.report.live_photos.as_ref().unwrap();
    assert_eq!(
        (r.pairs.len(), r.checked_candidates, r.rejected_candidates),
        (2, 4, 2)
    );
    assert_eq!(scan.items.len(), 4);
    let report = serde_json::to_value(&scan.report).unwrap();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let db_path = home.join("app.sqlite");
    let c = db::open_owned(&db_path).unwrap();
    db::save_settings(
        &c,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    let database = Arc::new(std::sync::Mutex::new(c));
    let mut cases = vec![];
    for (name, cancel) in [("PairA", false), ("PairB", true)] {
        let selected = store
            .scan
            .as_ref()
            .unwrap()
            .items
            .iter()
            .filter(|i| Path::new(&i.path).file_stem().unwrap() == name)
            .map(|i| i.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), 2);
        assert!(store.offer(&selected[..1], db::now()).is_err());
        let offer = store.offer(&selected, db::now()).unwrap();
        let confirmation = serde_json::to_value(&offer).unwrap();
        let plan = store.take(&offer.id, db::now()).unwrap();
        temp.disable_cleanup(true);
        std::fs::write(
            "../.cache/live-photo-native-rescue.json",
            serde_json::to_vec(&serde_json::json!({"ownedRoot":home,"plan":plan})).unwrap(),
        )
        .unwrap();
        let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        let own_home = home.clone();
        let mut calls = 0;
        let result = super::super::writer::run_with(
            database.clone(),
            home.clone(),
            plan,
            Arc::new(Job::new(TaskKind::PersonalTrash)),
            move |requested, job| {
                calls += 1;
                let (engine, home) = (engine.clone(), own_home.clone());
                async move {
                    if cancel && calls == 2 {
                        job.cancel()
                    }
                    crate::rules::run(&engine, &home, &requested, job).await
                }
            },
            |_| {},
        )
        .await
        .unwrap();
        std::fs::write(
            "../.cache/live-photo-native-rescue.json",
            serde_json::to_vec(&serde_json::json!({"ownedRoot":home,"result":result})).unwrap(),
        )
        .unwrap();
        for item in &result.items {
            if let Some(receipt) = &item.receipt {
                let m = std::fs::symlink_metadata(&receipt.destination).unwrap();
                assert_eq!((m.dev(), m.ino()), (receipt.device, receipt.inode));
                assert_eq!(
                    &std::fs::read(&receipt.destination).unwrap(),
                    &originals[Path::new(&item.path)]
                );
                assert!(!Path::new(&item.path).exists());
                objc2::rc::autoreleasepool(|_| {
                    use objc2_foundation::{NSFileManager, NSString, NSURL};
                    NSFileManager::defaultManager()
                        .moveItemAtURL_toURL_error(
                            &NSURL::fileURLWithPath(&NSString::from_str(&receipt.destination)),
                            &NSURL::fileURLWithPath(&NSString::from_str(&item.path)),
                        )
                        .unwrap()
                });
            }
        }
        assert_eq!(
            result.summary.moved_count,
            if cancel { 1 } else { 2 },
            "{}",
            serde_json::to_string(&result).unwrap()
        );
        assert_eq!(result.items.len(), 2);
        for (p, bytes) in &originals {
            assert_eq!(&std::fs::read(p).unwrap(), bytes)
        }
        cases.push(serde_json::json!({"cancelledAfterPhoto":cancel,"confirmation":confirmation,"result":result}));
    }
    drop(database);
    let mut reopened = db::open_owned(&db_path).unwrap();
    for c in &cases {
        let result = &c["result"];
        assert_eq!(
            serde_json::to_value(
                crate::execution::detail(&reopened, result["id"].as_str().unwrap()).unwrap()
            )
            .unwrap(),
            *result
        )
    }
    let draft = crate::execution_archive::prepare(&mut reopened).unwrap();
    let folder = home.join("archive");
    let header =
        crate::execution_archive::archive(&mut reopened, &folder, &draft.fingerprint, db::now())
            .unwrap();
    for c in &cases {
        assert_eq!(
            crate::execution_archive::detail(
                &folder,
                &header.id,
                crate::execution_archive::Kind::Cleanup,
                c["result"]["id"].as_str().unwrap()
            )
            .unwrap(),
            c["result"]
        )
    }
    drop(reopened);
    temp.disable_cleanup(false);
    let removed = temp.path().to_path_buf();
    drop(temp);
    assert!(!removed.exists());
    std::fs::remove_file("../.cache/live-photo-native-rescue.json").unwrap();
    std::fs::write("../.cache/live-photo-native.json",serde_json::to_vec(&serde_json::json!({"scope":"procedural exported pairs and production core; not a photo library or packaged App window","report":report,"cases":cases,"originalsAndUnselectedPreserved":true,"exactOwnReceiptsReturned":true,"databaseAndArchiveReopenVerified":true,"ownedTemporaryRootRemoved":true})).unwrap()).unwrap();
}
