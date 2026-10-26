use super::*;
use crate::{
    app_inspection::identity,
    db, execution,
    uninstall_plan::{Context, Source},
};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

const ID: &str = "io.moomole.journal.fixture";
struct Fixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    db: Connection,
    plan: Plan,
    ctx: Context,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let ctx = Context {
            home: home.clone(),
            engine_root: Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
            current_app: None,
            current_id: "io.github.rememberber.moomole".into(),
            protected_paths: vec![],
        };
        let plan = app_plan(&home, "Fixture.app", &ctx);
        let db = db::open_fixture(&home.join("app.sqlite")).unwrap();
        Self {
            _temp: temp,
            home,
            db,
            plan,
            ctx,
        }
    }
    fn start(&mut self) -> Session {
        begin(&mut self.db, &self.plan, db::now()).unwrap()
    }
    fn another(&self, name: &str) -> Plan {
        app_plan(&self.home, name, &self.ctx)
    }
    fn private_move(&self) -> Receipt {
        let (path, snapshot, _, _) = self.plan.bundle_binding();
        let to = self.home.join("private-recovery.app");
        fs::rename(path, &to).unwrap();
        let after = identity::capture(&to, ID).unwrap();
        assert_eq!(after.inventory, snapshot.inventory);
        let node = after.root.nodes.last().unwrap();
        Receipt {
            version: 1,
            actual: execution::Receipt {
                destination: to.to_str().unwrap().into(),
                device: node.dev,
                inode: node.ino,
            },
            observed_inventory: Some(after.inventory),
        }
    }
}
fn app_plan(home: &Path, name: &str, ctx: &Context) -> Plan {
    let path = identity::fixture(home, name, ID);
    let row = serde_json::json!({"path":path,"bundle_id":ID,"name":name,"source":"App"});
    Plan::prepare(
        Source::from_row(&uuid::Uuid::new_v4().to_string(), 0, &row).unwrap(),
        ctx,
    )
    .unwrap()
}
fn cleanup_plan(home: &Path) -> crate::review::Plan {
    use crate::{
        clean_preview::{Candidate, Preview, PROTOCOL},
        review,
    };
    let path = home.join(format!("cleanup-{}.log", uuid::Uuid::new_v4()));
    fs::write(&path, "owned cleanup fixture").unwrap();
    let meta = path.metadata().unwrap();
    let at = db::now();
    let scan = review::capture(
        uuid::Uuid::new_v4().to_string(),
        Preview {
            output: String::new(),
            dry_run: true,
            protocol: PROTOCOL,
            revision: crate::engine::REVISION.into(),
            system_included: false,
            items: vec![Candidate {
                id: String::new(),
                upstream_identity: format!("inode:{}:{}", meta.dev(), meta.ino()),
                path: path.to_str().unwrap().into(),
                section: "owned fixture".into(),
                size: Some(meta.len()),
                item_count: 1,
                covered_by: None,
                is_directory: false,
                resolved_path: None,
                impact: None,
                provenance: None,
            }],
        },
        at,
    )
    .unwrap();
    review::create(&scan, &[scan.items[0].candidate.id.clone()], &[], at).unwrap()
}

