use super::*;
use crate::{db, update_install};
use std::{os::unix::process::ExitStatusExt, path::PathBuf, process::Command};

struct Fixture {
    _root: tempfile::TempDir,
    db: Connection,
    at: i64,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let db = db::open_fixture(&root.path().join("db.sqlite")).unwrap();
        Self {
            _root: root,
            db,
            at: db::now(),
        }
    }
    fn selection(&self, steps: Vec<Step>) -> Selection {
        Selection::new(
            &uuid::Uuid::new_v4().to_string(),
            &uuid::Uuid::new_v4().to_string(),
            steps,
            &"a".repeat(64),
            self.at,
            self.at + TTL,
        )
        .unwrap()
    }
    fn begin(&mut self) -> Session {
        let selection = self.selection(Step::ALL.to_vec());
        begin(
            &mut self.db,
            &selection,
            &uuid::Uuid::new_v4().to_string(),
            self.at,
        )
        .unwrap()
    }
    fn success(&mut self, session: &Session, step: Step) {
        intent(&mut self.db, session, step, self.at + 1).unwrap();
        check_intent(&self.db, session, step, self.at + 2).unwrap();
        record_attempt(&mut self.db, session, &success(step), self.at + 3).unwrap();
    }
}
fn success(step: Step) -> Attempt {
    Attempt {
        step,
        outcome: Outcome::CommandSucceeded,
        exit_code: Some(0),
        stop: None,
        reason: "commandSucceeded",
    }
}

#[test]
fn both_steps_are_ordered_once_and_a_late_cancel_keeps_both_successes() {
    let mut f = Fixture::new();
    let s = f.begin();
    assert!(intent(&mut f.db, &s, Step::Generators, f.at).is_err());
    assert!(record_attempt(&mut f.db, &s, &success(Step::ThumbnailCache), f.at).is_err());
    f.success(&s, Step::ThumbnailCache);
    assert!(intent(&mut f.db, &s, Step::ThumbnailCache, f.at + 4).is_err());
    f.success(&s, Step::Generators);
    finish(&mut f.db, &s, true, f.at + TTL + 10).unwrap();
    let d = detail(&f.db, s.id()).unwrap();
    assert_eq!(d.summary.state, "completed");
    assert_eq!(d.summary.succeeded_count, 2);
    assert_eq!(d.events.len(), 6);
    assert!(check_intent(&f.db, &s, Step::Generators, f.at + 4).is_err());
    assert!(record_attempt(&mut f.db, &s, &success(Step::Generators), f.at + 5).is_err());
    assert_eq!(list(&f.db).unwrap().len(), 1);
}

#[test]
fn cancel_between_steps_preserves_first_success_and_only_cancels_pending_work() {
    let mut f = Fixture::new();
    let s = f.begin();
    f.success(&s, Step::ThumbnailCache);
    finish(&mut f.db, &s, true, f.at + 4).unwrap();
    let d = detail(&f.db, s.id()).unwrap();
    assert_eq!(d.summary.state, "cancelled");
    assert_eq!(d.summary.succeeded_count, 1);
    assert_eq!(d.items[1].state, "cancelled");
    assert_eq!(d.items[1].stop, Some(Stop::Cancelled));
    assert_eq!(d.events.len(), 5);
    update_install::prepare_data(&f.db).unwrap();
}

#[test]
fn started_unknown_results_block_the_next_step_and_new_modifications() {
    let mut f = Fixture::new();
    let s = f.begin();
    intent(&mut f.db, &s, Step::ThumbnailCache, f.at + 1).unwrap();
    record_attempt(
        &mut f.db,
        &s,
        &Attempt {
            step: Step::ThumbnailCache,
            outcome: Outcome::Uncertain,
            exit_code: Some(0),
            stop: Some(Stop::Cancelled),
            reason: "stoppedAfterStart",
        },
        f.at + 2,
    )
    .unwrap();
    assert!(intent(&mut f.db, &s, Step::Generators, f.at + 3).is_err());
    finish(&mut f.db, &s, true, f.at + 4).unwrap();
    let d = detail(&f.db, s.id()).unwrap();
    assert_eq!(d.summary.state, "needsReview");
    assert_eq!(d.items[0].exit_code, Some(0));
    assert_eq!(d.summary.uncertain_count, 1);
    let next = f.selection(vec![Step::Generators]);
    assert!(begin(
        &mut f.db,
        &next,
        &uuid::Uuid::new_v4().to_string(),
        f.at + 5
    )
    .unwrap_err()
    .contains("待核实"));
    let t = f.db.transaction().unwrap();
    assert!(crate::execution_admission::reserve(&t, 1).is_err());
}

