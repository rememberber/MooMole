use super::*;
use crate::{
    clean_preview::{Candidate, Preview, PROTOCOL},
    db, review,
};
use std::os::unix::fs::{MetadataExt, PermissionsExt};

mod crash;

struct Fixture {
    _root: tempfile::TempDir,
    root: std::path::PathBuf,
    db: Connection,
    plan: Plan,
    at: i64,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::create_dir(root.join("source")).unwrap();
        std::fs::create_dir(root.join("recovery")).unwrap();
        let items = ["中文\ncache *", "second", "unselected"]
            .iter()
            .map(|name| {
                let path = root.join("source").join(name);
                std::fs::write(&path, name).unwrap();
                let m = path.metadata().unwrap();
                Candidate {
                    id: String::new(),
                    upstream_identity: format!("inode:{}:{}", m.dev(), m.ino()),
                    path: path.to_str().unwrap().into(),
                    section: "test fixture".into(),
                    size: Some(m.len()),
                    item_count: 1,
                    covered_by: None,
                    is_directory: false,
                    resolved_path: None,
                    impact: None,
                    provenance: None,
                }
            })
            .collect();
        let at = db::now();
        let scan = review::capture(
            uuid::Uuid::new_v4().to_string(),
            Preview {
                output: String::new(),
                dry_run: true,
                protocol: PROTOCOL,
                revision: crate::engine::REVISION.into(),
                system_included: false,
                items,
            },
            at,
        )
        .unwrap();
        let plan = review::create(
            &scan,
            &[
                scan.items[0].candidate.id.clone(),
                scan.items[1].candidate.id.clone(),
            ],
            &[],
            at,
        )
        .unwrap();
        let db = db::open_fixture(&root.join("app.sqlite")).unwrap();
        db::save_scan(&db, &scan).unwrap();
        db::save_plan(&db, &plan).unwrap();
        Self {
            _root: temp,
            root,
            db,
            plan,
            at,
        }
    }
    fn begin(&mut self) -> String {
        begin(&mut self.db, &self.plan, self.at).unwrap()
    }
    fn item(&self, i: usize) -> String {
        self.plan.items[i].candidate.id.clone()
    }
    fn move_fixture(&mut self, run: &str, i: usize) {
        let item = &self.plan.items[i].candidate;
        let id = item.id.clone();
        let source = std::path::PathBuf::from(&item.path);
        let target = self.root.join("recovery").join(source.file_name().unwrap());
        let before = source.metadata().unwrap();
        // This driver only moves its own private fixtures. Not a real executor.
        intent(&mut self.db, run, &id, self.at + 1).unwrap();
        std::fs::rename(&source, &target).unwrap();
        let after = target.metadata().unwrap();
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        complete_item(
            &mut self.db,
            run,
            &id,
            Outcome::Moved(Receipt {
                destination: target.to_str().unwrap().into(),
                device: after.dev(),
                inode: after.ino(),
            }),
            "fixture identity verified",
            self.at + 2,
        )
        .unwrap();
    }
}

#[test]
fn selected_fixture_moves_persist_with_receipts_after_plan_pruning_and_restart() {
    let mut f = Fixture::new();
    let run = f.begin();
    f.move_fixture(&run, 0);
    skip(
        &mut f.db,
        &run,
        &f.plan.items[1].candidate.id,
        "fixture rule denies second",
        f.at + 3,
    )
    .unwrap();
    finish(&mut f.db, &run, false, f.at + 4).unwrap();
    f.db.execute("DELETE FROM clean_scans", []).unwrap();
    assert!(db::plans(&f.db).unwrap().is_empty());
    // Even a very old successful move retains its evidence beyond normal history.
    prune(&f.db, f.at + 100_000).unwrap();
    f.db = db::open_fixture(&f.root.join("app.sqlite")).unwrap();
    let d = detail(&f.db, &run).unwrap();
    assert_eq!(d.summary.state, "partial");
    assert_eq!(d.summary.moved_count, 1);
    assert_eq!(d.items[0].state, "moved");
    assert_eq!(d.items[1].state, "skipped");
    assert_eq!(d.events.len(), 3);
    assert_eq!(
        std::fs::read_to_string(&d.items[0].receipt.as_ref().unwrap().destination).unwrap(),
        "中文\ncache *"
    );
    assert!(f.root.join("source/unselected").is_file());
    assert!(f.root.join("source/second").is_file());
    assert!(d.summary.retained_for_review);
    // Journal receipts alone do not expose an automatic restore/retry API.
}

