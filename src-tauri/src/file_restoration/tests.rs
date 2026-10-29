use super::*;
use crate::engine::TaskKind;

struct Fixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    original: PathBuf,
    current: PathBuf,
    database: Arc<std::sync::Mutex<db::Database>>,
    detail: execution::Detail,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::Builder::new()
            .prefix("MooMole-restore-owned-")
            .tempdir()
            .unwrap();
        Self::at(temp)
    }
    fn at(temp: tempfile::TempDir) -> Self {
        let home = temp.path().canonicalize().unwrap();
        let directory = home.join("Downloads");
        let organized = home.join("Organized");
        std::fs::create_dir(&directory).unwrap();
        std::fs::create_dir(&organized).unwrap();
        let original = directory.join("chosen.txt");
        let current = organized.join("chosen.txt");
        std::fs::write(&original, "owned restoration content").unwrap();
        let w = crate::observation::witness(&original).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let now = db::now();
        let item = personal_files::Item {
            id: id.clone(),
            path: original.to_str().unwrap().into(),
            size: w.size,
            modified_at: now,
            witness: w,
        };
        let settings = db::Settings {
            acknowledged: true,
            ..Default::default()
        };
        let target_dir = movement::directory(organized.to_str().unwrap(), &settings).unwrap();
        let movement = movement::Movement {
            directory: organized.to_str().unwrap().into(),
            witness: target_dir.witness,
            conflict: movement::Conflict::Skip,
            targets: vec![movement::Target {
                id: id.clone(),
                destination: Some(current.to_str().unwrap().into()),
            }],
        };
        let plan = personal_files::Plan {
            module: personal_files::MODULE.into(),
            protocol: 1,
            revision: "MooMole.personal-files.move.v1".into(),
            id: uuid::Uuid::new_v4().to_string(),
            scan_id: uuid::Uuid::new_v4().to_string(),
            root: directory.to_str().unwrap().into(),
            created_at: now,
            expires_at: now + 30_000,
            items: vec![item.clone()],
            movement: Some(movement.clone()),
            duplicates: None,
            similar_photos: None,
            software_content: None,
        };
        let mut database = db::open_owned(&home.join("app.sqlite")).unwrap();
        db::save_settings(&database, &settings).unwrap();
        let run = execution::begin_personal(&mut database, &plan, now).unwrap();
        execution::intent(&mut database, &run, &id, now).unwrap();
        let (outcome, reason) =
            movement::Prepared::new(&item, &movement, current.to_str().unwrap(), &settings)
                .unwrap()
                .move_checked(|| Ok(()))
                .unwrap();
        execution::complete_item(&mut database, &run, &id, outcome, &reason, db::now()).unwrap();
        execution::finish(&mut database, &run, false, db::now()).unwrap();
        let detail = execution::detail(&database, &run).unwrap();
        assert_eq!(detail.summary.moved_count, 1);
        Self {
            _temp: temp,
            home,
            original,
            current,
            database: Arc::new(std::sync::Mutex::new(database)),
            detail,
        }
    }
    fn plan(&self) -> Plan {
        prepare(
            &self.detail,
            &self.detail.items[0].id,
            &self.home,
            &db::settings(&self.database.lock().unwrap()).unwrap(),
        )
        .unwrap()
    }
}
#[test]
fn actual_restore_keeps_original_record_and_survives_reopen_and_archive() {
    let f = Fixture::new();
    let original_record = serde_json::to_value(&f.detail).unwrap();
    let plan = f.plan();
    let mut store = Store::default();
    let draft = store.publish(plan);
    let plan = store.take(&draft.id).unwrap();
    assert!(store.take(&draft.id).is_err());
    let result = run(
        f.database.clone(),
        f.home.clone(),
        plan.clone(),
        Arc::new(Job::new(TaskKind::FileRestore)),
    )
    .unwrap();
    assert_eq!(result.origin, MODULE);
    assert_eq!(result.summary.moved_count, 1);
    assert!(!f.current.exists());
    assert_eq!(
        std::fs::read_to_string(&f.original).unwrap(),
        "owned restoration content"
    );
    let mut db = f.database.lock().unwrap();
    assert_eq!(
        serde_json::to_value(execution::detail(&db, &f.detail.summary.id).unwrap()).unwrap(),
        original_record
    );
    assert!(execution::begin_restoration(&mut db, &plan, db::now()).is_err());
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
}
#[test]
fn restoration_rejects_conflicts_changed_receipts_protection_and_unknown_results() {
    for mode in [
        "conflict",
        "replacement",
        "protected",
        "unknown",
        "missingParent",
    ] {
        let f = Fixture::new();
        let plan = f.plan();
        let mut settings = db::settings(&f.database.lock().unwrap()).unwrap();
        match mode {
            "conflict" => std::fs::write(&f.original, "do not overwrite").unwrap(),
            "replacement" => {
                std::fs::remove_file(&f.current).unwrap();
                std::fs::write(&f.current, "replacement").unwrap();
            }
            "protected" => settings
                .protected_paths
                .push(f.original.to_str().unwrap().into()),
            "missingParent" => {
                std::fs::rename(f.original.parent().unwrap(), f.home.join("old-dir")).unwrap()
            }
            "unknown" => {}
            _ => unreachable!(),
        }
        if mode == "unknown" {
            let mut detail = f.detail;
            detail.items[0].state = "uncertain".into();
            assert!(prepare(&detail, &detail.items[0].id, &f.home, &settings).is_err());
        } else {
            assert!(plan.validate(&f.home, &settings).is_err(), "{mode}");
        }
    }
    let f = Fixture::new();
    let job = Arc::new(Job::new(TaskKind::FileRestore));
    job.cancel();
    let result = run(f.database.clone(), f.home.clone(), f.plan(), job).unwrap();
    assert_eq!(result.summary.state, "cancelled");
    assert_eq!(result.summary.moved_count, 0);
    assert!(f.current.exists());
}

