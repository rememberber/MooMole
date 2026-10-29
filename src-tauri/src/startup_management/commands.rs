use super::*;
use crate::{engine::TaskKind, AppState};
fn context(state: &AppState) -> Result<Context, String> {
    Ok(Context {
        home: state.home.clone(),
        settings: db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?,
        own_app: std::env::current_exe()
            .ok()
            .and_then(|p| crate::app_protection::containing_app(&p).map(Path::to_path_buf)),
    })
}
fn register(state: &AppState, request: &str) -> Result<Arc<Job>, String> {
    uuid::Uuid::parse_str(request).map_err(|_| "启动管理任务标识无效。")?;
    if !context(state)?.settings.acknowledged {
        return Err("请先确认首次使用说明。".into());
    }
    let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
    if state.shutdown.closing()
        || jobs.len() >= 3
        || jobs.contains_key(request)
        || jobs.values().any(|j| j.kind.exclusive_operation())
    {
        return Err("请等待当前任务结束后再管理启动项。".into());
    }
    let job = Arc::new(Job::new(TaskKind::StartupManagement));
    jobs.insert(request.into(), job.clone());
    Ok(job)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    report: Option<Report>,
    records: Vec<Record>,
    active_request: Option<String>,
}
#[tauri::command]
pub(crate) fn get_startup_management(state: tauri::State<'_, AppState>) -> Result<View, String> {
    let active_request = state
        .jobs
        .lock()
        .map_err(|e| e.to_string())?
        .iter()
        .find(|(_, j)| j.kind == TaskKind::StartupManagement)
        .map(|(id, _)| id.clone());
    let report = state
        .startup_management
        .lock()
        .map_err(|e| e.to_string())?
        .scan
        .as_ref()
        .map(|s| s.report.clone());
    let records = journal::list(&*state.db.lock().map_err(|e| e.to_string())?)?;
    Ok(View {
        report,
        records,
        active_request,
    })
}
#[tauri::command]
pub(crate) async fn scan_startup_management(
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Report, String> {
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "另一个目录检查尚未完成。")?;
    let ctx = context(&state)?;
    let records = journal::list(&*state.db.lock().map_err(|e| e.to_string())?)?;
    let job = register(&state, &request_id)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ownership = (operation, slot);
        let result = scan(&ctx, &records, &job).and_then(|scan| {
            let report = scan.report.clone();
            let mut store = state.startup_management.lock().map_err(|e| e.to_string())?;
            store.scan = Some(scan);
            store.plans.clear();
            Ok(report)
        });
        if let Ok(mut jobs) = state.jobs.lock() {
            jobs.remove(&request_id);
        }
        result
    })
    .await
    .map_err(|_| "启动配置读取响应中断，请刷新状态。".to_string())?
}
#[tauri::command]
pub(crate) async fn prepare_startup_management(
    scan_id: String,
    item_id: String,
    action: Action,
    state: tauri::State<'_, AppState>,
) -> Result<Draft, String> {
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "启动配置检查尚未完成。")?;
    let ctx = context(&state)?;
    let records = journal::list(&*state.db.lock().map_err(|e| e.to_string())?)?;
    let snapshot = state
        .startup_management
        .lock()
        .map_err(|e| e.to_string())?
        .scan
        .clone()
        .filter(|s| s.report.id == scan_id)
        .ok_or("启动配置清单已变化。")?;
    let (plan, draft) = tauri::async_runtime::spawn_blocking(move || {
        let _ownership = (operation, slot);
        prepare(
            &snapshot,
            &item_id,
            action,
            &ctx,
            &records,
            &Job::new(TaskKind::StartupManagement),
        )
    })
    .await
    .map_err(|e| e.to_string())??;
    let mut store = state.startup_management.lock().map_err(|e| e.to_string())?;
    if store.scan.as_ref().is_none_or(|s| s.report.id != scan_id) {
        return Err("检查期间清单已变化，请重新选择。".into());
    }
    store
        .plans
        .retain(|_, (_, deadline)| Instant::now() < *deadline);
    if store.plans.len() >= 8 {
        return Err("请关闭旧的启动确认后重试。".into());
    }
    store.plans.insert(
        plan.id.clone(),
        (plan, Instant::now() + Duration::from_secs(30)),
    );
    Ok(draft)
}
#[tauri::command]
pub(crate) fn discard_startup_management(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state
        .startup_management
        .lock()
        .map_err(|e| e.to_string())?
        .plans
        .remove(&id);
    Ok(())
}
#[tauri::command]
pub(crate) async fn execute_startup_management(
    id: String,
    request_id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Record, String> {
    if !confirmed {
        return Err("请先确认所选配置和可能中断的后台工作。".into());
    }
    let operation = state.updates.installation.enter()?;
    let (plan, deadline) = state
        .startup_management
        .lock()
        .map_err(|e| e.to_string())?
        .plans
        .remove(&id)
        .ok_or("启动确认已使用或撤销。")?;
    if Instant::now() >= deadline {
        return Err("启动确认已过期，请重新检查。".into());
    }
    let job = register(&state, &request_id)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let result = run(
            state.db.clone(),
            plan,
            request_id.clone(),
            || context(&state),
            job,
        );
        if let Ok(mut store) = state.startup_management.lock() {
            store.scan = None;
            store.plans.clear();
        }
        if let Ok(mut jobs) = state.jobs.lock() {
            jobs.remove(&request_id);
        }
        result
    })
    .await
    .map_err(|_| "启动管理响应中断，请核对历史和配置；不会自动重试。".to_string())?
}
#[tauri::command]
pub(crate) fn acknowledge_startup_management(
    id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Record, String> {
    if !confirmed {
        return Err("请先核对配置与后台状态。".into());
    }
    journal::acknowledge(&*state.db.lock().map_err(|e| e.to_string())?, &id)
}
#[tauri::command]
pub(crate) async fn open_login_item_settings(app: tauri::AppHandle) -> Result<(), String> {
    let (send, receive) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = send.send(runtime::open_settings());
    })
    .map_err(|e| e.to_string())?;
    receive
        .await
        .map_err(|_| "系统登录项设置入口未返回结果。".to_string())?
}
