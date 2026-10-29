use super::*;
use crate::engine::TaskKind;
fn fixture() -> (tempfile::TempDir, Context, String, PathBuf) {
    let t = tempfile::Builder::new()
        .prefix("MooMole-startup-owned-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let ctx = Context {
        home,
        settings: db::Settings {
            acknowledged: true,
            ..Default::default()
        },
        own_app: None,
    };
    std::fs::create_dir_all(ctx.root()).unwrap();
    let label = format!("org.moomole.fixture.{}", uuid::Uuid::new_v4());
    let file = ctx.root().join(format!("{label}.plist"));
    let body=format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key><array><string>/bin/sleep</string><string>120</string></array><key>RunAtLoad</key><true/></dict></plist>");
    std::fs::write(&file, body).unwrap();
    (t, ctx, label, file)
}
#[test]
fn user_root_scan_retains_links_duplicates_protected_and_unrecorded_paused_files() {
    let (_t, mut ctx, _, file) = fixture();
    let job = Job::new(TaskKind::StartupManagement);
    let first = scan(&ctx, &[], &job).unwrap();
    assert_eq!(first.items.len(), 1);
    assert!(first.report.items[0].eligible);
    std::os::unix::fs::symlink(&file, ctx.root().join("alias.plist")).unwrap();
    std::fs::copy(&file, ctx.root().join("same-label.plist")).unwrap();
    let duplicate = scan(&ctx, &[], &job).unwrap();
    assert_eq!(duplicate.items.len(), 2);
    assert!(duplicate.items.iter().all(|i| !i.row.eligible));
    assert!(!duplicate.report.complete);
    std::fs::remove_file(ctx.root().join("same-label.plist")).unwrap();
    std::fs::remove_file(ctx.root().join("alias.plist")).unwrap();
    let paused = PathBuf::from(format!("{}{SUFFIX}", file.to_str().unwrap()));
    std::fs::rename(&file, &paused).unwrap();
    let without_receipt = scan(&ctx, &[], &job).unwrap();
    assert!(without_receipt.items[0].row.paused);
    assert!(!without_receipt.items[0].row.eligible);
    ctx.settings
        .protected_paths
        .push(ctx.root().to_str().unwrap().into());
    assert!(scan(&ctx, &[], &job).is_err());
}
#[test]
fn parser_does_not_infer_relative_programs_privileged_or_injected_service_labels() {
    for body in ["<dict><key>Label</key><string>x/y</string><key>Program</key><string>/bin/sleep</string></dict>","<dict><key>Label</key><string>owned</string><key>Program</key><string>relative</string></dict>","<dict><key>Label</key><string>owned</string><key>UserName</key><string>root</string><key>Program</key><string>/bin/sleep</string></dict>"]{
        assert!(plist::parse(format!("<plist version=\"1.0\">{body}</plist>").as_bytes()).is_err());
    }
}
struct OwnedService {
    label: String,
    definition: Definition,
}
#[test]
fn interrupted_intent_never_replays_and_modified_paused_files_lose_resume_authority() {
    let (_temp, ctx, label, file) = fixture();
    let job = Job::new(TaskKind::StartupManagement);
    let (bytes, witness) = read(&file, 512 * 1024, &job).unwrap();
    let (program_bytes, program) = read(Path::new("/bin/sleep"), 256 * 1024 * 1024, &job).unwrap();
    let now = db::now();
    let p = Plan {
        id: uuid::Uuid::new_v4().to_string(),
        scan_id: uuid::Uuid::new_v4().to_string(),
        created_at: now,
        expires_at: now + 30000,
        action: Action::Pause,
        home: ctx.home.clone(),
        root: root(&ctx).unwrap().witness,
        path: file.to_str().unwrap().into(),
        target: format!("{}{SUFFIX}", file.to_str().unwrap()),
        source: witness,
        sha256: hash(&bytes),
        definition: plist::parse(&bytes).unwrap(),
        program,
        program_sha256: hash(&program_bytes),
        expected: None,
    };
    let mut r = Record {
        id: p.id.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        created_at: now,
        finished_at: None,
        action: Action::Pause,
        path: p.path.clone(),
        target: p.target.clone(),
        label,
        program: "/bin/sleep".into(),
        state: "running".into(),
        phase: "configurationIntent".into(),
        configuration_changed: false,
        receipt: None,
        moved_witness: None,
        sha256: p.sha256.clone(),
        reason: String::new(),
        reviewed_at: None,
    };
    let mut database = db::open_owned(&ctx.home.join("database.sqlite")).unwrap();
    db::save_settings(&database, &ctx.settings).unwrap();
    journal::insert(&mut database, &p, &r).unwrap();
    drop(database);
    let database = db::open_owned(&ctx.home.join("database.sqlite")).unwrap();
    assert_eq!(journal::list(&database).unwrap()[0].state, "interrupted");
    assert!(file.exists());
    assert!(!Path::new(&p.target).exists());
    let reviewed = journal::acknowledge(&database, &p.id).unwrap();
    assert_eq!(reviewed.state, "interrupted");
    assert!(reviewed.reviewed_at.is_some());
    std::fs::rename(&file, &p.target).unwrap();
    let moved = AnchoredPath::open(Path::new(&p.target)).unwrap().witness;
    let n = moved.nodes.last().unwrap();
    r.state = "completed".into();
    r.configuration_changed = true;
    r.receipt = Some(crate::execution::Receipt {
        destination: p.target.clone(),
        device: n.dev,
        inode: n.ino,
    });
    r.moved_witness = Some(moved.clone());
    assert!(journal::resume_proof(
        &[r.clone()],
        Path::new(&p.target),
        &moved,
        &p.sha256
    ));
    std::fs::write(&p.target, "modified paused configuration").unwrap();
    let (changed, witness) = read(Path::new(&p.target), 512 * 1024, &job).unwrap();
    assert!(!journal::resume_proof(
        &[r],
        Path::new(&p.target),
        &witness,
        &hash(&changed)
    ));
}
impl Drop for OwnedService {
    fn drop(&mut self) {
        if runtime::query(&self.label).is_ok_and(|service| {
            service.is_some() && runtime::matches(&service, &self.definition).is_ok()
        }) {
            let _ = runtime::request(
                Action::Pause,
                &self.label,
                Path::new("/"),
                &Job::new(TaskKind::StartupManagement),
            );
        }
    }
}
#[test]
#[ignore = "register, pause and restore only a self-created temporary LaunchAgent in the current user GUI domain"]
fn native_pause_resume_preserves_owned_plist_and_verifies_service_registration() {
    let (_temp, ctx, label, file) = fixture();
    let original = std::fs::read(&file).unwrap();
    let _owned = OwnedService {
        label: label.clone(),
        definition: plist::parse(&original).unwrap(),
    };
    let attempt = runtime::request(
        Action::Resume,
        &label,
        &file,
        &Job::new(TaskKind::StartupManagement),
    )
    .unwrap();
    assert_eq!(attempt.exit_code, Some(0));
    let observed = runtime::query(&label).unwrap();
    assert!(observed.is_some());
    let db = db::open_owned(&ctx.home.join("database.sqlite")).unwrap();
    db::save_settings(&db, &ctx.settings).unwrap();
    let db = Arc::new(Mutex::new(db));
    let scanned = scan(&ctx, &[], &Job::new(TaskKind::StartupManagement)).unwrap();
    let (plan, _) = prepare(
        &scanned,
        &scanned.items[0].row.id,
        Action::Pause,
        &ctx,
        &[],
        &Job::new(TaskKind::StartupManagement),
    )
    .unwrap();
    let paused = run(
        db.clone(),
        plan,
        uuid::Uuid::new_v4().to_string(),
        || Ok(ctx.clone()),
        Arc::new(Job::new(TaskKind::StartupManagement)),
    )
    .unwrap();
    assert_eq!(paused.state, "completed", "{}", paused.reason);
    assert!(!file.exists());
    assert!(runtime::query(&label).unwrap().is_none());
    assert_eq!(std::fs::read(&paused.target).unwrap(), original);
    let records = journal::list(&*db.lock().unwrap()).unwrap();
    let scanned = scan(&ctx, &records, &Job::new(TaskKind::StartupManagement)).unwrap();
    assert!(scanned.items[0].row.eligible);
    std::fs::write(&file, "owned target collision").unwrap();
    assert!(prepare(
        &scanned,
        &scanned.items[0].row.id,
        Action::Resume,
        &ctx,
        &records,
        &Job::new(TaskKind::StartupManagement)
    )
    .is_err());
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "owned target collision"
    );
    assert!(runtime::query(&label).unwrap().is_none());
    std::fs::remove_file(&file).unwrap();
    let (plan, _) = prepare(
        &scanned,
        &scanned.items[0].row.id,
        Action::Resume,
        &ctx,
        &records,
        &Job::new(TaskKind::StartupManagement),
    )
    .unwrap();
    let resumed = run(
        db.clone(),
        plan,
        uuid::Uuid::new_v4().to_string(),
        || Ok(ctx.clone()),
        Arc::new(Job::new(TaskKind::StartupManagement)),
    )
    .unwrap();
    assert_eq!(resumed.state, "completed", "{}", resumed.reason);
    assert_eq!(std::fs::read(&file).unwrap(), original);
    assert!(runtime::query(&label).unwrap().is_some());
    let records = journal::list(&*db.lock().unwrap()).unwrap();
    let scanned = scan(&ctx, &records, &Job::new(TaskKind::StartupManagement)).unwrap();
    let (p, _) = prepare(
        &scanned,
        &scanned.items[0].row.id,
        Action::Pause,
        &ctx,
        &records,
        &Job::new(TaskKind::StartupManagement),
    )
    .unwrap();
    let cancel = Arc::new(Job::new(TaskKind::StartupManagement));
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let partial = run(
        db.clone(),
        p,
        uuid::Uuid::new_v4().to_string(),
        || {
            if calls.fetch_add(1, Ordering::SeqCst) == 2 {
                cancel.cancel();
            }
            Ok(ctx.clone())
        },
        cancel.clone(),
    )
    .unwrap();
    assert_eq!(partial.state, "partial");
    assert!(partial.configuration_changed);
    assert!(runtime::query(&label).unwrap().is_some());
    journal::acknowledge(&*db.lock().unwrap(), &partial.id).unwrap();
    let records = journal::list(&*db.lock().unwrap()).unwrap();
    let scanned = scan(&ctx, &records, &Job::new(TaskKind::StartupManagement)).unwrap();
    let (p, _) = prepare(
        &scanned,
        &scanned.items[0].row.id,
        Action::Resume,
        &ctx,
        &records,
        &Job::new(TaskKind::StartupManagement),
    )
    .unwrap();
    let recovered = run(
        db.clone(),
        p,
        uuid::Uuid::new_v4().to_string(),
        || Ok(ctx.clone()),
        Arc::new(Job::new(TaskKind::StartupManagement)),
    )
    .unwrap();
    assert_eq!(recovered.state, "completed");
    assert_eq!(std::fs::read(&file).unwrap(), original);
    drop(db);
    let reopened = db::open_owned(&ctx.home.join("database.sqlite")).unwrap();
    assert_eq!(journal::list(&reopened).unwrap().len(), 4);
    drop(_owned);
    assert!(runtime::query(&label).unwrap().is_none());
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/startup-native.json",serde_json::to_vec_pretty(&serde_json::json!({"paused":paused,"resumed":resumed,"cancelledAfterConfigMove":partial,"recovered":recovered,"ownedServiceRemoved":true,"originalConfigPreserved":true,"reopenVerified":true})).unwrap()).unwrap();
}