#[test]
fn archived_package_move_survives_restart_pruning_and_ephemeral_plan_loss() {
    let mut f = Fixture::new();
    let session = f.start();
    assert!(check_intent(&f.db, &session, db::now()).is_err());
    intent(&mut f.db, &session, db::now()).unwrap();
    check_intent(&f.db, &session, db::now()).unwrap();
    let r = f.private_move();
    complete(
        &mut f.db,
        &session,
        Outcome::Moved(r.clone()),
        "owned package metadata verified",
        db::now(),
    )
    .unwrap();
    // Read-only old records remain readable well after plan expiry. Nothing
    // recreates Plan/Session from a stored archive, and no operation is replayed.
    assert!(f.plan.archive().live_at(f.plan.bundle_binding().3).is_err());
    f.db.execute("DELETE FROM clean_scans", []).unwrap();
    db::save_settings(
        &f.db,
        &db::Settings {
            retention_days: 7,
            ..db::Settings::default()
        },
    )
    .unwrap();
    prune(&f.db, i64::MAX).unwrap();
    f.db = db::open_fixture(&f.home.join("app.sqlite")).unwrap();
    let d = detail(&f.db, session.id()).unwrap();
    assert_eq!(d.summary.method, "appBundleTrash");
    assert_eq!(
        (d.summary.state.as_str(), d.summary.item_state.as_str()),
        ("completed", "moved")
    );
    assert_eq!(d.receipt, Some(r.clone()));
    assert!(d.summary.retained_for_review);
    assert_eq!(
        d.events
            .iter()
            .map(|e| e.state.as_str())
            .collect::<Vec<_>>(),
        ["pending", "intent", "moved"]
    );
    assert!(Path::new(&r.actual.destination)
        .join("Contents/Info.plist")
        .is_file());
    assert!(!Path::new(f.plan.bundle_binding().0).exists());
    assert!(check_intent(&f.db, &session, db::now()).is_err());
    assert!(execution::list(&f.db).unwrap().is_empty());
    assert_eq!(list(&f.db).unwrap().len(), 1);
}

#[test]
fn startup_distinguishes_unstarted_from_unresolved_intent_without_touching_targets() {
    for had_intent in [false, true] {
        let mut f = Fixture::new();
        let session = f.start();
        if had_intent {
            intent(&mut f.db, &session, db::now()).unwrap();
        }
        let before = identity::capture(Path::new(f.plan.bundle_binding().0), ID).unwrap();
        f.db = db::open_fixture(&f.home.join("app.sqlite")).unwrap();
        let first = serde_json::to_value(detail(&f.db, session.id()).unwrap()).unwrap();
        assert_eq!(first["state"], "interrupted");
        assert_eq!(
            first["itemState"],
            if had_intent {
                "uncertain"
            } else {
                "notStarted"
            }
        );
        assert_eq!(
            before,
            identity::capture(Path::new(f.plan.bundle_binding().0), ID).unwrap()
        );
        f.db = db::open_fixture(&f.home.join("app.sqlite")).unwrap();
        assert_eq!(
            first,
            serde_json::to_value(detail(&f.db, session.id()).unwrap()).unwrap()
        );
        assert!(intent(&mut f.db, &session, db::now()).is_err());
        prune(&f.db, i64::MAX).unwrap();
        assert_eq!(list(&f.db).unwrap().len(), 1);
        let next = f.another("Next.app");
        assert_eq!(begin(&mut f.db, &next, db::now()).is_err(), had_intent);
    }
}

#[test]
fn worker_stop_does_not_rewrite_completed_results_and_clock_rollback_keeps_event_order() {
    for phase in ["pending", "intent", "moved"] {
        let mut f = Fixture::new();
        let session = f.start();
        if phase != "pending" {
            intent(&mut f.db, &session, db::now()).unwrap();
        }
        if phase == "moved" {
            let r = f.private_move();
            complete(
                &mut f.db,
                &session,
                Outcome::Moved(r),
                "own completed package",
                f.plan.bundle_binding().2 - 1,
            )
            .unwrap();
        }
        let prior = serde_json::to_value(detail(&f.db, session.id()).unwrap()).unwrap();
        let stopped = stop(&mut f.db, &session, db::now());
        let d = detail(&f.db, session.id()).unwrap();
        if phase == "moved" {
            assert!(stopped.is_err());
            assert!(cancel(&mut f.db, &session, db::now()).is_err());
            assert_eq!(serde_json::to_value(&d).unwrap(), prior);
            assert!(d.events.last().unwrap().created_at < d.events[0].created_at);
            assert_eq!(
                d.events
                    .iter()
                    .map(|e| e.state.as_str())
                    .collect::<Vec<_>>(),
                ["pending", "intent", "moved"]
            );
        } else {
            stopped.unwrap();
            assert_eq!(
                d.summary.item_state,
                if phase == "pending" {
                    "notStarted"
                } else {
                    "uncertain"
                }
            );
            assert!(d.receipt.is_none());
            assert!(Path::new(f.plan.bundle_binding().0).is_dir());
        }
    }
}

