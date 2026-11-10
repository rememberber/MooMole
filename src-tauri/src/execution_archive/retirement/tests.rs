use super::*;
use crate::engine::TaskKind;
fn job() -> Arc<Job> {
    Arc::new(Job::new(TaskKind::AppControl))
}
fn fixture() -> (
    tempfile::TempDir,
    Arc<Mutex<Box<Connection>>>,
    PathBuf,
    Header,
    PathBuf,
) {
    let (t, db, root, h) = super::super::tests::retirement_fixture();
    let copy = t.path().join("exported.moomole-history");
    (t, Arc::new(Mutex::new(Box::new(db))), root, h, copy)
}
#[test]
fn export_is_complete_independent_and_original_protection_change_or_expiry_prevents_retirement() {
    let (_t, _db, root, h, copy) = fixture();
    let source = filename(&root, &h.id).unwrap();
    let original = fs::read(&source).unwrap();
    let mut plan = Plan::prepare(&root, &h.id, &copy, &[], &job()).unwrap();
    assert_eq!(fs::read(&copy).unwrap(), original);
    assert_eq!(fs::read(&source).unwrap(), original);
    plan.validate(&root, &[], &job()).unwrap();
    assert!(plan
        .validate(&root, &[copy.to_str().unwrap().into()], &job())
        .is_err());
    fs::write(&copy, b"changed").unwrap();
    assert!(plan.validate(&root, &[], &job()).is_err());
    fs::write(&copy, &original).unwrap();
    plan.issued = Instant::now() - Duration::from_secs(121);
    assert!(plan.validate(&root, &[], &job()).is_err());
    assert!(Plan::prepare(
        &root,
        &h.id,
        &root.join("second.moomole-history"),
        &[],
        &job()
    )
    .is_err());
    assert!(Plan::prepare(&root, &h.id, &copy, &[], &job()).is_err()); // Never overwrites an export.
    assert_eq!(fs::read(&source).unwrap(), original);
}
#[test]
fn cancellation_and_failed_intent_keep_both_files_and_durable_results() {
    for cancel in [false, true] {
        let (_t, database, root, h, copy) = fixture();
        let control = job();
        let plan = Plan::prepare(&root, &h.id, &copy, &[], &control).unwrap();
        let source = plan.draft.path.clone();
        if cancel {
            control.cancel()
        } else {
            database
                .lock()
                .unwrap()
                .execute_batch(journal::TABLE)
                .unwrap();
            database.lock().unwrap().execute_batch("CREATE TRIGGER refuse_retirement_intent BEFORE UPDATE ON archive_retirements WHEN json_extract(NEW.body,'$.phase')='intent' BEGIN SELECT RAISE(FAIL,'owned intent failure');END;").unwrap();
        }
        let row = execute(
            plan,
            &root,
            database.clone(),
            control,
            uuid::Uuid::new_v4().to_string(),
            || Ok(vec![]),
        )
        .unwrap();
        assert_eq!(row.state, if cancel { "cancelled" } else { "retained" });
        assert!(row.receipt.is_none());
        assert_eq!(fs::read(&source).unwrap(), fs::read(&copy).unwrap());
        assert_eq!(
            journal::list(&database.lock().unwrap()).unwrap()[0].state,
            row.state
        );
    }
}
#[test]
fn durable_interruption_reopens_unknown_and_never_replays_or_discards_source_records() {
    let (t, database, root, h, copy) = fixture();
    let plan = Plan::prepare(&root, &h.id, &copy, &[], &job()).unwrap();
    let now = db::now();
    let mut row = Record {
        id: plan.draft.id.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        draft: plan.draft.clone(),
        state: "running".into(),
        phase: "checking".into(),
        reason: "owned fixture".into(),
        created_at: now,
        finished_at: None,
        receipt: None,
        reviewed_at: None,
    };
    journal::begin(&mut database.lock().unwrap(), &row).unwrap();
    row.phase = "intent".into();
    journal::save(&database.lock().unwrap(), &row).unwrap();
    assert_eq!(
        journal::reconcile(&database.lock().unwrap(), &[row.request_id.clone()]).unwrap()[0].state,
        "running"
    );
    drop(database);
    let reopened = db::open_fixture(&t.path().join("live.sqlite")).unwrap();
    let mut unknown = journal::list(&reopened).unwrap().remove(0);
    assert_eq!(unknown.state, "unknown");
    assert!(journal::admission(&reopened).is_err());
    assert!(filename(&root, &h.id).unwrap().is_file());
    assert!(copy.is_file());
    unknown.reviewed_at = Some(db::now().max(unknown.created_at));
    journal::save(&reopened, &unknown).unwrap();
    journal::admission(&reopened).unwrap();
    assert!(journal::begin(&mut { reopened }, &row).is_err());
}
#[test]
#[ignore = "explicit real native Trash of owned managed archive after verified export, then exact receipt return; includes lost completion and restart"]
fn real_archive_retirement_preserves_export_and_unknown_without_touching_original_files() {
    let mut reports = vec![];
    for lost in [false, true] {
        let (mut t, database, root, h, copy) = fixture();
        fs::write(t.path().join("untouched.txt"), "owned original keep").unwrap();
        let control = job();
        let plan = Plan::prepare(&root, &h.id, &copy, &[], &control).unwrap();
        let source = PathBuf::from(&plan.draft.path);
        let original = fs::read(&source).unwrap();
        if lost {
            database
                .lock()
                .unwrap()
                .execute_batch(journal::TABLE)
                .unwrap();
            database.lock().unwrap().execute_batch("CREATE TRIGGER lose_retirement_completion BEFORE UPDATE ON archive_retirements WHEN json_extract(NEW.body,'$.phase')='finished' BEGIN SELECT RAISE(FAIL,'owned completion failure');END;").unwrap();
        }
        t.disable_cleanup(true);
        eprintln!("Owned archive rescue root: {}", t.path().display());
        let mut actual = None;
        let result = execute_with(
            plan,
            &root,
            database.clone(),
            control,
            uuid::Uuid::new_v4().to_string(),
            || Ok(vec![]),
            |r| {
                actual = Some(r.clone());
                let rescue = serde_json::json!({"ownedRoot":t.path(),"record":r});
                eprintln!("Owned native archive receipt: {}", rescue);
                fs::create_dir_all("../.cache").unwrap();
                fs::write(
                    "../.cache/archive-retirement-native-rescue.json",
                    serde_json::to_vec(&rescue).unwrap(),
                )
                .unwrap();
            },
        );
        let native =
            actual.expect("native attempt must return a receipt for this explicit acceptance");
        assert_eq!(native.state, "moved");
        let receipt = native.receipt.as_ref().unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(&copy).unwrap(), original);
        let meta = fs::symlink_metadata(&receipt.destination).unwrap();
        assert!(meta.is_file());
        assert_eq!((meta.dev(), meta.ino()), (receipt.device, receipt.inode));
        assert_eq!(fs::read(&receipt.destination).unwrap(), original);
        if lost {
            assert!(result.is_err());
            database
                .lock()
                .unwrap()
                .execute_batch("DROP TRIGGER lose_retirement_completion;")
                .unwrap();
        } else {
            assert_eq!(result.unwrap().state, "moved");
        }
        drop(database);
        let reopened = db::open_fixture(&t.path().join("live.sqlite")).unwrap();
        let row = journal::list(&reopened).unwrap().remove(0);
        assert_eq!(row.state, if lost { "unknown" } else { "moved" });
        if lost {
            assert!(journal::admission(&reopened).is_err());
        }
        assert_eq!(super::super::load(&copy, false).unwrap().header, h);
        assert_eq!(
            fs::read_to_string(t.path().join("untouched.txt")).unwrap(),
            "owned original keep"
        );
        fs::rename(&receipt.destination, &source).unwrap();
        assert_eq!(fs::read(&source).unwrap(), original);
        assert_eq!(fs::read(&copy).unwrap(), original);
        reports.push(serde_json::json!({"record":row,"exportPreserved":true,"originalFilePreserved":true,"ownedNativeReceiptReturned":true,"lostCompletion":lost}));
        t.disable_cleanup(false);
    }
    fs::write(
        "../.cache/archive-retirement-native.json",
        serde_json::to_vec(&serde_json::json!({"reports":reports,"appWindowVerified":false}))
            .unwrap(),
    )
    .unwrap();
    fs::remove_file("../.cache/archive-retirement-native-rescue.json").unwrap();
}
