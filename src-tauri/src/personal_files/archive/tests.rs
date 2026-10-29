use super::*;
use crate::engine::TaskKind;
fn fixture() -> (tempfile::TempDir, Arc<Mutex<db::Database>>, Plan) {
    let temp = tempfile::Builder::new()
        .prefix("MooMole-file-archive-owned-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let root = home.join("Downloads");
    let destination = home.join("Archives");
    std::fs::create_dir_all(root.join("nested")).unwrap();
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(root.join("nested/notes.txt"), "owned nested content\n").unwrap();
    std::fs::write(root.join("notes.txt"), "separate owned content\n").unwrap();
    std::fs::write(root.join("empty.bin"), []).unwrap();
    let scan = super::super::scan::collect(&root, &[], Arc::new(Job::new(TaskKind::PersonalScan)))
        .unwrap();
    let ids = scan.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
    let mut store = super::super::Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = store.offer(&ids, db::now()).unwrap();
    let source = store.take(&offer.id, db::now()).unwrap();
    std::fs::create_dir(home.join("app-data")).unwrap();
    let database = db::open_owned(&home.join("app-data/database.sqlite")).unwrap();
    db::save_settings(
        &database,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    let plan = prepare(
        source,
        destination.to_str().unwrap(),
        "owned.zip",
        &db::settings(&database).unwrap(),
    )
    .unwrap();
    (temp, Arc::new(Mutex::new(database)), plan)
}
#[test]
fn verified_zip_preserves_originals_nested_names_and_persistent_history() {
    let (_temp, db, plan) = fixture();
    let originals = plan
        .source
        .items
        .iter()
        .map(|i| (i.path.clone(), std::fs::read(&i.path).unwrap()))
        .collect::<Vec<_>>();
    let record = run(
        db.clone(),
        plan,
        uuid::Uuid::new_v4().to_string(),
        Arc::new(Job::new(TaskKind::PersonalArchive)),
    )
    .unwrap();
    assert_eq!(record.state, "completed", "{}", record.reason);
    assert!(record
        .entries
        .iter()
        .all(|e| e.state == "archived" && e.sha256.is_some()));
    let mut zip = zip::ZipArchive::new(File::open(&record.output_path).unwrap()).unwrap();
    assert_eq!(zip.len(), 3);
    for (path, bytes) in originals {
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    let mut empty = Vec::new();
    zip.by_name("empty.bin")
        .unwrap()
        .read_to_end(&mut empty)
        .unwrap();
    assert!(empty.is_empty());
    assert!(zip.by_name("nested/notes.txt").is_ok());
    assert_eq!(
        journal::list(&*db.lock().unwrap()).unwrap()[0].id,
        record.id
    );
    assert!(!Path::new(&record.temporary_path).exists());
}
#[test]
fn collision_cancel_and_source_change_never_replace_files_or_publish_partial_zip() {
    let (_temp, db, plan) = fixture();
    std::fs::write(&plan.output_path, "keep existing target").unwrap();
    let record = run(
        db.clone(),
        plan.clone(),
        uuid::Uuid::new_v4().to_string(),
        Arc::new(Job::new(TaskKind::PersonalArchive)),
    )
    .unwrap();
    assert_eq!(record.state, "failed");
    assert_eq!(
        std::fs::read_to_string(&plan.output_path).unwrap(),
        "keep existing target"
    );
    assert!(!Path::new(&record.temporary_path).exists());
    let (_temp, db, plan) = fixture();
    let job = Arc::new(Job::new(TaskKind::PersonalArchive));
    job.cancel();
    let record = run(
        db.clone(),
        plan.clone(),
        uuid::Uuid::new_v4().to_string(),
        job,
    )
    .unwrap();
    assert_eq!(record.state, "cancelled");
    assert!(!Path::new(&record.output_path).exists());
    std::fs::write(&plan.source.items[0].path, "changed after scan").unwrap();
    assert!(run(
        db,
        plan,
        uuid::Uuid::new_v4().to_string(),
        Arc::new(Job::new(TaskKind::PersonalArchive))
    )
    .is_err());
}
#[test]
fn cancellation_after_temporary_creation_keeps_original_and_publishes_nothing() {
    let (_temp, db, mut plan) = fixture();
    let one = &mut plan.source.items[0];
    File::options()
        .write(true)
        .open(&one.path)
        .unwrap()
        .set_len(256 * 1024 * 1024)
        .unwrap();
    one.witness = AnchoredPath::open(Path::new(&one.path)).unwrap().witness;
    one.size = one.witness.size;
    one.modified_at = one.witness.mtime * 1000;
    let original = one.path.clone();
    let temporary = Path::new(&plan.directory.directory).join(&plan.temporary_name);
    let job = Arc::new(Job::new(TaskKind::PersonalArchive));
    let cancel = job.clone();
    let watcher = std::thread::spawn(move || {
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(3) {
            if temporary.exists() {
                cancel.cancel();
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    });
    let record = run(db, plan, uuid::Uuid::new_v4().to_string(), job).unwrap();
    assert!(watcher.join().unwrap());
    assert_eq!(record.state, "cancelled", "{}", record.reason);
    assert!(!Path::new(&record.output_path).exists());
    assert!(!Path::new(&record.temporary_path).exists());
    assert_eq!(
        std::fs::metadata(original).unwrap().len(),
        256 * 1024 * 1024
    );
}
#[test]
fn ambiguous_zip_entry_names_are_rejected_before_writing() {
    let (_temp, db, mut plan) = fixture();
    let item = &mut plan.source.items[0];
    let new = Path::new(&item.path)
        .parent()
        .unwrap()
        .join("ambiguous\\path.txt");
    std::fs::rename(&item.path, &new).unwrap();
    item.path = new.to_str().unwrap().into();
    item.witness = AnchoredPath::open(&new).unwrap().witness;
    let settings = db::settings(&*db.lock().unwrap()).unwrap();
    assert!(prepare(
        plan.source,
        &plan.directory.directory,
        "other.zip",
        &settings
    )
    .is_err());
    assert!(!Path::new(&plan.output_path).exists());
}
#[test]
fn reopen_marks_durable_intent_unknown_without_replay_or_removing_partial() {
    let (temp, db, plan) = fixture();
    let record = Record {
        id: plan.source.id.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        created_at: db::now(),
        finished_at: None,
        state: "running".into(),
        phase: "building".into(),
        output_path: plan.output_path.clone(),
        temporary_path: Path::new(&plan.directory.directory)
            .join(&plan.temporary_name)
            .to_string_lossy()
            .into_owned(),
        entries: vec![],
        artifact: None,
        reason: String::new(),
        reviewed_at: None,
    };
    journal::insert(&mut *db.lock().unwrap(), &plan, &record).unwrap();
    std::fs::write(&record.temporary_path, "owned interrupted partial").unwrap();
    drop(db);
    let database = db::open_owned(
        &temp
            .path()
            .canonicalize()
            .unwrap()
            .join("app-data/database.sqlite"),
    )
    .unwrap();
    let records = journal::list(&database).unwrap();
    assert_eq!(records[0].state, "interrupted");
    assert_eq!(
        std::fs::read_to_string(&record.temporary_path).unwrap(),
        "owned interrupted partial"
    );
    assert!(!Path::new(&record.output_path).exists());
    assert!(journal::acknowledge(&database, &record.id)
        .unwrap()
        .reviewed_at
        .is_some());
}
#[test]
#[ignore = "real ZIP publication and independent macOS ditto extraction on self-created files only"]
fn native_zip_publication_is_independently_readable_and_preserves_owned_sources() {
    let (temp, db, plan) = fixture();
    let root = PathBuf::from(&plan.source.root);
    let record = run(
        db.clone(),
        plan,
        uuid::Uuid::new_v4().to_string(),
        Arc::new(Job::new(TaskKind::PersonalArchive)),
    )
    .unwrap();
    assert_eq!(record.state, "completed", "{}", record.reason);
    let extracted = temp.path().join("independent-extraction");
    assert!(std::process::Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(&record.output_path)
        .arg(&extracted)
        .status()
        .unwrap()
        .success());
    for entry in &record.entries {
        assert_eq!(
            std::fs::read(root.join(&entry.name)).unwrap(),
            std::fs::read(extracted.join(&entry.name)).unwrap()
        );
    }
    drop(db);
    let reopened = db::open_owned(
        &temp
            .path()
            .canonicalize()
            .unwrap()
            .join("app-data/database.sqlite"),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&journal::list(&reopened).unwrap()[0]).unwrap(),
        serde_json::to_value(&record).unwrap()
    );
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/file-archive-native.json",serde_json::to_vec_pretty(&serde_json::json!({"record":record,"independentExtractionVerified":true,"originalsPreserved":true})).unwrap()).unwrap();
}
