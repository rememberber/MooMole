use super::*;

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