#[test]
fn cancellation_and_pre_call_refusals_preserve_known_no_api_boundary() {
    let mut f = Fixture::new();
    let cancelled = f.start();
    cancel(&mut f.db, &cancelled, db::now()).unwrap();
    assert!(begin(&mut f.db, &f.plan, db::now()).is_err());
    assert!(intent(&mut f.db, &cancelled, db::now()).is_err());
    assert_eq!(
        detail(&f.db, cancelled.id()).unwrap().summary.item_state,
        "cancelled"
    );

    let next = f.another("Refused.app");
    let prepared = crate::uninstall_target::PreparedBundle::prepare(&next, &f.ctx).unwrap();
    let session = begin(&mut f.db, &next, db::now()).unwrap();
    intent(&mut f.db, &session, db::now()).unwrap();
    assert!(cancel(&mut f.db, &session, db::now()).is_err());
    let rejected =
        prepared.move_to_trash_checked(|| Err("fixture cancellation before native API".into()));
    assert!(rejected.is_err());
    refused_before_call(&mut f.db, &session, &rejected.unwrap_err(), db::now()).unwrap();
    let d = detail(&f.db, session.id()).unwrap();
    assert_eq!(
        (d.summary.state.as_str(), d.summary.item_state.as_str()),
        ("noChanges", "failed")
    );
    assert!(d.receipt.is_none());
    assert!(Path::new(next.bundle_binding().0).is_dir());
    assert!(cancel(&mut f.db, &session, db::now()).is_err());
    prune(&f.db, i64::MAX).unwrap();
    assert!(list(&f.db).unwrap().is_empty());
}

#[test]
fn outcome_transition_receipt_and_expiry_checks_cannot_turn_unknown_into_success() {
    let mut f = Fixture::new();
    let session = f.start();
    assert!(complete(
        &mut f.db,
        &session,
        Outcome::Uncertain(None),
        "without intent",
        db::now()
    )
    .is_err());
    assert!(intent(&mut f.db, &session, f.plan.bundle_binding().3).is_err());
    assert_eq!(
        detail(&f.db, session.id()).unwrap().summary.item_state,
        "pending"
    );
    intent(&mut f.db, &session, db::now()).unwrap();
    assert!(intent(&mut f.db, &session, db::now()).is_err());
    assert!(skip(&mut f.db, &session, "too late", db::now()).is_err());
    let root = f.plan.bundle_binding().1.root.nodes.last().unwrap();
    let mut r = Receipt {
        version: 1,
        actual: execution::Receipt {
            destination: f
                .home
                .join("not-guessed-trash.app")
                .to_str()
                .unwrap()
                .into(),
            device: root.dev,
            inode: root.ino + 1,
        },
        observed_inventory: Some(f.plan.bundle_binding().1.inventory.clone()),
    };
    assert!(complete(
        &mut f.db,
        &session,
        Outcome::Moved(r.clone()),
        "wrong identity",
        db::now()
    )
    .is_err());
    r.actual.inode = root.ino;
    r.observed_inventory = None;
    assert!(complete(
        &mut f.db,
        &session,
        Outcome::Moved(r.clone()),
        "missing contents",
        db::now()
    )
    .is_err());
    r.actual.destination = f.plan.bundle_binding().0.into();
    assert!(complete(
        &mut f.db,
        &session,
        Outcome::Uncertain(Some(r)),
        "source is not a destination",
        db::now()
    )
    .is_err());
    // A native result arriving after plan expiry still must be recorded. It
    // cannot be discarded as a pre-call refusal or become retry authority.
    complete(
        &mut f.db,
        &session,
        Outcome::Uncertain(None),
        "native evidence unavailable",
        f.plan.bundle_binding().3 + 1,
    )
    .unwrap();
    let d = detail(&f.db, session.id()).unwrap();
    assert_eq!(d.summary.item_state, "uncertain");
    assert!(d.receipt.is_none());
    assert!(complete(
        &mut f.db,
        &session,
        Outcome::Uncertain(None),
        "duplicate",
        db::now()
    )
    .is_err());
    assert!(begin(&mut f.db, &f.plan, db::now()).is_err());
}

