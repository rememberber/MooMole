//! User-selected folder import into Spotlight. Never resets a volume index,
//! changes Spotlight privacy settings or deletes source files.
use super::quicklook::{self, Attempt, Outcome, Stop};
use crate::{
    db,
    engine::{Job, TaskKind},
    observation::AnchoredPath,
    AppState,
};
use serde::Serialize;
use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

const TOOL: &str = "/usr/bin/mdimport";
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Step {
    ImportFolder,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Draft {
    id: String,
    path: String,
    created_at: i64,
    expires_at: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    request_id: String,
    path: String,
    attempt: Attempt<Step>,
}
pub(crate) struct Plan {
    draft: Draft,
    root: AnchoredPath,
    tool: AnchoredPath,
    issued: Instant,
}
#[derive(Clone)]
struct Context {
    home: PathBuf,
    protected: Vec<String>,
}
impl Plan {
    fn prepare(path: &Path, ctx: &Context) -> Result<Self, String> {
        let root = AnchoredPath::open(path)?;
        let tool = AnchoredPath::open(Path::new(TOOL))?;
        let now = db::now();
        let p = Self {
            draft: Draft {
                id: uuid::Uuid::new_v4().to_string(),
                path: path.to_str().ok_or("目录路径无法编码。")?.into(),
                created_at: now,
                expires_at: now + 120_000,
            },
            root,
            tool,
            issued: Instant::now(),
        };
        p.validate(ctx)?;
        Ok(p)
    }
    fn validate(&self, ctx: &Context) -> Result<(), String> {
        let path = Path::new(&self.draft.path);
        let relative = path
            .strip_prefix(&ctx.home)
            .map_err(|_| "首批仅支持当前用户个人目录中的文件夹。")?;
        if relative.as_os_str().is_empty()
            || relative.components().any(|c| {
                let name = c.as_os_str().to_string_lossy();
                name.starts_with('.')
                    || ["Library", "Applications"].contains(&name.as_ref())
                    || name.ends_with(".app")
            })
            || self.draft.path.chars().any(char::is_control)
        {
            return Err("请选择个人文件夹，保留账户根目录、资源库、隐藏目录与应用包。".into());
        }
        if !crate::app_protection::check(&self.draft.path, None, None, "", &ctx.protected)?
            .reasons
            .is_empty()
        {
            return Err("所选目录与保护范围重叠，保留此任务。".into());
        }
        let m = self.root.target().metadata().map_err(|e| e.to_string())?;
        let t = self.tool.target().metadata().map_err(|e| e.to_string())?;
        let uid = unsafe { libc::geteuid() };
        if uid == 0
            || uid != unsafe { libc::getuid() }
            || !m.is_dir()
            || m.uid() != uid
            || !self.root.local_materialized()
            || !t.is_file()
            || t.mode() & 0o111 == 0
            || !quicklook::readonly_system_tool(&self.tool)
            || self
                .tool
                .witness
                .nodes
                .iter()
                .any(|n| n.uid != 0 || n.mode & 0o022 != 0)
            || db::now() < self.draft.created_at
            || db::now() >= self.draft.expires_at
            || self.issued.elapsed() >= Duration::from_secs(120)
        {
            return Err("目录、系统工具或确认时效无法核对，请重新选择。".into());
        }
        self.root.validate()?;
        self.tool.validate()
    }
    fn run(&self, ctx: &Context, job: &Job) -> Result<Attempt<Step>, String> {
        self.validate(ctx)?;
        let mut command = Command::new(TOOL);
        command
            .arg("-i")
            .arg(&self.draft.path)
            .env_clear()
            .env("HOME", &ctx.home)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("LANG", "C")
            .current_dir(&ctx.home);
        Ok(quicklook::supervise(
            Step::ImportFolder,
            command,
            job,
            Duration::from_secs(60),
            Duration::from_secs(1),
            |_| {},
        ))
    }
}
fn context(state: &AppState) -> Result<Context, String> {
    let settings = db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?;
    if !settings.acknowledged || state.shutdown.closing() {
        return Err("请先确认首次使用说明，且不要在退出期间操作。".into());
    }
    Ok(Context {
        home: state.home.clone(),
        protected: settings.protected_paths,
    })
}
#[tauri::command]
pub(crate) async fn prepare_search_repair(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Draft, String> {
    let ctx = context(&state)?;
    let permit = state
        .maintenance_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "维护检查仍在进行。")?;
    let operation = state.updates.installation.enter()?;
    let plan = tauri::async_runtime::spawn_blocking(move || {
        let (_permit, _operation) = (permit, operation);
        Plan::prepare(Path::new(&path), &ctx)
    })
    .await
    .map_err(|e| e.to_string())??;
    plan.validate(&context(&state)?)?;
    let draft = plan.draft.clone();
    *state.search_repairs.lock().map_err(|e| e.to_string())? = Some(plan);
    Ok(draft)
}
#[tauri::command]
pub(crate) fn discard_search_repair(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut plan = state.search_repairs.lock().map_err(|e| e.to_string())?;
    if plan.as_ref().is_some_and(|p| p.draft.id == id) {
        plan.take();
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn execute_search_repair(
    id: String,
    request_id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Report, String> {
    if !confirmed {
        return Err("请先确认索引目录与影响。".into());
    }
    uuid::Uuid::parse_str(&request_id).map_err(|_| "索引操作标识无效。")?;
    let operation = state.updates.installation.enter()?;
    let plan = {
        let mut entry = state.search_repairs.lock().map_err(|e| e.to_string())?;
        if !entry.as_ref().is_some_and(|p| p.draft.id == id) {
            return Err("索引确认已失效或已使用。".into());
        }
        entry.take().ok_or("索引确认已使用。")?
    };
    plan.validate(&context(&state)?)?;
    let job = Arc::new(Job::new(TaskKind::SearchRepair));
    {
        let database = state.db.lock().map_err(|e| e.to_string())?;
        let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
        if state.shutdown.closing()
            || jobs.len() >= 3
            || jobs.contains_key(&request_id)
            || jobs.values().any(|j| j.kind.exclusive_operation())
        {
            return Err("请等待当前任务结束。".into());
        }
        db::start(
            &database,
            &request_id,
            "searchrepair",
            Some(&plan.draft.path),
        )?;
        jobs.insert(request_id.clone(), job.clone());
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let result = context(&state).and_then(|ctx| plan.run(&ctx, &job));
        let (status, message) = match &result {
            Ok(a) if a.outcome == Outcome::CommandSucceeded => ("succeeded", "目录索引请求已完成，Spotlight 后台更新可能仍在继续；请用原来的文件名重新搜索，不能据此认定问题已修复。".to_string()),
            Ok(a) if a.stop == Some(Stop::Cancelled) => ("cancelled", "已停止本次索引命令；已提交给 Spotlight 的后台工作可能继续，无法撤销。".into()),
            Ok(_) => ("failed", "索引请求未确认完成；请检查权限与 Spotlight 隐私排除，不会自动重试。".into()),
            Err(e) => ("failed", e.clone()),
        };
        let saved = state.db.lock().map_err(|e| e.to_string()).and_then(|db| db::finish(&db, &request_id, status, &message));
        if let Ok(mut jobs) = state.jobs.lock() { jobs.remove(&request_id); }
        saved?;
        result.map(|attempt| Report { request_id, path: plan.draft.path, attempt })
    }).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests;
