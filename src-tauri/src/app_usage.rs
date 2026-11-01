//! On-demand usage hints from exact retained Mole rows. Never removal authority.
use crate::{
    app_inspection::identity,
    engine::{Job, TaskKind},
    uninstall_plan::Source,
    AppState,
};
use serde::Serialize;
use std::{
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
mod native;
#[cfg(test)]
mod tests;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Row {
    index: usize,
    path: String,
    bundle_id: String,
    state: &'static str,
    last_opened_at: Option<i64>,
    issue: Option<&'static str>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    version: u32,
    task_id: String,
    request_id: String,
    checked_at: i64,
    expires_at: i64,
    requested_indices: Vec<usize>,
    rows: Vec<Row>,
    complete: bool,
    cancelled: bool,
}
#[derive(Clone, Debug)]
enum Date {
    Value(f64),
    Missing,
    Unknown(&'static str),
}
#[cfg(test)]
pub(crate) fn native_fixture_hint(source: &Source) -> serde_json::Value {
    serde_json::to_value(observe(source, &[], native::read)).unwrap()
}
fn opened(date: Date, now: i64) -> Result<i64, &'static str> {
    match date {
        Date::Value(seconds) => {
            let ms = (seconds + 978307200.0) * 1000.;
            if !ms.is_finite() || ms < 0. || ms > now as f64 {
                return Err("invalidDate");
            }
            Ok(ms.floor() as i64)
        }
        Date::Missing => Err("notReported"),
        Date::Unknown(reason) => Err(reason),
    }
}
fn observe(
    source: &Source,
    rules: &[String],
    read: impl FnOnce(&str, &str) -> (Date, Option<bool>),
) -> Row {
    let (path, id, _, _) = source.control_target();
    let (_, index) = source.address();
    let mut row = Row {
        index,
        path: path.into(),
        bundle_id: id.into(),
        state: "unknown",
        last_opened_at: None,
        issue: Some("identityUnavailable"),
    };
    if !crate::app_protection::check(path, Some(id), None, "", rules)
        .is_ok_and(|p| p.reasons.is_empty())
    {
        row.state = "protected";
        row.issue = Some("protected");
        return row;
    }
    let Ok(before) = identity::capture_control(Path::new(path), id) else {
        return row;
    };
    let (date, running) = read(path, id);
    if identity::capture_control(Path::new(path), id).ok().as_ref() != Some(&before) {
        row.issue = Some("identityChanged");
        return row;
    }
    if running == Some(true) {
        row.state = "running";
        row.issue = None;
        row.last_opened_at = opened(date, crate::db::now()).ok();
        return row;
    }
    if running.is_none() {
        row.issue = Some("runningUnknown");
        return row;
    }
    match opened(date, crate::db::now()) {
        Ok(time) => {
            row.state = "observed";
            row.last_opened_at = Some(time);
            row.issue = None;
        }
        Err(issue) => row.issue = Some(issue),
    }
    row
}
fn inspect(
    task_id: String,
    request_id: String,
    sources: Vec<serde_json::Value>,
    indices: Vec<usize>,
    rules: Vec<String>,
    job: &Job,
) -> Report {
    let began = Instant::now();
    let mut rows = vec![];
    let requested_indices = indices.clone();
    for (index, raw) in indices.into_iter().zip(sources) {
        if job.cancelled.load(Ordering::SeqCst) || began.elapsed() >= Duration::from_secs(20) {
            break;
        }
        rows.push(match Source::from_row(&task_id, index, &raw) {
            Ok(source) => observe(&source, &rules, native::read),
            Err(_) => Row {
                index,
                path: raw
                    .get("path")
                    .and_then(|s| s.as_str())
                    .filter(|s| s.len() <= 4096)
                    .unwrap_or("")
                    .into(),
                bundle_id: raw
                    .get("bundle_id")
                    .and_then(|s| s.as_str())
                    .filter(|s| s.len() <= 255)
                    .unwrap_or("")
                    .into(),
                state: "unknown",
                last_opened_at: None,
                issue: Some("identityUnavailable"),
            },
        });
    }
    let now = crate::db::now();
    Report {
        version: 1,
        task_id,
        request_id,
        checked_at: now,
        expires_at: now + 300000,
        complete: rows.len() == requested_indices.len()
            && !job.cancelled.load(Ordering::SeqCst)
            && began.elapsed() < Duration::from_secs(20),
        requested_indices,
        rows,
        cancelled: job.cancelled.load(Ordering::SeqCst),
    }
}
struct Registered {
    state: AppState,
    id: String,
}
impl Drop for Registered {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.state.jobs.lock() {
            jobs.remove(&self.id);
        }
    }
}
struct Stop(Arc<Job>);
impl Drop for Stop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
#[tauri::command]
pub(crate) async fn inspect_application_usage(
    task_id: String,
    request_id: String,
    indices: Vec<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<Report, String> {
    uuid::Uuid::parse_str(&task_id).map_err(|_| "应用清单标识无效。")?;
    uuid::Uuid::parse_str(&request_id).map_err(|_| "使用线索请求标识无效。")?;
    if indices.is_empty()
        || indices.len() > 512
        || indices
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != indices.len()
    {
        return Err("每轮最多检查 512 个确切应用，不能重复。".into());
    }
    let operation = state.updates.installation.enter()?;
    let slot = state
        .usage_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "应用使用线索仍在读取，请稍后重试。")?;
    let sources = {
        let mut results = state.results.lock().map_err(|e| e.to_string())?;
        results.app_usage_rows(&task_id, &indices)?
    };
    let rules = crate::db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?.protected_paths;
    let job = Arc::new(Job::new(TaskKind::Apps));
    {
        let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
        if state.shutdown.closing()
            || jobs.len() >= 3
            || jobs.contains_key(&request_id)
            || jobs.values().any(|j| j.kind.exclusive_operation())
        {
            return Err("请先结束当前任务，再检查应用使用线索。".into());
        }
        jobs.insert(request_id.clone(), job.clone());
    }
    let registered = Registered {
        state: state.inner().clone(),
        id: request_id.clone(),
    };
    let stop = Stop(job.clone());
    let original = sources.clone();
    let requested = indices.clone();
    let task = task_id.clone();
    let request = request_id.clone();
    let control = job.clone();
    let rules_copy = rules.clone();
    let mut worker = tokio::task::spawn_blocking(move || {
        let _resources = (registered, slot, operation);
        inspect(task, request, sources, indices, rules_copy, &control)
    });
    let report = tokio::select! {r=&mut worker=>r.map_err(|_|"应用使用线索读取未完成。")?,_=tokio::time::sleep(Duration::from_secs(25))=>return Err("应用使用线索读取超时；仍在返回的系统读取保持占用，请稍后重试。".into())};
    drop(stop);
    if crate::db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?.protected_paths != rules
    {
        return Err("保护范围已变化，请重新检查使用线索。".into());
    }
    let mut results = state.results.lock().map_err(|e| e.to_string())?;
    if results.app_usage_rows(&task_id, &requested)? != original {
        return Err("应用清单已变化，请重新检查使用线索。".into());
    }
    Ok(report)
}
