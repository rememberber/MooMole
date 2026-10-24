use super::*;
use crate::{
    uninstall_confirmation::Confirmations,
    uninstall_runtime::{Code, Fixture},
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicUsize},
};
mod native;

struct Test {
    f: Fixture,
    db: Arc<Mutex<db::Database>>,
    job: Arc<Job>,
    id: String,
}
impl Test {
    fn new() -> Self {
        let f = Fixture::new(true);
        let database = db::open_owned(&f.home.join("app.sqlite")).unwrap();
        db::save_settings(
            &database,
            &db::Settings {
                acknowledged: true,
                ..Default::default()
            },
        )
        .unwrap();
        Self {
            f,
            db: Arc::new(Mutex::new(database)),
            job: Arc::new(Job::new(TaskKind::AppTrash)),
            id: uuid::Uuid::new_v4().to_string(),
        }
    }
    fn selection(&self) -> Selection {
        let mut s = Confirmations::default();
        let g = s.begin_review(self.f.plan.id(), db::now()).unwrap();
        let runtime = uninstall_runtime::fixture_observation(&self.f.plan, Code::NotObserved);
        let support = uninstall_support::fixture_observation(&self.f.plan, &self.f.ctx);
        let c = s
            .publish(g, &self.f.plan, &self.f.ctx, &runtime, &support, db::now())
            .unwrap()
            .unwrap();
        s.take(&c.id, &self.f.plan, &self.f.ctx, db::now()).unwrap()
    }
    fn context(&self) -> impl Fn() -> Result<Context, String> {
        let database = self.db.clone();
        let original = self.f.ctx.clone();
        move || {
            let settings = db::settings(&*database.lock().unwrap())?;
            if !settings.acknowledged {
                return Err("首次说明已撤回。".into());
            }
            Ok(Context {
                protected_paths: settings.protected_paths,
                ..original.clone()
            })
        }
    }
    fn backend(&self) -> Fake {
        Fake {
            destination: self.f.home.join("private-result.app"),
            code: Code::NotObserved,
            calls: 0,
            uncertain: false,
            hook: None,
        }
    }
    fn run(&self, backend: &mut Fake) -> Result<journal::Detail, String> {
        run_with(
            self.db.clone(),
            &self.id,
            self.f.plan.clone(),
            self.selection(),
            self.job.clone(),
            self.context(),
            backend,
        )
    }
    fn record(&self) -> journal::Detail {
        journal::detail(&*self.db.lock().unwrap(), &self.id).unwrap()
    }
}
struct Fake {
    destination: PathBuf,
    code: Code,
    calls: usize,
    uncertain: bool,
    hook: Option<Box<dyn FnMut(Phase, &str) -> Result<(), String> + Send>>,
}
impl Backend for Fake {
    fn observe(&mut self, plan: &Plan, ctx: &Context) -> Result<Prerequisites, String> {
        Ok(Prerequisites {
            runtime: uninstall_runtime::fixture_observation(plan, self.code),
            support: uninstall_support::fixture_observation(plan, ctx),
        })
    }
    fn trash(
        &mut self,
        target: &PreparedBundle,
        gate: impl FnOnce() -> Result<(), String>,
    ) -> Result<Attempt, String> {
        // Counts actual fixture API entry, not a pre-call validation refusal.
        let calls = &mut self.calls;
        let mut attempt = target.move_to_private_checked(&self.destination, || {
            gate()?;
            *calls += 1;
            Ok(())
        })?;
        if self.uncertain {
            attempt.outcome = match attempt.outcome {
                crate::uninstall_target::Outcome::Moved(r) => {
                    crate::uninstall_target::Outcome::Uncertain(Some(r))
                }
                other => other,
            };
            attempt.reason = "测试专用原生结果不确定";
        }
        Ok(attempt)
    }
    fn checkpoint(&mut self, phase: Phase, id: &str) -> Result<(), String> {
        if let Some(h) = &mut self.hook {
            h(phase, id)?;
        }
        Ok(())
    }
}