#[test]
fn supervision_failure_retains_even_zero_exit_as_unknown_and_stops_later_steps() {
    let mut f = Fixture::new();
    let s = f.begin();
    intent(&mut f.db, &s, Step::ThumbnailCache, f.at + 1).unwrap();
    record_attempt(
        &mut f.db,
        &s,
        &Attempt {
            step: Step::ThumbnailCache,
            outcome: Outcome::Uncertain,
            exit_code: Some(0),
            stop: Some(Stop::SupervisionFailed),
            reason: "childRegistrationFailed",
        },
        f.at + 2,
    )
    .unwrap();
    assert!(intent(&mut f.db, &s, Step::Generators, f.at + 3).is_err());
    finish(&mut f.db, &s, false, f.at + 4).unwrap();
    let d = detail(&f.db, s.id()).unwrap();
    assert_eq!(d.summary.state, "needsReview");
    assert_eq!(d.summary.succeeded_count, 0);
    assert_eq!(d.items[0].exit_code, Some(0));
    assert_eq!(d.items[0].stop, Some(Stop::SupervisionFailed));
    assert_eq!(d.events[3].stop, Some(Stop::SupervisionFailed));
    assert_eq!(d.items[1].state, "notStarted");
    assert!(crate::update_install::prepare_data(&f.db).is_ok());
    let t = f.db.transaction().unwrap();
    assert!(crate::execution_admission::reserve(&t, 1).is_err());
}

#[test]
fn a_single_step_can_be_selected_and_refusal_before_intent_is_not_started() {
    let mut f = Fixture::new();
    let selection = f.selection(vec![Step::Generators]);
    let s = begin(
        &mut f.db,
        &selection,
        &uuid::Uuid::new_v4().to_string(),
        f.at,
    )
    .unwrap();
    record_attempt(
        &mut f.db,
        &s,
        &Attempt {
            step: Step::Generators,
            outcome: Outcome::NotStarted,
            exit_code: None,
            stop: None,
            reason: "gateRefused",
        },
        f.at + 1,
    )
    .unwrap();
    finish(&mut f.db, &s, false, f.at + 2).unwrap();
    let d = detail(&f.db, s.id()).unwrap();
    assert_eq!(d.items.len(), 1);
    assert_eq!(d.items[0].step, Step::Generators);
    assert_eq!(d.summary.state, "noChanges");
    assert_eq!(d.events.len(), 2);
}

#[test]
fn authority_expiry_and_clock_rollback_block_start_but_never_discard_a_result() {
    let mut f = Fixture::new();
    let s = f.begin();
    assert!(intent(&mut f.db, &s, Step::ThumbnailCache, f.at - 1).is_err());
    assert!(intent(&mut f.db, &s, Step::ThumbnailCache, f.at + TTL).is_err());
    intent(&mut f.db, &s, Step::ThumbnailCache, f.at + 1).unwrap();
    record_attempt(&mut f.db, &s, &success(Step::ThumbnailCache), f.at - 1).unwrap();
    assert_eq!(
        detail(&f.db, s.id()).unwrap().items[0].finished_at,
        Some(f.at - 1)
    );
    finish(&mut f.db, &s, false, f.at + TTL + 1).unwrap();
    assert_eq!(detail(&f.db, s.id()).unwrap().summary.state, "partial");
}

