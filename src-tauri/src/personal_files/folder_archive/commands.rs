use super::*;
use crate::AppState;
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
fn register(state: &AppState, id: &str) -> Result<(Arc<Job>, Registered), String> {
    uuid::Uuid::parse_str(id).map_err(|_| "文件夹归档请求标识无效。")?;
    let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
    if state.shutdown.closing()
        || jobs.len() >= 3
        || jobs.contains_key(id)
        || jobs.values().any(|j| j.kind.exclusive_operation())
    {
        return Err("请先结束当前任务，再创建文件夹归档。".into());
    }
    let job = Arc::new(Job::new(TaskKind::PersonalArchive));
    jobs.insert(id.into(), job.clone());
    Ok((
        job,
        Registered {
            state: state.clone(),
            id: id.into(),
        },
    ))
}
#[tauri::command]
pub(crate) async fn prepare_folder_archive(
    root: String,
    directory: String,
    filename: String,
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Draft, String> {
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "其他文件检查仍在进行。")?;
    let (job, registered) = register(&state, &request_id)?;
    state
        .personal_archives
        .lock()
        .map_err(|e| e.to_string())?
        .folder_plan = None;
    let settings = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        journal::admission(&db)?;
        archive::journal::admission(&db)?;
        db::settings(&db)?
    };
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _resources = (operation, slot, registered);
        let plan = Plan::prepare(Path::new(&root), &directory, &filename, &settings, &job)?;
        if db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?.protected_paths
            != settings.protected_paths
        {
            return Err("保护设置在检查期间变化，请重新准备文件夹归档。".into());
        }
        plan.time()?;
        archive::check(&job, Instant::now())?;
        let draft = plan.draft.clone();
        state
            .personal_archives
            .lock()
            .map_err(|e| e.to_string())?
            .folder_plan = Some(plan);
        Ok(draft)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn discard_folder_archive(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut s = state.personal_archives.lock().map_err(|e| e.to_string())?;
    if s.folder_plan.as_ref().is_some_and(|p| p.draft.id == id) {
        s.folder_plan = None;
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn execute_folder_archive(
    id: String,
    request_id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Record, String> {
    if !confirmed {
        return Err("请先确认整份文件夹、隐藏内容、元数据和原件保留方式。".into());
    }
    let operation = state.updates.installation.enter()?;
    let slot = state
        .archive_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "另一项归档仍在读写。")?;
    let (job, registered) = register(&state, &request_id)?;
    let plan = {
        let mut s = state.personal_archives.lock().map_err(|e| e.to_string())?;
        if s.folder_plan.as_ref().is_none_or(|p| p.draft.id != id) {
            return Err("文件夹归档确认已使用或撤销，请重新检查。".into());
        }
        let p = s.folder_plan.take().unwrap();
        p.time()?;
        p
    };
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _resources = (operation, slot, registered);
        run(&state.db, plan, request_id, &job)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn get_folder_archives(
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
pub(crate) fn acknowledge_folder_archive(
    id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let _operation = state.updates.installation.enter()?;
    if !confirmed {
        return Err("请先核对整份归档的目标和临时位置。".into());
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut r = journal::list(&db)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("文件夹归档记录不存在。")?;
    if !["unknown", "interrupted"].contains(&r.state.as_str()) {
        return Err("此文件夹归档无需未知结果确认。".into());
    }
    r.reviewed_at = Some(db::now().max(r.created_at));
    journal::save(&db, &r)
}