#[test]
#[ignore = "real Trash and restoration only for a self-created folder under the current user's home"]
fn native_trash_receipt_restores_owned_file() {
    let real_home = PathBuf::from(std::env::var_os("HOME").unwrap())
        .canonicalize()
        .unwrap();
    let temp = tempfile::Builder::new()
        .prefix("MooMole-restore-owned-")
        .tempdir_in(&real_home)
        .unwrap();
    let f = Fixture::at(temp);
    let witness = crate::observation::witness(&f.current).unwrap();
    let item = personal_files::Item {
        id: uuid::Uuid::new_v4().to_string(),
        path: f.current.to_str().unwrap().into(),
        size: witness.size,
        modified_at: db::now(),
        witness,
    };
    let now = db::now();
    let plan = personal_files::Plan {
        module: personal_files::MODULE.into(),
        protocol: 1,
        revision: personal_files::REVISION.into(),
        id: uuid::Uuid::new_v4().to_string(),
        scan_id: uuid::Uuid::new_v4().to_string(),
        root: f.current.parent().unwrap().to_str().unwrap().into(),
        created_at: now,
        expires_at: now + 30_000,
        items: vec![item.clone()],
        movement: None,
        duplicates: None,
        similar_photos: None,
        software_content: None,
    };
    let run_id = execution::begin_personal(&mut f.database.lock().unwrap(), &plan, now).unwrap();
    execution::intent(&mut f.database.lock().unwrap(), &run_id, &item.id, now).unwrap();
    let moved = crate::cleanup_target::PreparedFile::prepare(&item.target())
        .unwrap()
        .move_to_trash_checked(|| Ok(()))
        .unwrap();
    execution::complete_item(
        &mut f.database.lock().unwrap(),
        &run_id,
        &item.id,
        moved.outcome,
        &moved.reason,
        db::now(),
    )
    .unwrap();
    execution::finish(&mut f.database.lock().unwrap(), &run_id, false, db::now()).unwrap();
    let source = execution::detail(&f.database.lock().unwrap(), &run_id).unwrap();
    assert_eq!(source.summary.moved_count, 1);
    let settings = db::settings(&f.database.lock().unwrap()).unwrap();
    let result = prepare(&source, &item.id, &real_home, &settings).and_then(|plan| {
        run(
            f.database.clone(),
            real_home.clone(),
            plan,
            Arc::new(Job::new(TaskKind::FileRestore)),
        )
    });
    // Always retrieve the exact owned receipt on a failed test, never guess or
    // remove unrelated Trash entries. This fallback is not restore success.
    if !result.as_ref().is_ok_and(|d| d.summary.moved_count == 1) {
        let receipt = source.items[0].receipt.as_ref().unwrap();
        let meta = std::fs::symlink_metadata(&receipt.destination).unwrap();
        assert_eq!((meta.dev(), meta.ino()), (receipt.device, receipt.inode));
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
    std::fs::create_dir_all("../.cache").unwrap();
    let evidence = match &result {
        Ok(r) => serde_json::to_value(r).unwrap(),
        Err(e) => serde_json::json!({"error":e}),
    };
    std::fs::write(
        "../.cache/restoration-native-trash.json",
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    let restored = result.unwrap();
    assert_eq!(
        restored.summary.moved_count,
        1,
        "{}",
        serde_json::to_string(&restored).unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(&f.current).unwrap(),
        "owned restoration content"
    );
}