#[test]
fn full_sync_and_intent_event_failure_stop_before_any_command() {
    let mut f = Fixture::new();
    let s = f.begin();
    f.db.pragma_update(None, "synchronous", "NORMAL").unwrap();
    assert!(intent(&mut f.db, &s, Step::ThumbnailCache, f.at).is_err());
    f.db.pragma_update(None, "synchronous", "FULL").unwrap();
    f.db.execute_batch("CREATE TRIGGER fail_intent BEFORE INSERT ON maintenance_events WHEN NEW.state='intent' BEGIN SELECT RAISE(ABORT,'fixture intent failure'); END;").unwrap();
    assert!(intent(&mut f.db, &s, Step::ThumbnailCache, f.at + 1).is_err());
    let d = detail(&f.db, s.id()).unwrap();
    assert!(d.items.iter().all(|i| i.state == "pending"));
    assert_eq!(d.events.len(), 2);
    assert!(check_intent(&f.db, &s, Step::ThumbnailCache, f.at + 1).is_err());
}

#[test]
fn result_storage_failure_retains_intent_and_cannot_finish_as_no_change() {
    let mut f = Fixture::new();
    let s = f.begin();
    intent(&mut f.db, &s, Step::ThumbnailCache, f.at + 1).unwrap();
    f.db.execute_batch("CREATE TRIGGER fail_result BEFORE INSERT ON maintenance_events WHEN NEW.state='commandSucceeded' BEGIN SELECT RAISE(ABORT,'fixture result failure'); END;").unwrap();
    assert!(record_attempt(&mut f.db, &s, &success(Step::ThumbnailCache), f.at + 2).is_err());
    assert_eq!(detail(&f.db, s.id()).unwrap().items[0].state, "intent");
    assert!(finish(&mut f.db, &s, true, f.at + 3).is_err());
    assert!(update_install::prepare_data(&f.db).is_err());
    stop_worker(&mut f.db, &s, f.at + 4).unwrap();
    let d = detail(&f.db, s.id()).unwrap();
    assert_eq!(d.items[0].state, "uncertain");
    assert_eq!(d.items[1].state, "notStarted");
}

#[test]
fn request_and_plan_replays_are_rejected_and_do_not_create_extra_events() {
    let mut f = Fixture::new();
    let selection = f.selection(Step::ALL.to_vec());
    let id = uuid::Uuid::new_v4().to_string();
    let s = begin(&mut f.db, &selection, &id, f.at).unwrap();
    finish(&mut f.db, &s, true, f.at + 1).unwrap();
    assert!(begin(
        &mut f.db,
        &selection,
        &uuid::Uuid::new_v4().to_string(),
        f.at + 2
    )
    .is_err());
    let next = f.selection(vec![Step::Generators]);
    assert!(begin(&mut f.db, &next, &id, f.at + 2).is_err());
    assert_eq!(list(&f.db).unwrap().len(), 1);
    assert_eq!(detail(&f.db, &id).unwrap().events.len(), 4);
}

