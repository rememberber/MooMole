use super::{
    quicklook::Step,
    refresh_execution, refresh_journal as journal,
    refresh_plan::{Authorized, Context, Draft},
};
use crate::{
    db,
    engine::{Job, TaskKind},
    AppState,
};
use serde::Serialize;
use std::{sync::Arc, time::Duration};

fn context(state: &AppState) -> Result<Context, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出。".into());
    }
    let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取维护保护。")?)?;
    Ok(Context {
        home: state.home.clone(),
        engine_root: state.engine_root.clone(),
        protected_paths: settings.protected_paths,
        acknowledged: settings.acknowledged,
    })
}
#[tauri::command]
pub(crate) async fn prepare_quicklook_refresh(
    selected: Vec<Step>,
    state: tauri::State<'_, AppState>,
) -> Result<Draft, String> {
    let ctx = context(&state)?;
    if state
        .jobs
        .lock()
        .map_err(|_| "无法核对任务。")?
        .values()
        .any(|j| j.kind.exclusive_operation())
    {
        return Err("请等待当前修改或权限检查结束。".into());
    }
    {
        let db = state.db.lock().map_err(|_| "无法核对修改准入。")?;
        let t = db
            .unchecked_transaction()
            .map_err(|_| "无法开始只读准入核对。")?;
        crate::execution_admission::reserve(&t, 32 * 1024)?;
        t.commit().map_err(|_| "无法结束准入核对。")?;
    }
    let permit = state
        .maintenance_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "上次维护检查仍未结束。")?;
    let operation = state.updates.installation.enter()?;
    let ticket = state
        .refresh_plans
        .lock()
        .map_err(|_| "无法建立维护检查。")?
        .begin()?;
    let work = tauri::async_runtime::spawn_blocking(move || {
        let a = Authorized::prepare(&ctx, selected)?;
        Ok::<_, String>((a, permit, operation))
    });
    let result = tokio::time::timeout(Duration::from_secs(8), work)
        .await
        .map_err(|_| "维护检查未及时结束；原读取可能仍在进行，请稍后重试。".to_string())
        .and_then(|r| r.map_err(|_| "维护检查未完成。".to_string()))
        .and_then(|r| r);
    let (a, _permit, _operation) = match result {
        Ok(a) => a,
        Err(e) => {
            if let Ok(mut s) = state.refresh_plans.lock() {
                s.abort(ticket);
            }
            return Err(e);
        }
    };
    // Recheck current settings outside the store mutex. No actual operation.
    if let Err(e) = context(&state).and_then(|ctx| a.validate_context(&ctx)) {
        state
            .refresh_plans
            .lock()
            .map_err(|_| "无法撤销维护检查。")?
            .abort(ticket);
        return Err(e);
    }
    state
        .refresh_plans
        .lock()
        .map_err(|_| "无法保存维护检查。")?
        .publish(ticket, a)
}
#[tauri::command]
pub(crate) fn discard_quicklook_refresh(
    confirmation_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state
        .refresh_plans
        .lock()
        .map_err(|_| "无法撤销维护确认。")?
        .discard(&confirmation_id);
    Ok(())
}
#[tauri::command]
pub(crate) async fn execute_quicklook_refresh(
    plan_id: String,
    confirmation_id: String,
    request_id: String,
    rebuild_confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<journal::Detail, String> {
    uuid::Uuid::parse_str(&request_id).map_err(|_| "维护请求标识无效。")?;
    if !rebuild_confirmed {
        return Err("请确认已了解预览影响、缓存重建和无法撤销的边界。".into());
    }
    let operation = state.updates.installation.enter()?;
    context(&state)?;
    let job = Arc::new(Job::new(TaskKind::MaintenanceRefresh));
    {
        let mut jobs = state.jobs.lock().map_err(|_| "无法登记维护任务。")?;
        if state.shutdown.closing()
            || jobs.len() >= 3
            || jobs.contains_key(&request_id)
            || jobs.values().any(|j| j.kind.exclusive_operation())
        {
            return Err("已有修改/权限检查任务，或应用正在退出。".into());
        }
        jobs.insert(request_id.clone(), job.clone());
    }
    let a = state
        .refresh_plans
        .lock()
        .map_err(|_| "无法取用维护确认。".to_string())
        .and_then(|mut s| s.take(&confirmation_id, &plan_id));
    let a = match a {
        Ok(a) => a,
        Err(e) => {
            if let Ok(mut jobs) = state.jobs.lock() {
                jobs.remove(&request_id);
            }
            return Err(e);
        }
    };
    let state = state.inner().clone();
    let gate_state = state.clone();
    let receive = crate::uninstall_execution::dispatch(
        request_id.clone(),
        state.jobs.clone(),
        job.clone(),
        operation,
        async move {
            refresh_execution::run(state.db.clone(), request_id, a, job, move || {
                context(&gate_state)
            })
            .await
        },
    );
    receive
        .await
        .map_err(|_| "维护响应中断，请按原请求读取记录，不要自动重试。".to_string())?
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    version: u32,
    request_id: String,
    active: bool,
    record: Option<journal::Detail>,
}
#[tauri::command]
pub(crate) fn get_quicklook_refresh(
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Snapshot, String> {
    uuid::Uuid::parse_str(&request_id).map_err(|_| "维护请求标识无效。")?;
    let active = || {
        state
            .jobs
            .lock()
            .map(|j| {
                j.get(&request_id)
                    .is_some_and(|j| j.kind == TaskKind::MaintenanceRefresh)
            })
            .map_err(|_| "无法核对维护任务。".to_string())
    };
    let read = || {
        let db = state.db.lock().map_err(|_| "无法读取维护记录。")?;
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM maintenance_runs WHERE id=?1)",
                [&request_id],
                |r| r.get(0),
            )
            .map_err(|_| "无法核对维护记录。")?;
        if exists {
            Ok::<_, String>(Some(journal::detail(&db, &request_id)?))
        } else {
            Ok(None)
        }
    };
    let was_active = active()?;
    let mut record = read()?;
    let active = active()?;
    if !active
        && (was_active
            || record
                .as_ref()
                .is_some_and(|r| r.summary.state == "running"))
    {
        record = read()?;
    }
    Ok(Snapshot {
        version: 1,
        request_id,
        active,
        record,
    })
}
#[tauri::command]
pub(crate) fn list_quicklook_refreshes(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<journal::Summary>, String> {
    journal::list(&*state.db.lock().map_err(|_| "无法读取维护历史。")?)
}
#[tauri::command]
pub(crate) fn cancel_quicklook_refresh(
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let jobs = state.jobs.lock().map_err(|_| "无法核对维护任务。")?;
    let job = jobs
        .get(&request_id)
        .filter(|j| j.kind == TaskKind::MaintenanceRefresh)
        .ok_or("此维护任务已结束，请读取实际结果。")?;
    job.cancel();
    Ok(())
}
#[tauri::command]
pub(crate) fn acknowledge_quicklook_refresh(
    request_id: String,
    snapshot_sha256: String,
    reviewed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<journal::Detail, String> {
    if !reviewed {
        return Err("请先核对结果并明确确认。".into());
    }
    let operation = state.updates.installation.enter()?;
    context(&state)?;
    if state
        .jobs
        .lock()
        .map_err(|_| "无法核对维护 worker。")?
        .contains_key(&request_id)
    {
        return Err("维护结果仍在收集中，请等待结束。".into());
    }
    let result = journal::acknowledge(
        &mut *state.db.lock().map_err(|_| "无法保存人工核查。")?,
        &request_id,
        &snapshot_sha256,
        db::now(),
    );
    drop(operation);
    result
}