#[test]
fn failed_transactions_do_not_acknowledge_intent_or_lose_post_operation_uncertainty() {
    let mut f = Fixture::new();
    let session = f.start();
    f.db.execute_batch("CREATE TRIGGER deny_intent BEFORE INSERT ON uninstall_events WHEN NEW.state='intent' BEGIN SELECT RAISE(ABORT,'fixture intent failure'); END;").unwrap();
    assert!(intent(&mut f.db, &session, db::now()).is_err());
    assert_eq!(detail(&f.db, session.id()).unwrap().events.len(), 1);
    assert!(Path::new(f.plan.bundle_binding().0).is_dir());
    f.db.execute_batch("DROP TRIGGER deny_intent;").unwrap();
    f.db.pragma_update(None, "synchronous", "OFF").unwrap();
    assert!(intent(&mut f.db, &session, db::now())
        .unwrap_err()
        .contains("FULL"));
    f.db.pragma_update(None, "synchronous", "FULL").unwrap();
    intent(&mut f.db, &session, db::now()).unwrap();
    let r = f.private_move();
    f.db.execute_batch("CREATE TRIGGER deny_result BEFORE INSERT ON uninstall_events WHEN NEW.state='moved' BEGIN SELECT RAISE(ABORT,'fixture completion failure'); END;").unwrap();
    assert!(complete(
        &mut f.db,
        &session,
        Outcome::Moved(r.clone()),
        "own moved package",
        db::now()
    )
    .is_err());
    assert_eq!(
        detail(&f.db, session.id()).unwrap().summary.item_state,
        "intent"
    );
    f.db = db::open_fixture(&f.home.join("app.sqlite")).unwrap();
    let d = detail(&f.db, session.id()).unwrap();
    assert_eq!(d.summary.item_state, "uncertain");
    assert!(d.receipt.is_none());
    assert!(Path::new(&r.actual.destination).is_dir());
    assert!(!Path::new(f.plan.bundle_binding().0).exists());
    assert!(execution::begin(&mut f.db, &cleanup_plan(&f.home), db::now()).is_err());
}

#[test]
fn cleanup_and_uninstall_share_durable_admission_in_both_directions() {
    let mut f = Fixture::new();
    let clean = cleanup_plan(&f.home);
    let clean_run = execution::begin(&mut f.db, &clean, db::now()).unwrap();
    assert!(begin(&mut f.db, &f.plan, db::now()).is_err());
    execution::finish(&mut f.db, &clean_run, true, db::now()).unwrap();
    let session = f.start();
    assert!(execution::begin(&mut f.db, &cleanup_plan(&f.home), db::now()).is_err());
    intent(&mut f.db, &session, db::now()).unwrap();
    complete(
        &mut f.db,
        &session,
        Outcome::Uncertain(None),
        "own fixture unknown",
        db::now(),
    )
    .unwrap();
    assert!(execution::begin(&mut f.db, &cleanup_plan(&f.home), db::now()).is_err());
    let next = f.another("Other.app");
    assert!(begin(&mut f.db, &next, db::now()).is_err());

    let mut other = Fixture::new();
    let c = cleanup_plan(&other.home);
    let run = execution::begin(&mut other.db, &c, db::now()).unwrap();
    execution::intent(&mut other.db, &run, &c.items[0].candidate.id, db::now()).unwrap();
    execution::complete_item(
        &mut other.db,
        &run,
        &c.items[0].candidate.id,
        execution::Outcome::Uncertain(None),
        "own cleanup unknown",
        db::now(),
    )
    .unwrap();
    assert!(begin(&mut other.db, &other.plan, db::now()).is_err());
}

