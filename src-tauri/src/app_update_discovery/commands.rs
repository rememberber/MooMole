use super::*;
struct Stop(Arc<Job>);
impl Drop for Stop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
async fn worker<T: Send + 'static>(
    state: &AppState,
    current: String,
    request_id: String,
    work: impl FnOnce(AppState, Context, Arc<Job>) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "其他应用或文件检查仍在进行，请稍后重试。")?;
    let ctx = crate::app_control::context(state, &current)?;
    let (job, registered) = crate::application_update::commands::register(state, &request_id)?;
    let stop = Stop(job.clone());
    let state = state.clone();
    let mut task = tokio::task::spawn_blocking(move || {
        let _resources = (operation, slot, registered);
        work(state, ctx, job)
    });
    let result = tokio::select! {r=&mut task=>r.map_err(|_|"应用更新来源读取未完成。")?,_=tokio::time::sleep(Duration::from_secs(30))=>return Err("更新来源检查等待超时；系统读取仍占用资源，停止后续步骤，请稍后重试。".into())};
    drop(stop);
    result
}
fn validate(state: &AppState, plan: &Plan, current: &str, job: &Job) -> Result<(), String> {
    let ctx = crate::app_control::context(state, current)?;
    plan.validate(&ctx, job)?;
    let (task, index) = plan.source.address();
    let source = state
        .results
        .lock()
        .map_err(|e| e.to_string())?
        .uninstall_target(&task, index)?;
    if source != plan.source {
        return Err("更新检查的应用清单已变化，请重新读取。".into());
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn prepare_app_update_discovery(
    task_id: String,
    index: usize,
    request_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Probe, String> {
    let source = state
        .results
        .lock()
        .map_err(|e| e.to_string())?
        .uninstall_target(&task_id, index)?;
    let current = app.config().identifier.clone();
    worker(
        &state,
        current.clone(),
        request_id,
        move |state, ctx, job| {
            {
                let mut store = state
                    .app_update_discovery
                    .lock()
                    .map_err(|e| e.to_string())?;
                store.plan = None;
                store.report = None;
            }
            let plan = Plan::prepare(source, ctx, &job)?;
            validate(&state, &plan, &current, &job)?;
            check(&job)?;
            let probe = plan.probe.clone();
            state
                .app_update_discovery
                .lock()
                .map_err(|e| e.to_string())?
                .plan = Some(plan);
            Ok(probe)
        },
    )
    .await
}
#[tauri::command]
pub(crate) fn discard_app_update_discovery(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut store = state
        .app_update_discovery
        .lock()
        .map_err(|e| e.to_string())?;
    if store.plan.as_ref().is_some_and(|p| p.probe.id == id) {
        store.plan = None;
        store.report = None;
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn check_app_update_discovery(
    id: String,
    request_id: String,
    confirmed: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Report, String> {
    if !confirmed {
        return Err("请明确选择向显示的应用更新地址检查，不会自动连接。".into());
    }
    let current = app.config().identifier.clone();
    let runtime = tokio::runtime::Handle::current();
    worker(
        &state,
        current.clone(),
        request_id.clone(),
        move |state, _ctx, job| {
            let plan = {
                let mut store = state
                    .app_update_discovery
                    .lock()
                    .map_err(|e| e.to_string())?;
                let p = store
                    .plan
                    .as_ref()
                    .filter(|p| p.probe.id == id && p.probe.state == "ready")
                    .ok_or("更新来源已撤销或不受支持，请重新读取。")?
                    .clone();
                store.report = None;
                p
            };
            validate(&state, &plan, &current, &job)?;
            let bytes = runtime.block_on(network::fetch(
                plan.probe.feed_url.as_deref().ok_or("更新地址缺失。")?,
                job.clone(),
            ))?;
            check(&job)?;
            let arch = if std::env::consts::ARCH == "aarch64" {
                "arm64"
            } else {
                std::env::consts::ARCH
            };
            let (releases, inspected, skipped, complete) = feed::parse(
                &bytes,
                plan.probe
                    .current_build
                    .as_deref()
                    .ok_or("应用构建号缺失。")?,
                &system_version()?,
                arch,
                &job,
            )?;
            validate(&state, &plan, &current, &job)?;
            let now = db::now();
            let report = Report {
                probe_id: id.clone(),
                request_id,
                checked_at: now,
                expires_at: plan.probe.expires_at,
                state: if !releases.is_empty() {
                    "newerReported"
                } else if complete {
                    "noNewerInFeed"
                } else {
                    "unknown"
                },
                complete,
                inspected,
                skipped,
                releases,
            };
            let mut store = state
                .app_update_discovery
                .lock()
                .map_err(|e| e.to_string())?;
            if store.plan.as_ref().is_none_or(|p| p.probe.id != id) {
                return Err("更新来源已撤销，未保留迟到结果。".into());
            }
            check(&job)?;
            store.report = Some(report.clone());
            Ok(report)
        },
    )
    .await
}
#[tauri::command]
pub(crate) async fn open_app_update_candidate(
    probe_id: String,
    candidate_id: String,
    request_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let current = app.config().identifier.clone();
    let runtime = tokio::runtime::Handle::current();
    worker(
        &state,
        current.clone(),
        request_id,
        move |state, _ctx, job| {
            let (plan, release) = {
                let store = state
                    .app_update_discovery
                    .lock()
                    .map_err(|e| e.to_string())?;
                let p = store
                    .plan
                    .as_ref()
                    .filter(|p| p.probe.id == probe_id)
                    .ok_or("更新来源已撤销，请重新检查。")?;
                let r = store
                    .report
                    .as_ref()
                    .filter(|r| r.probe_id == probe_id)
                    .and_then(|r| r.releases.iter().find(|r| r.id == candidate_id))
                    .ok_or("所选更新记录已失效，请重新检查。")?;
                (p.clone(), r.clone())
            };
            validate(&state, &plan, &current, &job)?;
            let url = network::url(&release.url)?;
            network::resolve(&url)?;
            validate(&state, &plan, &current, &job)?;
            if state
                .app_update_discovery
                .lock()
                .map_err(|e| e.to_string())?
                .report
                .as_ref()
                .is_none_or(|r| {
                    r.probe_id != probe_id || !r.releases.iter().any(|r| r.id == candidate_id)
                })
            {
                return Err("更新记录已撤销，未打开来源。".into());
            }
            check(&job)?;
            runtime.block_on(async move {
                let status = tokio::time::timeout(
                    Duration::from_secs(5),
                    tokio::process::Command::new("/usr/bin/open")
                        .arg("--")
                        .arg(url.as_str())
                        .kill_on_drop(true)
                        .status(),
                )
                .await
                .map_err(|_| "浏览器打开请求超时，未重试。")?
                .map_err(|_| "无法请求浏览器打开更新来源。")?;
                if !status.success() {
                    return Err("浏览器未接受打开更新来源的请求。".into());
                }
                Ok(())
            })
        },
    )
    .await
}
