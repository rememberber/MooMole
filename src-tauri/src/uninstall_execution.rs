//! Sealed ordinary-user, one-App execution from a one-use server selection.
//! No path/command/eligibility input. Native references stay on one blocking
//! worker; observer loss or late cancellation never abandons a native outcome.
use crate::{
    db,
    engine::{Job, TaskKind},
    uninstall_confirmation::Selection,
    uninstall_journal as journal,
    uninstall_plan::{Context, Plan},
    uninstall_runtime, uninstall_support,
    uninstall_target::{Attempt, PreparedBundle},
};
use std::{
    collections::HashMap,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{atomic::Ordering, Arc, Mutex},
};

struct Prerequisites {
    runtime: uninstall_runtime::Observation,
    support: uninstall_support::Observation,
}
impl Prerequisites {
    fn validate(&self, plan: &Plan, ctx: &Context) -> Result<(), String> {
        self.support.permits_confirmation(plan, ctx, db::now())?;
        self.runtime.permits_confirmation(plan, db::now())
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Begun,
    Prepared,
    Observed,
    Intent,
    NativeReturned,
    Committed,
}
trait Backend {
    fn observe(&mut self, plan: &Plan, ctx: &Context) -> Result<Prerequisites, String>;
    fn trash(
        &mut self,
        target: &PreparedBundle,
        gate: impl FnOnce() -> Result<(), String>,
    ) -> Result<Attempt, String>;
    fn checkpoint(&mut self, _phase: Phase, _id: &str) -> Result<(), String> {
        Ok(())
    }
}
struct Native;
impl Backend for Native {
    fn observe(&mut self, plan: &Plan, ctx: &Context) -> Result<Prerequisites, String> {
        let support = uninstall_support::observe(plan, ctx)?;
        let runtime = uninstall_runtime::observe(plan, ctx)?;
        Ok(Prerequisites { runtime, support })
    }
    fn trash(
        &mut self,
        target: &PreparedBundle,
        gate: impl FnOnce() -> Result<(), String>,
    ) -> Result<Attempt, String> {
        target.move_to_trash_checked(gate)
    }
}
fn bounded(raw: &str) -> String {
    let mut end = raw.len().min(1000);
    while !raw.is_char_boundary(end) {
        end -= 1;
    }
    let s = raw[..end].replace('\0', " ");
    if s.trim().is_empty() {
        "卸载条件无法确认，本项保留。".into()
    } else {
        s
    }
}
fn cancelled(job: &Job) -> bool {
    job.cancelled.load(Ordering::SeqCst)
}

pub(crate) fn dispatch(
    request_id: String,
    jobs: Arc<Mutex<HashMap<String, Arc<Job>>>>,
    job: Arc<Job>,
    operation: crate::update_install::Operation,
    work: impl std::future::Future<Output = Result<journal::Detail, String>> + Send + 'static,
) -> tokio::sync::oneshot::Receiver<Result<journal::Detail, String>> {
    let (send, receive) = tokio::sync::oneshot::channel();
    tauri::async_runtime::spawn(async move {
        let result = work.await;
        drop(operation);
        if let Ok(mut jobs) = jobs.lock() {
            if jobs
                .get(&request_id)
                .is_some_and(|current| Arc::ptr_eq(current, &job))
            {
                jobs.remove(&request_id);
            }
        }
        let _ = send.send(result);
    });
    receive
}

pub(crate) async fn run(
    database: Arc<Mutex<db::Database>>,
    request_id: String,
    plan: Plan,
    selection: Selection,
    job: Arc<Job>,
    context: impl Fn() -> Result<Context, String> + Send + 'static,
) -> Result<journal::Detail, String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_with(
            database,
            &request_id,
            plan,
            selection,
            job,
            context,
            &mut Native,
        )
    })
    .await
    .map_err(|_| "卸载 worker 响应中断，请按请求标识核对持久记录；不要自动重试。".to_string())?
}
fn run_with(
    database: Arc<Mutex<db::Database>>,
    request_id: &str,
    plan: Plan,
    selection: Selection,
    job: Arc<Job>,
    context: impl Fn() -> Result<Context, String>,
    backend: &mut impl Backend,
) -> Result<journal::Detail, String> {
    if job.kind != TaskKind::AppTrash {
        return Err("卸载只能通过专属执行任务提交。".into());
    }
    let initial = context()?;
    selection.validate(&plan, &initial, db::now())?;
    let session = journal::begin_bound(
        &mut *database.lock().map_err(|_| "无法取得卸载记录。")?,
        &plan,
        request_id,
        db::now(),
    )?;
    // The session survives panic collection. stop is called only AFTER this
    // blocking/native work has unwound, never while the OS call is in flight.
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
        job.bind_execution(session.id())?;
        backend.checkpoint(Phase::Begun, session.id())?;
        let prepared = (|| {
            if cancelled(&job) {
                return Err("卸载已取消。".into());
            }
            let ctx = context()?;
            selection.validate(&plan, &ctx, db::now())?;
            let target = PreparedBundle::prepare(&plan, &ctx)?;
            backend.checkpoint(Phase::Prepared, session.id())?;
            if cancelled(&job) {
                return Err("卸载已取消。".into());
            }
            let proof = backend.observe(&plan, &ctx)?;
            backend.checkpoint(Phase::Observed, session.id())?;
            let latest = context()?;
            selection.validate(&plan, &latest, db::now())?;
            plan.recheck(&latest, db::now())?;
            proof.validate(&plan, &latest)?;
            if cancelled(&job) {
                return Err("卸载已取消。".into());
            }
            Ok::<_, String>((target, proof))
        })();
        let (target, proof) = match prepared {
            Ok(value) => value,
            Err(reason) => {
                let mut db = database.lock().map_err(|_| "无法记录卸载保留结果。")?;
                if cancelled(&job) {
                    journal::cancel(&mut db, &session, db::now())?;
                } else {
                    journal::skip(&mut db, &session, &bounded(&reason), db::now())?;
                }
                return Ok(());
            }
        };
        journal::intent(
            &mut *database.lock().map_err(|_| "无法提交卸载意图。")?,
            &session,
            db::now(),
        )?;
        backend.checkpoint(Phase::Intent, session.id())?;
        let attempted = backend.trash(&target, || {
            if cancelled(&job) {
                return Err("卸载已取消，未调用原生处理。".into());
            }
            let latest = context()?;
            selection.validate(&plan, &latest, db::now())?;
            plan.recheck(&latest, db::now())?;
            proof.validate(&plan, &latest)?;
            journal::check_intent(
                &*database.lock().map_err(|_| "无法核对已提交卸载意图。")?,
                &session,
                db::now(),
            )?;
            if cancelled(&job) {
                return Err("卸载已取消，未调用原生处理。".into());
            }
            Ok(())
        });
        match attempted {
            Err(reason) => {
                let mut db = database.lock().map_err(|_| "无法记录原生调用前拒绝。")?;
                if cancelled(&job) {
                    journal::cancelled_before_call(&mut db, &session, db::now())?;
                } else {
                    journal::refused_before_call(&mut db, &session, &bounded(&reason), db::now())?;
                }
            }
            Ok(attempt) => {
                backend.checkpoint(Phase::NativeReturned, session.id())?;
                // Cancellation after the call cannot rewrite an actual move
                // or an unknown native outcome into an untouched item.
                journal::complete(
                    &mut *database.lock().map_err(|_| "无法记录卸载实际结果。")?,
                    &session,
                    attempt.outcome,
                    attempt.reason,
                    db::now(),
                )?;
            }
        }
        backend.checkpoint(Phase::Committed, session.id())?;
        Ok(())
    }))
    .unwrap_or_else(|_| Err("卸载 worker 异常结束。".into()));
    if let Err(error) = result {
        if let Ok(mut db) = database.lock() {
            let _ = journal::stop(&mut db, &session, db::now());
        }
        return Err(format!(
            "{} 请求 {} 的实际结果请在卸载记录中核对，不能自动重试。",
            bounded(&error),
            request_id
        ));
    }
    journal::detail(
        &*database.lock().map_err(|_| "无法读取卸载实际记录。")?,
        session.id(),
    )
}

#[cfg(test)]
mod tests;
