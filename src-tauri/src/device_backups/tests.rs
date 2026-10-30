use super::*;
use std::{fs, sync::Mutex};
fn backup(path: &Path, finished: bool) {
    fs::create_dir_all(path.join("ab")).unwrap();
    fs::write(path.join("Info.plist"),br#"<?xml version="1.0"?><plist version="1.0"><dict><key>Device Name</key><string>Owned Phone</string><key>Product Type</key><string>iPhone16,2</string></dict></plist>"#).unwrap();
    fs::write(path.join("Manifest.plist"),br#"<?xml version="1.0"?><plist version="1.0"><dict><key>IsEncrypted</key><false/></dict></plist>"#).unwrap();
    fs::write(path.join("Status.plist"),format!(r#"<?xml version="1.0"?><plist version="1.0"><dict><key>SnapshotState</key><string>{}</string></dict></plist>"#,if finished{"finished"}else{"uploading"})).unwrap();
    let c = rusqlite::Connection::open(path.join("Manifest.db")).unwrap();
    c.execute_batch("CREATE TABLE owned_fixture(value TEXT);INSERT INTO owned_fixture VALUES('self-created metadata, not a device-restorable backup');").unwrap();
    drop(c);
    fs::write(
        path.join("ab/owned-payload"),
        b"owned unique backup payload\n",
    )
    .unwrap();
}
struct Fixture {
    _temp: tempfile::TempDir,
    ctx: Context,
    source: PathBuf,
    db: Arc<Mutex<db::Database>>,
}
impl Fixture {
    fn reopen(&mut self) {
        let handoff = self.ctx.home.join("reopen-handoff");
        fs::create_dir_all(&handoff).unwrap();
        let placeholder = db::open_owned(&handoff.join("owned.sqlite")).unwrap();
        let previous = std::mem::replace(&mut self.db, Arc::new(Mutex::new(placeholder)));
        assert_eq!(Arc::strong_count(&previous), 1);
        drop(previous);
        self.db = Arc::new(Mutex::new(
            db::open_owned(&self.ctx.home.join("owned.sqlite")).unwrap(),
        ));
    }
    fn new() -> Self {
        let temp = tempfile::Builder::new()
            .prefix("MooMole-owned-device-backups-")
            .tempdir()
            .unwrap();
        let home = temp.path().canonicalize().unwrap();
        let source = home.join("BackupCopies");
        fs::create_dir(&source).unwrap();
        backup(
            &source.join(format!("MooMole-owned-backup-{}", uuid::Uuid::new_v4())),
            true,
        );
        let ctx = Context {
            home: home.clone(),
            settings: db::Settings {
                acknowledged: true,
                ..Default::default()
            },
        };
        let database = db::open_owned(&home.join("owned.sqlite")).unwrap();
        db::save_settings(&database, &ctx.settings).unwrap();
        Self {
            _temp: temp,
            ctx,
            source,
            db: Arc::new(Mutex::new(database)),
        }
    }
    fn plan(&self) -> Plan {
        let scan = collect(&self.ctx, &self.source, &Job::new(TaskKind::PersonalScan)).unwrap();
        assert!(
            scan.report.complete,
            "{}",
            scan.report.items.first().unwrap().reason
        );
        let item = scan.candidates[0].row.id.clone();
        let scan_id = scan.report.id.clone();
        let mut store = Store {
            scan: Some(scan),
            ..Default::default()
        };
        let draft = store.prepare(&scan_id, &item, &self.ctx).unwrap();
        assert!(draft.expires_at <= db::now() + 30_000);
        store.take(&draft.id).unwrap()
    }
    fn worker(&self) -> commands::Worker {
        commands::Worker {
            db: self.db.clone(),
            home: self.ctx.home.clone(),
            engine_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
        }
    }
}
#[test]
fn discovers_whole_backups_and_preserves_incomplete_links_and_protection() {
    let f = Fixture::new();
    backup(&f.source.join("incomplete"), false);
    backup(&f.source.join("protected"), true);
    backup(&f.source.join("disguised.app"), true);
    let mut ctx = f.ctx.clone();
    ctx.settings
        .protected_paths
        .push(f.source.join("protected").to_str().unwrap().into());
    std::os::unix::fs::symlink(f.source.join("incomplete"), f.source.join("alias")).unwrap();
    let scan = collect(&ctx, &f.source, &Job::new(TaskKind::PersonalScan)).unwrap();
    assert!(!scan.report.complete);
    assert_eq!(scan.candidates.len(), 1);
    assert_eq!(scan.report.items.iter().filter(|r| r.eligible).count(), 1);
    assert!(
        scan.report
            .items
            .iter()
            .find(|r| r.name == "incomplete")
            .unwrap()
            .marked_finished
            == false
    );
    let owned = &scan.candidates[0];
    assert_eq!(owned.row.device_name.as_deref(), Some("Owned Phone"));
    assert_eq!(owned.row.encrypted, Some(false));
    assert!(owned.snapshot.bytes > 0);
    assert!(root(&ctx, &ctx.home).is_err());
    let job = Job::new(TaskKind::PersonalScan);
    job.cancel();
    assert!(collect(&ctx, &f.source, &job).is_err());
}
#[test]
fn backup_confirmation_is_exact_single_use_and_content_changes_are_refused() {
    let f = Fixture::new();
    let p = f.plan();
    let job = Job::new(TaskKind::PersonalTrash);
    assert!(validate(&p, &f.ctx, &job).is_ok());
    fs::write(
        Path::new(&p.item.row.path).join("ab/owned-payload"),
        "changed after selection",
    )
    .unwrap();
    assert!(validate(&p, &f.ctx, &job).is_err());
    let mut store = Store::default();
    store.offers.insert(
        p.id.clone(),
        (p.clone(), Instant::now() - Duration::from_secs(1)),
    );
    assert!(store.take(&p.id).is_err());
    assert!(store.take(&p.id).is_err());
    let mut p = f.plan();
    p.created_at = db::now() + 30_000;
    store.offers.insert(
        p.id.clone(),
        (p.clone(), Instant::now() + Duration::from_secs(30)),
    );
    assert!(store.take(&p.id).is_err());
    let p = f.plan();
    let mut ctx = f.ctx.clone();
    ctx.settings.protected_paths.push(
        Path::new(&p.item.row.path)
            .join("ab/owned-payload")
            .to_str()
            .unwrap()
            .into(),
    );
    assert!(validate(&p, &ctx, &job).is_err());
    assert!(native(&p, &f.ctx, &job, || Err("intent storage refused".into())).is_err());
    assert!(Path::new(&p.item.row.path).exists());
}
fn record(plan: Plan, phase: &str) -> Record {
    Record {
        revision: REVISION.into(),
        id: plan.id.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        created_at: db::now(),
        finished_at: None,
        state: "running".into(),
        phase: phase.into(),
        plan,
        reason: String::new(),
        receipt: None,
        reviewed_at: None,
    }
}
#[test]
fn durable_restart_preserves_intent_unknown_and_never_replays() {
    let mut f = Fixture::new();
    let mut r = record(f.plan(), "intent");
    {
        let mut db = f.db.lock().unwrap();
        journal::begin(&mut db, &r).unwrap();
    }
    f.reopen();
    let rows = journal::reconcile(&*f.db.lock().unwrap(), &[]).unwrap();
    assert_eq!(rows[0].state, "unknown");
    assert!(Path::new(&r.plan.item.row.path).exists());
    assert!(journal::admission(&*f.db.lock().unwrap()).is_err());
    r = rows[0].clone();
    r.reviewed_at = Some(db::now());
    journal::save(&*f.db.lock().unwrap(), &r).unwrap();
    assert!(journal::admission(&*f.db.lock().unwrap()).is_ok());
    assert_eq!(
        journal::list(&*f.db.lock().unwrap()).unwrap()[0].state,
        "unknown"
    );
}
#[test]
fn backup_snapshot_handles_more_than_cache_tree_limits_and_rejects_shared_files() {
    let f = Fixture::new();
    let p = f.plan();
    let folder = Path::new(&p.item.row.path).join("ab");
    for n in 0..10_010 {
        fs::write(folder.join(format!("owned-{n}")), b"x").unwrap();
    }
    let p = f.plan();
    assert!(p.item.snapshot.entries > 10_000);
    fs::hard_link(folder.join("owned-0"), folder.join("shared")).unwrap();
    assert!(validate(&p, &f.ctx, &Job::new(TaskKind::PersonalTrash)).is_err());
}
fn reference(path: &str) -> objc2::rc::Retained<objc2_foundation::NSURL> {
    use objc2_foundation::{NSString, NSURL};
    NSURL::fileURLWithPath(&NSString::from_str(path))
        .fileReferenceURL()
        .unwrap()
}
fn retrieve(reference: &objc2_foundation::NSURL, original: &str, expected: &Candidate) {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let actual = reference.filePathURL().unwrap().path().unwrap().to_string();
    let m = fs::symlink_metadata(&actual).unwrap();
    let n = expected.witness.nodes.last().unwrap();
    assert_eq!((m.dev(), m.ino()), (n.dev, n.ino));
    assert!(!Path::new(original).exists());
    NSFileManager::defaultManager()
        .moveItemAtURL_toURL_error(
            reference,
            &NSURL::fileURLWithPath(&NSString::from_str(original)),
        )
        .unwrap();
    assert_eq!(
        fs::read(Path::new(original).join("ab/owned-payload")).unwrap(),
        b"owned unique backup payload\n"
    );
}
#[tokio::test]
#[ignore = "real formal Mole rules and native whole-directory Trash on unique self-created backup-shaped samples only; no real device content"]
async fn real_whole_backup_trash_preserves_unselected_and_durable_receipts() {
    let mut f = Fixture::new();
    let p = f.plan();
    let original = p.item.row.path.clone();
    let expected = p.item.clone();
    let reference = reference(&original);
    backup(&f.source.join("unselected"), true);
    commands::run(
        f.worker(),
        p,
        uuid::Uuid::new_v4().to_string(),
        Arc::new(Job::new(TaskKind::PersonalTrash)),
    )
    .await
    .unwrap();
    f.reopen();
    let rows = journal::list(&*f.db.lock().unwrap()).unwrap();
    let r = &rows[0];
    assert_eq!(r.state, "moved", "{}", r.reason);
    assert!(r.receipt.is_some());
    assert!(f.source.join("unselected/ab/owned-payload").exists());
    let evidence =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.cache/device-backups-native.json");
    fs::write(evidence, serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
    retrieve(&reference, &original, &expected);
    assert!(validate(&f.plan(), &f.ctx, &Job::new(TaskKind::PersonalScan)).is_ok());
}
#[tokio::test]
#[ignore = "real native Trash with owned SQLite receipt-write fault; preserves unknown and never retries the operation"]
async fn real_backup_receipt_failure_preserves_intent_and_blocks_another_attempt() {
    let mut f = Fixture::new();
    let p = f.plan();
    let original = p.item.row.path.clone();
    let expected = p.item.clone();
    let reference = reference(&original);
    // Create only the optional schema, then refuse the post-native receipt.
    {
        let mut db = f.db.lock().unwrap();
        let r = record(p.clone(), "checking");
        journal::begin(&mut db, &r).unwrap();
        db.execute("DELETE FROM device_backup_runs", []).unwrap();
        db.execute_batch("CREATE TRIGGER owned_receipt_fault BEFORE UPDATE ON device_backup_runs WHEN json_extract(NEW.body,'$.state')='moved' BEGIN SELECT RAISE(ABORT,'owned receipt fault');END;").unwrap();
    }
    assert!(commands::run(
        f.worker(),
        p,
        uuid::Uuid::new_v4().to_string(),
        Arc::new(Job::new(TaskKind::PersonalTrash))
    )
    .await
    .is_err());
    f.reopen();
    let rows = journal::reconcile(&*f.db.lock().unwrap(), &[]).unwrap();
    assert_eq!(rows[0].state, "unknown");
    assert!(rows[0].receipt.is_none());
    assert!(!Path::new(&original).exists());
    assert!(journal::admission(&*f.db.lock().unwrap()).is_err());
    fs::write(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.cache/device-backups-fault.json"),
        serde_json::to_vec_pretty(&rows).unwrap(),
    )
    .unwrap();
    retrieve(&reference, &original, &expected);
}
