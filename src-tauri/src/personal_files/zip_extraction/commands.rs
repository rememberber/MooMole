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
    uuid::Uuid::parse_str(id).map_err(|_| "压缩包解压请求标识无效。")?;
    let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
    if state.shutdown.closing()
        || jobs.len() >= 3
        || jobs.contains_key(id)
        || jobs.values().any(|j| j.kind.exclusive_operation())
    {
        return Err("请先结束当前任务，再开始压缩包解压。".into());
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
pub(crate) async fn prepare_zip_extraction(
    root: String,
    directory: String,
    filename: String,
    request_id: String,
    restore_metadata: Option<bool>,
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
        .extraction_plan = None;
    let settings = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        journal::admission(&db)?;
        archive::journal::admission(&db)?;
        folder_archive::journal::admission(&db)?;
        db::settings(&db)?
    };
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _resources = (operation, slot, registered);
        let plan = Plan::prepare_mode(
            Path::new(&root),
            &directory,
            &filename,
            &settings,
            &job,
            restore_metadata.unwrap_or(false),
        )?;
        if db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?.protected_paths
            != settings.protected_paths
        {
            return Err("保护设置在检查期间变化，请重新准备压缩包解压。".into());
        }
        plan.time()?;
        archive::check(&job, Instant::now())?;
        let draft = plan.draft.clone();
        state
            .personal_archives
            .lock()
            .map_err(|e| e.to_string())?
            .extraction_plan = Some(plan);
        // The private snapshot can be large. Release it when this confirmation
        // expires even if its window is left open; do not hold the AppState/DB.
        let store = Arc::downgrade(&state.personal_archives);
        let id = draft.id.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(121)).await;
            if let Some(store) = store.upgrade() {
                if let Ok(mut s) = store.lock() {
                    if s.extraction_plan
                        .as_ref()
                        .is_some_and(|p| p.draft.id == id && p.time().is_err())
                    {
                        s.extraction_plan = None;
                    }
                }
            }
        });
        Ok(draft)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn discard_zip_extraction(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut s = state.personal_archives.lock().map_err(|e| e.to_string())?;
    if s.extraction_plan.as_ref().is_some_and(|p| p.draft.id == id) {
        s.extraction_plan = None;
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn execute_zip_extraction(
    id: String,
    request_id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Record, String> {
    if !confirmed {
        return Err("请先确认完整解压范围、隐藏内容、权限及原压缩包保留方式。".into());
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
        if s.extraction_plan.as_ref().is_none_or(|p| p.draft.id != id) {
            return Err("压缩包解压确认已使用或撤销，请重新检查。".into());
        }
        let p = s.extraction_plan.take().unwrap();
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
pub(crate) fn get_zip_extractions(
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
pub(crate) fn acknowledge_zip_extraction(
    id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let _operation = state.updates.installation.enter()?;
    if !confirmed {
        return Err("请先核对解压结果的目标和临时位置。".into());
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut r = journal::list(&db)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("压缩包解压记录不存在。")?;
    if !["unknown", "interrupted"].contains(&r.state.as_str()) {
        return Err("此压缩包解压无需未知结果确认。".into());
    }
    r.reviewed_at = Some(db::now().max(r.created_at));
    journal::save(&db, &r)
}
