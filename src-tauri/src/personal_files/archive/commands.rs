use super::*;
use crate::{engine::TaskKind, AppState};
#[tauri::command]
pub(crate) async fn prepare_personal_archive(
    scan_id: String,
    selected_ids: Vec<String>,
    directory: String,
    filename: String,
    state: tauri::State<'_, AppState>,
) -> Result<Draft, String> {
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "文件检查仍在进行。")?;
    if state.shutdown.closing()
        || state
            .jobs
            .lock()
            .map_err(|e| e.to_string())?
            .values()
            .any(|j| j.kind.exclusive_operation())
    {
        return Err("请等待当前任务结束后再确认归档。".into());
    }
    let settings = db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?;
    let source = {
        let mut store = state.personal_files.lock().map_err(|e| e.to_string())?;
        if store.scan.as_ref().is_none_or(|s| s.id != scan_id) {
            return Err("归档选择不属于当前扫描。".into());
        }
        let draft = store.offer(&selected_ids, db::now())?;
        store.take(&draft.id, db::now())?
    };
    let plan = tauri::async_runtime::spawn_blocking(move || {
        let _ownership = (operation, slot);
        prepare(source, &directory, &filename, &settings)
    })
    .await
    .map_err(|e| e.to_string())??;
    state
        .personal_archives
        .lock()
        .map_err(|e| e.to_string())?
        .publish(plan)
}
#[tauri::command]
pub(crate) fn discard_personal_archive(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state
        .personal_archives
        .lock()
        .map_err(|e| e.to_string())?
        .plans
        .remove(&id);
    Ok(())
}
#[tauri::command]
pub(crate) fn get_personal_archives(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Record>, String> {
    journal::list(&*state.db.lock().map_err(|e| e.to_string())?)
}
#[tauri::command]
pub(crate) fn acknowledge_personal_archive(
    id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Record, String> {
    if !confirmed {
        return Err("请先核对目标和临时归档，再确认已检查。".into());
    }
    journal::acknowledge(&*state.db.lock().map_err(|e| e.to_string())?, &id)
}
#[tauri::command]
pub(crate) async fn execute_personal_archive(
    id: String,
    request_id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Record, String> {
    if !confirmed {
        return Err("请先确认归档内容、目标和原文件保留方式。".into());
    }
    uuid::Uuid::parse_str(&request_id).map_err(|_| "归档请求标识无效。")?;
    let operation = state.updates.installation.enter()?;
    let slot = state
        .archive_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "另一项归档仍在读写。")?;
    let plan = state
        .personal_archives
        .lock()
        .map_err(|e| e.to_string())?
        .take(&id)?;
    let job = Arc::new(Job::new(TaskKind::PersonalArchive));
    {
        let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
        if state.shutdown.closing()
            || jobs.len() >= 3
            || jobs.contains_key(&request_id)
            || jobs.values().any(|j| j.kind.exclusive_operation())
        {
            return Err("请等待当前任务结束后重新确认归档。".into());
        }
        jobs.insert(request_id.clone(), job.clone());
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ownership = (operation, slot);
        let result = run(state.db.clone(), plan, request_id.clone(), job);
        if let Ok(mut jobs) = state.jobs.lock() {
            jobs.remove(&request_id);
        }
        result
    })
    .await
    .map_err(|_| "归档响应中断，请在文件归档记录中核对；不会自动重试。".to_string())?
}
