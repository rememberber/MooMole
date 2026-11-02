use super::*;
use crate::AppState;
struct Stop(Arc<Job>);
impl Drop for Stop {
    fn drop(&mut self) {
        self.0.cancel()
    }
}
fn current_source(
    state: &AppState,
    plan: &Plan,
    current: &str,
    job: &Job,
) -> Result<Context, String> {
    let ctx = crate::app_control::context(state, current)?;
    let (t, i) = plan.source.address();
    if state
        .results
        .lock()
        .map_err(|e| e.to_string())?
        .uninstall_target(&t, i)?
        != plan.source
    {
        return Err("原应用清单已变化，请重新检查。".into());
    }
    plan.validate(&ctx, job)?;
    Ok(ctx)
}
async fn worker<T: Send + 'static>(
    state: &AppState,
    current: String,
    request: String,
    preparation: bool,
    work: impl FnOnce(AppState, Context, Arc<Job>, tokio::runtime::Handle) -> Result<T, String>
        + Send
        + 'static,
) -> Result<T, String> {
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "其他文件任务仍在进行。")?;
    if !db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?.acknowledged {
        return Err("请先确认首次使用说明。".into());
    }
    let ctx = crate::app_control::context(state, &current)?;
    let (job, registration) = crate::application_update::commands::register(state, &request)?;
    let stop = preparation.then(|| Stop(job.clone()));
    let runtime = tokio::runtime::Handle::current();
    let state = state.clone();
    // The blocking owner retains resources and durable result collection even if
    // the WebView stops waiting. Actual process groups are supervised separately.
    let task = tauri::async_runtime::spawn_blocking(move || {
        let _resources = (operation, slot, registration);
        objc2::rc::autoreleasepool(|_| work(state, ctx, job, runtime))
    });
    let result = task
        .await
        .map_err(|_| "Homebrew 任务响应中断，请核对持久记录。")?;
    drop(stop);
    result
}
#[tauri::command]
pub(crate) async fn prepare_managed_app(
    task_id: String,
    index: usize,
    request_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    controller: tauri::State<'_, Controller>,
) -> Result<Draft, String> {
    let source = state
        .results
        .lock()
        .map_err(|e| e.to_string())?
        .uninstall_target(&task_id, index)?;
    controller.0.lock().map_err(|e| e.to_string())?.plan = None;
    let owner = controller.inner().clone();
    let current = app.config().identifier.clone();
    worker(
        &state,
        current.clone(),
        request_id,
        true,
        move |state, ctx, job, runtime| {
            let plan = runtime.block_on(Plan::prepare(source, ctx, job.clone()))?;
            current_source(&state, &plan, &current, &job)?;
            let out = plan.draft.clone();
            owner.0.lock().map_err(|e| e.to_string())?.plan = Some(plan);
            Ok(out)
        },
    )
    .await
}
#[tauri::command]
pub(crate) fn discard_managed_app(
    id: String,
    controller: tauri::State<'_, Controller>,
) -> Result<(), String> {
    let mut s = controller.0.lock().map_err(|e| e.to_string())?;
    if s.plan.as_ref().is_some_and(|p| p.draft.id == id) {
        s.plan = None
    }
    Ok(())
}
pub(super) fn execute(
    plan: Plan,
    database: &Arc<Mutex<db::Database>>,
    request: &str,
    job: Arc<Job>,
    runtime: &tokio::runtime::Handle,
    live: impl Fn(&Plan, &Job) -> Result<Context, String>,
) -> Result<Record, String> {
    // Only the worker calls this function after consuming a current live plan.
    let mut r = Record {
        id: plan.draft.id.clone(),
        request_id: request.into(),
        draft: plan.draft.clone(),
        state: "running".into(),
        phase: "checking".into(),
        created_at: db::now(),
        finished_at: None,
        reason: "正在重新检查 Homebrew 包。".into(),
        reviewed_at: None,
    };
    journal::begin(&mut *database.lock().map_err(|e| e.to_string())?, &r)?;
    let outcome = (|| -> Result<(), String> {
        let ctx = live(&plan, &job)?;
        let catalog = runtime.block_on(plan.tool.installed(&ctx.home, job.clone()))?;
        if !catalog.packages.iter().any(|p| {
            p.token == plan.draft.token
                && p.path == plan.draft.path
                && p.sha256 == plan.package_sha256
        }) {
            return Err("Homebrew 包、版本或内容声明已变化，保留。".into());
        }
        live(&plan, &job)?;
        check(&job)?;
        r.phase = "intent".into();
        r.reason = "已保存执行意图，正在请求 Homebrew 卸载所选 Cask。".into();
        journal::save(&*database.lock().map_err(|e| e.to_string())?, &r)?;
        let command = plan.tool.uninstall_command(&ctx.home, &plan.draft.token)?;
        runtime
            .block_on(crate::process::output_bounded(
                command,
                job.clone(),
                Duration::from_secs(120),
                CAP as u64,
            ))
            .map_err(|_| "Homebrew 未取得完整成功结果；可能部分完成，请核对，不会自动重试。")?;
        check(&job)?;
        let after = runtime.block_on(plan.tool.installed(&ctx.home, job.clone()))?;
        if after.tokens.contains(&plan.draft.token)
            || !matches!(std::fs::symlink_metadata(&plan.draft.path),Err(e)if e.kind()==std::io::ErrorKind::NotFound)
        {
            return Err("Homebrew 命令已返回，但包登记或应用位置仍存在/未知，请核对。".into());
        }
        r.state = "removed".into();
        r.reason="Homebrew 命令已成功返回，且重新读取未发现该包登记或原 App 路径。未使用 zap、force 或自动移除依赖；此流程不提供废纸篓收据或应用内恢复。".into();
        Ok(())
    })();
    if let Err(e) = outcome {
        r.state = if r.phase == "intent" {
            "unknown"
        } else if job.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            "cancelled"
        } else {
            "retained"
        }
        .into();
        r.reason = e.chars().take(1000).collect()
    }
    r.phase = "finished".into();
    r.finished_at = Some(db::now().max(r.created_at));
    journal::save(&*database.lock().map_err(|e| e.to_string())?, &r)?;
    Ok(r)
}
#[tauri::command]
pub(crate) async fn execute_managed_app(
    id: String,
    request_id: String,
    confirmed: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    controller: tauri::State<'_, Controller>,
) -> Result<Record, String> {
    if !confirmed {
        return Err("请单独确认 Homebrew 卸载及恢复边界。".into());
    }
    let plan = {
        let mut s = controller.0.lock().map_err(|e| e.to_string())?;
        if s.plan.as_ref().is_none_or(|p| p.draft.id != id) {
            return Err("Homebrew 确认已撤销，请重新检查。".into());
        }
        s.plan.take().unwrap()
    };
    let current = app.config().identifier.clone();
    let req = request_id.clone();
    worker(
        &state,
        current.clone(),
        request_id,
        false,
        move |state, _ctx, job, runtime| {
            execute(plan, &state.db, &req, job, &runtime, |p, j| {
                current_source(&state, p, &current, j)
            })
        },
    )
    .await
}
#[tauri::command]
pub(crate) fn get_managed_apps(state: tauri::State<'_, AppState>) -> Result<Vec<Record>, String> {
    let observed = journal::list(&*state.db.lock().map_err(|e| e.to_string())?)?
        .into_iter()
        .map(|r| r.id)
        .collect::<Vec<_>>();
    let active = state
        .jobs
        .lock()
        .map_err(|e| e.to_string())?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    journal::reconcile(
        &*state.db.lock().map_err(|e| e.to_string())?,
        &active,
        &observed,
    )
}
#[tauri::command]
pub(crate) fn acknowledge_managed_app(
    id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Record, String> {
    if !confirmed {
        return Err("请先核对实际 App 与 Homebrew 登记状态。".into());
    }
    let c = state.db.lock().map_err(|e| e.to_string())?;
    let mut r = journal::list(&c)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("未找到 Homebrew 记录。")?;
    if r.state == "running" {
        return Err("任务仍在进行，请等待或停止。".into());
    }
    r.reviewed_at = Some(db::now().max(r.created_at));
    journal::save(&c, &r)?;
    Ok(r)
}