#[test]
fn startup_reconciles_committed_intent_and_preserves_completed_receipt_idempotently() {
    let mut f = Fixture::new();
    let run = f.begin();
    f.move_fixture(&run, 0);
    intent(&mut f.db, &run, &f.plan.items[1].candidate.id, f.at + 3).unwrap();
    let source = f.root.join("source/second");
    let destination = f.root.join("recovery/second");
    // Simulate loss after an OS change but before recording its completion.
    std::fs::rename(&source, &destination).unwrap();
    f.db = db::open_fixture(&f.root.join("app.sqlite")).unwrap();
    let first = serde_json::to_value(detail(&f.db, &run).unwrap()).unwrap();
    assert_eq!(first["state"], "interrupted");
    assert_eq!(first["items"][0]["state"], "moved");
    assert_eq!(first["items"][1]["state"], "uncertain");
    assert_eq!(first["uncertainCount"], 1);
    assert!(destination.is_file());
    assert!(!source.exists()); // Reconciliation performs no target IO/replay.
    f.db = db::open_fixture(&f.root.join("app.sqlite")).unwrap();
    assert_eq!(
        serde_json::to_value(detail(&f.db, &run).unwrap()).unwrap(),
        first
    );
    let mut other = f.plan.clone();
    other.id = uuid::Uuid::new_v4().to_string();
    assert!(begin(&mut f.db, &other, f.at + 5)
        .unwrap_err()
        .contains("待核实"));
    assert!(intent(&mut f.db, &run, &f.plan.items[1].candidate.id, f.at + 5).is_err());
}

#[test]
fn pre_intent_crash_does_not_claim_attempt_and_cancel_keeps_completed_items() {
    let mut f = Fixture::new();
    let run = f.begin();
    reconcile(&mut f.db, f.at + 1).unwrap();
    let d = detail(&f.db, &run).unwrap();
    assert_eq!(
        d.items.iter().map(|i| i.state.as_str()).collect::<Vec<_>>(),
        vec!["notStarted", "notStarted"]
    );
    assert_eq!(d.summary.uncertain_count, 0);
    assert!(begin(&mut f.db, &f.plan, f.at + 2).is_err()); // Same immutable plan cannot replay.
    f.plan.id = uuid::Uuid::new_v4().to_string();
    let next = f.begin();
    f.move_fixture(&next, 0);
    finish(&mut f.db, &next, true, f.at + 4).unwrap();
    let d = detail(&f.db, &next).unwrap();
    assert_eq!(d.summary.state, "cancelled");
    assert_eq!(d.items[0].state, "moved");
    assert_eq!(d.items[1].state, "cancelled");
    assert_eq!(
        d.events.iter().filter(|e| e.item_id == f.item(1)).count(),
        1
    );
    let mut all = Fixture::new();
    let completed = all.begin();
    all.move_fixture(&completed, 0);
    all.move_fixture(&completed, 1);
    finish(&mut all.db, &completed, true, all.at + 4).unwrap();
    assert_eq!(
        detail(&all.db, &completed).unwrap().summary.state,
        "completed"
    );
}

