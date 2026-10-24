//! Detached observer, owned blocking worker, fixed per-step system calls.
use super::{
    quicklook::{Attempt, Outcome, Prepared, Step, Stop},
    refresh_journal as journal,
    refresh_plan::{Authorized, Context},
};
use crate::{
    db,
    engine::{Job, TaskKind},
};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{atomic::Ordering, Arc, Mutex},
};

fn bounded(value: &str) -> String {
    let mut end = value.len().min(900);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let value = value[..end].replace('\0', " ");
    if value.trim().is_empty() {
        "维护条件无法确认。".into()
    } else {
        value
    }
}
trait Backend {
    fn run(
        &mut self,
        target: &Prepared,
        step: Step,
        job: &Job,
        gate: impl FnOnce() -> Result<(), String>,
    ) -> Attempt;
}
struct Native;
impl Backend for Native {
    fn run(
        &mut self,
        target: &Prepared,
        step: Step,
        job: &Job,
        gate: impl FnOnce() -> Result<(), String>,
    ) -> Attempt {
        target.run(step, &job.cancelled, gate)
    }
}
pub(crate) async fn run(
    database: Arc<Mutex<db::Database>>,
    request: String,
    authorized: Authorized,
    job: Arc<Job>,
    context: impl Fn() -> Result<Context, String> + Send + 'static,
) -> Result<journal::Detail, String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_with(database, &request, authorized, job, context, &mut Native)
    })
    .await
    .map_err(|_| "维护 worker 响应中断，请按原请求核对结果，不要自动重试。".to_string())?
}
fn run_with(
    database: Arc<Mutex<db::Database>>,
    request: &str,
    authorized: Authorized,
    job: Arc<Job>,
    context: impl Fn() -> Result<Context, String>,
    backend: &mut impl Backend,
) -> Result<journal::Detail, String> {
    if job.kind != TaskKind::MaintenanceRefresh {
        return Err("请使用维护专属入口。".into());
    }
    let ctx = context()?;
    authorized.validate(&ctx)?;
    let target = Prepared::prepare(&ctx.home)?;
    let session = journal::begin(
        &mut *database.lock().map_err(|_| "无法建立维护记录。")?,
        &authorized.journal_selection()?,
        request,
        db::now(),
    )?;
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
        job.bind_execution(session.id())?;
        for step in authorized.selected() {
            if job.cancelled.load(Ordering::SeqCst) {
                break;
            }
            let mut gate_error = None;
            let mut attempted = backend.run(&target, *step, &job, || {
                let result: Result<(), String> = (|| {
                    if job.cancelled.load(Ordering::SeqCst) {
                        return Err("开始前已取消。".into());
                    }
                    authorized.validate(&context()?)?;
                    journal::intent(
                        &mut *database.lock().map_err(|_| "无法保存维护意图。")?,
                        &session,
                        *step,
                        db::now(),
                    )?;
                    authorized.validate(&context()?)?;
                    journal::check_intent(
                        &*database.lock().map_err(|_| "无法核对维护意图。")?,
                        &session,
                        *step,
                        db::now(),
                    )?;
                    if job.cancelled.load(Ordering::SeqCst) {
                        return Err("开始前已取消。".into());
                    }
                    Ok(())
                })();
                if let Err(ref e) = result {
                    gate_error = Some(bounded(e));
                }
                result
            });
            if attempted.outcome == Outcome::NotStarted && job.cancelled.load(Ordering::SeqCst) {
                attempted.stop = Some(Stop::Cancelled);
            }
            let why = gate_error.as_deref().unwrap_or(attempted.reason);
            journal::record_attempt_explained(
                &mut *database.lock().map_err(|_| "无法保存维护结果。")?,
                &session,
                &attempted,
                why,
                db::now(),
            )?;
            if attempted.outcome != Outcome::CommandSucceeded {
                break;
            }
        }
        journal::finish(
            &mut *database.lock().map_err(|_| "无法结束维护记录。")?,
            &session,
            job.cancelled.load(Ordering::SeqCst),
            db::now(),
        )
    }))
    .unwrap_or_else(|_| Err("维护 worker 异常结束。".into()));
    if let Err(e) = result {
        if let Ok(mut db) = database.lock() {
            let _ = journal::stop_worker(&mut db, &session, db::now());
        }
        return Err(format!(
            "{} 请求 {} 请核对持久结果，不要自动重试。",
            bounded(&e),
            request
        ));
    }
    journal::detail(
        &*database.lock().map_err(|_| "无法读取维护结果。")?,
        session.id(),
    )
}

#[cfg(test)]
mod tests;
