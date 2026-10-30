use super::*;
use crate::engine::TaskKind;
struct Fixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    database: Arc<std::sync::Mutex<db::Database>>,
    detail: execution::Detail,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::Builder::new()
            .prefix("MooMole-batch-restore-owned-")
            .tempdir()
            .unwrap();
        let home = temp.path().canonicalize().unwrap();
        let root = home.join("Downloads");
        let nested = root.join("Sub");
        let organized = home.join("Organized");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir(&organized).unwrap();
        let now = db::now();
        let settings = db::Settings {
            acknowledged: true,
            ..Default::default()
        };
        let items = [root.join("first.txt"), nested.join("second.txt")]
            .into_iter()
            .enumerate()
            .map(|(n, path)| {
                std::fs::write(&path, format!("owned batch content {n}")).unwrap();
                let witness = crate::observation::witness(&path).unwrap();
                personal_files::Item {
                    id: uuid::Uuid::new_v4().to_string(),
                    path: path.to_str().unwrap().into(),
                    size: witness.size,
                    modified_at: now,
                    witness,
                }
            })
            .collect::<Vec<_>>();
        let movement = movement::Movement {
            directory: organized.to_str().unwrap().into(),
            witness: movement::directory(organized.to_str().unwrap(), &settings)
                .unwrap()
                .witness,
            conflict: movement::Conflict::Skip,
            targets: items
                .iter()
                .map(|i| movement::Target {
                    id: i.id.clone(),
                    destination: Some(
                        organized
                            .join(Path::new(&i.path).file_name().unwrap())
                            .to_str()
                            .unwrap()
                            .into(),
                    ),
                })
                .collect(),
        };
        let plan = personal_files::Plan {
            module: personal_files::MODULE.into(),
            protocol: 1,
            revision: "MooMole.personal-files.move.v1".into(),
            id: uuid::Uuid::new_v4().to_string(),
            scan_id: uuid::Uuid::new_v4().to_string(),
            root: root.to_str().unwrap().into(),
            created_at: now,
            expires_at: now + 30_000,
            items,
            movement: Some(movement.clone()),
            duplicates: None,
            similar_photos: None,
            software_content: None,
            automation: None,
        };
        let mut db = db::open_owned(&home.join("app.sqlite")).unwrap();
        db::save_settings(&db, &settings).unwrap();
        let id = execution::begin_personal(&mut db, &plan, now).unwrap();
        for item in &plan.items {
            execution::intent(&mut db, &id, &item.id, db::now()).unwrap();
            let target = movement
                .targets
                .iter()
                .find(|t| t.id == item.id)
                .unwrap()
                .destination
                .as_ref()
                .unwrap();
            let (outcome, reason) = movement::Prepared::new(item, &movement, target, &settings)
                .unwrap()
                .move_checked(|| Ok(()))
                .unwrap();
            execution::complete_item(&mut db, &id, &item.id, outcome, &reason, db::now()).unwrap();
        }
        execution::finish(&mut db, &id, false, db::now()).unwrap();
        let detail = execution::detail(&db, &id).unwrap();
        assert_eq!(detail.summary.moved_count, 2);
        Self {
            _temp: temp,
            home,
            database: Arc::new(std::sync::Mutex::new(db)),
            detail,
        }
    }
    fn plan(&self) -> Plan {
        prepare(
            &self.detail,
            &self
                .detail
                .items
                .iter()
                .map(|i| i.id.clone())
                .collect::<Vec<_>>(),
            &self.home,
            &db::settings(&self.database.lock().unwrap()).unwrap(),
        )
        .unwrap()
    }
}
#[test]
fn actual_batch_restores_distinct_original_directories_with_one_durable_record_and_archive() {
    let f = Fixture::new();
    let original = serde_json::to_value(&f.detail).unwrap();
    let plan = f.plan();
    let result = run(
        f.database.clone(),
        f.home.clone(),
        plan.clone(),
        Arc::new(Job::new(TaskKind::FileRestore)),
        |_| {},
    )
    .unwrap();
    assert_eq!(result.summary.moved_count, 2);
    assert_eq!(result.origin, super::super::MODULE);
    assert_eq!(result.summary.plan_id, plan.id);
    for (n, p) in plan.plans.iter().enumerate() {
        assert_eq!(
            std::fs::read_to_string(&p.source.original_path).unwrap(),
            format!("owned batch content {n}")
        );
        assert!(!Path::new(&p.item.path).exists());
    }
    let mut db = f.database.lock().unwrap();
    assert_eq!(
        serde_json::to_value(execution::detail(&db, &f.detail.summary.id).unwrap()).unwrap(),
        original
    );
    assert!(execution::begin_batch_restoration(&mut db, &plan, db::now()).is_err());
    let expected = serde_json::to_value(&result).unwrap();
    drop(db);
    drop(
        Arc::try_unwrap(f.database)
            .ok()
            .unwrap()
            .into_inner()
            .unwrap(),
    );
    let mut reopened = db::open_owned(&f.home.join("app.sqlite")).unwrap();
    assert_eq!(
        serde_json::to_value(execution::detail(&reopened, &result.summary.id).unwrap()).unwrap(),
        expected
    );
    let preview = crate::execution_archive::prepare(&mut reopened).unwrap();
    let root = f.home.join("archives");
    let header =
        crate::execution_archive::archive(&mut reopened, &root, &preview.fingerprint, db::now())
            .unwrap();
    assert_eq!(
        crate::execution_archive::detail(
            &root,
            &header.id,
            crate::execution_archive::Kind::Cleanup,
            &result.summary.id
        )
        .unwrap(),
        expected
    );
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/batch-restoration-native.json",serde_json::to_vec_pretty(&serde_json::json!({"result":expected,"originalRecordUnchanged":true,"reopenedAndArchived":true,"ownedOrdinaryDirectoriesOnly":true})).unwrap()).unwrap();
}
#[test]
fn conflicts_are_preserved_while_other_files_restore_and_cancellation_keeps_completed_files() {
    for cancel in [false, true] {
        let f = Fixture::new();
        let plan = f.plan();
        let job = Arc::new(Job::new(TaskKind::FileRestore));
        if !cancel {
            std::fs::write(&plan.plans[0].source.original_path, "keep conflict").unwrap();
        }
        let result = run(
            f.database.clone(),
            f.home.clone(),
            plan.clone(),
            job.clone(),
            |n| {
                if cancel && n == 0 {
                    job.cancel();
                }
            },
        )
        .unwrap();
        assert_eq!(result.summary.moved_count, 1);
        if cancel {
            assert_eq!(result.summary.state, "cancelled");
            assert!(Path::new(&plan.plans[1].item.path).exists());
            assert!(!Path::new(&plan.plans[0].item.path).exists());
        } else {
            assert_eq!(result.summary.state, "partial");
            assert_eq!(
                std::fs::read_to_string(&plan.plans[0].source.original_path).unwrap(),
                "keep conflict"
            );
            assert!(Path::new(&plan.plans[0].item.path).exists());
            assert!(!Path::new(&plan.plans[1].item.path).exists());
        }
    }
}
#[test]
fn duplicate_unknown_archived_or_changed_sources_and_expired_confirmations_do_not_authorize_restore(
) {
    let f = Fixture::new();
    let settings = db::settings(&f.database.lock().unwrap()).unwrap();
    let ids = f
        .detail
        .items
        .iter()
        .map(|i| i.id.clone())
        .collect::<Vec<_>>();
    for selected in [
        vec![],
        vec![ids[0].clone(), ids[0].clone()],
        vec!["not-a-source".into()],
        vec![ids[0].clone(); 21],
    ] {
        assert!(prepare(&f.detail, &selected, &f.home, &settings).is_err());
    }
    let mut detail = execution::detail(&f.database.lock().unwrap(), &f.detail.summary.id).unwrap();
    detail.items[0].state = "uncertain".into();
    assert!(prepare(&detail, &ids, &f.home, &settings).is_err());
    let plan = f.plan();
    let decoded: Plan = serde_json::from_value(serde_json::to_value(&plan).unwrap()).unwrap();
    assert!(decoded.fresh().is_err());
    let mut expired = plan.clone();
    expired.issued = Some(Instant::now() - Duration::from_secs(31));
    assert!(expired.fresh().is_err());
    let mut bad = plan.clone();
    bad.plans.push(bad.plans[0].clone());
    assert!(bad.validate_record().is_err());
    let mut store = Store::default();
    store.begin().unwrap();
    store.batch = Some(plan.clone());
    store.discard(&plan.id);
    assert!(store.batch.is_none());
    store.batch = Some(plan);
    store.begin().unwrap();
    assert!(store.batch.is_none());
}

