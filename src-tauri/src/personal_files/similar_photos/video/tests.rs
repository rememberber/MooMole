use super::*;
#[test]
fn video_source_content_refuses_protection_cancellation_and_changed_objects_before_copying() {
    use crate::engine::TaskKind;
    let t = tempfile::Builder::new()
        .prefix("MooMole-video-source-")
        .tempdir()
        .unwrap();
    let root = t.path().canonicalize().unwrap();
    let path = root.join("clip.mp4");
    let bytes = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/media/owned-color.mp4"));
    std::fs::write(&path, bytes).unwrap();
    let job = Arc::new(Job::new(TaskKind::PersonalScan));
    let scan = super::super::super::scan::collect(&root, &[], job.clone()).unwrap();
    let item = scan.items.first().unwrap();
    let mut copied = File::create(root.join("copy.partial")).unwrap();
    let mut count = 0;
    assert!(content(
        item,
        &[path.to_str().unwrap().into()],
        &job,
        Instant::now(),
        Some(&mut copied),
        &mut count
    )
    .is_err());
    assert_eq!(count, 0);
    assert_eq!(copied.metadata().unwrap().len(), 0);
    job.cancelled.store(true, Ordering::SeqCst);
    assert!(content(
        item,
        &[],
        &job,
        Instant::now(),
        Some(&mut copied),
        &mut count
    )
    .is_err());
    assert_eq!(count, 0);
    assert_eq!(copied.metadata().unwrap().len(), 0);
    job.cancelled.store(false, Ordering::SeqCst);
    assert_eq!(
        content(item, &[], &job, Instant::now(), None, &mut count).unwrap(),
        format!("{:x}", Sha256::digest(bytes))
    );
    std::fs::write(&path, "changed owned clip").unwrap();
    count = 0;
    assert!(content(
        item,
        &[],
        &job,
        Instant::now(),
        Some(&mut copied),
        &mut count
    )
    .is_err());
    assert_eq!(count, 0);
    assert_eq!(copied.metadata().unwrap().len(), 0);
}

