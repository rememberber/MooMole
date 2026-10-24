use super::super::refresh_plan::Store;
use super::*;
use std::{fs, path::PathBuf};
struct Test {
    _root: tempfile::TempDir,
    ctx: Context,
    db: Arc<Mutex<db::Database>>,
    job: Arc<Job>,
    id: String,
}
impl Test {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().canonicalize().unwrap();
        let database = db::open_owned(&home.join("app.sqlite")).unwrap();
        db::save_settings(
            &database,
            &db::Settings {
                acknowledged: true,
                ..Default::default()
            },
        )
        .unwrap();
        Self {
            _root: root,
            ctx: Context {
                home,
                engine_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
                protected_paths: vec![],
                acknowledged: true,
            },
            db: Arc::new(Mutex::new(database)),
            job: Arc::new(Job::new(TaskKind::MaintenanceRefresh)),
            id: uuid::Uuid::new_v4().to_string(),
        }
    }
    fn authorized(&self) -> Authorized {
        Authorized::prepare(&self.ctx, Step::ALL.to_vec()).unwrap()
    }
    fn context(&self) -> impl Fn() -> Result<Context, String> {
        let db = self.db.clone();
        let ctx = self.ctx.clone();
        move || {
            let settings = db::settings(&*db.lock().unwrap())?;
            Ok(Context {
                protected_paths: settings.protected_paths,
                acknowledged: settings.acknowledged,
                ..ctx.clone()
            })
        }
    }
    fn run(&self, mode: usize) -> Result<journal::Detail, String> {
        let a = self.authorized();
        run_with(
            self.db.clone(),
            &self.id,
            a,
            self.job.clone(),
            self.context(),
            &mut Fake {
                db: self.db.clone(),
                mode,
                calls: 0,
            },
        )
    }
}
struct Fake {
    db: Arc<Mutex<db::Database>>,
    mode: usize,
    calls: usize,
}
impl Backend for Fake {
    fn run(
        &mut self,
        _: &Prepared,
        step: Step,
        job: &Job,
        gate: impl FnOnce() -> Result<(), String>,
    ) -> Attempt {
        self.calls += 1;
        if self.mode == 4 {
            let db = self.db.lock().unwrap();
            let mut s = db::settings(&db).unwrap();
            s.protected_paths.push("/fixture/protected".into());
            db::save_settings(&db, &s).unwrap();
        }
        if gate().is_err() {
            return Attempt {
                step,
                outcome: Outcome::NotStarted,
                exit_code: None,
                stop: None,
                reason: "gateRefused",
            };
        }
        let state: String = self
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT state FROM maintenance_steps WHERE step=?1",
                [step.id()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            state, "intent",
            "the actual call must follow committed intent"
        );
        if self.mode == 3 {
            panic!("fixture worker failure after intent");
        }
        if self.mode == 5 {
            self.db.lock().unwrap().execute_batch("CREATE TRIGGER fail_result BEFORE INSERT ON maintenance_events WHEN NEW.state='commandSucceeded' BEGIN SELECT RAISE(ABORT,'fixture result failure'); END;").unwrap();
        }
        if self.mode == 1 {
            job.cancel();
        }
        Attempt {
            step,
            outcome: if self.mode == 2 {
                Outcome::Uncertain
            } else {
                Outcome::CommandSucceeded
            },
            exit_code: Some(if self.mode == 2 { 7 } else { 0 }),
            stop: None,
            reason: if self.mode == 2 {
                "commandFailedAfterStart"
            } else {
                "commandSucceeded"
            },
        }
    }
}
#[test]
fn fixed_execution_commits_each_intent_then_result_and_finishes() {
    let t = Test::new();
    let d = t.run(0).unwrap();
    assert_eq!(d.summary.state, "completed");
    assert_eq!(d.summary.succeeded_count, 2);
    assert_eq!(d.events.len(), 6);
}
#[test]
fn late_cancel_and_unknown_preserve_actual_results_without_starting_step_two() {
    for (mode, state, first) in [
        (1, "cancelled", "commandSucceeded"),
        (2, "needsReview", "uncertain"),
    ] {
        let t = Test::new();
        let d = t.run(mode).unwrap();
        assert_eq!(d.summary.state, state);
        assert_eq!(d.items[0].state, first);
        assert!(matches!(
            d.items[1].state.as_str(),
            "cancelled" | "notStarted"
        ));
        assert_eq!(d.events.iter().filter(|e| e.state == "intent").count(), 1);
    }
}
#[test]
fn worker_fault_and_result_failure_leave_unknown_intent_not_false_untouched() {
    for mode in [3, 5] {
        let t = Test::new();
        assert!(t.run(mode).is_err());
        let d = journal::detail(&*t.db.lock().unwrap(), &t.id).unwrap();
        assert_eq!(d.summary.state, "interrupted");
        assert_eq!(d.items[0].state, "uncertain");
        assert_eq!(d.items[1].state, "notStarted");
    }
}
#[test]
fn protection_change_in_final_gate_retains_every_step_without_intent() {
    let t = Test::new();
    let d = t.run(4).unwrap();
    assert_eq!(d.summary.state, "noChanges");
    assert_eq!(d.events.len(), 4);
    assert!(d.items[0].reason.contains("保护"));
}
#[test]
fn confirmations_are_single_use_revocable_and_late_publication_is_rejected() {
    let t = Test::new();
    let mut s = Store::default();
    let old = s.begin().unwrap();
    let a = t.authorized();
    let now = s.begin().unwrap();
    assert!(s.publish(old, a).is_err());
    let d = s.publish(now, t.authorized()).unwrap();
    let a = s.take(&d.id, &d.plan_id).unwrap();
    assert!(s.take(&d.id, &d.plan_id).is_err());
    a.validate(&t.ctx).unwrap();
    let generation = s.begin().unwrap();
    let d = s.publish(generation, t.authorized()).unwrap();
    s.discard(&d.id);
    assert!(s.take(&d.id, &d.plan_id).is_err());
}
#[test]
fn policy_is_fixed_to_maintenance_rules_and_protected_service_scope() {
    let t = Test::new();
    let mut ctx = t.ctx.clone();
    ctx.protected_paths.push("/fixture/keep".into());
    assert!(Authorized::prepare(&ctx, Step::ALL.to_vec()).is_err());
    let config = t.ctx.home.join(".config/mole");
    fs::create_dir_all(&config).unwrap();
    for (body, allowed) in [
        ("cache_refresh\n", false),
        ("/fixture/keep\n", false),
        ("network_optimization\n", true),
    ] {
        fs::write(config.join("whitelist_optimize"), body).unwrap();
        assert_eq!(
            Authorized::prepare(&t.ctx, Step::ALL.to_vec()).is_ok(),
            allowed
        );
    }
    assert!(Authorized::prepare(&t.ctx, vec![Step::Generators, Step::ThumbnailCache]).is_err());
}
#[test]
fn context_and_rules_cannot_change_under_an_original_confirmation() {
    let t = Test::new();
    let a = t.authorized();
    let config = t.ctx.home.join(".config/mole");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("whitelist_optimize"), "cache_refresh\n").unwrap();
    assert!(a.validate(&t.ctx).is_err());
}