fn independent(path: &Path) -> Connection {
    let db = Connection::open(path).unwrap();
    db.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
    db.pragma_update(None, "foreign_keys", "ON").unwrap();
    db.pragma_update(None, "synchronous", "FULL").unwrap();
    db
}
#[test]
fn independent_connections_cannot_admit_competing_operation_types() {
    let f = Fixture::new();
    let plan = f.plan.clone();
    let clean = cleanup_plan(&f.home);
    let path = f.home.join("app.sqlite");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let first_path = path.clone();
    let first_barrier = barrier.clone();
    let a = std::thread::spawn(move || {
        let mut db = independent(&first_path);
        first_barrier.wait();
        begin(&mut db, &plan, db::now()).is_ok()
    });
    let b = std::thread::spawn(move || {
        let mut db = independent(&path);
        barrier.wait();
        execution::begin(&mut db, &clean, db::now()).is_ok()
    });
    assert_eq!(u8::from(a.join().unwrap()) + u8::from(b.join().unwrap()), 1);
    let count:i64=f.db.query_row("SELECT (SELECT count(*) FROM uninstall_runs WHERE state='running')+(SELECT count(*) FROM execution_runs WHERE state='running')",[],|r|r.get(0)).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn snapshots_and_event_receipts_are_checked_without_rebuilding_live_plans() {
    let mut f = Fixture::new();
    let archive = f.plan.archive();
    let original = serde_json::to_value(&archive).unwrap();
    let mut older = original.clone();
    older["revision"] = "older-fixed-engine".into();
    let src: Source = serde_json::from_value(older["source"].clone()).unwrap();
    let snap: identity::Snapshot = serde_json::from_value(older["snapshot"].clone()).unwrap();
    older["digest"] = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&("older-fixed-engine", src, snap)).unwrap())
    )
    .into();
    let old: Archive = serde_json::from_value(older).unwrap();
    old.validate().unwrap();
    assert!(old.live_at(db::now()).is_err());
    for field in ["scope", "relatedData", "operation"] {
        let mut changed = original.clone();
        changed[field] = "unexpected".into();
        assert!(serde_json::from_value::<Archive>(changed)
            .unwrap()
            .validate()
            .is_err());
    }
    let session = f.start();
    let body: String =
        f.db.query_row(
            "SELECT snapshot FROM uninstall_runs WHERE id=?1",
            [session.id()],
            |r| r.get(0),
        )
        .unwrap();
    f.db.execute(
        "UPDATE uninstall_runs SET snapshot=snapshot || ' ' WHERE id=?1",
        [session.id()],
    )
    .unwrap();
    assert!(detail(&f.db, session.id()).is_err());
    assert!(intent(&mut f.db, &session, db::now()).is_err());
    f.db.execute(
        "UPDATE uninstall_runs SET snapshot=?2 WHERE id=?1",
        params![session.id(), body],
    )
    .unwrap();
    intent(&mut f.db, &session, db::now()).unwrap();
    let r = f.private_move();
    complete(
        &mut f.db,
        &session,
        Outcome::Moved(r),
        "verified owned fixture",
        db::now(),
    )
    .unwrap();
    f.db.execute(
        "UPDATE uninstall_events SET reason='mismatched event' WHERE run_id=?1 AND state='moved'",
        [session.id()],
    )
    .unwrap();
    assert!(detail(&f.db, session.id()).is_err());
    assert!(list(&f.db).is_err());
    assert!(Path::new(&f.home.join("private-recovery.app")).is_dir());
}