#[test]
fn receipt_storage_failure_retains_real_moves_as_unknown_and_never_replays_after_reopen() {
    let f = Fixture::new();
    let plan = f.plan();
    let result = run(
        f.database.clone(),
        f.home.clone(),
        plan.clone(),
        Arc::new(Job::new(TaskKind::FileRestore)),
        |n| {
            if n == 0 {
                f.database.lock().unwrap().execute_batch("CREATE TRIGGER owned_receipt_failure BEFORE UPDATE OF state ON execution_items WHEN NEW.state='moved' BEGIN SELECT RAISE(ABORT,'owned receipt failure'); END;").unwrap();
            }
        },
    );
    assert!(result.is_err());
    for p in &plan.plans {
        assert!(Path::new(&p.source.original_path).is_file());
        assert!(!Path::new(&p.item.path).exists());
    }
    let db = f.database.lock().unwrap();
    let record = execution::list(&db)
        .unwrap()
        .into_iter()
        .find(|r| r.plan_id == plan.id)
        .unwrap();
    let detail = execution::detail(&db, &record.id).unwrap();
    assert_eq!(detail.summary.moved_count, 1);
    assert_eq!(detail.summary.uncertain_count, 1);
    assert_eq!(detail.items[1].state, "uncertain");
    drop(db);
    drop(
        Arc::try_unwrap(f.database)
            .ok()
            .unwrap()
            .into_inner()
            .unwrap(),
    );
    let mut reopened = db::open_owned(&f.home.join("app.sqlite")).unwrap();
    let detail = execution::detail(&reopened, &record.id).unwrap();
    assert_eq!(detail.summary.uncertain_count, 1);
    assert!(execution::begin_batch_restoration(&mut reopened, &plan, db::now()).is_err());
}