#[test]
fn single_app_execution_commits_intent_then_actual_receipt_and_does_not_touch_external_data() {
    let t = Test::new();
    let mut b = t.backend();
    let outside = t.f.home.join("retained.txt");
    fs::write(&outside, "retained").unwrap();
    let database = t.db.clone();
    let entered = Arc::new(AtomicBool::new(false));
    let seen = entered.clone();
    b.hook = Some(Box::new(move |phase, id| {
        let record = journal::detail(&*database.lock().unwrap(), id).unwrap();
        if phase == Phase::Intent {
            assert_eq!(record.summary.item_state, "intent");
            seen.store(true, Ordering::SeqCst);
        }
        Ok(())
    }));
    let detail = t.run(&mut b).unwrap();
    assert_eq!(detail.summary.id, t.id);
    assert_eq!(detail.summary.item_state, "moved");
    assert_eq!(detail.summary.state, "completed");
    assert_eq!(
        detail
            .events
            .iter()
            .map(|e| e.state.as_str())
            .collect::<Vec<_>>(),
        ["pending", "intent", "moved"]
    );
    assert!(entered.load(Ordering::SeqCst));
    assert_eq!(b.calls, 1);
    assert!(!t.f.app.exists());
    assert!(b.destination.is_dir());
    assert_eq!(fs::read_to_string(outside).unwrap(), "retained");
    assert_eq!(
        detail.receipt.unwrap().actual.destination,
        b.destination.to_str().unwrap()
    );
    // Request replay cannot rewrite the result or invoke another move.
    assert!(t.run(&mut b).is_err());
    assert_eq!(b.calls, 1);
    assert_eq!(t.record().summary.item_state, "moved");
}
#[test]
fn unknown_busy_unsupported_running_conditions_are_recorded_as_retained_without_intent() {
    for code in [Code::Unknown, Code::Observed, Code::Unsupported] {
        let t = Test::new();
        let mut b = t.backend();
        b.code = code;
        let detail = t.run(&mut b).unwrap();
        assert_eq!(detail.summary.item_state, "skipped");
        assert_eq!(b.calls, 0);
        assert!(t.f.app.exists());
        assert_eq!(detail.events.len(), 2);
        assert!(detail.receipt.is_none());
    }
}
#[test]
fn cancellation_before_begin_preparation_observation_and_after_intent_never_moves_the_app() {
    for phase in [
        Phase::Begun,
        Phase::Prepared,
        Phase::Observed,
        Phase::Intent,
    ] {
        let t = Test::new();
        let mut b = t.backend();
        let job = t.job.clone();
        b.hook = Some(Box::new(move |at, _| {
            if at == phase {
                job.cancel();
            }
            Ok(())
        }));
        let detail = t.run(&mut b).unwrap();
        assert_eq!(detail.summary.item_state, "cancelled");
        assert_eq!(detail.summary.state, "cancelled");
        assert_eq!(b.calls, 0);
        assert!(t.f.app.is_dir());
        assert_eq!(
            detail.events.iter().any(|e| e.state == "intent"),
            phase == Phase::Intent
        );
    }
    let t = Test::new();
    t.job.cancel();
    let mut b = t.backend();
    assert_eq!(t.run(&mut b).unwrap().summary.state, "cancelled");
    assert_eq!(b.calls, 0);
}
#[test]
fn protection_acknowledgement_and_source_revocation_are_rechecked_after_intent() {
    for mode in ["protected", "ack", "source", "content"] {
        let t = Test::new();
        let mut b = t.backend();
        let database = t.db.clone();
        let app = t.f.app.clone();
        let revoked = Arc::new(AtomicBool::new(false));
        let flag = revoked.clone();
        b.hook = Some(Box::new(move |phase, _| {
            if phase == Phase::Intent {
                if mode == "source" {
                    flag.store(true, Ordering::SeqCst);
                } else if mode == "content" {
                    fs::write(app.join("Contents/changed.txt"), "changed").unwrap();
                } else {
                    let db = database.lock().unwrap();
                    let mut settings = db::settings(&db).unwrap();
                    if mode == "protected" {
                        settings.protected_paths.push(app.to_str().unwrap().into());
                    } else {
                        settings.acknowledged = false;
                    }
                    db::save_settings(&db, &settings).unwrap();
                }
            }
            Ok(())
        }));
        let context = t.context();
        let detail = run_with(
            t.db.clone(),
            &t.id,
            t.f.plan.clone(),
            t.selection(),
            t.job.clone(),
            move || {
                if revoked.load(Ordering::SeqCst) {
                    Err("原源清单已撤下".into())
                } else {
                    context()
                }
            },
            &mut b,
        )
        .unwrap();
        assert_eq!(detail.summary.item_state, "failed", "{mode}");
        assert_eq!(b.calls, 0);
        assert!(t.f.app.is_dir());
    }
}
#[test]
fn late_cancel_preserves_actual_move_or_uncertain_outcome_and_receipt() {
    for uncertain in [false, true] {
        let t = Test::new();
        let mut b = t.backend();
        b.uncertain = uncertain;
        let job = t.job.clone();
        b.hook = Some(Box::new(move |at, _| {
            if at == Phase::NativeReturned {
                job.cancel();
            }
            Ok(())
        }));
        let detail = t.run(&mut b).unwrap();
        assert_eq!(
            detail.summary.item_state,
            if uncertain { "uncertain" } else { "moved" }
        );
        assert_eq!(b.calls, 1);
        assert!(detail.receipt.is_some());
        assert!(!t.f.app.exists());
    }
}
#[test]
fn intent_and_result_storage_failures_never_advance_without_evidence_or_replay() {
    for phase in ["intent", "result"] {
        let t = Test::new();
        let mut b = t.backend();
        let sql = if phase == "intent" {
            "CREATE TRIGGER injected BEFORE INSERT ON uninstall_events WHEN NEW.state='intent' BEGIN SELECT RAISE(ABORT,'injected'); END;"
        } else {
            "CREATE TRIGGER injected BEFORE UPDATE ON uninstall_runs WHEN NEW.item_state='moved' BEGIN SELECT RAISE(ABORT,'injected'); END;"
        };
        t.db.lock().unwrap().execute_batch(sql).unwrap();
        assert!(t.run(&mut b).is_err());
        let detail = t.record();
        if phase == "intent" {
            assert_eq!(detail.summary.item_state, "notStarted");
            assert_eq!(b.calls, 0);
            assert!(t.f.app.exists());
        } else {
            assert_eq!(detail.summary.item_state, "uncertain");
            assert_eq!(b.calls, 1);
            assert!(!t.f.app.exists());
        }
    }
}
#[test]
fn worker_faults_collect_only_after_native_work_is_terminal_and_never_rewrite_committed_facts() {
    for phase in [
        Phase::Begun,
        Phase::Intent,
        Phase::NativeReturned,
        Phase::Committed,
    ] {
        let t = Test::new();
        let mut b = t.backend();
        b.hook = Some(Box::new(move |at, _| {
            if at == phase {
                panic!("owned fixture fault");
            }
            Ok(())
        }));
        assert!(t.run(&mut b).is_err());
        let detail = t.record();
        let expected = match phase {
            Phase::Begun => "notStarted",
            Phase::Intent | Phase::NativeReturned => "uncertain",
            Phase::Committed => "moved",
            _ => unreachable!(),
        };
        assert_eq!(detail.summary.item_state, expected);
        assert_eq!(
            b.calls,
            usize::from(matches!(phase, Phase::NativeReturned | Phase::Committed))
        );
    }
}
#[test]
fn bound_jobs_share_exclusion_but_old_or_other_module_cancellations_do_not_cross_runs() {
    assert!(TaskKind::AppTrash.exclusive_operation());
    assert!(TaskKind::LogTrash.exclusive_operation());
    assert!(TaskKind::VisibilityProbe.exclusive_operation());
    assert!(!TaskKind::Apps.exclusive_operation());
    let old = Job::new(TaskKind::AppTrash);
    let newer = Job::new(TaskKind::AppTrash);
    old.bind_execution("old").unwrap();
    newer.bind_execution("new").unwrap();
    assert!(!newer.cancel_execution("old"));
    assert!(!newer.cancelled.load(Ordering::SeqCst));
    assert!(old.cancel_execution("old"));
    assert!(Job::new(TaskKind::Apps).bind_execution("new").is_err());
    let t = Test::new();
    let mut b = t.backend();
    let result = run_with(
        t.db.clone(),
        &t.id,
        t.f.plan.clone(),
        t.selection(),
        Arc::new(Job::new(TaskKind::Apps)),
        t.context(),
        &mut b,
    );
    assert!(result.is_err());
    assert!(journal::list(&*t.db.lock().unwrap()).unwrap().is_empty());
    assert_eq!(b.calls, 0);
}