#[tokio::test]
async fn observer_loss_retains_job_update_and_database_ownership_until_worker_finishes() {
    use std::collections::HashMap;
    let t = Test::new();
    let a = t.authorized();
    let ctx = t.ctx.clone();
    let db = t.db.clone();
    let id = t.id.clone();
    let job = t.job.clone();
    let jobs = Arc::new(Mutex::new(HashMap::from([(id.clone(), job.clone())])));
    let install = Arc::new(crate::update_install::Gate::default());
    let release = Arc::new(tokio::sync::Notify::new());
    let wait = release.clone();
    let recv = crate::uninstall_execution::dispatch(
        id.clone(),
        jobs.clone(),
        job.clone(),
        install.enter().unwrap(),
        async move {
            wait.notified().await;
            tauri::async_runtime::spawn_blocking(move || {
                run_with(
                    db.clone(),
                    &id,
                    a,
                    job,
                    move || Ok(ctx.clone()),
                    &mut Fake {
                        db: db.clone(),
                        mode: 0,
                        calls: 0,
                    },
                )
            })
            .await
            .unwrap()
        },
    );
    drop(recv);
    assert!(install.begin().is_err());
    assert!(!jobs.lock().unwrap().is_empty());
    assert!(db::open_owned(&t.ctx.home.join("app.sqlite")).is_err());
    release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        while !jobs.lock().unwrap().is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        journal::detail(&*t.db.lock().unwrap(), &t.id)
            .unwrap()
            .summary
            .state,
        "completed"
    );
    assert!(install.begin().is_ok());
}