#[test]
fn shared_capacity_blocks_new_records_without_discarding_existing_evidence() {
    let mut f = Fixture::new();
    let c = cleanup_plan(&f.home);
    let run = execution::begin(&mut f.db, &c, db::now()).unwrap();
    execution::finish(&mut f.db, &run, true, db::now()).unwrap();
    let first = f.start();
    cancel(&mut f.db, &first, db::now()).unwrap();
    for _ in 0..62 {
        let mut archive = serde_json::to_value(f.plan.archive()).unwrap();
        let plan_id = uuid::Uuid::new_v4().to_string();
        archive["planId"] = plan_id.clone().into();
        let body = serde_json::to_string(&archive).unwrap();
        let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
        f.db.execute("INSERT INTO uninstall_runs SELECT ?1,?2,method,state,item_state,created_at,finished_at,?3,?4,?5,reason,receipt FROM uninstall_runs WHERE id=?6",params![uuid::Uuid::new_v4().to_string(),plan_id,body,digest,(body.len()+RESERVE)as i64,first.id()]).unwrap();
    }
    let next = f.another("Capacity.app");
    assert!(begin(&mut f.db, &next, db::now())
        .unwrap_err()
        .contains("容量"));
    assert!(
        execution::begin(&mut f.db, &cleanup_plan(&f.home), db::now())
            .unwrap_err()
            .contains("容量")
    );
    assert_eq!(
        f.db.query_row::<i64, _, _>("SELECT count(*) FROM uninstall_runs", [], |r| r.get(0))
            .unwrap(),
        63
    );
}

#[test]
fn v3_wal_migration_backs_up_before_reconciling_old_cleanup_intent() {
    let mut f = Fixture::new();
    let c = cleanup_plan(&f.home);
    let run = execution::begin(&mut f.db, &c, db::now()).unwrap();
    execution::intent(&mut f.db, &run, &c.items[0].candidate.id, db::now()).unwrap();
    f.db.execute_batch(
        "DROP TABLE execution_reviews; DROP TABLE uninstall_reviews; DROP TABLE maintenance_reviews; DROP TABLE maintenance_events; DROP TABLE maintenance_steps; DROP TABLE maintenance_runs; DROP TABLE uninstall_events; DROP TABLE uninstall_runs; PRAGMA user_version=3;",
    )
    .unwrap();
    let upgraded = db::open_fixture(&f.home.join("app.sqlite")).unwrap();
    assert_eq!(
        upgraded
            .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        7
    );
    assert_eq!(
        execution::detail(&upgraded, &run).unwrap().items[0].state,
        "uncertain"
    );
    assert!(list(&upgraded).unwrap().is_empty());
    let backup = fs::read_dir(&f.home)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("moomole-before-v7-")
        })
        .unwrap();
    let before = Connection::open(&backup).unwrap();
    assert_eq!(
        before
            .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        3
    );
    assert_eq!(
        execution::detail(&before, &run).unwrap().items[0].state,
        "intent"
    );
    assert!(before.prepare("SELECT * FROM uninstall_runs").is_err());
    assert!(Path::new(&c.items[0].candidate.path).is_file());
    drop(upgraded);
    drop(db::open_fixture(&f.home.join("app.sqlite")).unwrap());
    assert_eq!(
        fs::read_dir(&f.home)
            .unwrap()
            .filter(|e| e
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("moomole-before-v7-"))
            .count(),
        1
    );
}