#[test]
fn transitions_are_bound_to_run_item_and_committed_intent() {
    let mut f = Fixture::new();
    let run = f.begin();
    let a = f.item(0);
    let b = f.item(1);
    assert!(complete_item(&mut f.db, &run, &a, Outcome::Failed, "error", f.at).is_err());
    assert!(intent(&mut f.db, &run, "foreign", f.at).is_err());
    assert!(finish(&mut f.db, &run, false, f.at).is_err());
    intent(&mut f.db, &run, &a, f.at).unwrap();
    assert!(intent(&mut f.db, &run, &a, f.at).is_err());
    assert!(intent(&mut f.db, &run, &b, f.at).is_err());
    assert!(finish(&mut f.db, &run, true, f.at).is_err());
    assert!(skip(&mut f.db, &run, &b, "deny", f.at).is_err());
    complete_item(
        &mut f.db,
        &run,
        &a,
        Outcome::Failed,
        "known no change",
        f.at + 1,
    )
    .unwrap();
    assert!(complete_item(&mut f.db, &run, &a, Outcome::Failed, "duplicate", f.at).is_err());
    intent(&mut f.db, &run, &b, f.at + 2).unwrap();
    complete_item(
        &mut f.db,
        &run,
        &b,
        Outcome::Uncertain(Some(Receipt {
            destination: "/fixture/observed-uncertain".into(),
            device: u64::MAX,
            inode: 9_007_199_254_740_993,
        })),
        "identity cannot be verified",
        f.at + 3,
    )
    .unwrap();
    let d = detail(&f.db, &run).unwrap();
    assert_eq!(d.summary.state, "interrupted");
    assert_eq!(d.items[0].state, "failed");
    assert_eq!(d.items[1].state, "uncertain");
    let receipt = d.items[1].receipt.as_ref().unwrap();
    assert_eq!(receipt.device, u64::MAX);
    assert_eq!(
        serde_json::to_value(receipt).unwrap()["inode"],
        "9007199254740993"
    );

    assert!(finish(&mut f.db, &run, false, f.at + 4).is_err());
}

#[test]
fn transaction_failure_rolls_back_item_and_event_and_never_allows_fixture_io() {
    let mut f = Fixture::new();
    let run = f.begin();
    let item = f.item(0);
    f.db.execute_batch("CREATE TRIGGER reject_event BEFORE INSERT ON execution_events BEGIN SELECT RAISE(ABORT,'injected write failure'); END;").unwrap();
    let mut attempted = false;
    if intent(&mut f.db, &run, &item, f.at).is_ok() {
        attempted = true;
    }
    assert!(!attempted);
    let d = detail(&f.db, &run).unwrap();
    assert_eq!(d.items[0].state, "pending");
    assert!(d.events.is_empty());
    f.db.execute_batch("DROP TRIGGER reject_event").unwrap();
    intent(&mut f.db, &run, &item, f.at).unwrap();
    f.db.execute_batch("CREATE TRIGGER reject_event BEFORE INSERT ON execution_events BEGIN SELECT RAISE(ABORT,'injected result failure'); END;").unwrap();
    assert!(complete_item(
        &mut f.db,
        &run,
        &item,
        Outcome::Failed,
        "known no change",
        f.at
    )
    .is_err());
    assert_eq!(detail(&f.db, &run).unwrap().items[0].state, "intent");
    f.db.execute_batch("DROP TRIGGER reject_event").unwrap();
    reconcile(&mut f.db, f.at + 1).unwrap();
    assert_eq!(detail(&f.db, &run).unwrap().items[0].state, "uncertain");
    assert_eq!(detail(&f.db, &run).unwrap().items[1].state, "notStarted");
}

#[test]
fn lock_readonly_and_full_database_errors_block_intent() {
    let mut f = Fixture::new();
    let run = f.begin();
    let item = f.item(0);
    let second = Connection::open(f.root.join("app.sqlite")).unwrap();
    f.db.busy_timeout(std::time::Duration::ZERO).unwrap();
    second.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(intent(&mut f.db, &run, &item, f.at).is_err());
    second.execute_batch("ROLLBACK").unwrap();
    f.db.pragma_update(None, "query_only", true).unwrap();
    assert!(intent(&mut f.db, &run, &item, f.at).is_err());
    f.db.pragma_update(None, "query_only", false).unwrap();
    f.db.execute_batch("CREATE TABLE fill_space(value BLOB); CREATE TRIGGER fill_on_intent BEFORE UPDATE ON execution_items BEGIN INSERT INTO fill_space VALUES (zeroblob(1048576)); END;").unwrap();
    let pages: i64 =
        f.db.pragma_query_value(None, "page_count", |r| r.get(0))
            .unwrap();
    f.db.pragma_update(None, "max_page_count", pages).unwrap();
    // Force allocation inside the intent transaction under SQLite's page limit.
    let error = intent(&mut f.db, &run, &item, f.at).unwrap_err();
    assert!(error.contains("full"), "{error}");
    assert_eq!(detail(&f.db, &run).unwrap().items[0].state, "pending");
    assert!(detail(&f.db, &run).unwrap().events.is_empty());
}

