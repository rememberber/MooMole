use super::*;
use crate::{engine::TaskKind, AppState};
fn source(
    state: &AppState,
    scan_id: &str,
    selected: &[String],
) -> Result<super::super::Plan, String> {
    let mut store = state.personal_files.lock().map_err(|e| e.to_string())?;
    if store.scan.as_ref().is_none_or(|s| s.id != scan_id) {
        return Err("文件清单已变化，请重新扫描。".into());
    }
    let offer = store.offer(selected, db::now())?;
    store.take(&offer.id, db::now())
}
#[tauri::command]
pub(crate) fn personal_move_strategy(
    scan_id: String,
    selected_ids: Vec<String>,
    destination: String,
    state: tauri::State<'_, AppState>,
) -> Result<&'static str, String> {
    let _operation = state.updates.installation.enter()?;
    let p = source(&state, &scan_id, &selected_ids)?;
    let settings = db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?;
    let held = movement::directory(&destination, &settings)?;
    let device = held.target().metadata().map_err(|e| e.to_string())?.dev();
    if p.items
        .iter()
        .all(|i| i.witness.nodes.last().is_some_and(|n| n.dev == device))
    {
        Ok("sameVolume")
    } else if p
        .items
        .iter()
        .all(|i| i.witness.nodes.last().is_some_and(|n| n.dev != device))
    {
        Ok("crossVolume")
    } else {
        Err("源文件的磁盘关系不一致，请重新扫描。".into())
    }
}
#[tauri::command]
pub(crate) async fn prepare_personal_transfer(
    scan_id: String,
    selected_ids: Vec<String>,
    destination: String,
    conflict: movement::Conflict,
    state: tauri::State<'_, AppState>,
) -> Result<Draft, String> {
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "文件检查尚未结束。")?;
    let source = source(&state, &scan_id, &selected_ids)?;
    let settings = db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?;
    let plan = tauri::async_runtime::spawn_blocking(move || {
        let _ownership = (operation, slot);
        prepare(source, &destination, conflict, &settings)
    })
    .await
    .map_err(|e| e.to_string())??;
    let draft = Draft {
        id: plan.source.id.clone(),
        scan_id: plan.source.scan_id.clone(),
        expires_at: plan.source.expires_at,
        directory: plan.movement.directory.clone(),
        conflict,
        items: plan.source.items.iter().map(row).collect(),
        targets: plan.movement.targets.clone(),
    };
    let mut store = state.personal_transfers.lock().map_err(|e| e.to_string())?;
    store
        .plans
        .retain(|_, (_, deadline)| Instant::now() < *deadline);
    if store.plans.len() >= 8 {
        return Err("请先关闭旧的跨磁盘确认。".into());
    }
    store.plans.insert(
        draft.id.clone(),
        (plan, Instant::now() + Duration::from_secs(30)),
    );
    Ok(draft)
}
#[tauri::command]
pub(crate) fn discard_personal_transfer(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state
        .personal_transfers
        .lock()
        .map_err(|e| e.to_string())?
        .plans
        .remove(&id);
    Ok(())
}
#[tauri::command]
pub(crate) fn get_personal_transfers(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let records = journal::list(&*state.db.lock().map_err(|e| e.to_string())?)?;
    let active = state
        .jobs
        .lock()
        .map_err(|e| e.to_string())?
        .iter()
        .filter(|(_, j)| j.kind == TaskKind::PersonalTransfer)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let db = state.db.lock().map_err(|e| e.to_string())?;
    for r in records
        .iter()
        .filter(|r| r.state == "running" && !active.contains(&r.request_id))
    {
        journal::interrupt(&db, &r.id)?;
    }
    Ok(serde_json::json!({"records":journal::list(&db)?,"activeRequests":active}))
}
#[tauri::command]
pub(crate) fn acknowledge_personal_transfer(
    id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Record, String> {
    if !confirmed {
        return Err("请先核对原件、副本及临时位置。".into());
    }
    journal::acknowledge(&*state.db.lock().map_err(|e| e.to_string())?, &id)
}
#[tauri::command]
pub(crate) async fn execute_personal_transfer(
    id: String,
    request_id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Record, String> {
    if !confirmed {
        return Err("请先确认副本校验、原件废纸篓处理和取消边界。".into());
    }
    uuid::Uuid::parse_str(&request_id).map_err(|_| "跨磁盘请求标识无效。")?;
    let operation = state.updates.installation.enter()?;
    let (plan, deadline) = state
        .personal_transfers
        .lock()
        .map_err(|e| e.to_string())?
        .plans
        .remove(&id)
        .ok_or("跨磁盘确认已使用或撤销。")?;
    if Instant::now() >= deadline {
        return Err("跨磁盘确认已过期。".into());
    }
    let job = Arc::new(Job::new(TaskKind::PersonalTransfer));
    {
        let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
        if state.shutdown.closing()
            || jobs.len() >= 3
            || jobs.contains_key(&request_id)
            || jobs.values().any(|j| j.kind.exclusive_operation())
        {
            return Err("请等待当前任务结束后重新确认。".into());
        }
        jobs.insert(request_id.clone(), job.clone());
    }
    let state = state.inner().clone();
    let (send, receive) = tokio::sync::oneshot::channel();
    tauri::async_runtime::spawn(async move {
        let _operation = operation;
        let ctx = Context {
            database: state.db.clone(),
            home: state.home.clone(),
            engine_root: state.engine_root.clone(),
        };
        let result = run(ctx, plan, request_id.clone(), job).await;
        if let Ok(mut jobs) = state.jobs.lock() {
            jobs.remove(&request_id);
        }
        let _ = send.send(result);
    });
    receive
        .await
        .map_err(|_| "跨磁盘响应中断，请核对历史和两端文件，不会自动重试。".to_string())?
}