#[test]
fn v3_schema_conflict_rolls_back_entire_upgrade_and_preserves_old_version() {
    let f = Fixture::new();
    f.db.execute_batch("DROP TABLE execution_reviews; DROP TABLE uninstall_reviews; DROP TABLE maintenance_reviews; DROP TABLE maintenance_events; DROP TABLE maintenance_steps; DROP TABLE maintenance_runs; DROP TABLE uninstall_events; DROP TABLE uninstall_runs; CREATE TABLE uninstall_events(sentinel TEXT); INSERT INTO uninstall_events VALUES ('keep'); PRAGMA user_version=3;").unwrap();
    assert!(db::open_fixture(&f.home.join("app.sqlite")).is_err());
    assert_eq!(
        f.db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        3
    );
    assert_eq!(
        f.db.query_row::<String, _, _>("SELECT sentinel FROM uninstall_events", [], |r| r.get(0))
            .unwrap(),
        "keep"
    );
    assert!(f.db.prepare("SELECT * FROM uninstall_runs").is_err());
    assert!(Path::new(f.plan.bundle_binding().0).is_dir());
}

#[test]
fn v4_wal_backup_keeps_settings_and_uninstall_intent_before_maintenance_upgrade() {
    let mut f = Fixture::new();
    let run = f.start();
    intent(&mut f.db, &run, db::now()).unwrap();
    let settings = db::Settings {
        theme: "dark".into(),
        acknowledged: true,
        protected_paths: vec!["/fixture/protected".into()],
        ..db::Settings::default()
    };
    db::save_settings(&f.db, &settings).unwrap();
    f.db.execute_batch("DROP TABLE execution_reviews; DROP TABLE uninstall_reviews; DROP TABLE maintenance_reviews; DROP TABLE maintenance_events; DROP TABLE maintenance_steps; DROP TABLE maintenance_runs; PRAGMA user_version=4;").unwrap();
    let upgraded = db::open_fixture(&f.home.join("app.sqlite")).unwrap();
    assert_eq!(
        upgraded
            .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        7
    );
    assert_eq!(
        detail(&upgraded, run.id()).unwrap().summary.item_state,
        "uncertain"
    );
    assert!(crate::maintenance::refresh_journal::list(&upgraded)
        .unwrap()
        .is_empty());
    assert_eq!(
        serde_json::to_value(db::settings(&upgraded).unwrap()).unwrap(),
        serde_json::to_value(&settings).unwrap()
    );
    let backups: Vec<_> = fs::read_dir(&f.home)
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("moomole-before-v7-")
        })
        .collect();
    assert_eq!(backups.len(), 1);
    let backup = Connection::open(&backups[0]).unwrap();
    assert_eq!(
        backup
            .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        4
    );
    assert_eq!(
        detail(&backup, run.id()).unwrap().summary.item_state,
        "intent"
    );
    assert!(backup.prepare("SELECT * FROM maintenance_runs").is_err());
    drop(upgraded);
    drop(db::open_fixture(&f.home.join("app.sqlite")).unwrap());
    assert_eq!(
        fs::read_dir(&f.home)
            .unwrap()
            .filter(|p| p
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("moomole-before-v7-"))
            .count(),
        1
    );
}