#[test]
fn malformed_selection_and_corrupted_history_never_rebuild_a_live_session() {
    let mut f = Fixture::new();
    for steps in [
        vec![],
        vec![Step::Generators, Step::ThumbnailCache],
        vec![Step::Generators, Step::Generators],
    ] {
        assert!(Selection::new(
            &uuid::Uuid::new_v4().to_string(),
            &uuid::Uuid::new_v4().to_string(),
            steps,
            &"a".repeat(64),
            f.at,
            f.at + TTL
        )
        .is_err());
    }
    let s = f.begin();
    f.db.execute(
        "UPDATE maintenance_events SET reason='corrupted' WHERE position=0",
        [],
    )
    .unwrap();
    assert!(detail(&f.db, s.id()).is_err());
    assert!(intent(&mut f.db, &s, Step::ThumbnailCache, f.at + 1).is_err());
    assert!(reconcile(&mut f.db, f.at + 2).is_err());
    let state: String =
        f.db.query_row(
            "SELECT state FROM maintenance_runs WHERE id=?1",
            [s.id()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "running");
}

#[test]
fn modification_admission_and_installation_barrier_include_maintenance() {
    let mut f = Fixture::new();
    let s = f.begin();
    assert!(update_install::prepare_data(&f.db).is_err());
    {
        let t = f.db.transaction().unwrap();
        assert!(crate::execution_admission::reserve(&t, 1).is_err());
    }
    finish(&mut f.db, &s, true, f.at + 1).unwrap();
    update_install::prepare_data(&f.db).unwrap();
    f.db.execute("INSERT INTO execution_runs VALUES ('fixture','fixture','trash','running',1,NULL,'{}','fixture',1)",[]).unwrap();
    let next = f.selection(vec![Step::Generators]);
    assert!(begin(
        &mut f.db,
        &next,
        &uuid::Uuid::new_v4().to_string(),
        f.at + 2
    )
    .is_err());
    f.db.execute("DELETE FROM execution_runs", []).unwrap();
    f.db.execute("INSERT INTO uninstall_runs VALUES ('fixture','fixture','appBundleTrash','running','pending',1,NULL,'{}','fixture',1,'fixture',NULL)",[]).unwrap();
    assert!(begin(
        &mut f.db,
        &next,
        &uuid::Uuid::new_v4().to_string(),
        f.at + 3
    )
    .is_err());
}

#[test]
fn shared_capacity_is_not_satisfied_by_pruning_recovery_evidence() {
    let mut f = Fixture::new();
    for _ in 0..crate::execution_admission::MAX_RUNS {
        let s = f.begin();
        finish(&mut f.db, &s, true, f.at + 1).unwrap();
    }
    let selection = f.selection(vec![Step::Generators]);
    assert!(begin(
        &mut f.db,
        &selection,
        &uuid::Uuid::new_v4().to_string(),
        f.at + 2
    )
    .unwrap_err()
    .contains("容量"));
    assert_eq!(list(&f.db).unwrap().len(), 64);
}

#[test]
fn retention_keeps_interrupted_and_unknown_evidence_without_scan_foreign_keys() {
    let mut f = Fixture::new();
    let cancelled = f.begin();
    finish(&mut f.db, &cancelled, true, f.at + 1).unwrap();
    let interrupted = f.begin();
    stop_worker(&mut f.db, &interrupted, f.at + 2).unwrap();
    let unknown = f.begin();
    intent(&mut f.db, &unknown, Step::ThumbnailCache, f.at + 3).unwrap();
    stop_worker(&mut f.db, &unknown, f.at + 4).unwrap();
    let t = f.db.transaction().unwrap();
    prune(&t, f.at + TTL).unwrap();
    t.commit().unwrap();
    assert!(detail(&f.db, cancelled.id()).is_err());
    assert_eq!(
        detail(&f.db, interrupted.id()).unwrap().summary.state,
        "interrupted"
    );
    assert_eq!(
        detail(&f.db, unknown.id()).unwrap().summary.uncertain_count,
        1
    );
}

#[test]
fn separate_connections_observe_the_same_durable_modification_admission() {
    let mut f = Fixture::new();
    let s = f.begin();
    // A raw second connection tests SQLite admission, not a second App startup
    // which would first need to acquire the exclusive data-directory lease.
    let mut second = Connection::open(f._root.path().join("db.sqlite")).unwrap();
    second.pragma_update(None, "synchronous", "FULL").unwrap();
    let next = f.selection(vec![Step::Generators]);
    assert!(begin(
        &mut second,
        &next,
        &uuid::Uuid::new_v4().to_string(),
        f.at + 1
    )
    .is_err());
    finish(&mut f.db, &s, true, f.at + 2).unwrap();
    let next_run = begin(
        &mut second,
        &next,
        &uuid::Uuid::new_v4().to_string(),
        f.at + 3,
    )
    .unwrap();
    assert_eq!(
        detail(&f.db, next_run.id()).unwrap().summary.state,
        "running"
    );
}

#[test]
fn event_order_corruption_cannot_turn_overlapping_steps_into_success() {
    let mut f = Fixture::new();
    let s = f.begin();
    f.success(&s, Step::ThumbnailCache);
    f.success(&s, Step::Generators);
    finish(&mut f.db, &s, false, f.at + 4).unwrap();
    // Preserve both per-step chains but put generator intent before thumbnail
    // completion. Seq is the ordering source even when wall time rolls back.
    let first_done:i64 = f.db.query_row("SELECT seq FROM maintenance_events WHERE run_id=?1 AND position=0 AND state='commandSucceeded'",[s.id()],|r|r.get(0)).unwrap();
    let second_intent: i64 =
        f.db.query_row(
            "SELECT seq FROM maintenance_events WHERE run_id=?1 AND position=1 AND state='intent'",
            [s.id()],
            |r| r.get(0),
        )
        .unwrap();
    f.db.execute(
        "UPDATE maintenance_events SET seq=100 WHERE seq=?1",
        [first_done],
    )
    .unwrap();
    f.db.execute(
        "UPDATE maintenance_events SET seq=?2 WHERE seq=?1",
        params![second_intent, first_done],
    )
    .unwrap();
    f.db.execute(
        "UPDATE maintenance_events SET seq=?1 WHERE seq=100",
        [second_intent],
    )
    .unwrap();
    assert!(detail(&f.db, s.id()).unwrap_err().contains("顺序"));
}

#[test]
fn v4_schema_conflict_rolls_back_new_tables_and_preserves_backup() {
    let f = Fixture::new();
    f.db.execute_batch("DROP TABLE execution_reviews; DROP TABLE uninstall_reviews; DROP TABLE maintenance_reviews; DROP TABLE maintenance_events; DROP TABLE maintenance_steps; DROP TABLE maintenance_runs; CREATE TABLE maintenance_steps(sentinel TEXT); INSERT INTO maintenance_steps VALUES ('keep'); PRAGMA user_version=4;").unwrap();
    assert!(db::open_fixture(&f._root.path().join("db.sqlite")).is_err());
    assert_eq!(
        f.db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        4
    );
    assert_eq!(
        f.db.query_row::<String, _, _>("SELECT sentinel FROM maintenance_steps", [], |r| r.get(0))
            .unwrap(),
        "keep"
    );
    assert!(f.db.prepare("SELECT * FROM maintenance_runs").is_err());
    let backups: Vec<_> = std::fs::read_dir(f._root.path())
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
            .query_row::<String, _, _>("SELECT sentinel FROM maintenance_steps", [], |r| r.get(0))
            .unwrap(),
        "keep"
    );
}

