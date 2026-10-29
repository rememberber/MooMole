//! Explicit ordinary-App lifecycle actions from a retained Mole list row.
//! Never force quits, runs a shell, changes login items or grants uninstall rights.
use crate::{
    app_inspection::identity,
    db,
    engine::{Job, TaskKind},
    uninstall_plan::{Context, Source},
    AppState,
};
use objc2::rc::{autoreleasepool, Retained};
use objc2_app_kit::{
    NSApplicationActivationOptions, NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration,
};
use objc2_foundation::{NSString, NSURL};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Action {
    Open,
    Quit,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    pub request_id: String,
    pub action: Action,
    pub path: String,
    pub bundle_id: String,
    pub outcome: &'static str,
    pub requested: bool,
    pub pids: Vec<i32>,
    pub reason: String,
}
struct Plan {
    source: Source,
    snapshot: identity::ControlIdentity,
    action: Action,
}
impl Plan {
    fn prepare(source: Source, action: Action, ctx: &Context) -> Result<Self, String> {
        let (path, id, _, kind) = source.control_target();
        if kind != "App" || unsafe { libc::geteuid() } == 0 {
            return Err("请选择普通 App，并使用普通账户运行。".into());
        }
        let snapshot = identity::capture_control(Path::new(path), id)?;
        let plan = Self {
            source,
            snapshot,
            action,
        };
        plan.validate(ctx)?;
        Ok(plan)
    }
    fn validate(&self, ctx: &Context) -> Result<(), String> {
        let (path, id, _, _) = self.source.control_target();
        if self.action == Action::Quit {
            let protection = crate::app_protection::check(
                path,
                Some(id),
                ctx.current_app.as_deref(),
                &ctx.current_id,
                &ctx.protected_paths,
            )?;
            let policy = crate::app_uninstall_rules::load(&ctx.engine_root)?;
            if !protection.reasons.is_empty()
                || crate::app_uninstall_rules::observe(Some(&policy), Some(id), "matching").code
                    != "notMatched"
            {
                return Err("所选应用受保护或属于系统/当前客户端，不能从这里请求退出。".into());
            }
            if !matches!(
                self.snapshot.background,
                crate::app_background::Marker::Absent | crate::app_background::Marker::Disabled
            ) {
                return Err("后台专用或身份未知的应用请通过其管理工具停止。".into());
            }
        }
        if identity::capture_control(Path::new(path), id)? != self.snapshot {
            return Err("应用包或身份已变化，请刷新列表后重试。".into());
        }
        Ok(())
    }
}
fn pid_absent(pid: i32) -> bool {
    pid > 0
        && unsafe { libc::kill(pid, 0) } == -1
        && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}
