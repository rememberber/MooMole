use super::*;
use crate::AppState;
use std::sync::Arc;
pub(crate) struct Registered {
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
pub(crate) fn register(state: &AppState, id: &str) -> Result<(Arc<Job>, Registered), String> {
    uuid::Uuid::parse_str(id).map_err(|_| "应用更新请求标识无效。")?;
    let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
    if state.shutdown.closing()
        || jobs.len() >= 3
        || jobs.contains_key(id)
        || jobs.values().any(|j| j.kind.exclusive_operation())
    {
        return Err("请先结束当前任务，再更新应用。".into());
    }
    let job = Arc::new(Job::new(TaskKind::AppControl));
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
pub(crate) async fn prepare_application_update(
    task_id: String,
    index: usize,
    replacement: String,
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
    let source = state
        .results
        .lock()
        .map_err(|e| e.to_string())?
        .uninstall_target(&task_id, index)?;
    let current = app.config().identifier.clone();
    let ctx = crate::app_control::context(&state, &current)?;
    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        journal::admission(&db)?;
    }
    let (job, registered) = register(&state, &request_id)?;
    {
        let mut s = state
            .application_updates
            .lock()
            .map_err(|e| e.to_string())?;
        s.plan = None;
        s.restoration = None;
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _resources = (operation, slot, registered);
        let plan = objc2::rc::autoreleasepool(|_| {
            Plan::prepare(source, PathBuf::from(replacement), ctx, &job)
        })?;
        cancelled(&job)?;
        if !context_equal(
            &plan.context,
            &crate::app_control::context(&state, &current)?,
        ) {
            return Err("保护设置变化，请重新准备。".into());
        }
        let result = plan.draft.clone();
        state
            .application_updates
            .lock()
            .map_err(|e| e.to_string())?
            .plan = Some(plan);
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn discard_application_update(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut store = state
        .application_updates
        .lock()
        .map_err(|e| e.to_string())?;
    if store.plan.as_ref().is_some_and(|p| p.draft.id == id) {
        store.plan = None;
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn execute_application_update(
    id: String,
    request_id: String,
    confirmed: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    if !confirmed {
        return Err("请先确认替换应用包及旧版恢复边界。".into());
    }
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "其他文件检查正在进行。")?;
    let (job, registered) = register(&state, &request_id)?;
    let plan = {
        let mut store = state
            .application_updates
            .lock()
            .map_err(|e| e.to_string())?;
        if store.plan.as_ref().is_none_or(|p| p.draft.id != id) {
            return Err("应用更新确认不存在或已使用，请重新准备。".into());
        }
        store.plan.take().unwrap()
    };
    let parent = Path::new(&plan.draft.path)
        .parent()
        .ok_or("缺少安装目录。")?;
    let backup = parent
        .join(format!(".moomole-app-update-{id}"))
        .join(Path::new(&plan.draft.path).file_name().unwrap());
    let mut record = Record {
        id: id.clone(),
        request_id,
        created_at: db::now(),
        finished_at: None,
        draft: plan.draft.clone(),
        state: "running".into(),
        phase: "checking".into(),
        reason: "正在复查应用更新，尚未替换。".into(),
        backup: backup.to_string_lossy().into_owned(),
        reviewed_at: None,
        recovery_receipt: None,
    };
    {
        let mut database = state.db.lock().map_err(|e| e.to_string())?;
        journal::begin(&mut database, &record)?;
    }
    let current = app.config().identifier.clone();
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _resources = (operation, slot, registered);
        let result = objc2::rc::autoreleasepool(|_| {
            perform(
                &plan,
                &job,
                || crate::app_control::context(&state, &current),
                &mut record,
                |r| journal::save(&*state.db.lock().map_err(|e| e.to_string())?, r),
            )
        });
        if let Err(e) = result {
            record.state = if record.phase == "intent" {
                "unknown"
            } else if job.cancelled.load(Ordering::SeqCst) {
                "cancelled"
            } else {
                "retained"
            }
            .into();
            record.reason = e;
        }
        record.phase = "finished".into();
        record.finished_at = Some(db::now().max(record.created_at));
        journal::save(&*state.db.lock().map_err(|e| e.to_string())?, &record)?;
        record.view()
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn get_application_updates(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let active = state
        .jobs
        .lock()
        .map_err(|e| e.to_string())?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    journal::reconcile(&db, &active)?
        .iter()
        .map(Record::view)
        .collect()
}
#[tauri::command]
pub(crate) fn acknowledge_application_update(
    id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let _operation = state.updates.installation.enter()?;
    if !confirmed {
        return Err("请先检查安装位置和暂存/旧版位置。".into());
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut r = journal::list(&db)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("应用更新记录不存在。")?;
    if !["unknown", "interrupted"].contains(&r.state.as_str()) {
        return Err("这条记录无需人工核对确认。".into());
    }
    r.reviewed_at = Some(db::now().max(r.created_at));
    journal::save(&db, &r)
}