#[test]
fn corrupt_terminal_history_blocks_retention_without_losing_other_records_or_settings() {
    let mut f = Fixture::new();
    let valid = f.begin();
    finish(&mut f.db, &valid, true, f.at + 1).unwrap();
    let corrupt = f.begin();
    finish(&mut f.db, &corrupt, true, f.at + 2).unwrap();
    f.db.execute(
        "UPDATE maintenance_runs SET finished_at=1 WHERE id=?1",
        [valid.id()],
    )
    .unwrap();
    f.db.execute(
        "UPDATE maintenance_runs SET finished_at=1,snapshot_sha256='broken' WHERE id=?1",
        [corrupt.id()],
    )
    .unwrap();
    // Use an expired known result for a valid candidate, preserving the event
    // time fact (finish time itself is allowed to differ after clock changes).
    let settings = db::Settings {
        theme: "dark".into(),
        ..db::Settings::default()
    };
    assert!(db::save_settings(&f.db, &settings).is_err());
    assert_eq!(db::settings(&f.db).unwrap().theme, "system");
    assert_eq!(
        f.db.query_row::<i64, _, _>("SELECT count(*) FROM maintenance_runs", [], |r| r.get(0))
            .unwrap(),
        2
    );
    assert_eq!(
        detail(&f.db, valid.id()).unwrap().summary.state,
        "cancelled"
    );
    assert!(detail(&f.db, corrupt.id()).is_err());
}