fn running(path: &Path, id: &str) -> Result<Vec<Retained<NSRunningApplication>>, String> {
    let apps =
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(id));
    if apps.count() > 32 {
        return Err("同标识运行副本过多，请在应用自身操作。".into());
    }
    let mut found = Vec::new();
    for app in apps.iter() {
        // AppKit notifications may lag on a worker thread. Only ESRCH proves
        // disappearance; a denied/unknown process observation remains blocked.
        if app.isTerminated() || pid_absent(app.processIdentifier()) {
            continue;
        }
        let at = app
            .bundleURL()
            .and_then(|u| u.path())
            .map(|s| PathBuf::from(s.to_string()))
            .ok_or("有运行副本的位置无法核对，请先在应用自身确认。")?;
        if at == path {
            let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
            let size = std::mem::size_of::<libc::proc_bsdinfo>();
            let read = unsafe {
                libc::proc_pidinfo(
                    app.processIdentifier(),
                    libc::PROC_PIDTBSDINFO,
                    0,
                    info.as_mut_ptr().cast(),
                    size as i32,
                )
            };
            if read != size as i32
                || unsafe { info.assume_init() }.pbi_uid != unsafe { libc::geteuid() }
            {
                return Err("运行副本不属于当前账户或身份无法核对。".into());
            }
            found.push(app);
        }
    }
    Ok(found)
}
fn perform(
    plan: &Plan,
    ctx: impl Fn() -> Result<Context, String>,
    job: &Job,
    request: &str,
) -> Result<Report, String> {
    let context = ctx()?;
    plan.validate(&context)?;
    let (path, id, _, _) = plan.source.control_target();
    let mut report = Report {
        request_id: request.into(),
        action: plan.action,
        path: path.into(),
        bundle_id: id.into(),
        outcome: "unknown",
        requested: false,
        pids: vec![],
        reason: String::new(),
    };
    if job.cancelled.load(Ordering::SeqCst) {
        report.outcome = "cancelled";
        report.reason = "系统请求前已取消，未请求打开或退出应用。".into();
        return Ok(report);
    }
    let apps = running(Path::new(path), id)?;
    report.pids = apps.iter().map(|a| a.processIdentifier()).collect();
    if plan.action == Action::Quit && apps.is_empty() {
        report.outcome = "notRunning";
        report.reason = "当前未发现所选安装位置正在运行的应用，未发送退出请求。".into();
        return Ok(report);
    }
    plan.validate(&ctx()?)?;
    if job.cancelled.load(Ordering::SeqCst) {
        report.outcome = "cancelled";
        report.reason = "系统请求前已取消。".into();
        return Ok(report);
    }
    match plan.action {
        Action::Open if !apps.is_empty() => {
            let accepted = apps
                .iter()
                .any(|a| a.activateWithOptions(NSApplicationActivationOptions::empty()));
            report.requested = accepted;
            report.outcome = "alreadyRunning";
            report.reason = if accepted {
                "所选安装位置已经运行，已请求显示该应用；没有新开其他安装副本。"
            } else {
                "所选安装位置已经运行，但系统未接受显示请求；请在应用自身查看。"
            }
            .into();
            return Ok(report);
        }
        Action::Open => {
            let configuration = NSWorkspaceOpenConfiguration::new();
            configuration.setCreatesNewApplicationInstance(true);
            configuration.setActivates(true);
            let url = NSURL::fileURLWithPath(&NSString::from_str(path));
            NSWorkspace::sharedWorkspace().openApplicationAtURL_configuration_completionHandler(
                &url,
                &configuration,
                None,
            );
            report.requested = true;
        }
        Action::Quit => {
            for app in &apps {
                if app.processIdentifier() == std::process::id() as i32
                    || app
                        .bundleURL()
                        .and_then(|u| u.path())
                        .is_none_or(|s| s.to_string() != path)
                {
                    return Err("运行对象在请求前变化，已保留。".into());
                }
            }
            // A successfully sent request is not proof of termination. Applications
            // may ask the user to save documents, refuse or exit much later.
            report.requested = apps.iter().fold(false, |sent, app| app.terminate() || sent);
        }
    }
    let began = Instant::now();
    loop {
        let current = running(Path::new(path), id);
        if let Ok(current) = current {
            if plan.action == Action::Open && !current.is_empty() {
                report.pids = current.iter().map(|a| a.processIdentifier()).collect();
                report.outcome = "opened";
                report.reason = "已观察到所选安装位置的应用正在运行。".into();
                break;
            }
            if plan.action == Action::Quit
                && apps
                    .iter()
                    .all(|a| a.isTerminated() || pid_absent(a.processIdentifier()))
                && current.is_empty()
            {
                report.outcome = "quit";
                report.reason = "已观察到本次选中的运行副本退出；未强制结束任何进程。".into();
                break;
            }
        }
        if job.cancelled.load(Ordering::SeqCst) {
            report.outcome = "cancelled";
            report.reason =
                "已停止等待。已经发送的打开/退出请求无法撤销，请在应用中核对实际状态。".into();
            break;
        }
        if began.elapsed() >= Duration::from_secs(10) {
            report.outcome = "pending";
            report.reason=if plan.action==Action::Quit {"退出请求后仍未确认完全退出，可能有保存提示或应用拒绝退出；请在应用自身处理，不会强制结束。"}else{"尚未观察到所选应用启动；请检查系统提示、Gatekeeper 或应用自身状态，不会自动重试。"}.into();
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(report)
}
fn context(state: &AppState, current_id: &str) -> Result<Context, String> {
    let settings = db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?;
    if !settings.acknowledged {
        return Err("请先确认首次使用说明。".into());
    }
    Ok(Context {
        home: state.home.clone(),
        engine_root: state.engine_root.clone(),
        current_app: std::env::current_exe().ok().and_then(|p| {
            p.ancestors()
                .find(|p| p.extension().is_some_and(|e| e == "app"))
                .map(Path::to_path_buf)
        }),
        current_id: current_id.into(),
        protected_paths: settings.protected_paths,
    })
}
#[tauri::command]
pub(crate) async fn control_application(
    task_id: String,
    index: usize,
    action: Action,
    request_id: String,
    confirmed: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Report, String> {
    if !confirmed {
        return Err("请先确认所选应用与操作影响。".into());
    }
    uuid::Uuid::parse_str(&request_id).map_err(|_| "应用操作标识无效。")?;
    let operation = state.updates.installation.enter()?;
    let source = state
        .results
        .lock()
        .map_err(|e| e.to_string())?
        .uninstall_target(&task_id, index)?;
    let current_id = app.config().identifier.clone();
    let _context = context(&state, &current_id)?;
    let job = Arc::new(Job::new(TaskKind::AppControl));
    {
        let database = state.db.lock().map_err(|e| e.to_string())?;
        let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
        if state.shutdown.closing()
            || jobs.len() >= 3
            || jobs.contains_key(&request_id)
            || jobs.values().any(|j| j.kind.exclusive_operation())
        {
            return Err("请等待当前任务结束再操作应用。".into());
        }
        db::start(
            &database,
            &request_id,
            if action == Action::Open {
                "appopen"
            } else {
                "appquit"
            },
            Some(source.path()),
        )?;
        jobs.insert(request_id.clone(), job.clone());
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let result = autoreleasepool(|_| {
            let plan = Plan::prepare(source, action, &context(&state, &current_id)?)?;
            perform(&plan, || context(&state, &current_id), &job, &request_id)
        });
        let (state_name, message) = match &result {
            Ok(r) => (
                match r.outcome {
                    "opened" | "alreadyRunning" | "quit" | "notRunning" => "succeeded",
                    "cancelled" => "cancelled",
                    _ => "failed",
                },
                r.reason.clone(),
            ),
            Err(e) => ("failed", e.clone()),
        };
        let saved = state
            .db
            .lock()
            .map_err(|e| e.to_string())
            .and_then(|db| db::finish(&db, &request_id, state_name, &message));
        if let Ok(mut jobs) = state.jobs.lock() {
            jobs.remove(&request_id);
        }
        saved?;
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests;
