use super::*;
use std::path::PathBuf;
fn fixture() -> (tempfile::TempDir, db::Database, PathBuf) {
    let t = tempfile::Builder::new()
        .prefix("MooMole-folder-watch-owned-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let root = home.join("Downloads");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("one.txt"), "one").unwrap();
    std::fs::write(root.join("two.txt"), "two").unwrap();
    let db = db::open_owned(&home.join("data.sqlite")).unwrap();
    db::save_settings(
        &db,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    (t, db, root)
}
fn edit(id: Option<String>, root: &Path, enabled: bool) -> Edit {
    Edit {
        id,
        root: root.to_str().unwrap().into(),
        enabled,
        interval_minutes: 15,
    }
}
fn add(db: &mut db::Database, root: &Path) -> Saved {
    journal::configure(db, edit(None, root, true)).unwrap();
    journal::watches(db).unwrap().pop().unwrap()
}
fn run(db: &mut db::Database, w: &Saved, job: Arc<Job>) -> Record {
    let settings = db::settings(db).unwrap();
    let r = journal::begin(
        db,
        w,
        Trigger::Manual,
        &uuid::Uuid::new_v4().to_string(),
        db::now(),
    )
    .unwrap();
    let observed = observe(w, &settings.protected_paths, job);
    journal::finish(db, w, r, &settings.protected_paths, observed).unwrap()
}
fn job() -> Arc<Job> {
    Arc::new(Job::new(TaskKind::FolderScan))
}
#[test]
fn actual_owned_scans_persist_changes_and_cancellation_never_replaces_a_complete_baseline() {
    let (_temp, mut db, root) = fixture();
    assert!(journal::watches(&db).unwrap().is_empty());
    let w = add(&mut db, &root);
    let first = run(&mut db, &w, job());
    assert_eq!(first.state, State::Completed);
    let s = first.summary.as_ref().unwrap();
    assert_eq!(s.files, 2);
    assert_eq!(s.comparison, Comparison::First);
    std::fs::write(root.join("one.txt"), "changed owned one").unwrap();
    std::fs::rename(
        root.join("two.txt"),
        root.parent().unwrap().join("kept-two.txt"),
    )
    .unwrap();
    std::fs::write(root.join("three.txt"), "three").unwrap();
    std::fs::write(root.join(".hidden.txt"), "hidden keep").unwrap();
    let second = run(&mut db, &w, job());
    let s = second.summary.as_ref().unwrap();
    assert_eq!(
        (s.added, s.changed, s.not_seen),
        (Some(1), Some(1), Some(1))
    );
    assert!(s.changes.iter().all(|c| !c.path.contains(".hidden")));
    let stopped = job();
    stopped.cancel();
    let cancelled = run(&mut db, &w, stopped);
    assert_eq!(cancelled.state, State::Cancelled);
    assert_eq!(
        cancelled.summary.unwrap().comparison,
        Comparison::Unavailable
    );
    let fourth = run(&mut db, &w, job());
    let s = fourth.summary.as_ref().unwrap();
    assert_eq!(
        (s.added, s.changed, s.not_seen, s.logical_delta_bytes),
        (Some(0), Some(0), Some(0), Some(0))
    );
    assert_eq!(
        std::fs::read_to_string(root.parent().unwrap().join("kept-two.txt")).unwrap(),
        "two"
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".hidden.txt")).unwrap(),
        "hidden keep"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("one.txt")).unwrap(),
        "changed owned one"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("three.txt")).unwrap(),
        "three"
    );
    assert!(crate::execution::list(&db).unwrap().is_empty());
    let database = root.parent().unwrap().join("data.sqlite");
    drop(db);
    let reopened = db::open_owned(&database).unwrap();
    assert_eq!(journal::records(&reopened).unwrap().len(), 4);
    assert!(journal::watches(&reopened).unwrap()[0].config.enabled);
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/folder-schedule-native.json",serde_json::to_vec_pretty(&serde_json::json!({"first":first,"changes":second,"reopened":journal::records(&reopened).unwrap(),"ordinaryFilesRetained":true,"executionHistoryUnchanged":true})).unwrap()).unwrap();
}
#[test]
fn changed_configuration_and_protection_discard_in_flight_results_and_reset_comparisons() {
    let (_temp, mut db, root) = fixture();
    let w = add(&mut db, &root);
    run(&mut db, &w, job());
    std::fs::write(root.join("new.txt"), "new keep").unwrap();
    let r = journal::begin(
        &mut db,
        &w,
        Trigger::Scheduled,
        &uuid::Uuid::new_v4().to_string(),
        db::now() + 900_000,
    )
    .unwrap();
    let observed = observe(&w, &[], job());
    journal::configure(&mut db, edit(Some(w.config.id.clone()), &root, false)).unwrap();
    let r = journal::finish(&mut db, &w, r, &[], observed).unwrap();
    assert_eq!(r.state, State::Superseded);
    assert!(r.summary.is_none());
    let w = journal::watches(&db).unwrap().pop().unwrap();
    let manual = run(&mut db, &w, job());
    assert_eq!(manual.summary.unwrap().added, Some(1));
    let r = journal::begin(
        &mut db,
        &w,
        Trigger::Manual,
        &uuid::Uuid::new_v4().to_string(),
        db::now(),
    )
    .unwrap();
    let observed = observe(&w, &[], job());
    let mut settings = db::settings(&db).unwrap();
    settings.protected_paths = vec![root.join("new.txt").to_str().unwrap().into()];
    db::save_settings(&db, &settings).unwrap();
    let r = journal::finish(&mut db, &w, r, &[], observed).unwrap();
    assert_eq!(r.state, State::Superseded);
    let next = run(&mut db, &w, job());
    assert_eq!(next.summary.unwrap().comparison, Comparison::Reset);
    assert_eq!(
        std::fs::read_to_string(root.join("new.txt")).unwrap(),
        "new keep"
    );
}
#[test]
fn replaced_unavailable_and_protected_roots_refuse_scan_but_can_always_be_paused_or_removed() {
    let (_temp, mut db, root) = fixture();
    let w = add(&mut db, &root);
    assert!(journal::configure(&mut db, edit(None, &root, true)).is_err());
    let kept = root.parent().unwrap().join("kept-downloads");
    std::fs::rename(&root, &kept).unwrap();
    assert!(observe(&w, &[], job()).is_err());
    journal::configure(&mut db, edit(Some(w.config.id.clone()), &root, false)).unwrap();
    std::fs::create_dir(&root).unwrap();
    assert!(observe(&w, &[], job()).is_err());
    let mut settings = db::settings(&db).unwrap();
    settings.protected_paths = vec![root.to_str().unwrap().into()];
    db::save_settings(&db, &settings).unwrap();
    assert!(journal::configure(&mut db, edit(Some(w.config.id.clone()), &root, true)).is_err());
    journal::remove(&db, &w.config.id).unwrap();
    assert!(journal::watches(&db).unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(kept.join("one.txt")).unwrap(),
        "one"
    );
}
#[test]
fn due_time_and_attempts_survive_restart_and_interruption_without_replaying_a_missed_attempt() {
    let (_temp, mut db, root) = fixture();
    let w = add(&mut db, &root);
    assert!(due(&w, 1_000_000));
    let r = journal::begin(
        &mut db,
        &w,
        Trigger::Scheduled,
        &uuid::Uuid::new_v4().to_string(),
        1_000_000,
    )
    .unwrap();
    let current = journal::watches(&db).unwrap().pop().unwrap();
    assert!(!due(&current, 1_899_999));
    assert!(due(&current, 1_900_000));
    assert!(due(&current, 999_999));
    let path = root.parent().unwrap().join("data.sqlite");
    drop(db);
    let mut db = db::open_owned(&path).unwrap();
    assert_eq!(journal::records(&db).unwrap()[0].state, State::Interrupted);
    assert_eq!(journal::records(&db).unwrap()[0].id, r.id);
    let current = journal::watches(&db).unwrap().pop().unwrap();
    assert!(!due(&current, 1_000_001));
    journal::configure(&mut db, edit(Some(current.config.id), &root, false)).unwrap();
    assert!(!due(&journal::watches(&db).unwrap()[0], i64::MAX));
}
#[test]
fn partial_observations_do_not_infer_missing_files_and_history_retention_keeps_per_watch_attempts()
{
    let (_temp, mut db, root) = fixture();
    let w = add(&mut db, &root);
    run(&mut db, &w, job());
    let old = observe(&w, &[], job()).unwrap();
    let mut partial = observe(&w, &[], job()).unwrap();
    partial.complete = false;
    partial.baseline.files.clear();
    let summary = summarize(&partial, Some(&old.baseline)).unwrap();
    assert_eq!(summary.not_seen, None);
    assert_eq!(summary.comparison, Comparison::Unavailable);
    let first_attempt = journal::watches(&db).unwrap()[0].config.last_attempt_at;
    let other = root.parent().unwrap().join("Other");
    std::fs::create_dir(&other).unwrap();
    let other = add(&mut db, &other);
    for i in 0..70 {
        let r = journal::begin(
            &mut db,
            &other,
            Trigger::Manual,
            &uuid::Uuid::new_v4().to_string(),
            2_000_000 + i,
        )
        .unwrap();
        let observed = observe(&other, &[], job());
        journal::finish(&mut db, &other, r, &[], observed).unwrap();
    }
    assert_eq!(journal::records(&db).unwrap().len(), MAX_RUNS);
    let first = journal::watches(&db)
        .unwrap()
        .into_iter()
        .find(|s| s.config.id == w.config.id)
        .unwrap();
    assert_eq!(first.config.last_attempt_at, first_attempt);
    assert!(!due(&first, first_attempt.unwrap() + 1));
    db.execute(
        "UPDATE folder_schedules SET body=json_set(body,'$.version',99) WHERE id=?1",
        [&first.config.id],
    )
    .unwrap();
    assert!(journal::watches(&db).is_err());
    assert!(journal::configure(&mut db, edit(Some(first.config.id), &root, false)).is_err());
}
