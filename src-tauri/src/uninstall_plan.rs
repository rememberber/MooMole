//! Independent, ephemeral whole-App plans from retained Mole list rows. A plan
//! is NOT a confirmation, runtime observation or authority to move anything.
use crate::{
    app_background::Marker, app_inspection::identity, app_protection, app_uninstall_rules, engine,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, path::PathBuf};

const LIFETIME_MS: i64 = 5 * 60_000;
const MAX_PLANS: usize = 8;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Source {
    task_id: String,
    index: usize,
    path: String,
    bundle_id: String,
    name: String,
    source: String,
}
impl Source {
    pub(crate) fn path(&self) -> &str {
        &self.path
    }
    // Called only for a retained successful apps snapshot by tasks::Results.
    pub(crate) fn from_row(task_id: &str, index: usize, row: &Value) -> Result<Self, String> {
        uuid::Uuid::parse_str(task_id).map_err(|_| "应用清单标识无效。")?;
        let text = |key: &str, limit| {
            row.get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty() && s.len() <= limit && !s.chars().any(char::is_control))
                .map(str::to_owned)
                .ok_or("清单中的应用身份或来源不完整，请重新读取列表。".to_string())
        };
        Ok(Self {
            task_id: task_id.into(),
            index,
            path: text("path", 4096)?,
            bundle_id: text("bundle_id", 255)?,
            name: text("name", 1024)?,
            source: text("source", 64)?,
        })
    }
}

#[derive(Clone)]
pub(crate) struct Context {
    pub home: PathBuf,
    pub engine_root: PathBuf,
    pub current_app: Option<PathBuf>,
    pub current_id: String,
    pub protected_paths: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct Plan {
    id: String,
    source: Source,
    snapshot: identity::Snapshot,
    home: PathBuf,
    uid: u32,
    created_at: i64,
    expires_at: i64,
    revision: String,
    digest: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    version: u32,
    id: String,
    source_task_id: String,
    source_index: usize,
    created_at: i64,
    expires_at: i64,
    checked_at: i64,
    revision: String,
    name: String,
    path: String,
    bundle_id: String,
    app_version: Option<String>,
    build: Option<String>,
    snapshot_sha256: String,
    read_only: bool,
    can_execute: bool,
    scope: &'static str,
    related_data: &'static str,
    state: &'static str,
    next: &'static str,
}

fn guards(source: &Source, ctx: &Context) -> Result<(), String> {
    if source.source != "App" {
        return Err(
            "首批仅准备普通 App 包；Homebrew 或未知管理来源应保留并使用相应卸载方式。".into(),
        );
    }
    let path = std::path::Path::new(&source.path);
    if path.parent() != Some(ctx.home.join("Applications").as_path())
        && path.parent() != Some(std::path::Path::new("/Applications"))
    {
        return Err("首批仅准备 Applications 根目录中的完整 App 包；其他安装位置暂不支持。".into());
    }
    if !app_protection::check(
        &source.path,
        Some(&source.bundle_id),
        ctx.current_app.as_deref(),
        &ctx.current_id,
        &ctx.protected_paths,
    )?
    .reasons
    .is_empty()
    {
        return Err("应用位于保护范围或属于 MooMole 自身，不能生成卸载计划。".into());
    }
    let policy = app_uninstall_rules::load(&ctx.engine_root)?;
    if app_uninstall_rules::observe(Some(&policy), Some(&source.bundle_id), "matching").code
        != "notMatched"
        || app_uninstall_rules::observe_official(
            Some(&policy),
            Some(&source.bundle_id),
            "matching",
            Some(&source.name),
            &source.path,
        )
        .code()
            != "notMatched"
        || app_uninstall_rules::observe_path_with_identity(
            Some(&policy),
            &ctx.home,
            path,
            "present",
            &mut app_uninstall_rules::PathBudget::default(),
        )
        .code
            != "notMatched"
    {
        return Err("Mole 要求保留、使用官方卸载器，或卸载保护仍未知；此 App 保留。".into());
    }
    Ok(())
}

fn digest(source: &Source, snapshot: &identity::Snapshot) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(engine::REVISION, source, snapshot))
        .map_err(|_| "无法固定本次 App 身份。")?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