#[tokio::test]
#[ignore = "real supervised AVFoundation/Vision, procedural clips, Mole and native Trash on auto-cleaned owned fixtures"]
async fn native_video_candidates_select_keep_trash_reopen_and_clean_fixtures() {
    use crate::{db, engine::TaskKind, rules};
    use std::os::unix::fs::MetadataExt;
    let temp = tempfile::Builder::new()
        .prefix("MooMole-video-owned-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let root = home.join("Media");
    std::fs::create_dir(&root).unwrap();
    let mut generate = tokio::process::Command::new("/usr/bin/swift");
    generate
        .arg("-module-cache-path")
        .arg(home.join("swift-modules"))
        .arg(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/fixtures/media/generate-candidates.swift"),
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
    let different = std::fs::read(root.join("different.mp4")).unwrap();
    let longer = std::fs::read(root.join("longer.mp4")).unwrap();
    let selected_bytes = std::fs::read(root.join("reference.mp4")).unwrap();
    let keeper_bytes = std::fs::read(root.join("resized.mp4")).unwrap();
    assert_ne!(
        Sha256::digest(&selected_bytes),
        Sha256::digest(&keeper_bytes)
    );
    std::fs::write(root.join("invalid.mp4"), "owned invalid container").unwrap();
    File::create(root.join("a-huge.mp4"))
        .unwrap()
        .set_len(FILE_LIMIT + 1)
        .unwrap();
    let job = Arc::new(Job::new(TaskKind::PersonalScan));
    let scan = super::super::super::scan::collect(&root, &[], job.clone()).unwrap();
    let stopped = Arc::new(Job::new(TaskKind::PersonalScan));
    stopped.cancelled.store(true, Ordering::SeqCst);
    let cancelled = objc2::rc::autoreleasepool(|_| collect(scan.clone(), stopped, &[])).unwrap();
    assert!(cancelled.report.cancelled);
    assert!(cancelled.items.is_empty());
    let scan = tokio::task::spawn_blocking(move || {
        objc2::rc::autoreleasepool(|_| collect(scan, job, &[]))
    })
    .await
    .unwrap()
    .unwrap();
    let report = scan.report.similar_photos.as_ref().unwrap();
    assert_eq!(report.method, METHOD);
    assert_eq!(report.checked_files, 4);
    assert_eq!(
        report.groups.len(),
        1,
        "{}",
        serde_json::to_string(&scan.report).unwrap()
    );
    assert_eq!(scan.items.len(), 2);
    assert!(!scan.report.complete);
    assert_eq!(
        report.bytes_read,
        selected_bytes.len() as u64
            + keeper_bytes.len() as u64
            + different.len() as u64
            + longer.len() as u64
            + "owned invalid container".len() as u64
    );
    let public_report = serde_json::to_value(&scan.report).unwrap();
    let selected = scan
        .items
        .iter()
        .find(|i| i.path.ends_with("/reference.mp4"))
        .unwrap()
        .clone();
    let ids = scan.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    assert!(store.offer(&ids, db::now()).is_err());
    let offer = store.offer(&[selected.id.clone()], db::now()).unwrap();
    assert_eq!(offer.kept_files.as_ref().unwrap().len(), 1);
    let plan = store.take(&offer.id, db::now()).unwrap();
    assert_eq!(plan.revision, REVISION);
    plan.validate_record().unwrap();
    assert!(plan.similar_photos.as_ref().unwrap()[0].video);
    let rebound: Plan = serde_json::from_slice(&serde_json::to_vec(&plan).unwrap()).unwrap();
    rebound.validate_record().unwrap();
    let mut wrong = plan.clone();
    wrong.revision = super::super::REVISION.into();
    assert!(wrong.validate_record().is_err());
    let mut wrong = plan.clone();
    wrong.similar_photos.as_mut().unwrap()[0].digest = "0".repeat(64);
    let check = Job::new(TaskKind::PersonalTrash);
    assert!(super::super::verify(&wrong, &selected, &check, &[]).is_err());
    let db_path = home.join("app.sqlite");
    let connection = db::open_owned(&db_path).unwrap();
    db::save_settings(
        &connection,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    let database = Arc::new(std::sync::Mutex::new(connection));
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
    let rule_home = home.clone();
    let result = super::super::super::writer::run_with(
        database.clone(),
        home.clone(),
        plan.clone(),
        Arc::new(Job::new(TaskKind::PersonalTrash)),
        move |requested, job| {
            let (engine, home) = (engine.clone(), rule_home.clone());
            async move { rules::run(&engine, &home, &requested, job).await }
        },
        |_| {},
    )
    .await
    .unwrap();
    for item in &result.items {
        if let Some(receipt) = &item.receipt {
            let m = std::fs::symlink_metadata(&receipt.destination).unwrap();
            assert_eq!((m.dev(), m.ino()), (receipt.device, receipt.inode));
            assert_eq!(std::fs::read(&receipt.destination).unwrap(), selected_bytes);
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
    assert_eq!(
        result.summary.moved_count,
        1,
        "{}",
        serde_json::to_string(&result).unwrap()
    );
    assert_eq!(
        std::fs::read(root.join("resized.mp4")).unwrap(),
        keeper_bytes
    );
    assert_eq!(
        std::fs::read(root.join("different.mp4")).unwrap(),
        different
    );
    assert_eq!(std::fs::read(root.join("longer.mp4")).unwrap(), longer);
    let expected = serde_json::to_value(&result).unwrap();
    let id = result.summary.id.clone();
    drop(database);
    let mut reopened = db::open_owned(&db_path).unwrap();
    assert_eq!(
        serde_json::to_value(crate::execution::detail(&reopened, &id).unwrap()).unwrap(),
        expected
    );
    let preview = crate::execution_archive::prepare(&mut reopened).unwrap();
    let folder = home.join("archive");
    let header =
        crate::execution_archive::archive(&mut reopened, &folder, &preview.fingerprint, db::now())
            .unwrap();
    assert_eq!(
        crate::execution_archive::detail(
            &folder,
            &header.id,
            crate::execution_archive::Kind::Cleanup,
            &id
        )
        .unwrap(),
        expected
    );
    std::fs::write(root.join("resized.mp4"), "owned changed keeper").unwrap();
    assert!(super::super::validate_keeper(&plan, &selected).is_err());
    let evidence = serde_json::json!({"report":public_report,"result":expected,"nativeWorker":true,"distinctResolutionAndBytes":true,"differentAndLongerExcluded":true,"keeperAndUnselectedPreserved":true,"allSelectedRefused":true,"cancelledBeforeDecode":true,"badDigestRefused":true,"changedKeeperRefused":true,"reopenAndArchiveVerified":true,"ownedReceiptRetrieved":true});
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.cache/verification");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("native-evidence.json");
    let mut bundle: serde_json::Value = std::fs::read(&path)
        .ok()
        .map(|v| serde_json::from_slice(&v).unwrap())
        .unwrap_or_else(|| serde_json::json!({"schemaVersion":1,"entries":{}}));
    bundle["entries"]["video-candidates-native.json"] = serde_json::json!({"sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&evidence).unwrap())),"data":evidence});
    std::fs::write(path, serde_json::to_vec(&bundle).unwrap()).unwrap();
    println!("Native candidate flow passed: four real clips, two candidates, one actual Trash receipt, preservation, reopen and archive verified; fixtures auto-cleaned.");
}
