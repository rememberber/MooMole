use super::*;
use crate::{
    db,
    engine::{Job, TaskKind},
    AppState,
};
use std::{path::PathBuf, sync::Arc};

fn register(state: &AppState, request: &str, kind: TaskKind) -> Result<Arc<Job>, String> {
    uuid::Uuid::parse_str(request).map_err(|_| "个人文件任务标识无效。")?;
    if !db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?.acknowledged {
        return Err("请先确认首次使用说明。".into());
    }
    let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
    if state.shutdown.closing()
        || jobs.len() >= 3
        || jobs.contains_key(request)
        || jobs.values().any(|job| job.kind.exclusive_operation())
    {
        return Err("其他任务正在运行或应用正在退出，请稍后重试。".into());
    }
    let job = Arc::new(Job::new(kind));
    jobs.insert(request.into(), job.clone());
    Ok(job)
}
fn finish(state: &AppState, request: &str, phase: &'static str, error: Option<String>) {
    if let Ok(mut store) = state.personal_files.lock() {
        if store.request_id.as_deref() == Some(request) {
            store.phase = phase;
            store.error = error;
        }
    }
    if let Ok(mut jobs) = state.jobs.lock() {
        jobs.remove(request);
    }
}
#[tauri::command]
pub(crate) fn get_personal_files(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    serde_json::to_value(
        state
            .personal_files
            .lock()
            .map_err(|e| e.to_string())?
            .snapshot(),
    )
    .map_err(|e| e.to_string())
}
#[tauri::command]
pub(crate) fn start_personal_scan(
    root: String,
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "文件扫描尚未结束，请稍后重试。")?;
    let settings = db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?;
    let job = register(&state, &request_id, TaskKind::PersonalScan)?;
    {
        let mut store = state.personal_files.lock().map_err(|e| e.to_string())?;
        *store = Store {
            phase: "scanning",
            request_id: Some(request_id.clone()),
            ..Default::default()
        };
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ownership = (operation, slot);
        let result = scan::collect(&PathBuf::from(root), &settings.protected_paths, job);
        match result {
            Ok(scan) => {
                if let Ok(mut store) = state.personal_files.lock() {
                    if store.request_id.as_deref() == Some(&request_id) {
                        store.scan = Some(scan);
                    }
                }
                finish(&state, &request_id, "ready", None);
            }
            Err(error) => finish(&state, &request_id, "failed", Some(error)),
        }
    });
    Ok(())
}
#[tauri::command]
pub(crate) fn prepare_personal_trash(
    scan_id: String,
    selected_ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Confirmation, String> {
    let _operation = state.updates.installation.enter()?;
    if state.shutdown.closing()
        || state
            .jobs
            .lock()
            .map_err(|e| e.to_string())?
            .values()
            .any(|job| job.kind.exclusive_operation())
    {
        return Err("请等待当前任务结束后再确认个人文件。".into());
    }
    let mut store = state.personal_files.lock().map_err(|e| e.to_string())?;
    if store.scan.as_ref().is_none_or(|scan| scan.id != scan_id) {
        return Err("个人文件扫描已变化，请重新选择。".into());
    }
    store.offer(&selected_ids, db::now())
}
#[tauri::command]
pub(crate) fn discard_personal_trash(
    confirmation_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state
        .personal_files
        .lock()
        .map_err(|e| e.to_string())?
        .offers
        .remove(&confirmation_id);
    Ok(())
}
#[tauri::command]
pub(crate) fn execute_personal_trash(
    confirmation_id: String,
    request_id: String,
    personal_files_confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if !personal_files_confirmed {
        return Err("请确认个人文件内容、影响和恢复方式。".into());
    }
    let operation = state.updates.installation.enter()?;
    let job = register(&state, &request_id, TaskKind::PersonalTrash)?;
    let taken = {
        state
            .personal_files
            .lock()
            .map_err(|e| e.to_string())?
            .take(&confirmation_id, db::now())
    };
    let plan = match taken {
        Ok(plan) => plan,
        Err(error) => {
            finish(&state, &request_id, "failed", Some(error.clone()));
            return Err(error);
        }
    };
    {
        let mut store = state.personal_files.lock().map_err(|e| e.to_string())?;
        store.phase = "processing";
        store.request_id = Some(request_id.clone());
        store.run_id = None;
        store.error = None;
        store.offers.clear();
        store.scan = None;
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let _operation = operation;
        let result = writer::run(state.clone(), plan, job, request_id.clone()).await;
        match result {
            Ok(_) => finish(&state, &request_id, "completed", None),
            Err(error) => finish(&state, &request_id, "failed", Some(error)),
        }
    });
    Ok(())
}