#[test]
fn explicit_review_releases_admission_but_never_rewrites_or_replays_unknown_results() {
    let mut f = Fixture::new();
    let s = f.begin();
    intent(&mut f.db, &s, Step::ThumbnailCache, f.at + 1).unwrap();
    let d = detail(&f.db, s.id()).unwrap();
    assert!(acknowledge(&mut f.db, s.id(), &d.snapshot_sha256, f.at + 2).is_err());
    stop_worker(&mut f.db, &s, f.at + 3).unwrap();
    let before = detail(&f.db, s.id()).unwrap();
    assert!(acknowledge(&mut f.db, s.id(), &"b".repeat(64), f.at + 4).is_err());
    let reviewed = acknowledge(&mut f.db, s.id(), &before.snapshot_sha256, f.at + 4).unwrap();
    assert_eq!(reviewed.items, before.items);
    assert_eq!(reviewed.events.len(), before.events.len());
    assert_eq!(reviewed.summary.reviewed_at, Some(f.at + 4));
    assert!(!reviewed.summary.retained_for_review);
    let again = acknowledge(&mut f.db, s.id(), &before.snapshot_sha256, f.at + 5).unwrap();
    assert_eq!(again.summary.reviewed_at, Some(f.at + 4));
    assert!(intent(&mut f.db, &s, Step::ThumbnailCache, f.at + 5).is_err());
    let next = f.selection(vec![Step::Generators]);
    let run = begin(
        &mut f.db,
        &next,
        &uuid::Uuid::new_v4().to_string(),
        f.at + 6,
    )
    .unwrap();
    finish(&mut f.db, &run, true, f.at + 7).unwrap();
    let t = f.db.transaction().unwrap();
    prune(&t, f.at + TTL).unwrap();
    t.commit().unwrap();
    assert!(detail(&f.db, s.id()).is_err());
}

#[test]
fn corrupted_review_cannot_release_modification_admission() {
    let mut f = Fixture::new();
    let s = f.begin();
    intent(&mut f.db, &s, Step::ThumbnailCache, f.at + 1).unwrap();
    stop_worker(&mut f.db, &s, f.at + 2).unwrap();
    f.db.execute(
        "INSERT INTO maintenance_reviews VALUES (?1,?2,?3)",
        params![s.id(), "b".repeat(64), f.at + 3],
    )
    .unwrap();
    assert!(detail(&f.db, s.id()).is_err());
    let t = f.db.transaction().unwrap();
    assert!(crate::execution_admission::reserve(&t, 1).is_err());
}

#[test]
fn v5_wal_backup_precedes_review_upgrade_and_intent_reconciliation() {
    let mut f = Fixture::new();
    let s = f.begin();
    intent(&mut f.db, &s, Step::ThumbnailCache, f.at + 1).unwrap();
    f.db.execute_batch("DROP TABLE execution_reviews; DROP TABLE uninstall_reviews; DROP TABLE maintenance_reviews; PRAGMA user_version=5;")
        .unwrap();
    let upgraded = db::open_fixture(&f._root.path().join("db.sqlite")).unwrap();
    assert_eq!(
        upgraded
            .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        7
    );
    assert_eq!(
        detail(&upgraded, s.id()).unwrap().items[0].state,
        "uncertain"
    );
    let backup = std::fs::read_dir(f._root.path())
        .unwrap()
        .map(|p| p.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("moomole-before-v7-")
        })
        .unwrap();
    let old = Connection::open(backup).unwrap();
    assert_eq!(
        old.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        5
    );
    assert_eq!(
        old.query_row::<String, _, _>(
            "SELECT state FROM maintenance_steps WHERE run_id=?1 AND position=0",
            [s.id()],
            |r| r.get(0)
        )
        .unwrap(),
        "intent"
    );
    assert!(old.prepare("SELECT * FROM maintenance_reviews").is_err());
}