#[test]
fn unknown_uninstall_review_is_bound_idempotent_preserves_facts_and_other_module_blocks() {
    use crate::execution_review::{acknowledge, Kind};
    let mut f = Fixture::new();
    let session = f.start();
    let digest = detail(&f.db, session.id()).unwrap().snapshot_sha256;
    assert!(acknowledge(&mut f.db, Kind::Uninstall, session.id(), &digest, db::now()).is_err());
    intent(&mut f.db, &session, db::now()).unwrap();
    reconcile(&mut f.db, db::now()).unwrap();
    let before = serde_json::to_value(detail(&f.db, session.id()).unwrap()).unwrap();
    let at = db::now();
    {
        let t = f.db.transaction().unwrap();
        assert!(crate::execution_admission::reserve(&t, 1).is_err());
    }
    assert!(acknowledge(
        &mut f.db,
        Kind::Uninstall,
        session.id(),
        &"0".repeat(64),
        at
    )
    .is_err());
    acknowledge(&mut f.db, Kind::Uninstall, session.id(), &digest, at).unwrap();
    let mut expected = before.clone();
    expected["reviewedAt"] = serde_json::json!(at);
    assert_eq!(
        serde_json::to_value(detail(&f.db, session.id()).unwrap()).unwrap(),
        expected
    );
    let clean = cleanup_plan(&f.home);
    let clean_id = execution::begin(&mut f.db, &clean, db::now()).unwrap();
    execution::intent(
        &mut f.db,
        &clean_id,
        &clean.items[0].candidate.id,
        db::now(),
    )
    .unwrap();
    execution::reconcile(&mut f.db, db::now()).unwrap();
    acknowledge(&mut f.db, Kind::Uninstall, session.id(), &digest, at + 1).unwrap();
    {
        let t = f.db.transaction().unwrap();
        assert!(crate::execution_admission::reserve(&t, 1).is_err());
    }
    let clean_digest = execution::detail(&f.db, &clean_id).unwrap().snapshot_sha256;
    acknowledge(&mut f.db, Kind::Cleanup, &clean_id, &clean_digest, at + 1).unwrap();
    prune(&f.db, i64::MAX).unwrap();
    f.db = db::open_fixture(&f.home.join("app.sqlite")).unwrap();
    assert_eq!(
        serde_json::to_value(detail(&f.db, session.id()).unwrap()).unwrap(),
        expected
    );
    assert_eq!(list(&f.db).unwrap()[0].reviewed_at, Some(at));
    assert!(Path::new(f.plan.bundle_binding().0).is_dir());
    let t = f.db.transaction().unwrap();
    crate::execution_admission::reserve(&t, 1).unwrap();
}

#[test]
fn mismatched_saved_review_is_not_accepted_or_used_to_release_admission() {
    use crate::execution_review::{acknowledge, Kind};
    let mut f = Fixture::new();
    let session = f.start();
    intent(&mut f.db, &session, db::now()).unwrap();
    reconcile(&mut f.db, db::now()).unwrap();
    let digest = detail(&f.db, session.id()).unwrap().snapshot_sha256;
    f.db.execute(
        "INSERT INTO uninstall_reviews VALUES (?1,?2,?3)",
        params![session.id(), "0".repeat(64), db::now()],
    )
    .unwrap();
    assert!(detail(&f.db, session.id()).is_err());
    assert!(acknowledge(&mut f.db, Kind::Uninstall, session.id(), &digest, db::now()).is_err());
    let t = f.db.transaction().unwrap();
    assert!(crate::execution_admission::reserve(&t, 1).is_err());
}

#[test]
fn v6_migration_preserves_uninstall_unknown_and_backs_up_original_without_review() {
    let mut f = Fixture::new();
    let session = f.start();
    intent(&mut f.db, &session, db::now()).unwrap();
    reconcile(&mut f.db, db::now()).unwrap();
    let before = serde_json::to_value(detail(&f.db, session.id()).unwrap()).unwrap();
    f.db.execute_batch(
        "DROP TABLE execution_reviews; DROP TABLE uninstall_reviews; PRAGMA user_version=6;",
    )
    .unwrap();
    f.db = db::open_fixture(&f.home.join("app.sqlite")).unwrap();
    assert_eq!(
        f.db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        7
    );
    assert_eq!(
        serde_json::to_value(detail(&f.db, session.id()).unwrap()).unwrap(),
        before
    );
    let backups = fs::read_dir(&f.home)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("moomole-before-v7-")
        })
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    let old = Connection::open(&backups[0]).unwrap();
    assert_eq!(
        old.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        6
    );
    assert_eq!(
        old.query_row::<String, _, _>(
            "SELECT item_state FROM uninstall_runs WHERE id=?1",
            [session.id()],
            |r| r.get(0)
        )
        .unwrap(),
        "uncertain"
    );
    assert_eq!(old.query_row::<i64,_,_>("SELECT count(*) FROM sqlite_master WHERE name IN ('execution_reviews','uninstall_reviews')",[],|r|r.get(0)).unwrap(),0);
}