impl Plan {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }
    pub(crate) fn prepare(source: Source, ctx: &Context) -> Result<Self, String> {
        guards(&source, ctx)?;
        if unsafe { libc::geteuid() } == 0 {
            return Err("请使用普通账户运行 MooMole。".into());
        }
        let snapshot = identity::capture(std::path::Path::new(&source.path), &source.bundle_id)?;
        if !matches!(snapshot.background, Marker::Absent | Marker::Disabled) {
            return Err("首批不准备后台专用或后台标记未知的 App。".into());
        }
        let now = crate::db::now();
        let digest = digest(&source, &snapshot)?;
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            source,
            snapshot,
            home: ctx.home.clone(),
            uid: unsafe { libc::geteuid() },
            created_at: now,
            expires_at: now.saturating_add(LIFETIME_MS),
            revision: engine::REVISION.into(),
            digest,
        })
    }

    pub(crate) fn source_matches(&self, source: &Source) -> bool {
        self.source == *source
    }
    pub(crate) fn source_address(&self) -> (&str, usize) {
        (&self.source.task_id, self.source.index)
    }

    pub(crate) fn recheck(&self, ctx: &Context, now: i64) -> Result<View, String> {
        if now < self.created_at
            || now >= self.expires_at
            || self.revision != engine::REVISION
            || self.home != ctx.home
            || self.uid != unsafe { libc::geteuid() }
        {
            return Err("卸载计划已失效或运行账户/目录变化，请重新读取应用列表。".into());
        }
        guards(&self.source, ctx)?;
        let snapshot = identity::capture(
            std::path::Path::new(&self.source.path),
            &self.source.bundle_id,
        )?;
        if snapshot != self.snapshot || digest(&self.source, &snapshot)? != self.digest {
            return Err("App 包、主程序、身份或目录内容已变化，请重新读取列表并生成计划。".into());
        }
        let checked = crate::db::now();
        if checked < now || checked >= self.expires_at {
            return Err("核对期间卸载计划已失效，请重新生成。".into());
        }
        Ok(self.view(checked))
    }

    pub(crate) fn view(&self, checked: i64) -> View {
        View { version:1, id:self.id.clone(),source_task_id:self.source.task_id.clone(),source_index:self.source.index,
            created_at:self.created_at,expires_at:self.expires_at,checked_at:checked,revision:self.revision.clone(),
            name:self.source.name.clone(),path:self.source.path.clone(),bundle_id:self.snapshot.bundle_id.clone(),
            app_version:self.snapshot.app_version.clone(),build:self.snapshot.build.clone(),snapshot_sha256:self.digest.clone(),
            read_only:true,can_execute:false,scope:"appBundleOnly",related_data:"retainAll",state:"requiresExecutionReview",
            next:"已固定此 App 包；关联数据保留。运行/后台、管理方式、权限、执行确认与持久收据尚未接入，当前不能卸载。" }
    }
}

#[derive(Default)]
pub(crate) struct Store {
    entries: HashMap<String, Plan>,
    generation: u64,
    pending: HashMap<String, (u64, Option<String>)>,
}
impl Store {
    pub(crate) fn begin(&mut self, path: &str, plan_id: Option<&str>) -> Result<u64, String> {
        self.prune(crate::db::now());
        if plan_id.is_some_and(|id| !self.entries.get(id).is_some_and(|p| p.source.path == path)) {
            return Err("此卸载计划已被撤下或取代，请使用最新计划。".into());
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("请重新打开应用后准备卸载计划。")?;
        self.entries.retain(|_, p| p.source.path != path);
        self.pending
            .insert(path.into(), (self.generation, plan_id.map(str::to_owned)));
        Ok(self.generation)
    }
    pub(crate) fn publish(
        &mut self,
        generation: u64,
        plan: Plan,
        now: i64,
    ) -> Result<View, String> {
        if !self
            .pending
            .get(&plan.source.path)
            .is_some_and(|(g, id)| *g == generation && id.as_ref().is_none_or(|id| id == &plan.id))
        {
            return Err("此计划已被新的检查取代，请使用最新结果。".into());
        }
        self.pending.remove(&plan.source.path);
        self.insert(plan, now)
    }
    pub(crate) fn finish_attempt(&mut self, path: &str, generation: u64) {
        if self
            .pending
            .get(path)
            .is_some_and(|(g, _)| *g == generation)
        {
            self.pending.remove(path);
        }
    }
    fn prune(&mut self, now: i64) {
        self.entries
            .retain(|_, p| now >= p.created_at && now < p.expires_at);
    }
    fn insert(&mut self, plan: Plan, now: i64) -> Result<View, String> {
        self.prune(now);
        if now < plan.created_at || now >= plan.expires_at {
            return Err("卸载计划已失效。".into());
        }
        self.entries
            .retain(|_, p| p.source.path != plan.source.path);
        if self.entries.len() >= MAX_PLANS {
            return Err("待审阅卸载计划已满，请关闭旧计划后重试。".into());
        }
        let view = plan.view(now);
        self.entries.insert(plan.id.clone(), plan);
        Ok(view)
    }
    pub(crate) fn get(&mut self, id: &str, now: i64) -> Result<Plan, String> {
        self.prune(now);
        self.entries
            .get(id)
            .cloned()
            .ok_or("卸载计划不存在或已过期，请重新读取应用列表。".into())
    }
    pub(crate) fn discard(&mut self, id: &str) {
        self.entries.remove(id);
        self.pending
            .retain(|_, (_, pending_id)| pending_id.as_deref() != Some(id));
    }
}

#[cfg(test)]
mod tests;
