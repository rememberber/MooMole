use super::*;
use crate::engine::TaskKind;
fn draft(root: &Path) -> Draft {
    let now = db::now();
    Draft {
        id: uuid::Uuid::new_v4().to_string(),
        run_id: uuid::Uuid::new_v4().to_string(),
        item_id: uuid::Uuid::new_v4().to_string(),
        copy_path: root.join("copy.txt").to_string_lossy().into_owned(),
        original_path: root.join("original.txt").to_string_lossy().into_owned(),
        bytes: 4,
        created_at: now,
        expires_at: now + 120_000,
    }
}
fn row(d: Draft) -> Recovery {
    Recovery {
        id: d.id.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        created_at: db::now(),
        finished_at: None,
        state: "running".into(),
        phase: "checking".into(),
        temporary_path: Path::new(&d.original_path)
            .parent()
            .unwrap()
            .join(format!(".moomole-restore-{}.tmp", d.id))
            .to_string_lossy()
            .into_owned(),
        draft: d,
        receipt: None,
        reason: "owned fixture".into(),
        reviewed_at: None,
    }
}
#[test]
fn recovery_journal_reopens_unknown_without_replay_and_keeps_manual_review_separate() {
    let t = tempfile::Builder::new()
        .prefix("MooMole-restore-record-")
        .tempdir()
        .unwrap();
    let root = t.path().canonicalize().unwrap();
    let file = root.join("state.sqlite");
    let mut r = row(draft(&root));
    {
        let mut db = db::open_owned(&file).unwrap();
        journal::begin(&mut db, &r).unwrap();
        r.phase = "intent".into();
        journal::save(&db, &r).unwrap();
        let snapshot = commands::snapshot(
            &db,
            &[r.request_id.clone(), uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();
        assert_eq!(
            snapshot["activeRequests"],
            serde_json::json!([r.request_id])
        );
        assert_eq!(snapshot["records"][0]["state"], "running");
        assert_eq!(snapshot["records"][0]["draft"]["runId"], r.draft.run_id);
    }
    let db = db::open_owned(&file).unwrap();
    let snapshot = commands::snapshot(&db, &[]).unwrap();
    assert_eq!(snapshot["activeRequests"], serde_json::json!([]));
    let rows: Vec<Recovery> = serde_json::from_value(snapshot["records"].clone()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, "unknown");
    assert!(journal::admission(&db).is_err());
    assert!(!Path::new(&r.draft.original_path).exists());
    let mut reviewed = rows[0].clone();
    reviewed.reviewed_at = Some(db::now());
    journal::save(&db, &reviewed).unwrap();
    assert!(journal::admission(&db).is_ok());
    assert_eq!(journal::list(&db).unwrap()[0].state, "unknown");
    reviewed.temporary_path = root.join("different.tmp").to_string_lossy().into_owned();
    assert!(journal::save(&db, &reviewed).is_err());
}
#[test]
fn incomplete_receipts_and_unknown_source_records_never_grant_restore_authority() {
    let t = tempfile::Builder::new()
        .prefix("MooMole-restore-binding-")
        .tempdir()
        .unwrap();
    let root = t.path().canonicalize().unwrap();
    let file = root.join("state.sqlite");
    let db = db::open_owned(&file).unwrap();
    assert!(source(&db, &uuid::Uuid::new_v4().to_string(), "foreign").is_err());
    let mut entry = Entry {
        id: uuid::Uuid::new_v4().to_string(),
        path: root.join("old").to_string_lossy().into_owned(),
        target: None,
        temporary_path: None,
        state: "moved".into(),
        reason: String::new(),
        copy_receipt: None,
        source_trash_receipt: None,
        sha256: None,
        metadata_sha256: None,
        copy_witness: None,
    };
    assert!(native::recorded(&entry).is_err());
    let path = root.join("copy.txt");
    std::fs::write(&path, b"copy").unwrap();
    let witness = crate::observation::witness(&path).unwrap();
    let node = witness.nodes.last().unwrap();
    entry.target = Some(path.to_string_lossy().into_owned());
    entry.copy_receipt = Some(Receipt {
        destination: path.to_string_lossy().into_owned(),
        device: node.dev,
        inode: node.ino,
    });
    entry.copy_witness = Some(witness);
    entry.sha256 = Some("0".repeat(64));
    entry.metadata_sha256 = Some("0".repeat(64));
    let copy = native::recorded(&entry).unwrap();
    assert!(native::verify_copy(&copy, &Job::new(TaskKind::FileRestore), Instant::now()).is_err());
    entry.copy_receipt.as_mut().unwrap().inode += 1;
    assert!(native::recorded(&entry).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"copy");
}
#[tokio::test]
#[ignore = "requires MOOMOLE_TEST_RESTORE_VOLUME pointing to a newly created owned APFS test image; actually copies, Trashes and restores only disposable samples across two devices"]
async fn two_volume_restore_preserves_copy_original_trash_and_reconciles_lost_completion() {
    let mount = PathBuf::from(
        std::env::var_os("MOOMOLE_TEST_RESTORE_VOLUME")
            .expect("supply a dedicated owned mounted test image"),
    );
    let original = tempfile::Builder::new()
        .prefix("MooMole-restore-original-")
        .tempdir_in(&mount)
        .unwrap();
    let root = original.path().canonicalize().unwrap();
    let temp = tempfile::Builder::new()
        .prefix("MooMole-restore-copy-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let target = home.join("Copies");
    std::fs::create_dir(&target).unwrap();
    assert_ne!(
        std::fs::metadata(&root).unwrap().dev(),
        std::fs::metadata(&target).unwrap().dev()
    );
    for name in ["one.txt", "two.txt", "unselected.txt"] {
        std::fs::write(root.join(name), b"owned restore bytes\n").unwrap();
    }
    use std::os::fd::AsRawFd;
    for name in ["one.txt", "two.txt"] {
        let held = AnchoredPath::open(&root.join(name)).unwrap();
        for (key, value) in [
            (c"com.apple.ResourceFork", b"owned resource fork".as_slice()),
            (
                c"com.apple.quarantine",
                b"0081;68000000;MooMoleRestoreOwned;00000000-0000-4000-8000-000000000001"
                    .as_slice(),
            ),
            (c"user.moomole.restore", b"owned attribute".as_slice()),
        ] {
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
    }
    let settings = db::Settings {
        acknowledged: true,
        ..Default::default()
    };
    let scan = crate::personal_files::scan::collect(
        &root,
        &[],
        Arc::new(Job::new(TaskKind::PersonalScan)),
    )
    .unwrap();
    let ids = scan
        .items
        .iter()
        .filter(|i| !i.path.ends_with("unselected.txt"))
        .map(|i| i.id.clone())
        .collect::<Vec<_>>();
    let mut files = crate::personal_files::Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = files.offer(&ids, db::now()).unwrap();
    let source_plan = files.take(&offer.id, db::now()).unwrap();
    let dbpath = home.join("state.sqlite");
    let db = db::open_owned(&dbpath).unwrap();
    db::save_settings(&db, &settings).unwrap();
    let database = Arc::new(Mutex::new(db));
    let context = || Context {
        database: database.clone(),
        home: home.clone(),
        engine_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
    };
    let transfer = super::super::prepare(
        source_plan,
        target.to_str().unwrap(),
        movement::Conflict::Skip,
        &settings,
    )
    .unwrap();
    let moved = super::super::run(
        context(),
        transfer,
        uuid::Uuid::new_v4().to_string(),
        Arc::new(Job::new(TaskKind::PersonalTransfer)),
    )
    .await
    .unwrap();
    assert_eq!(
        moved.state,
        "completed",
        "{}",
        serde_json::to_string_pretty(&moved).unwrap()
    );
    assert_eq!(moved.items.len(), 2);
    let control = Job::new(TaskKind::FileRestore);
    let first = moved
        .items
        .iter()
        .find(|i| i.path.ends_with("one.txt"))
        .unwrap();
    let second = moved
        .items
        .iter()
        .find(|i| i.path.ends_with("two.txt"))
        .unwrap();
    let prepare = |entry: &Entry| {
        let (selected, settings) = {
            let db = database.lock().unwrap();
            (
                source(&db, &moved.id, &entry.id).unwrap(),
                db::settings(&db).unwrap(),
            )
        };
        Plan::prepare(selected, &moved.id, &entry.id, &home, &settings, &control)
    };
    let plan = prepare(first).unwrap();
    assert!(now_valid(&plan, Instant::now() - Duration::from_secs(121)).is_err());
    let mut protected = settings.clone();
    protected.protected_paths.push(first.path.clone());
    assert!(plan.gate(&protected, &home, &control).is_err());
    std::fs::write(&first.path, b"occupied original").unwrap();
    assert!(prepare(first).is_err());
    assert_eq!(std::fs::read(&first.path).unwrap(), b"occupied original");
    std::fs::remove_file(&first.path).unwrap();
    let cancelled = Job::new(TaskKind::FileRestore);
    cancelled.cancel();
    assert!(plan.gate(&settings, &home, &cancelled).is_err());
    assert!(!Path::new(&first.path).exists());
    let mut refused = row(plan.draft.clone());
    assert!(perform(
        &plan,
        &control,
        || Ok(settings.clone()),
        || Ok(()),
        &mut refused,
        |r| if r.phase == "intent" {
            Err("owned injected intent save failure".into())
        } else {
            Ok(())
        }
    )
    .is_err());
    assert!(!Path::new(&first.path).exists());
    let restored =
        commands::run(context(), plan, uuid::Uuid::new_v4().to_string(), &control).unwrap();
    assert_eq!(
        restored.state,
        "restored",
        "{}",
        serde_json::to_string_pretty(&restored).unwrap()
    );
    assert_eq!(
        std::fs::read(&first.path).unwrap(),
        b"owned restore bytes\n"
    );
    native::verify_copy(&native::recorded(first).unwrap(), &control, Instant::now()).unwrap();
    let plan = prepare(second).unwrap();
    database.lock().unwrap().execute_batch("CREATE TRIGGER owned_restore_completion_loss BEFORE UPDATE ON transfer_restorations WHEN json_extract(NEW.body,'$.state')='restored' BEGIN SELECT RAISE(ABORT,'owned completion fault');END;").unwrap();
    assert!(commands::run(context(), plan, uuid::Uuid::new_v4().to_string(), &control).is_err());
    assert_eq!(
        std::fs::read(&second.path).unwrap(),
        b"owned restore bytes\n"
    );
    assert_eq!(
        std::fs::read(root.join("unselected.txt")).unwrap(),
        b"owned restore bytes\n"
    );
    for e in &moved.items {
        let trash = e.source_trash_receipt.as_ref().unwrap();
        assert_eq!(
            std::fs::read(&trash.destination).unwrap(),
            b"owned restore bytes\n"
        );
        native::verify_copy(&native::recorded(e).unwrap(), &control, Instant::now()).unwrap();
    }
    let before = serde_json::to_value(&moved).unwrap();
    assert_eq!(
        serde_json::to_value(
            super::super::journal::list(&database.lock().unwrap())
                .unwrap()
                .remove(0)
        )
        .unwrap(),
        before
    );
    drop(database);
    let reopened = db::open_owned(&dbpath).unwrap();
    let records = journal::reconcile(&reopened, &[]).unwrap();
    assert_eq!(records.len(), 2);
    assert!(records
        .iter()
        .any(|r| r.state == "restored" && r.receipt.is_some()));
    assert!(records
        .iter()
        .any(|r| r.state == "unknown" && r.receipt.is_none()));
    assert!(journal::admission(&reopened).is_err());
    assert_eq!(
        serde_json::to_value(super::super::journal::list(&reopened).unwrap().remove(0)).unwrap(),
        before
    );
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/transfer-restoration-native.json",serde_json::to_vec_pretty(&serde_json::json!({"twoRealVolumes":true,"resourceForkAndQuarantineVerified":true,"copiesAndOriginalTrashPreserved":true,"unselectedPreserved":true,"originalTransferRecordUnchanged":true,"reopenedWithoutReplay":true,"records":records,"scope":"production native file-copy recovery, not App window acceptance"})).unwrap()).unwrap();
}