#[test]
fn process_crash_after_each_durable_boundary_reopens_without_replay() {
    for boundary in ["pending", "intent", "between", "secondIntent", "allResults"] {
        let root = tempfile::Builder::new()
            .prefix("moomole-refresh-crash-")
            .tempdir()
            .unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "maintenance::refresh_journal::tests::crash_child",
            ])
            .env("MOOMOLE_REFRESH_JOURNAL_CHILD_ROOT", root.path())
            .env("MOOMOLE_REFRESH_JOURNAL_CHILD_BOUNDARY", boundary)
            .output()
            .unwrap()
            .status;
        assert_eq!(status.signal(), Some(libc::SIGKILL), "boundary {boundary}");
        let db = db::open_fixture(&root.path().join("db.sqlite")).unwrap();
        let rows = list(&db).unwrap();
        assert_eq!(rows.len(), 1);
        let d = detail(&db, &rows[0].id).unwrap();
        assert_eq!(d.summary.state, "interrupted");
        let expected = match boundary {
            "pending" => ["notStarted", "notStarted"],
            "intent" => ["uncertain", "notStarted"],
            "between" => ["commandSucceeded", "notStarted"],
            "secondIntent" => ["commandSucceeded", "uncertain"],
            _ => ["commandSucceeded", "commandSucceeded"],
        };
        assert_eq!(
            d.items.iter().map(|i| i.state.as_str()).collect::<Vec<_>>(),
            expected
        );
        let before = serde_json::to_value(&d).unwrap();
        drop(db);
        let reopened = db::open_fixture(&root.path().join("db.sqlite")).unwrap();
        assert_eq!(
            serde_json::to_value(detail(&reopened, &rows[0].id).unwrap()).unwrap(),
            before
        );
    }
}

#[test]
#[ignore = "parent-only process crash fixture; never run against user data"]
fn crash_child() {
    let root = PathBuf::from(
        std::env::var_os("MOOMOLE_REFRESH_JOURNAL_CHILD_ROOT")
            .expect("parent fixture root required"),
    );
    assert!(root
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("moomole-refresh-crash-"));
    let boundary = std::env::var("MOOMOLE_REFRESH_JOURNAL_CHILD_BOUNDARY").unwrap();
    let mut db = db::open_fixture(&root.join("db.sqlite")).unwrap();
    let at = db::now();
    let selection = Selection::new(
        &uuid::Uuid::new_v4().to_string(),
        &uuid::Uuid::new_v4().to_string(),
        Step::ALL.to_vec(),
        &"a".repeat(64),
        at,
        at + TTL,
    )
    .unwrap();
    let s = begin(&mut db, &selection, &uuid::Uuid::new_v4().to_string(), at).unwrap();
    if boundary != "pending" {
        intent(&mut db, &s, Step::ThumbnailCache, at + 1).unwrap();
        if boundary != "intent" {
            record_attempt(&mut db, &s, &success(Step::ThumbnailCache), at + 2).unwrap();
            if boundary != "between" {
                intent(&mut db, &s, Step::Generators, at + 3).unwrap();
                if boundary == "allResults" {
                    record_attempt(&mut db, &s, &success(Step::Generators), at + 4).unwrap();
                }
            }
        }
    }
    unsafe {
        libc::kill(libc::getpid(), libc::SIGKILL);
    }
    panic!("fixture must die before graceful DB close");
}

#[test]
fn malformed_review_timestamp_does_not_release_modification_admission() {
    let mut f = Fixture::new();
    let s = f.begin();
    intent(&mut f.db, &s, Step::ThumbnailCache, f.at + 1).unwrap();
    stop_worker(&mut f.db, &s, f.at + 2).unwrap();
    let digest = detail(&f.db, s.id()).unwrap().snapshot_sha256;
    // SQLite's older INTEGER affinity plus CHECK(at>=0) accepts text. This is
    // corrupted fixture data, never a production write route or user database.
    f.db.execute(
        "INSERT INTO maintenance_reviews VALUES (?1,?2,'invalid time')",
        params![s.id(), digest],
    )
    .unwrap();
    assert!(detail(&f.db, s.id()).is_err());
    let t = f.db.transaction().unwrap();
    assert!(crate::execution_admission::reserve(&t, 1).is_err());
}