#[test]
fn independent_connections_reject_concurrent_runs_and_duplicate_plan() {
    let mut f = Fixture::new();
    let run = f.begin();
    let mut second = Connection::open(f.root.join("app.sqlite")).unwrap();
    second.pragma_update(None, "synchronous", "FULL").unwrap();
    let mut another = f.plan.clone();
    another.id = uuid::Uuid::new_v4().to_string();
    assert!(begin(&mut second, &another, f.at).is_err());
    finish(&mut f.db, &run, true, f.at).unwrap();
    assert!(begin(&mut second, &f.plan, f.at).is_err());
    assert_eq!(list(&second).unwrap().len(), 1);
    assert_eq!(detail(&second, &run).unwrap().items.len(), 2);
}

#[test]
fn invalid_inputs_capacity_retention_and_snapshot_corruption_fail_closed() {
    let mut f = Fixture::new();
    let mut bad = f.plan.clone();
    bad.items.push(bad.items[0].clone());
    assert!(begin(&mut f.db, &bad, f.at).is_err());
    assert!(begin(&mut f.db, &f.plan, f.plan.expires_at).is_err());
    let run = f.begin();
    let a = f.item(0);
    intent(&mut f.db, &run, &a, f.at).unwrap();
    assert!(complete_item(
        &mut f.db,
        &run,
        &a,
        Outcome::Moved(Receipt {
            destination: "relative".into(),
            device: 1,
            inode: 1
        }),
        "bad",
        f.at
    )
    .is_err());
    assert!(complete_item(
        &mut f.db,
        &run,
        &a,
        Outcome::Failed,
        &"a".repeat(MAX_REASON + 1),
        f.at
    )
    .is_err());
    complete_item(&mut f.db, &run, &a, Outcome::Failed, "no change", f.at).unwrap();
    finish(&mut f.db, &run, true, f.at + 1).unwrap();
    f.db.execute(
        "UPDATE execution_runs SET reserved_bytes=?1",
        [MAX_RESERVED],
    )
    .unwrap();
    let mut next = f.plan.clone();
    next.id = uuid::Uuid::new_v4().to_string();
    assert!(begin(&mut f.db, &next, f.at).unwrap_err().contains("容量"));
    f.db.execute("UPDATE execution_runs SET snapshot='{}'", [])
        .unwrap();
    assert!(detail(&f.db, &run).is_err());
    prune(&f.db, f.at + 2).unwrap();
    assert!(list(&f.db).unwrap().is_empty()); // Proven no-change outcomes can expire.
    f.db.pragma_update(None, "synchronous", "NORMAL").unwrap();
    assert!(begin(&mut f.db, &next, f.at).unwrap_err().contains("FULL"));
    f.db.pragma_update(None, "synchronous", "FULL").unwrap();
    for _ in 0..MAX_RUNS {
        next.id = uuid::Uuid::new_v4().to_string();
        let id = begin(&mut f.db, &next, f.at).unwrap();
        finish(&mut f.db, &id, true, f.at + 1).unwrap();
    }
    next.id = uuid::Uuid::new_v4().to_string();
    assert!(begin(&mut f.db, &next, f.at).unwrap_err().contains("容量"));
}

#[test]
fn second_owned_startup_cannot_reconcile_a_live_execution_intent() {
    let f = Fixture::new();
    let path = f.root.join("app.sqlite");
    let mut owner = db::open_owned(&path).unwrap();
    let run = begin(&mut owner, &f.plan, f.at).unwrap();
    intent(&mut owner, &run, &f.item(0), f.at + 1).unwrap();
    assert!(matches!(
        db::open_owned(&path),
        Err(db::OpenError::Instance(crate::instance::Error::Busy))
    ));
    let live = detail(&owner, &run).unwrap();
    assert_eq!(live.summary.state, "running");
    assert_eq!(live.items[0].state, "intent");
    assert_eq!(live.items[1].state, "pending");
    drop(owner);
    let next = db::open_owned(&path).unwrap();
    let recovered = detail(&next, &run).unwrap();
    assert_eq!(recovered.summary.state, "interrupted");
    assert_eq!(recovered.items[0].state, "uncertain");
    assert_eq!(recovered.items[1].state, "notStarted");
    assert!(f.root.join("source/second").is_file());
    assert!(f.root.join("source/中文\ncache *").is_file());
}
