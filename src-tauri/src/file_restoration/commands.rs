use super::*;
use crate::{engine::TaskKind, AppState};

#[tauri::command]
pub(crate) async fn prepare_file_restoration(
    run_id: String,
    item_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Draft, String> {
    let operation = state.updates.installation.enter()?;
    let permit = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "文件检查仍在进行。")?;
    let (detail, settings) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        (execution::detail(&db, &run_id)?, db::settings(&db)?)
    };
    let home = state.home.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || {
        let (_operation, _permit) = (operation, permit);
        prepare(&detail, &item_id, &home, &settings)
    })
    .await
    .map_err(|e| e.to_string())??;
    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        plan.validate_source(&db)?;
        plan.validate(&state.home, &db::settings(&db)?)?;
    }
    Ok(state
        .file_restorations
        .lock()
        .map_err(|e| e.to_string())?
        .publish(plan))
}
#[tauri::command]
pub(crate) fn discard_file_restoration(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state
        .file_restorations
        .lock()
        .map_err(|e| e.to_string())?
        .discard(&id);
    Ok(())
}
#[tauri::command]
pub(crate) async fn execute_file_restoration(
    id: String,
    request_id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<execution::Detail, String> {
    if !confirmed {
        return Err("请先确认当前文件与原位置。".into());
    }
    uuid::Uuid::parse_str(&request_id).map_err(|_| "恢复请求标识无效。")?;
    let operation = state.updates.installation.enter()?;
    let plan = state
        .file_restorations
        .lock()
        .map_err(|e| e.to_string())?
        .take(&id)?;
    let job = Arc::new(Job::new(TaskKind::FileRestore));
    {
        let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
        if state.shutdown.closing()
            || jobs.len() >= 3
            || jobs.contains_key(&request_id)
            || jobs.values().any(|j| j.kind.exclusive_operation())
        {
            return Err("请等待当前任务结束后再恢复。".into());
        }
        jobs.insert(request_id.clone(), job.clone());
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let result = run(state.db.clone(), state.home.clone(), plan, job);
        if let Ok(mut jobs) = state.jobs.lock() {
            jobs.remove(&request_id);
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
