use super::*;
struct Fixture {
    _t: tempfile::TempDir,
    home: PathBuf,
    rule: Rule,
    db: Arc<Mutex<db::Database>>,
}
impl Fixture {
    fn new(action: Action) -> Self {
        let t = tempfile::Builder::new()
            .prefix("MooMole-scheduled-owned-")
            .tempdir()
            .unwrap();
        let home = t.path().canonicalize().unwrap();
        let root = home.join("Downloads");
        let destination = home.join("Organized");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&destination).unwrap();
        let old = std::time::SystemTime::now() - Duration::from_secs(40 * 86_400);
        for name in ["old-a.txt", "old-b.txt", "recent.txt", "old-other.bin"] {
            let p = root.join(name);
            std::fs::write(&p, format!("owned scheduled {name}")).unwrap();
            if name != "recent.txt" {
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(&p)
                    .unwrap()
                    .set_times(std::fs::FileTimes::new().set_modified(old))
                    .unwrap();
            }
        }
        let rule = Rule {
            root: root.to_str().unwrap().into(),
            action,
            destination: if action == Action::Move {
                Some(destination.to_str().unwrap().into())
            } else {
                None
            },
            extensions: vec!["txt".into()],
            minimum_age_days: 30,
            maximum_files: 1,
            maximum_bytes: 1_048_576,
            interval_minutes: 60,
        };
        let db = db::open_owned(&home.join("app.sqlite")).unwrap();
        db::save_settings(
            &db,
            &db::Settings {
                acknowledged: true,
                ..Default::default()
            },
        )
        .unwrap();
        Self {
            _t: t,
            home,
            rule,
            db: Arc::new(Mutex::new(db)),
        }
    }
    fn offer(&self) -> Offered {
        preview(
            Edit {
                id: None,
                rule: self.rule.clone(),
            },
            &self.home,
            &db::settings(&self.db.lock().unwrap()).unwrap(),
        )
        .unwrap()
    }
    fn save(&self) -> Saved {
        let offered = self.offer();
        assert_eq!(offered.draft.matched_files, 2);
        assert_eq!(offered.draft.selected.len(), 1);
        assert_eq!(offered.draft.held_by_limits, 1);
        journal::save(&mut self.db.lock().unwrap(), offered, &self.home).unwrap();
        journal::policies(&self.db.lock().unwrap())
            .unwrap()
            .remove(0)
    }
}
#[test]
fn rules_are_off_by_default_preview_is_read_only_and_due_never_catches_up_at_restart() {
    let f = Fixture::new(Action::Move);
    assert!(journal::policies(&f.db.lock().unwrap()).unwrap().is_empty());
    let offered = f.offer();
    assert!(Path::new(&offered.draft.selected[0].path).is_file());
    let saved = f.save();
    let now = db::now();
    assert!(!due(&saved, now, now));
    assert!(!due(&saved, now + 60_000, now));
    assert!(due(&saved, now + 3_600_001, now));
    let mut stale = f.offer();
    stale.issued = Instant::now() - Duration::from_secs(121);
    assert!(journal::save(&mut f.db.lock().unwrap(), stale, &f.home).is_err());
    let request = uuid::Uuid::new_v4().to_string();
    journal::begin(&mut f.db.lock().unwrap(), &saved, &request, "scheduled").unwrap();
    journal::reconcile(&mut f.db.lock().unwrap(), &[]).unwrap();
    assert!(
        !journal::policies(&f.db.lock().unwrap()).unwrap()[0]
            .policy
            .enabled
    );
    assert_eq!(
        journal::records(&f.db.lock().unwrap()).unwrap()[0].state,
        "interrupted"
    );
    assert!(Path::new(&f.rule.root).join("old-a.txt").exists());
}
#[test]
fn changed_paused_removed_or_serialized_authority_cannot_reach_file_mutation() {
    let f = Fixture::new(Action::Move);
    let saved = f.save();
    let scan = scan::collect(
        Path::new(&f.rule.root),
        &[],
        Arc::new(Job::new(TaskKind::PersonalScan)),
    )
    .unwrap();
    let (_, ids, _) = select(&scan, &f.rule, db::now());
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let settings = db::settings(&f.db.lock().unwrap()).unwrap();
    let offered = store
        .offer_move(
            &ids,
            f.rule.destination.as_ref().unwrap(),
            movement::Conflict::Skip,
            &settings,
            db::now(),
        )
        .unwrap();
    let mut plan = store.take(&offered.id, db::now()).unwrap();
    plan.module = MODULE.into();
    plan.revision = REVISION.into();
    plan.automation = Some(Binding {
        request_id: uuid::Uuid::new_v4().to_string(),
        saved: saved.clone(),
        started_at: db::now(),
        issued: Some(Instant::now()),
    });
    plan.validate_record().unwrap();
    current(&f.db.lock().unwrap(), &plan, &f.home).unwrap();
    let archived: super::super::Plan =
        serde_json::from_value(serde_json::to_value(&plan).unwrap()).unwrap();
    assert!(current(&f.db.lock().unwrap(), &archived, &f.home).is_err());
    journal::pause(&f.db.lock().unwrap(), &saved.policy.id).unwrap();
    assert!(current(&f.db.lock().unwrap(), &plan, &f.home).is_err());
    journal::remove(&f.db.lock().unwrap(), &saved.policy.id).unwrap();
    assert!(current(&f.db.lock().unwrap(), &plan, &f.home).is_err());
    for bad in ["*", "TXT", "a/b"] {
        let mut r = f.rule.clone();
        r.extensions = vec![bad.into()];
        assert!(r.validate().is_err());
    }
    let mut r = f.rule.clone();
    r.root = f.home.to_str().unwrap().into();
    assert!(bind(&r, &f.home, &settings).is_err());
    let alias = f.home.with_file_name(
        f.home
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_ascii_lowercase(),
    );
    if alias.exists() {
        r.root = alias.to_str().unwrap().into();
        assert!(bind(&r, &f.home, &settings).is_err());
    }
    r = f.rule.clone();
    r.destination = Some(format!("{}/nested", r.root));
    assert!(r.validate().is_err());
}

