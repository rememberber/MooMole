//! Explicit native bridge acceptance, restricted to owned system-temp files.
//! No account cleanup traversal and no real administrator password are used.
use super::{bridge, journal, plan::Plan, terminal, Controller};
use crate::{
    db,
    engine::{self, Job, TaskKind},
};
use std::{
    fs,
    os::unix::net::UnixListener,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc, Mutex},
    time::Duration,
};
fn setup(
    system: bool,
) -> (
    tempfile::TempDir,
    Arc<Mutex<db::Database>>,
    Plan,
    PathBuf,
    PathBuf,
) {
    let t = tempfile::Builder::new().prefix("mcn-").tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let worker = std::env::var_os("MOOMOLE_NATIVE_EVENT_WORKER")
        .expect("explicit native worker build required");
    assert!(Path::new(&worker).is_absolute());
    let event = root.join("event-worker");
    fs::copy(&worker, &event).unwrap();
    fs::copy(&worker, root.join("moomole-clean-askpass")).unwrap();
    let database = Arc::new(Mutex::new(
        db::open_owned(&root.join("state.sqlite")).unwrap(),
    ));
    let now = db::now();
    let plan = Plan {
        version: 1,
        id: uuid::Uuid::new_v4().to_string(),
        revision: engine::REVISION.into(),
        engine_version: "1.65.0".into(),
        home: root.to_str().unwrap().into(),
        system,
        method: "molePermanent".into(),
        protected_paths: vec![],
        created_at: now,
        expires_at: now + 120_000,
        scope: vec![],
    };
    (t, database, plan, event, root)
}
fn command(root: &Path, event: &Path, socket: &Path, body: &str) -> tokio::process::Command {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut command = engine::command(Path::new("/bin/bash"));
    command
        .args([
            "-c",
            &format!(
                r#"
source "$MOOMOLE_NATIVE_ENGINE/bin/clean.sh"
source "$MOOMOLE_NATIVE_ADAPTER"
MOOMOLE_PROTECTED_PATHS=()
export MOOMOLE_EVENT_WORKER="$MOOMOLE_NATIVE_WORKER" MOOMOLE_EVENT_SOCKET="$MOOMOLE_NATIVE_SOCKET"
moomole_install_hooks
{body}
"#
            ),
        ])
        .env("MOOMOLE_NATIVE_ENGINE", manifest.join("resources/mole"))
        .env(
            "MOOMOLE_NATIVE_ADAPTER",
            manifest.join("../scripts/mole-clean-run.sh"),
        )
        .env("MOOMOLE_NATIVE_WORKER", event)
        .env("MOOMOLE_NATIVE_SOCKET", socket)
        .env("MOOMOLE_NATIVE_ROOT", root)
        .env("MOLE_TEST_MODE", "0")
        .env("MOLE_DRY_RUN", "0");
    command
}
#[tokio::test]
#[ignore = "requires explicit MOOMOLE_NATIVE_EVENT_WORKER from this source build"]
async fn native_bridge_deletes_owned_samples_and_protection_or_lost_ack_keeps_them() {
    let (_t, db, mut plan, event, root) = setup(false);
    let target = root.join("owned.txt");
    fs::write(&target, b"owned removable").unwrap();
    let protected = root.join("protected.txt");
    fs::write(&protected, b"owned protected").unwrap();
    plan.protected_paths
        .push(protected.to_str().unwrap().into());
    let id = uuid::Uuid::new_v4().to_string();
    journal::create(&mut db.lock().unwrap(), &id, &plan, None).unwrap();
    let socket = root.join("events.sock");
    let job = Arc::new(Job::new(TaskKind::MoleClean));
    let mut server = bridge::Server::start(
        UnixListener::bind(&socket).unwrap(),
        db.clone(),
        id.clone(),
        plan,
        job.clone(),
        Arc::new(Controller::default()),
        || Ok(()),
    )
    .unwrap();
    let c = command(
        &root,
        &event,
        &socket,
        r#"
owned_deny() { return 1; }
_MOLE_SAFE_REMOVE_FINAL_GUARD=owned_deny
safe_remove "$MOOMOLE_NATIVE_ROOT/owned.txt" true && exit 32
unset _MOLE_SAFE_REMOVE_FINAL_GUARD
safe_remove "$MOOMOLE_NATIVE_ROOT/owned.txt" true
safe_remove "$MOOMOLE_NATIVE_ROOT/protected.txt" true || :
moomole_emit complete '' 0 > /dev/null
"#,
    );
    crate::process::output(c, job, Duration::from_secs(10))
        .await
        .unwrap();
    server.close();
    assert!(server.completion.load(Ordering::SeqCst));
    assert!(!server.incomplete.load(Ordering::SeqCst));
    assert!(!target.exists());
    assert_eq!(fs::read(&protected).unwrap(), b"owned protected");
    let record = journal::read(&db.lock().unwrap(), &id).unwrap().unwrap();
    assert_eq!(
        record.events.iter().filter(|e| e.kind == "intent").count(),
        2
    );
    assert!(record
        .events
        .iter()
        .any(|e| e.kind == "removed" && e.path == target.to_str().unwrap()));
    assert!(record
        .events
        .iter()
        .any(|e| e.kind == "retained" && e.path == protected.to_str().unwrap()));
}
#[tokio::test]
#[ignore = "requires explicit MOOMOLE_NATIVE_EVENT_WORKER from this source build"]
async fn native_bridge_cancellation_keeps_pending_intent_and_never_replays_it() {
    let (_t, db, plan, event, root) = setup(false);
    let target = root.join("owned.txt");
    fs::write(&target, b"owned retained after cancellation").unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    journal::create(&mut db.lock().unwrap(), &id, &plan, None).unwrap();
    let socket = root.join("events.sock");
    let job = Arc::new(Job::new(TaskKind::MoleClean));
    let mut server = bridge::Server::start(
        UnixListener::bind(&socket).unwrap(),
        db.clone(),
        id.clone(),
        plan,
        job.clone(),
        Arc::new(Controller::default()),
        || Ok(()),
    )
    .unwrap();
    let c = command(
        &root,
        &event,
        &socket,
        r#"
owned_wait() { while [[ ! -e "$MOOMOLE_NATIVE_ROOT/release" ]]; do /bin/sleep 0.02; done; }
_MOLE_SAFE_REMOVE_FINAL_GUARD=owned_wait
safe_remove "$MOOMOLE_NATIVE_ROOT/owned.txt" true
moomole_emit complete '' 0 > /dev/null
"#,
    );
    let j = job.clone();
    let work =
        tokio::spawn(async move { crate::process::output(c, j, Duration::from_secs(10)).await });
    let start = std::time::Instant::now();
    loop {
        if journal::read(&db.lock().unwrap(), &id)
            .unwrap()
            .unwrap()
            .events
            .iter()
            .any(|e| e.kind == "intent")
        {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(6));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    job.cancel();
    assert!(work.await.unwrap().is_err());
    server.close();
    assert!(server.incomplete.load(Ordering::SeqCst));
    assert!(!server.completion.load(Ordering::SeqCst));
    assert_eq!(
        fs::read(&target).unwrap(),
        b"owned retained after cancellation"
    );
    journal::recover(&db.lock().unwrap()).unwrap();
    let record = journal::read(&db.lock().unwrap(), &id).unwrap().unwrap();
    assert_eq!(record.state, "interrupted");
    assert!(record.events.iter().all(|e| e.kind != "removed"));
}
#[tokio::test]
#[ignore = "requires explicit MOOMOLE_NATIVE_EVENT_WORKER from this source build"]
async fn native_askpass_is_one_private_terminal_input_and_never_an_event_or_application_output() {
    let (_t, db, plan, event, root) = setup(true);
    let id = uuid::Uuid::new_v4().to_string();
    journal::create(&mut db.lock().unwrap(), &id, &plan, None).unwrap();
    let socket = root.join("events.sock");
    let job = Arc::new(Job::new(TaskKind::MoleClean));
    let controller = Arc::new(Controller::default());
    let mut server = bridge::Server::start(
        UnixListener::bind(&socket).unwrap(),
        db.clone(),
        id.clone(),
        plan,
        job.clone(),
        controller.clone(),
        || Ok(()),
    )
    .unwrap();
    let (mut master, slave) = terminal::open().unwrap();
    let c = command(
        &root,
        &event,
        &socket,
        r#"
moomole_emit authorization '' begin > /dev/null
value=$("${MOOMOLE_EVENT_WORKER%/*}/moomole-clean-askpass")
[[ "$value" == owned-fixture-input ]] || exit 31
unset value
moomole_emit authorization '' finished > /dev/null
moomole_emit complete '' 0 > /dev/null
printf verified
"#,
    );
    let work = tokio::spawn(async move {
        crate::process::output_with_terminal(c, job, Duration::from_secs(10), slave).await
    });
    let start = std::time::Instant::now();
    while !controller.authorization.load(Ordering::SeqCst) {
        assert!(start.elapsed() < Duration::from_secs(6));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    terminal::submit(&mut master, "owned-fixture-input".into()).unwrap();
    let output = work.await.unwrap().unwrap();
    {
        use std::io::{Read, Write};
        let mut stream = std::os::unix::net::UnixStream::connect(&socket).unwrap();
        stream.write_all(b"authorization\0\0required\0\0").unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        let mut ack = String::new();
        stream.read_to_string(&mut ack).unwrap();
        assert_eq!(ack, "refused");
    }
    server.close();
    assert_eq!(output, b"verified");
    assert!(!controller.authorization.load(Ordering::SeqCst));
    let record = journal::read(&db.lock().unwrap(), &id).unwrap().unwrap();
    assert!(!serde_json::to_string(&record)
        .unwrap()
        .contains("owned-fixture-input"));
    assert!(server.completion.load(Ordering::SeqCst));
}