#[test]
fn latest_context_is_reacquired_before_preparation_intent_and_native_call() {
    let t = Test::new();
    let mut b = t.backend();
    let counter = AtomicUsize::new(0);
    let context = t.context();
    let detail = run_with(
        t.db.clone(),
        &t.id,
        t.f.plan.clone(),
        t.selection(),
        t.job.clone(),
        || {
            counter.fetch_add(1, Ordering::SeqCst);
            context()
        },
        &mut b,
    )
    .unwrap();
    assert_eq!(detail.summary.item_state, "moved");
    assert!(counter.load(Ordering::SeqCst) >= 4);
}

#[tokio::test]
async fn lost_observer_keeps_live_job_and_update_lease_through_native_result_commit() {
    let t = Arc::new(Test::new());
    let mut backend = t.backend();
    let jobs = Arc::new(Mutex::new(HashMap::from([(t.id.clone(), t.job.clone())])));
    let installation = Arc::new(crate::update_install::Gate::default());
    let operation = installation.enter().unwrap();
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let mut entered = Some(entered);
    let (release, blocked) = std::sync::mpsc::channel();
    backend.hook = Some(Box::new(move |at, _| {
        if at == Phase::NativeReturned {
            let _ = entered.take().unwrap().send(());
            blocked
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        }
        Ok(())
    }));
    let work = t.clone();
    let response = dispatch(
        t.id.clone(),
        jobs.clone(),
        t.job.clone(),
        operation,
        async move {
            tokio::task::spawn_blocking(move || work.run(&mut backend))
                .await
                .unwrap()
        },
    );
    drop(response);
    tokio::time::timeout(std::time::Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap();
    assert!(jobs.lock().unwrap().contains_key(&t.id));
    assert!(installation.begin().is_err());
    assert_eq!(t.record().summary.item_state, "intent");
    assert!(!t.f.app.exists());
    t.job.cancel();
    release.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while jobs.lock().unwrap().contains_key(&t.id) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(t.record().summary.item_state, "moved");
    let lease = installation.begin().unwrap();
    drop(lease);
}
