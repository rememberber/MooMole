use super::*;

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
