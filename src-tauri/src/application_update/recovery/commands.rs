use super::*;
use crate::AppState;
#[tauri::command]
pub(crate) async fn prepare_application_restoration(
    source_id: String,
    request_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Draft, String> {
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "其他文件检查正在进行。")?;
    let (job, registered) = super::super::commands::register(&state, &request_id)?;
    {
        let mut s = state
            .application_updates
            .lock()
            .map_err(|e| e.to_string())?;
        s.plan = None;
        s.restoration = None;
    }
    let original = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        super::super::journal::admission(&db)?;
        source(&db, &source_id)?
    };
    let current = app.config().identifier.clone();
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _resources = (operation, slot, registered);
        let plan = objc2::rc::autoreleasepool(|_| {
            Plan::prepare(
                original,
                crate::app_control::context(&state, &current)?,
                &job,
            )
        })?;
        {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            plan.binding(&db)?;
        }
        if !context_equal(
            &plan.context,
            &crate::app_control::context(&state, &current)?,
        ) {
            return Err("保护设置在准备期间变化，请重新检查旧版恢复。".into());
        }
        plan.time()?;
        cancelled(&job)?;
        let draft = plan.draft.clone();
        state
            .application_updates
            .lock()
            .map_err(|e| e.to_string())?
            .restoration = Some(plan);
        Ok(draft)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn discard_application_restoration(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut s = state
        .application_updates
        .lock()
        .map_err(|e| e.to_string())?;
    if s.restoration.as_ref().is_some_and(|p| p.draft.id == id) {
        s.restoration = None;
    }
    Ok(())
}
pub(super) fn run(
    plan: Plan,
    request_id: String,
    job: &Job,
    db: &std::sync::Mutex<db::Database>,
    ctx: impl Fn() -> Result<Context, String>,
) -> Result<Record, String> {
    let mut r = Record {
        id: plan.draft.id.clone(),
        request_id,
        draft: plan.draft.clone(),
        created_at: db::now(),
        finished_at: None,
        state: "running".into(),
        phase: "checking".into(),
        reason: "正在复查旧版恢复，尚未交换应用。".into(),
        receipt: None,
        reviewed_at: None,
    };
    {
        let mut c = db.lock().map_err(|e| e.to_string())?;
        plan.binding(&c)?;
        journal::begin(&mut c, &r)?;
    }
    let result = objc2::rc::autoreleasepool(|_| {
        perform(
            &plan,
            job,
            ctx,
            || plan.binding(&*db.lock().map_err(|e| e.to_string())?),
            &mut r,
            |r| journal::save(&*db.lock().map_err(|e| e.to_string())?, r),
        )
    });
    if let Err(e) = result {
        r.state = if r.phase == "intent" {
            "unknown"
        } else if job.cancelled.load(Ordering::SeqCst) {
            "cancelled"
        } else {
            "retained"
        }
        .into();
        r.reason = e;
    }
    r.phase = "finished".into();
    r.finished_at = Some(db::now().max(r.created_at));
    journal::save(&*db.lock().map_err(|e| e.to_string())?, &r)?;
    Ok(r)
}
#[tauri::command]
pub(crate) async fn execute_application_restoration(
    id: String,
    request_id: String,
    confirmed: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Record, String> {
    if !confirmed {
        return Err("请先确认恢复旧版、保留新版及应用数据不回退的边界。".into());
    }
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "其他文件检查正在进行。")?;
    let (job, registered) = super::super::commands::register(&state, &request_id)?;
    let plan = {
        let mut s = state
            .application_updates
            .lock()
            .map_err(|e| e.to_string())?;
        if s.restoration.as_ref().is_none_or(|p| p.draft.id != id) {
            return Err("旧版恢复确认已使用或撤销，请重新检查。".into());
        }
        let p = s.restoration.take().unwrap();
        p.time()?;
        p
    };
    let current = app.config().identifier.clone();
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _resources = (operation, slot, registered);
        run(plan, request_id, &job, &state.db, || {
            crate::app_control::context(&state, &current)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn get_application_restorations(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let active = state
        .jobs
        .lock()
        .map_err(|e| e.to_string())?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let records = journal::reconcile(&db, &active)?;
    let requests = records
        .iter()
        .filter(|r| r.state == "running" && active.contains(&r.request_id))
        .map(|r| r.request_id.clone())
        .collect::<Vec<_>>();
    Ok(serde_json::json!({"records":records,"activeRequests":requests}))
}
#[tauri::command]
pub(crate) fn acknowledge_application_restoration(
    id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let _operation = state.updates.installation.enter()?;
    if !confirmed {
        return Err("请先核对安装位置和保留包，不自动继续恢复。".into());
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut r = journal::list(&db)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("旧版恢复记录不存在。")?;
    if !["unknown", "interrupted"].contains(&r.state.as_str()) {
        return Err("此旧版恢复记录无需人工确认。".into());
    }
    r.reviewed_at = Some(db::now().max(r.created_at));
    journal::save(&db, &r)
}
