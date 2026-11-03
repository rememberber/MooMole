//! Main-App-only login registration. System state is authoritative; never replay.
use crate::{db, AppState};
use native::{Action, Reply, Status, System};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
mod native;
#[cfg(test)]
mod tests;
const SCHEMA:&str="CREATE TABLE IF NOT EXISTS main_app_login_records(id TEXT PRIMARY KEY,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=16384));";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Record {
    version: u32,
    id: String,
    enabled: bool,
    before: System,
    after: Option<System>,
    created_at: i64,
    finished_at: Option<i64>,
    state: String,
    error: Option<String>,
    native_code: Option<i64>,
    reviewed_at: Option<i64>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    system: Option<System>,
    records: Vec<Record>,
    active_requests: Vec<String>,
    error: Option<String>,
}
#[derive(Default)]
pub(crate) struct Controller {
    gate: Arc<tokio::sync::Mutex<()>>,
    active: Arc<std::sync::Mutex<Option<OwnedJob>>>,
}
struct OwnedJob {
    id: String,
    job: std::sync::Weak<crate::engine::Job>,
}
struct Active {
    current: Arc<std::sync::Mutex<Option<OwnedJob>>>,
    id: String,
}
impl Drop for Active {
    fn drop(&mut self) {
        if let Ok(mut current) = self.current.lock() {
            if current.as_ref().is_some_and(|v| v.id == self.id) {
                *current = None;
            }
        }
    }
}
fn validate(r: &Record) -> Result<(), String> {
    r.before.validate(&r.before.bundle_id)?;
    if r.version != 1
        || uuid::Uuid::parse_str(&r.id).is_err()
        || r.created_at < 0
        || !["running", "accepted", "noChange", "notStarted", "unknown"].contains(&r.state.as_str())
        || (r.state == "running") != r.finished_at.is_none()
        || r.finished_at.is_some_and(|v| v < r.created_at)
        || r.reviewed_at
            .is_some_and(|v| r.state != "unknown" || v < r.finished_at.unwrap_or(r.created_at))
        || r.native_code
            .is_some_and(|v| v.unsigned_abs() > 8_640_000_000_000_000)
        || r.error.as_ref().is_some_and(|v| {
            v.len() > 128
                || !v.starts_with("login")
                || !v.chars().all(|c| c.is_ascii_alphanumeric())
        })
        || (r.state == "running"
            && (r.after.is_some() || r.error.is_some() || r.native_code.is_some()))
        || (r.state == "noChange" && !matches_target(r.before.status, r.enabled))
        || (r.state == "notStarted"
            && (r.after.as_ref() != Some(&r.before)
                || r.error.as_deref() != Some("loginCancelledBeforeRequest")))
    {
        return Err("loginInvalidHistory".into());
    }
    if let Some(after) = &r.after {
        after.validate(&r.before.bundle_id)?;
        if after.bundle_path != r.before.bundle_path {
            return Err("loginInvalidHistory".into());
        }
    }
    if ["accepted", "noChange"].contains(&r.state.as_str())
        && (r.error.is_some()
            || r.after
                .as_ref()
                .is_none_or(|s| !matches_target(s.status, r.enabled)))
    {
        return Err("loginInvalidHistory".into());
    }
    Ok(())
}
fn records(c: &Connection) -> Result<Vec<Record>, String> {
    let exists:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='main_app_login_records')",[],|r|r.get(0)).map_err(|_|"loginStorageUnavailable")?;
    if !exists {
        return Ok(vec![]);
    }
    let mut q=c.prepare("SELECT id,CASE WHEN length(CAST(body AS BLOB))<=16384 THEN body END FROM main_app_login_records ORDER BY rowid DESC LIMIT 51").map_err(|_|"loginStorageUnavailable")?;
    let rows = q
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .map_err(|_| "loginStorageUnavailable")?;
    let mut result = vec![];
    for row in rows {
        let (id, raw) = row.map_err(|_| "loginStorageUnavailable")?;
        let r: Record = serde_json::from_str(&raw.ok_or("loginInvalidHistory")?)
            .map_err(|_| "loginInvalidHistory")?;
        validate(&r)?;
        if r.id != id {
            return Err("loginInvalidHistory".into());
        }
        result.push(r);
    }
    if result.len() > 50 {
        return Err("loginInvalidHistory".into());
    }
    Ok(result)
}
fn save(c: &Connection, r: &Record) -> Result<(), String> {
    validate(r)?;
    let body = serde_json::to_string(r).map_err(|_| "loginStorageUnavailable")?;
    c.execute("INSERT INTO main_app_login_records VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body",params![r.id,body]).map_err(|_|"loginStorageUnavailable")?;
    Ok(())
}
fn reconcile(c: &Connection, active: &[String]) -> Result<Vec<Record>, String> {
    let mut rows = records(c)?;
    for r in &mut rows {
        if r.state == "running" && !active.contains(&r.id) {
            r.state = "unknown".into();
            r.finished_at = Some(db::now().max(r.created_at));
            r.error = Some("loginInterrupted".into());
            save(c, r)?;
        }
    }
    Ok(rows)
}
fn matches_target(status: Status, enabled: bool) -> bool {
    if enabled {
        matches!(status, Status::Enabled | Status::RequiresApproval)
    } else {
        status == Status::NotRegistered
    }
}
fn begin(c: &mut Connection, id: &str, enabled: bool, before: System) -> Result<Record, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "loginInvalidRequest")?;
    before.validate(&before.bundle_id)?;
    if !db::settings(c)?.acknowledged || (before.status == Status::NotFound && !enabled) {
        return Err("loginUnavailable".into());
    }
    let rows = records(c)?;
    if rows.iter().any(|r| {
        r.id == id || r.state == "running" || (r.state == "unknown" && r.reviewed_at.is_none())
    }) {
        return Err("loginNeedsReview".into());
    }
    let sync: i64 = c
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(|_| "loginStorageUnavailable")?;
    if sync < 2 {
        return Err("loginStorageUnavailable".into());
    }
    let r = Record {
        version: 1,
        id: id.into(),
        enabled,
        before,
        after: None,
        created_at: db::now(),
        finished_at: None,
        state: "running".into(),
        error: None,
        native_code: None,
        reviewed_at: None,
    };
    let tx = c.transaction().map_err(|_| "loginStorageUnavailable")?;
    tx.execute_batch(SCHEMA)
        .map_err(|_| "loginStorageUnavailable")?;
    if rows.len() == 50 {
        let oldest = rows.last().unwrap();
        tx.execute(
            "DELETE FROM main_app_login_records WHERE id=?1",
            [&oldest.id],
        )
        .map_err(|_| "loginStorageUnavailable")?;
    }
    save(&tx, &r)?;
    tx.commit().map_err(|_| "loginStorageUnavailable")?;
    Ok(r)
}
fn finish(c: &Connection, mut r: Record, result: Result<Reply, String>) -> Result<Record, String> {
    r.finished_at = Some(db::now().max(r.created_at));
    match result {
        Ok(reply)
            if reply.system.validate(&r.before.bundle_id).is_ok()
                && reply.system.bundle_path == r.before.bundle_path =>
        {
            r.state = if reply.accepted
                && reply.error.is_none()
                && matches_target(reply.system.status, r.enabled)
            {
                if matches_target(r.before.status, r.enabled) {
                    "noChange"
                } else {
                    "accepted"
                }
            } else {
                "unknown"
            }
            .into();
            r.after = Some(reply.system);
            r.error = reply.error;
            r.native_code = reply.native_code;
            if r.state == "unknown" && r.error.is_none() {
                r.error = Some("loginResultUnknown".into());
            }
        }
        Ok(_) => {
            r.state = "unknown".into();
            r.error = Some("loginInvalidResponse".into());
        }
        Err(e) => {
            if e == "loginCancelledBeforeRequest" {
                r.state = "notStarted".into();
                r.after = Some(r.before.clone());
            } else {
                r.state = "unknown".into();
            }
            r.error = Some(e);
        }
    }
    save(c, &r)?;
    Ok(r)
}
fn apply(
    database: &std::sync::Mutex<db::Database>,
    id: &str,
    bundle: &str,
    enabled: bool,
    check: impl Fn() -> Result<(), String>,
    native: impl Fn(Action) -> Result<Reply, String>,
) -> Result<Record, String> {
    check()?;
    let reply = native(Action::Read)?;
    if !reply.accepted || reply.error.is_some() {
        return Err("loginNativeUnavailable".into());
    }
    reply.system.validate(bundle)?;
    check()?;
    let r = {
        let mut c = database.lock().map_err(|_| "loginStorageUnavailable")?;
        begin(&mut c, id, enabled, reply.system)?
    };
    let result = check().and_then(|_| {
        native(if enabled {
            Action::Enable
        } else {
            Action::Disable
        })
    });
    finish(
        &*database.lock().map_err(|_| "loginStorageUnavailable")?,
        r,
        result,
    )
}
fn acknowledge(c: &Connection, id: &str, system: &System) -> Result<(), String> {
    let mut r = records(c)?
        .into_iter()
        .find(|r| r.id == id && r.state == "unknown")
        .ok_or("loginInvalidRequest")?;
    system.validate(&r.before.bundle_id)?;
    if system.status == Status::NotFound {
        return Err("loginUnavailable".into());
    }
    r.reviewed_at = Some(db::now().max(r.finished_at.unwrap_or(r.created_at)));
    save(c, &r)
}
fn active(
    owned: Option<&OwnedJob>,
    jobs: &std::collections::HashMap<String, Arc<crate::engine::Job>>,
) -> Vec<String> {
    owned
        .filter(|o| {
            jobs.get(&o.id)
                .is_some_and(|j| o.job.ptr_eq(&Arc::downgrade(j)))
        })
        .map(|o| vec![o.id.clone()])
        .unwrap_or_default()
}
fn local_view(state: &AppState, control: &Controller) -> Result<View, String> {
    // Keep registration ownership fixed through reconciliation. A reader must
    // not see a newly written intent with an earlier empty ownership snapshot.
    let owned = control.active.lock().map_err(|_| "loginBusy")?;
    let ids = active(
        owned.as_ref(),
        &*state.jobs.lock().map_err(|_| "loginBusy")?,
    );
    let c = state.db.lock().map_err(|_| "loginStorageUnavailable")?;
    let rows = reconcile(&c, &ids)?;
    let active_requests = rows
        .iter()
        .filter(|r| r.state == "running" && ids.contains(&r.id))
        .map(|r| r.id.clone())
        .collect();
    Ok(View {
        system: None,
        records: rows,
        active_requests,
        error: None,
    })
}
#[tauri::command]
pub(crate) async fn get_main_app_login(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    control: tauri::State<'_, Controller>,
) -> Result<View, String> {
    let mut view = local_view(&state, &control)?;
    let guard = match control.gate.clone().try_lock_owned() {
        Ok(g) => g,
        Err(_) => {
            view.error = Some("loginBusy".into());
            return Ok(view);
        }
    };
    let operation = state.updates.installation.enter()?;
    let bundle = app.config().identifier.clone();
    let work = tauri::async_runtime::spawn_blocking(move || {
        let _resources = (guard, operation);
        native::call(&bundle, Action::Read).map(|r| r.system)
    });
    match tokio::time::timeout(std::time::Duration::from_secs(10), work).await {
        Ok(Ok(Ok(system))) => view.system = Some(system),
        Ok(Ok(Err(e))) => view.error = Some(e),
        _ => view.error = Some("loginReadPending".into()),
    }
    Ok(view)
}
#[tauri::command]
pub(crate) async fn set_main_app_login(
    enabled: bool,
    request_id: String,
    confirmed: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    control: tauri::State<'_, Controller>,
) -> Result<Record, String> {
    if !confirmed || state.shutdown.closing() {
        return Err("loginConfirmationRequired".into());
    }
    uuid::Uuid::parse_str(&request_id).map_err(|_| "loginInvalidRequest")?;
    let guard = control
        .gate
        .clone()
        .try_lock_owned()
        .map_err(|_| "loginBusy")?;
    let operation = state.updates.installation.enter()?;
    let (job, registered) = crate::application_update::commands::register(&state, &request_id)
        .map_err(|_| "loginBusy")?;
    *control.active.lock().map_err(|_| "loginBusy")? = Some(OwnedJob {
        id: request_id.clone(),
        job: Arc::downgrade(&job),
    });
    let active = Active {
        current: control.active.clone(),
        id: request_id.clone(),
    };
    let state = state.inner().clone();
    let bundle = app.config().identifier.clone();
    let work = tauri::async_runtime::spawn_blocking(move || {
        let _resources = (guard, operation, registered, active);
        // Intent is FULL-synced before the fixed system request. No replay or
        // forced cancellation of an already submitted ServiceManagement call.
        apply(
            &state.db,
            &request_id,
            &bundle,
            enabled,
            || {
                if job.cancelled.load(std::sync::atomic::Ordering::SeqCst)
                    || state.shutdown.closing()
                    || !db::settings(&*state.db.lock().map_err(|_| "loginStorageUnavailable")?)?
                        .acknowledged
                {
                    Err("loginCancelledBeforeRequest".into())
                } else {
                    Ok(())
                }
            },
            |action| native::call(&bundle, action),
        )
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), work)
        .await
        .map_err(|_| "loginRequestPending".to_string())?
        .map_err(|_| "loginResultUnknown".to_string())?
}
#[tauri::command]
pub(crate) async fn acknowledge_main_app_login(
    id: String,
    confirmed: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    control: tauri::State<'_, Controller>,
) -> Result<(), String> {
    if !confirmed || state.shutdown.closing() {
        return Err("loginConfirmationRequired".into());
    }
    let guard = control
        .gate
        .clone()
        .try_lock_owned()
        .map_err(|_| "loginBusy")?;
    let operation = state.updates.installation.enter()?;
    let state = state.inner().clone();
    let bundle = app.config().identifier.clone();
    let work = tauri::async_runtime::spawn_blocking(move || {
        let _resources = (guard, operation);
        let system = native::call(&bundle, Action::Read)?.system;
        acknowledge(
            &*state.db.lock().map_err(|_| "loginStorageUnavailable")?,
            &id,
            &system,
        )
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), work)
        .await
        .map_err(|_| "loginReadPending".to_string())?
        .map_err(|_| "loginResultUnknown".to_string())?
}
