use super::*;
use crate::db;
#[tokio::test]
#[ignore = "real EXIF, Vision, Mole and native Trash on owned generated JPEGs only; retrieves exact returned receipt"]
async fn real_capture_sequence_selection_trash_reopen_and_archive_preserve_other_frames() {
    use crate::{engine::TaskKind, rules};
    use std::os::unix::fs::MetadataExt;
    let t = tempfile::Builder::new()
        .prefix("MooMole-sequence-native-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let root = home.join("Pictures");
    std::fs::create_dir(&root).unwrap();
    for (name, sub, width, height) in [
        ("first.jpg", "100", 128, 96),
        ("second.jpg", "400", 256, 192),
        ("keeper.jpg", "800", 320, 240),
    ] {
        std::fs::write(
            root.join(name),
            capture::fixture_image(
                &media::fixture_png(width, height),
                "2026:10:31 12:00:00",
                sub,
                "Owned camera",
            ),
        )
        .unwrap();
    }
    let untouched = media::fixture_other_png(128, 96);
    std::fs::write(root.join("unselected.png"), &untouched).unwrap();
    let job = Arc::new(Job::new(TaskKind::PersonalScan));
    let scan = super::super::scan::collect(&root, &[], job.clone()).unwrap();
    let scan = objc2::rc::autoreleasepool(|_| collect_with_sequence(scan, job, &[], true)).unwrap();
    let public_report = serde_json::to_value(&scan.report).unwrap();
    assert_eq!(scan.report.similar_photos.as_ref().unwrap().groups.len(), 1);
    let selected = scan
        .items
        .iter()
        .find(|i| i.path.ends_with("/first.jpg"))
        .unwrap()
        .clone();
    let selected_bytes = std::fs::read(&selected.path).unwrap();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = store.offer(&[selected.id.clone()], db::now()).unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    let keeper = plan.similar_photos.as_ref().unwrap()[0].keeper.clone();
    let keeper_bytes = std::fs::read(&keeper.path).unwrap();
    let other_bytes = std::fs::read(root.join("keeper.jpg")).unwrap();
    let db_path = home.join("app.sqlite");
    let database = db::open_owned(&db_path).unwrap();
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
    let result = super::super::writer::run_with(
        database.clone(),
        home.clone(),
        plan,
        Arc::new(Job::new(TaskKind::PersonalTrash)),
        move |requested, job| {
            let (engine, home) = (engine.clone(), rule_home.clone());
            async move { rules::run(&engine, &home, &requested, job).await }
        },
        |_| {},
    )
    .await
    .unwrap();
    // Cleanup only the exact owned object returned by the native operation.
    for item in &result.items {
        if let Some(r) = &item.receipt {
            let m = std::fs::symlink_metadata(&r.destination).unwrap();
            assert_eq!((m.dev(), m.ino()), (r.device, r.inode));
            assert!(!Path::new(&item.path).exists());
            assert_eq!(std::fs::read(&r.destination).unwrap(), selected_bytes);
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
    assert_eq!(result.origin, MODULE);
    assert_eq!(std::fs::read(&keeper.path).unwrap(), keeper_bytes);
    assert_eq!(std::fs::read(root.join("keeper.jpg")).unwrap(), other_bytes);
    assert_eq!(
        std::fs::read(root.join("unselected.png")).unwrap(),
        untouched
    );
    assert_eq!(std::fs::read(&selected.path).unwrap(), selected_bytes);
    let expected = serde_json::to_value(&result).unwrap();
    drop(database);
    let mut reopened = db::open_owned(&db_path).unwrap();
    assert_eq!(
        serde_json::to_value(crate::execution::detail(&reopened, &result.summary.id).unwrap())
            .unwrap(),
        expected
    );
    let preview = crate::execution_archive::prepare(&mut reopened).unwrap();
    let directory = home.join("archive");
    let header = crate::execution_archive::archive(
        &mut reopened,
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
    std::fs::write("../.cache/photo-sequences-native.json",serde_json::to_vec_pretty(&serde_json::json!({"scope":"native core, not App window acceptance","report":public_report,"result":expected,"keeperAndUnselectedPreserved":true,"ownedSampleRetrieved":true,"databaseReopenAndArchiveReadbackVerified":true})).unwrap()).unwrap();
}
#[test]
fn capture_sequences_use_real_exif_visual_evidence_and_bounded_anchor_groups() {
    let t = tempfile::Builder::new()
        .prefix("MooMole-capture-owned-")
        .tempdir()
        .unwrap();
    let root = t.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("other-folder")).unwrap();
    let png = media::fixture_png(128, 96);
    for (name, time, sub, model) in [
        ("first.jpg", "2026:10:31 12:00:00", "100", "One"),
        ("second.jpg", "2026:10:31 12:00:00", "400", "One"),
        ("outside-anchor.jpg", "2026:10:31 12:00:02", "200", "One"),
        ("far.jpg", "2026:10:31 12:00:10", "100", "One"),
        ("other-camera.jpg", "2026:10:31 12:00:00", "100", "Two"),
        (
            "other-folder/other.jpg",
            "2026:10:31 12:00:00",
            "100",
            "One",
        ),
    ] {
        let jpeg = capture::fixture_image(&png, time, sub, model);
        let metadata = capture::read(&jpeg).unwrap();
        assert_eq!(metadata.camera.1, model);
        assert_eq!(metadata.offset.as_deref(), Some("+08:00"));
        std::fs::write(root.join(name), jpeg).unwrap();
    }
    std::fs::write(root.join("no-exif.png"), &png).unwrap();
    std::fs::write(
        root.join("different-scene.jpg"),
        capture::fixture_image(
            &media::fixture_other_png(128, 96),
            "2026:10:31 12:00:00",
            "200",
            "One",
        ),
    )
    .unwrap();
    let job = Arc::new(Job::new(crate::engine::TaskKind::PersonalScan));
    let scan = super::super::scan::collect(&root, &[], job.clone()).unwrap();
    let scan = objc2::rc::autoreleasepool(|_| collect_with_sequence(scan, job.clone(), &[], true))
        .unwrap();
    let report = scan.report.similar_photos.as_ref().unwrap();
    assert_eq!(report.checked_files, 8);
    assert_eq!(report.sequence.as_ref().unwrap().missing_metadata, 1);
    assert_eq!(
        report.groups.len(),
        1,
        "{}",
        serde_json::to_string(&scan.report).unwrap()
    );
    assert_eq!(report.groups[0].capture_span_ms, Some(300));
    assert_eq!(
        scan.items
            .iter()
            .map(|i| Path::new(&i.path).file_name().unwrap().to_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["first.jpg", "second.jpg"]
    );
    assert!(report.images.iter().all(|p| p.captured_at.is_some()));
    assert_ne!(report.images[0].digest, report.images[1].digest);
    let ids = scan.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    assert!(store.offer(&ids, db::now()).is_err());
    let offer = store.offer(&ids[..1], db::now()).unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    plan.validate_record().unwrap();
    verify(&plan, &plan.items[0], &job, &[]).unwrap();
    std::fs::write(
        &plan.similar_photos.as_ref().unwrap()[0].keeper.path,
        b"changed metadata and content",
    )
    .unwrap();
    assert!(verify(&plan, &plan.items[0], &job, &[]).is_err());
}
#[test]
fn native_visual_groups_are_not_byte_duplicates_and_keep_one_unchanged_original() {
    let temp = tempfile::Builder::new()
        .prefix("MooMole-similar-owned-")
        .tempdir()
        .unwrap();
    let root = temp.path().canonicalize().unwrap();
    let first = media::fixture_png(128, 96);
    let second = media::fixture_png(256, 192);
    assert_ne!(first, second);
    std::fs::write(root.join("first.png"), &first).unwrap();
    std::fs::write(root.join("resized.png"), &second).unwrap();
    std::fs::write(
        root.join("unrelated.png"),
        media::fixture_other_png(128, 96),
    )
    .unwrap();
    let job = Arc::new(Job::new(crate::engine::TaskKind::PersonalScan));
    let scan = super::super::scan::collect(&root, &[], job.clone()).unwrap();
    let scan = objc2::rc::autoreleasepool(|_| collect(scan, job.clone(), &[])).unwrap();
    let report = scan.report.similar_photos.as_ref().unwrap();
    assert_eq!(report.checked_files, 3);
    assert_eq!(scan.items.len(), 2);
    assert_eq!(
        report.groups.len(),
        1,
        "{}",
        serde_json::to_string(&scan.report).unwrap()
    );
    assert_ne!(report.images[0].digest, report.images[1].digest);
    let all = scan.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
    let one = all[0].clone();
    let now = crate::db::now();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    assert!(store.offer(&all, now).is_err());
    let confirmation = store.offer(&[one.clone()], now).unwrap();
    let plan = store.take(&confirmation.id, now).unwrap();
    assert_eq!(plan.origin(), MODULE);
    assert!(plan.duplicates.is_none());
    verify(&plan, &plan.items[0], &job, &[]).unwrap();
    let keeper = &plan.similar_photos.as_ref().unwrap()[0].keeper;
    std::fs::write(&keeper.path, "changed keeper").unwrap();
    assert!(verify(&plan, &plan.items[0], &job, &[]).is_err());
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/similar-photos-native-groups.json",serde_json::to_vec_pretty(&serde_json::json!({"confirmation":confirmation,"byteDifferent":true,"preserveOneEnforced":true,"changedKeeperRefused":true})).unwrap()).unwrap();
}