#[test]
fn restart_pauses_idle_rules_and_restoration_requires_pausing_the_physical_source_scope() {
    let f = Fixture::new(Action::Move);
    let saved = f.save();
    let directory =
        movement::directory(&f.rule.root, &db::settings(&f.db.lock().unwrap()).unwrap()).unwrap();
    assert!(allow_restoration(&f.db.lock().unwrap(), &directory.witness.nodes).is_err());
    journal::initialize(&mut f.db.lock().unwrap()).unwrap();
    assert!(
        !journal::policies(&f.db.lock().unwrap()).unwrap()[0]
            .policy
            .enabled
    );
    allow_restoration(&f.db.lock().unwrap(), &directory.witness.nodes).unwrap();
    let nodes = saved.nodes.clone();
    std::fs::rename(&f.rule.root, f.home.join("OldSource")).unwrap();
    std::fs::create_dir(&f.rule.root).unwrap();
    assert_ne!(
        movement::directory(&f.rule.root, &db::settings(&f.db.lock().unwrap()).unwrap())
            .unwrap()
            .witness
            .nodes,
        nodes
    );
    assert!(saved
        .validate_paths(&f.home, &db::settings(&f.db.lock().unwrap()).unwrap())
        .is_err());
    let offered = preview(
        Edit {
            id: Some(saved.policy.id),
            rule: f.rule.clone(),
        },
        &f.home,
        &db::settings(&f.db.lock().unwrap()).unwrap(),
    )
    .unwrap();
    assert!(journal::save(&mut f.db.lock().unwrap(), offered, &f.home).is_err());
    assert!(f.home.join("OldSource/old-a.txt").is_file());
}
#[tokio::test]
async fn cancellation_before_complete_scan_pauses_without_creating_an_execution() {
    let f = Fixture::new(Action::Trash);
    let saved = f.save();
    let id = uuid::Uuid::new_v4().to_string();
    journal::begin(&mut f.db.lock().unwrap(), &saved, &id, "manual").unwrap();
    let job = Arc::new(Job::new(TaskKind::PersonalTrash));
    job.cancel();
    let result = process(
        f.db.clone(),
        f.home.clone(),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
        saved,
        id.clone(),
        job,
    )
    .await;
    assert!(result.is_err());
    journal::finish(&mut f.db.lock().unwrap(), &id, result).unwrap();
    assert_eq!(
        journal::records(&f.db.lock().unwrap()).unwrap()[0].state,
        "paused"
    );
    assert!(crate::execution::list(&f.db.lock().unwrap())
        .unwrap()
        .is_empty());
    assert!(
        !journal::policies(&f.db.lock().unwrap()).unwrap()[0]
            .policy
            .enabled
    );
}
#[tokio::test]
#[ignore = "real periodic-rule executor, formal Mole rules and native move/Trash for self-created files only; retrieves only its exact Trash receipt"]
async fn real_scheduled_move_and_trash_keep_limits_rules_history_and_archives() {
    for action in [Action::Move, Action::Trash] {
        let f = Fixture::new(action);
        let saved = f.save();
        let id = uuid::Uuid::new_v4().to_string();
        journal::begin(&mut f.db.lock().unwrap(), &saved, &id, "scheduled").unwrap();
        let result = process(
            f.db.clone(),
            f.home.clone(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
            saved.clone(),
            id.clone(),
            Arc::new(Job::new(TaskKind::PersonalTrash)),
        )
        .await;
        journal::finish(&mut f.db.lock().unwrap(), &id, result).unwrap();
        let record = journal::records(&f.db.lock().unwrap()).unwrap().remove(0);
        assert_eq!(record.state, "completed", "{}", record.reason);
        let execution =
            crate::execution::detail(&f.db.lock().unwrap(), record.execution_id.as_ref().unwrap())
                .unwrap();
        assert_eq!(execution.origin, MODULE);
        assert_eq!(execution.summary.moved_count, 1);
        assert_eq!(execution.automation.as_ref().unwrap().root, f.rule.root);
        assert_eq!(record.held_by_limits, 1);
        assert!(!Path::new(&f.rule.root).join("old-a.txt").exists());
        for name in ["old-b.txt", "recent.txt", "old-other.bin"] {
            assert_eq!(
                std::fs::read_to_string(Path::new(&f.rule.root).join(name)).unwrap(),
                format!("owned scheduled {name}")
            );
        }
        let receipt = execution.items[0].receipt.as_ref().unwrap();
        let metadata = std::fs::symlink_metadata(&receipt.destination).unwrap();
        assert_eq!(
            (metadata.dev(), metadata.ino()),
            (receipt.device, receipt.inode)
        );
        if action == Action::Trash {
            objc2::rc::autoreleasepool(|_| {
                use objc2_foundation::{NSFileManager, NSString, NSURL};
                NSFileManager::defaultManager()
                    .moveItemAtURL_toURL_error(
                        &NSURL::fileURLWithPath(&NSString::from_str(&receipt.destination)),
                        &NSURL::fileURLWithPath(&NSString::from_str(&execution.items[0].path)),
                    )
                    .unwrap();
            });
        } else {
            assert_eq!(
                std::fs::read_to_string(&receipt.destination).unwrap(),
                "owned scheduled old-a.txt"
            );
        }
        let expected = serde_json::to_value(&execution).unwrap();
        drop(Arc::try_unwrap(f.db).ok().unwrap().into_inner().unwrap());
        let mut reopened = db::open_owned(&f.home.join("app.sqlite")).unwrap();
        assert_eq!(
            serde_json::to_value(
                crate::execution::detail(&reopened, &execution.summary.id).unwrap()
            )
            .unwrap(),
            expected
        );
        let preview = crate::execution_archive::prepare(&mut reopened).unwrap();
        let dir = f.home.join("Archives");
        let archived =
            crate::execution_archive::archive(&mut reopened, &dir, &preview.fingerprint, db::now())
                .unwrap();
        assert_eq!(
            crate::execution_archive::detail(
                &dir,
                &archived.id,
                crate::execution_archive::Kind::Cleanup,
                &execution.summary.id
            )
            .unwrap(),
            expected
        );
        std::fs::create_dir_all("../.cache").unwrap();
        std::fs::write(format!("../.cache/scheduled-files-{}.json",if action==Action::Trash{"trash"}else{"move"}),serde_json::to_vec_pretty(&serde_json::json!({"record":record,"execution":expected,"unselectedFilesPreserved":true,"historyReopenedAndArchived":true,"trashReceiptRetrievedForCleanup":action==Action::Trash})).unwrap()).unwrap();
    }
}

#[tokio::test]
#[ignore = "real native move followed by injected receipt-storage failure on owned files only; verifies pause and admission refusal before any subsequent file modification"]
async fn real_scheduled_receipt_failure_pauses_and_preserves_unknown_without_replay() {
    let f = Fixture::new(Action::Move);
    let saved = f.save();
    let id = uuid::Uuid::new_v4().to_string();
    f.db.lock().unwrap().execute_batch("CREATE TRIGGER owned_scheduled_failure BEFORE UPDATE OF state ON execution_items WHEN NEW.state='moved' BEGIN SELECT RAISE(ABORT,'owned scheduled receipt failure'); END;").unwrap();
    journal::begin(&mut f.db.lock().unwrap(), &saved, &id, "scheduled").unwrap();
    let result = process(
        f.db.clone(),
        f.home.clone(),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
        saved.clone(),
        id.clone(),
        Arc::new(Job::new(TaskKind::PersonalTrash)),
    )
    .await;
    assert!(result.is_err());
    journal::finish(&mut f.db.lock().unwrap(), &id, result).unwrap();
    let record = journal::records(&f.db.lock().unwrap()).unwrap().remove(0);
    let detail =
        crate::execution::detail(&f.db.lock().unwrap(), record.execution_id.as_ref().unwrap())
            .unwrap();
    assert_eq!(detail.summary.uncertain_count, 1);
    assert!(
        !journal::policies(&f.db.lock().unwrap()).unwrap()[0]
            .policy
            .enabled
    );
    assert!(!Path::new(&f.rule.root).join("old-a.txt").exists());
    assert!(Path::new(f.rule.destination.as_ref().unwrap())
        .join("old-a.txt")
        .exists());
    let offered = preview(
        Edit {
            id: Some(saved.policy.id),
            rule: f.rule.clone(),
        },
        &f.home,
        &db::settings(&f.db.lock().unwrap()).unwrap(),
    )
    .unwrap();
    journal::save(&mut f.db.lock().unwrap(), offered, &f.home).unwrap();
    let saved = journal::policies(&f.db.lock().unwrap()).unwrap().remove(0);
    let next = uuid::Uuid::new_v4().to_string();
    journal::begin(&mut f.db.lock().unwrap(), &saved, &next, "manual").unwrap();
    let result = process(
        f.db.clone(),
        f.home.clone(),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
        saved,
        next.clone(),
        Arc::new(Job::new(TaskKind::PersonalTrash)),
    )
    .await;
    assert!(result.is_err());
    journal::finish(&mut f.db.lock().unwrap(), &next, result).unwrap();
    assert!(Path::new(&f.rule.root).join("old-b.txt").exists());
    let expected = serde_json::to_value(&detail).unwrap();
    drop(Arc::try_unwrap(f.db).ok().unwrap().into_inner().unwrap());
    let reopened = db::open_owned(&f.home.join("app.sqlite")).unwrap();
    assert_eq!(
        serde_json::to_value(crate::execution::detail(&reopened, &detail.summary.id).unwrap())
            .unwrap(),
        expected
    );
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/scheduled-files-storage-failure.json",serde_json::to_vec_pretty(&serde_json::json!({"execution":expected,"rulePaused":true,"subsequentAdmissionRefused":true,"nextFilePreserved":true,"unknownSurvivesReopen":true})).unwrap()).unwrap();
}
