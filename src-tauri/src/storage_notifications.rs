//! Explicit opt-in alerts from fresh local capacity observations. No file actions.
use crate::{cleanup_space::Sample, db, language, AppState};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::Manager;
mod native;
const SCHEMA:&str="CREATE TABLE IF NOT EXISTS storage_notification_state(id INTEGER PRIMARY KEY CHECK(id=1),version INTEGER NOT NULL,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=4096)); CREATE TABLE IF NOT EXISTS storage_notification_records(id TEXT PRIMARY KEY,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=4096));";
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Saved {
    enabled: bool,
    gate: Gate,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Gate {
    key: String,
    observed_at: i64,
    latched: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    id: String,
    created_at: i64,
    finished_at: Option<i64>,
    kind: String,
    state: String,
    available_bytes: Option<u64>,
    threshold_bytes: Option<u64>,
    error: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    enabled: bool,
    monitoring_enabled: bool,
    permission: Option<native::Permission>,
    error: Option<String>,
    records: Vec<Record>,
}
#[derive(Default)]
pub(crate) struct Controller {
    gate: tokio::sync::Mutex<()>,
}
fn exists(c: &Connection) -> Result<bool, String> {
    c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='storage_notification_state')",[],|r|r.get(0)).map_err(|_|"storageUnavailable".into())
}
fn load(c: &Connection) -> Result<Saved, String> {
    if !exists(c)? {
        return Ok(Saved::default());
    }
    let raw:Option<(u32,Option<String>)>=c.query_row("SELECT version,CASE WHEN length(CAST(body AS BLOB))<=4096 THEN body END FROM storage_notification_state WHERE id=1",[],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|_|"storageUnavailable")?;
    let Some((version, body)) = raw else {
        return Ok(Saved::default());
    };
    if version != 1 {
        return Err("unsupportedSettings".into());
    }
    let saved: Saved =
        serde_json::from_str(&body.ok_or("invalidSettings")?).map_err(|_| "invalidSettings")?;
    if saved.gate.key.len() > 512
        || saved.gate.observed_at < 0
        || (!saved.gate.key.is_empty() && saved.gate.key.split('|').count() != 3)
    {
        return Err("invalidSettings".into());
    }
    Ok(saved)
}
fn write(c: &Connection, saved: &Saved) -> Result<(), String> {
    c.execute_batch(SCHEMA).map_err(|_| "storageUnavailable")?;
    c.execute("INSERT INTO storage_notification_state VALUES(1,1,?1) ON CONFLICT(id) DO UPDATE SET version=1,body=excluded.body",[serde_json::to_string(saved).map_err(|_|"storageUnavailable")?]).map_err(|_|"storageUnavailable")?;
    Ok(())
}
fn configure(
    c: &mut Connection,
    enabled: bool,
    permission: Option<&native::Permission>,
) -> Result<(), String> {
    let mut saved = load(c)?;
    if !db::settings(c)?.acknowledged {
        return Err("firstUseRequired".into());
    }
    if enabled && permission.is_none_or(|p| !p.allowed()) {
        return Err("permissionDenied".into());
    }
    saved.enabled = enabled;
    let tx = c.transaction().map_err(|_| "storageUnavailable")?;
    write(&tx, &saved)?;
    tx.commit().map_err(|_| "storageUnavailable".into())
}
fn records(c: &Connection) -> Result<Vec<Record>, String> {
    if !exists(c)? {
        return Ok(vec![]);
    }
    let mut q = c
        .prepare("SELECT CASE WHEN length(CAST(body AS BLOB))<=4096 THEN body END FROM storage_notification_records ORDER BY rowid DESC LIMIT 51")
        .map_err(|_| "storageUnavailable")?;
    let rows = q
        .query_map([], |r| r.get::<_, Option<String>>(0))
        .map_err(|_| "storageUnavailable")?;
    let mut out = vec![];
    for row in rows {
        let body = row
            .map_err(|_| "storageUnavailable")?
            .ok_or("invalidRecords")?;
        if body.len() > 4096 {
            return Err("invalidRecords".into());
        }
        let r: Record = serde_json::from_str(&body).map_err(|_| "invalidRecords")?;
        if uuid::Uuid::parse_str(&r.id).is_err()
            || r.created_at < 0
            || r.created_at > 8_640_000_000_000_000
            || !["lowSpace", "test"].contains(&r.kind.as_str())
            || !["submitting", "submitted", "blocked", "unknown"].contains(&r.state.as_str())
            || (r.state == "submitting") != r.finished_at.is_none()
            || r.finished_at
                .is_some_and(|t| t < r.created_at || t > 8_640_000_000_000_000)
            || (["submitting", "submitted"].contains(&r.state.as_str()) && r.error.is_some())
            || (["blocked", "unknown"].contains(&r.state.as_str())
                && r.error
                    .as_ref()
                    .is_none_or(|s| s.is_empty() || s.len() > 1024))
            || (r.kind == "test" && (r.available_bytes.is_some() || r.threshold_bytes.is_some()))
            || (r.kind == "lowSpace"
                && (r.threshold_bytes.is_none_or(|n| {
                    ![
                        5 * 1024_u64.pow(3),
                        10 * 1024_u64.pow(3),
                        20 * 1024_u64.pow(3),
                    ]
                    .contains(&n)
                }) || r
                    .available_bytes
                    .is_none_or(|n| n >= r.threshold_bytes.unwrap_or(0))))
        {
            return Err("invalidRecords".into());
        }
        out.push(r);
    }
    if out.len() > 50 {
        return Err("invalidRecords".into());
    }
    Ok(out)
}
fn save_record(c: &Connection, r: &Record) -> Result<(), String> {
    c.execute("INSERT INTO storage_notification_records VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body",rusqlite::params![r.id,serde_json::to_string(r).map_err(|_|"storageUnavailable")?]).map_err(|_|"storageUnavailable")?;
    c.execute("DELETE FROM storage_notification_records WHERE id NOT IN (SELECT id FROM storage_notification_records ORDER BY rowid DESC LIMIT 50)",[]).map_err(|_|"storageUnavailable")?;
    Ok(())
}
fn recover(c: &mut Connection, now: i64) -> Result<(), String> {
    let _ = load(c)?;
    let old = records(c)?;
    let tx = c.transaction().map_err(|_| "storageUnavailable")?;
    for mut r in old {
        if r.state == "submitting" {
            r.state = "unknown".into();
            r.finished_at = Some(now.max(r.created_at));
            r.error = Some("interrupted".into());
            save_record(&tx, &r)?;
        }
    }
    tx.commit().map_err(|_| "storageUnavailable".into())
}
// Same threshold/volume episode, one submission. Failures consume the episode
// too: unknown completion must never lead to an automatic duplicate.
fn decision(saved: &mut Saved, view: &crate::storage_monitor::View, now: i64) -> Option<Sample> {
    if !saved.enabled || !view.config.enabled || view.error.is_some() || view.collecting {
        return None;
    }
    let sample = view.samples.last()?;
    if !sample.valid()
        || now < sample.measured_at
        || now.saturating_sub(sample.measured_at) > i64::from(view.config.interval_minutes) * 120000
    {
        return None;
    }
    let key = format!(
        "{}|{}|{}",
        sample.volume_id, sample.total_bytes, view.config.warn_below_bytes
    );
    // Wall-clock rollback cannot make an older observation look newly actionable.
    if sample.measured_at <= saved.gate.observed_at {
        return None;
    }
    if key != saved.gate.key {
        saved.gate.key = key;
        saved.gate.latched = false;
    }
    saved.gate.observed_at = sample.measured_at;
    if sample.available_bytes
        >= view
            .config
            .warn_below_bytes
            .saturating_add(1024 * 1024 * 1024)
    {
        saved.gate.latched = false;
    }
    if sample.available_bytes < view.config.warn_below_bytes && !saved.gate.latched {
        saved.gate.latched = true;
        return Some(sample.clone());
    }
    None
}
fn begin(
    c: &mut Connection,
    view: Option<&crate::storage_monitor::View>,
    now: i64,
) -> Result<Option<Record>, String> {
    let mut saved = load(c)?;
    let previous = saved.clone();
    if !db::settings(c)?.acknowledged {
        return Err("firstUseRequired".into());
    }
    let sync: i64 = c
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(|_| "storageUnavailable")?;
    if sync < 2 {
        return Err("storageUnavailable".into());
    }
    let sample = if let Some(v) = view {
        decision(&mut saved, v, now)
    } else {
        None
    };
    if view.is_some() && sample.is_none() {
        if !saved.enabled || saved == previous {
            return Ok(None);
        }
        let tx = c.transaction().map_err(|_| "storageUnavailable")?;
        write(&tx, &saved)?;
        tx.commit().map_err(|_| "storageUnavailable")?;
        return Ok(None);
    }
    let row = Record {
        id: uuid::Uuid::new_v4().to_string(),
        created_at: now,
        finished_at: None,
        kind: if view.is_some() { "lowSpace" } else { "test" }.into(),
        state: "submitting".into(),
        available_bytes: sample.as_ref().map(|s| s.available_bytes),
        threshold_bytes: view.map(|v| v.config.warn_below_bytes),
        error: None,
    };
    let tx = c.transaction().map_err(|_| "storageUnavailable")?;
    write(&tx, &saved)?;
    save_record(&tx, &row)?;
    tx.commit().map_err(|_| "storageUnavailable")?;
    Ok(Some(row))
}
fn finish(
    c: &mut Connection,
    mut row: Record,
    result: Result<(), String>,
    now: i64,
) -> Result<(), String> {
    row.finished_at = Some(now.max(row.created_at));
    row.state = match result.as_ref() {
        Ok(_) => "submitted",
        Err(e) if ["permissionDenied", "requiresPackagedApp"].contains(&e.as_str()) => "blocked",
        Err(_) => "unknown",
    }
    .into();
    row.error = result.err();
    let tx = c.transaction().map_err(|_| "storageUnavailable")?;
    save_record(&tx, &row)?;
    tx.commit().map_err(|_| "storageUnavailable".into())
}
async fn permission(
    app: &tauri::AppHandle,
    state: &AppState,
    request: bool,
) -> Result<native::Permission, String> {
    let bundle = app.config().identifier.clone();
    let operation = state.updates.installation.enter()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _keep = operation;
        native::permission(&bundle, request)
    })
    .await
    .map_err(|_| "nativeUnavailable".to_owned())?
}
async fn view(app: &tauri::AppHandle, state: &AppState) -> Result<View, String> {
    let (saved, rows, monitoring) = {
        let c = state.db.lock().map_err(|_| "storageUnavailable")?;
        (
            load(&c)?,
            records(&c)?,
            state.storage_monitor.view(&c)?.config.enabled,
        )
    };
    let result = permission(app, state, false).await;
    Ok(View {
        enabled: saved.enabled,
        monitoring_enabled: monitoring,
        permission: result.as_ref().ok().cloned(),
        error: result.err(),
        records: rows,
    })
}
fn wording(r: &Record, preference: &str) -> (String, String) {
    let title = language::text(
        preference,
        "MooMole 空间提醒",
        "MooMole space reminder",
        "MooMole 空き容量のお知らせ",
    )
    .into();
    let body = if r.kind == "test" {
        language::text(preference,"这是一条测试提醒。打开 MooMole 的持续管理查看空间；未处理任何文件。","This is a test reminder. Open Ongoing Management in MooMole to check space. No files were processed.","テスト通知です。MooMole の継続管理で空き容量を確認できます。ファイルは処理していません。").into()
    } else {
        let available = r.available_bytes.unwrap_or(0) as f64 / 1024_f64.powi(3);
        let threshold = r.threshold_bytes.unwrap_or(0) / 1024_u64.pow(3);
        match language::resolve(preference,&language::system_language()){"zh"=>format!("最近观察：账户所在卷可用空间 {available:.1} GiB，低于 {threshold} GiB。打开 MooMole 的持续管理查看与整理。"),"ja"=>format!("直近の記録：アカウントのあるボリュームの空き容量は {available:.1} GiB（しきい値 {threshold} GiB 未満）です。MooMole の継続管理で確認できます。"),_=>format!("Recent observation: {available:.1} GiB available on your account’s volume, below {threshold} GiB. Open Ongoing Management in MooMole to review.")}
    };
    (title, body)
}
async fn submit(app: &tauri::AppHandle, state: &AppState, row: Record) -> Result<(), String> {
    let operation = state.updates.installation.enter();
    let bundle = app.config().identifier.clone();
    let preference = language::saved(state);
    let (title, body) = wording(&row, &preference);
    let id = format!("moomole-space-{}", row.id);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _keep = operation.map_err(|_| "closing")?;
        native::send(&bundle, &id, &title, &body)
    })
    .await
    .unwrap_or_else(|_| Err("submissionUnknown".into()));
    finish(
        &mut *state.db.lock().map_err(|_| "storageUnavailable")?,
        row,
        result,
        db::now(),
    )
}
#[tauri::command]
pub(crate) async fn get_storage_notifications(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    control: tauri::State<'_, Controller>,
) -> Result<View, String> {
    let _gate = control.gate.try_lock().map_err(|_| "notificationBusy")?;
    view(&app, &state).await
}
#[tauri::command]
pub(crate) async fn request_storage_notification_permission(
    confirmed: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    control: tauri::State<'_, Controller>,
) -> Result<View, String> {
    if !confirmed || state.shutdown.closing() {
        return Err("confirmationRequired".into());
    }
    let _gate = control.gate.try_lock().map_err(|_| "notificationBusy")?;
    {
        let c = state.db.lock().map_err(|_| "storageUnavailable")?;
        let _ = load(&c)?;
        if !db::settings(&c)?.acknowledged {
            return Err("firstUseRequired".into());
        }
    }
    let _ = permission(&app, &state, true).await?;
    view(&app, &state).await
}
#[tauri::command]
pub(crate) async fn configure_storage_notifications(
    enabled: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    control: tauri::State<'_, Controller>,
) -> Result<View, String> {
    if state.shutdown.closing() {
        return Err("closing".into());
    }
    let _gate = control.gate.try_lock().map_err(|_| "notificationBusy")?;
    let p = if enabled {
        Some(permission(&app, &state, false).await?)
    } else {
        None
    };
    configure(
        &mut *state.db.lock().map_err(|_| "storageUnavailable")?,
        enabled,
        p.as_ref(),
    )?;
    view(&app, &state).await
}
#[tauri::command]
pub(crate) async fn test_storage_notification(
    confirmed: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    control: tauri::State<'_, Controller>,
) -> Result<View, String> {
    if !confirmed || state.shutdown.closing() {
        return Err("confirmationRequired".into());
    }
    let _gate = control.gate.try_lock().map_err(|_| "notificationBusy")?;
    let p = permission(&app, &state, false).await?;
    if !p.allowed() {
        return Err("permissionDenied".into());
    }
    let row = begin(
        &mut *state.db.lock().map_err(|_| "storageUnavailable")?,
        None,
        db::now(),
    )?
    .ok_or("storageUnavailable")?;
    submit(&app, &state, row).await?;
    view(&app, &state).await
}
pub(crate) fn start(app: &tauri::AppHandle) {
    app.manage(Controller::default());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let _ = recover(
            &mut *state.db.lock().unwrap_or_else(|e| e.into_inner()),
            db::now(),
        );
        loop {
            if state.shutdown.closing() {
                break;
            }
            let control = app.state::<Controller>();
            if let Ok(_guard) = control.gate.try_lock() {
                let row = (|| {
                    let mut c = state.db.lock().map_err(|_| "storageUnavailable")?;
                    if !load(&c)?.enabled {
                        return Ok(None);
                    }
                    let v = state.storage_monitor.view(&c)?;
                    begin(&mut c, Some(&v), db::now())
                })();
                if let Ok(Some(row)) = row {
                    if !state.shutdown.closing() {
                        let _ = submit(&app, &state, row).await;
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    });
}
#[cfg(test)]
mod tests;
