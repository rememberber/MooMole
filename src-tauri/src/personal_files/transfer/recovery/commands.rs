use super::*;
use crate::{engine::TaskKind, AppState};
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
    uuid::Uuid::parse_str(id).map_err(|_| "恢复请求标识无效。")?;
    let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
    if state.shutdown.closing()
        || jobs.len() >= 3
        || jobs.contains_key(id)
        || jobs.values().any(|j| j.kind.exclusive_operation())
    {
        return Err("请先结束当前任务，再恢复文件。".into());
    }
    let job = Arc::new(Job::new(TaskKind::FileRestore));
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
pub(crate) async fn prepare_transfer_restoration(
    run_id: String,
    item_id: String,
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Draft, String> {
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "其他文件检查正在进行。")?;
    let (job, registered) = register(&state, &request_id)?;
    state
        .personal_transfers
        .lock()
        .map_err(|e| e.to_string())?
        .recovery = None;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _resources = (operation, slot, registered);
        let (selected, settings) = {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            journal::admission(&db)?;
            let settings = db::settings(&db)?;
            (source(&db, &run_id, &item_id)?, settings)
        };
        let plan = Plan::prepare(selected, &run_id, &item_id, &state.home, &settings, &job)?;
        let settings = {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            plan.binding(&db)?;
            db::settings(&db)?
        };
        plan.gate(&settings, &state.home, &job)?;
        stopped(&job, Instant::now())?;
        let draft = plan.draft.clone();
        state
            .personal_transfers
            .lock()
            .map_err(|e| e.to_string())?
            .recovery = Some((plan, Instant::now()));
        Ok(draft)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn discard_transfer_restoration(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut s = state.personal_transfers.lock().map_err(|e| e.to_string())?;
    if s.recovery.as_ref().is_some_and(|(p, _)| p.draft.id == id) {
        s.recovery = None;
    }
    Ok(())
}
pub(super) fn run(
    state: Context,
    plan: Plan,
    request: String,
    job: &Job,
) -> Result<Recovery, String> {
    let temporary = Path::new(&plan.draft.original_path)
        .parent()
        .ok_or("原位置缺少父目录。")?
        .join(format!(".moomole-restore-{}.tmp", plan.draft.id));
    let mut r = Recovery {
        id: plan.draft.id.clone(),
        request_id: request,
        draft: plan.draft.clone(),
        created_at: db::now(),
        finished_at: None,
        state: "running".into(),
        phase: "checking".into(),
        temporary_path: temporary.to_string_lossy().into_owned(),
        receipt: None,
        reason: "正在复查跨磁盘恢复来源，尚未写入原位置。".into(),
        reviewed_at: None,
    };
    {
        let mut db = state.database.lock().map_err(|e| e.to_string())?;
        plan.binding(&db)?;
        journal::begin(&mut db, &r)?;
    }
    let result = perform(
        &plan,
        job,
        || db::settings(&*state.database.lock().map_err(|e| e.to_string())?),
        || {
            let db = state.database.lock().map_err(|e| e.to_string())?;
            plan.binding(&db)
        },
        &mut r,
        |r| journal::save(&*state.database.lock().map_err(|e| e.to_string())?, r),
    );
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
    journal::save(&*state.database.lock().map_err(|e| e.to_string())?, &r)?;
    Ok(r)
}
#[tauri::command]
pub(crate) async fn execute_transfer_restoration(
    id: String,
    request_id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Recovery, String> {
    if !confirmed {
        return Err("请先确认从副本恢复，且保留副本及原废纸篓项目。".into());
    }
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "其他文件检查正在进行。")?;
    let (job, registered) = register(&state, &request_id)?;
    let plan = {
        let mut s = state.personal_transfers.lock().map_err(|e| e.to_string())?;
        if s.recovery.as_ref().is_none_or(|(p, _)| p.draft.id != id) {
            return Err("恢复确认已使用或撤销，请重新准备。".into());
        }
        let (p, issued) = s.recovery.take().unwrap();
        now_valid(&p, issued)?;
        p
    };
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _resources = (operation, slot, registered);
        let ctx = Context {
            database: state.db.clone(),
            home: state.home.clone(),
            engine_root: state.engine_root.clone(),
        };
        run(ctx, plan, request_id, &job)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn get_transfer_restorations(
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
    snapshot(&db, &active)
}
pub(super) fn snapshot(db: &db::Database, active: &[String]) -> Result<serde_json::Value, String> {
    let records = journal::reconcile(db, active)?;
    let active_requests = records
        .iter()
        .filter(|r| r.state == "running" && active.contains(&r.request_id))
        .map(|r| r.request_id.clone())
        .collect::<Vec<_>>();
    Ok(serde_json::json!({"records":records,"activeRequests":active_requests}))
}
#[tauri::command]
pub(crate) fn acknowledge_transfer_restoration(
    id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let _operation = state.updates.installation.enter()?;
    if !confirmed {
        return Err("请先核对原位置、副本和临时路径。".into());
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut r = journal::list(&db)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("恢复记录不存在。")?;
    if !["unknown", "interrupted"].contains(&r.state.as_str()) {
        return Err("本记录无需未知结果核对。".into());
    }
    r.reviewed_at = Some(db::now().max(r.created_at));
    journal::save(&db, &r)
}
