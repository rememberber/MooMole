//! One-use, short-lived fixed service selection. No archive can restore this.
use super::{
    quicklook::{Prepared, Step},
    refresh_journal as journal,
};
use crate::{db, engine};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Serialize)]
pub(crate) struct Context {
    pub home: PathBuf,
    pub engine_root: PathBuf,
    pub protected_paths: Vec<String>,
    pub acknowledged: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Draft {
    pub version: u32,
    pub id: String,
    pub plan_id: String,
    pub action: &'static str,
    pub method: &'static str,
    pub scope: &'static str,
    pub revision: &'static str,
    pub selected: Vec<Step>,
    pub created_at: i64,
    pub expires_at: i64,
    pub snapshot_sha256: String,
    pub can_submit: bool,
    pub preparation_read_only: bool,
    pub requires_rebuild_acknowledgement: bool,
}
#[derive(Clone)]
pub(crate) struct Authorized {
    draft: Draft,
    context: String,
    policy: String,
    tool: String,
    issued: Instant,
}
pub(super) fn digest(value: &impl Serialize) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|_| "无法固定维护上下文。")?)
    ))
}
fn policy(ctx: &Context) -> Result<String, String> {
    if !ctx.acknowledged {
        return Err("请先确认首次使用说明。".into());
    }
    if !ctx.protected_paths.is_empty() {
        return Err("当前设置了目录保护。Quick Look 服务不能按目录排除刷新，首批保留此任务；不会自动更改保护设置。".into());
    }
    let (source, patterns) = super::rules(&ctx.home);
    let patterns = patterns.ok_or("维护保护配置无法完整读取，保留此任务。")?;
    if patterns.iter().any(|p| p == "cache_refresh") {
        return Err("Mole 维护白名单要求跳过 Finder 缓存刷新。".into());
    }
    let catalog: serde_json::Value =
        serde_json::from_str(include_str!("../../../src/lib/maintenance-catalog.json"))
            .map_err(|_| "维护目录无法核对。")?;
    let known = catalog["items"].as_array().ok_or("维护目录不完整。")?;
    if patterns
        .iter()
        .any(|p| !known.iter().any(|i| i["id"].as_str() == Some(p)))
    {
        return Err(
            "维护配置含路径或无法映射的保护项；服务刷新无法逐路径跳过，保留此任务。".into(),
        );
    }
    let info = engine::info(&ctx.engine_root);
    if !info.available || info.version.as_deref() != Some("1.65.0") {
        return Err("Mole 引擎基线无法核对。".into());
    }
    digest(&(source, patterns))
}
impl Authorized {
    pub(crate) fn prepare(ctx: &Context, selected: Vec<Step>) -> Result<Self, String> {
        if !matches!(
            selected.as_slice(),
            [Step::ThumbnailCache] | [Step::Generators] | [Step::ThumbnailCache, Step::Generators]
        ) {
            return Err("请按原顺序选择一项或两项 Quick Look 刷新。".into());
        }
        let policy = policy(ctx)?;
        let tool = Prepared::prepare(&ctx.home)?.binding()?;
        let context = digest(&(ctx, unsafe { libc::geteuid() }))?;
        let snapshot_sha256 = digest(&(&context, &policy, &tool, &selected, engine::REVISION))?;
        let created_at = db::now();
        Ok(Self {
            draft: Draft {
                version: 1,
                id: uuid::Uuid::new_v4().to_string(),
                plan_id: uuid::Uuid::new_v4().to_string(),
                action: "cache_refresh",
                method: "quickLookRefresh",
                scope: "quickLookServiceOnly",
                revision: engine::REVISION,
                selected,
                created_at,
                expires_at: created_at.saturating_add(30_000),
                snapshot_sha256,
                can_submit: true,
                preparation_read_only: true,
                requires_rebuild_acknowledgement: true,
            },
            context,
            policy,
            tool,
            issued: Instant::now(),
        })
    }
    pub(crate) fn view(&self) -> Draft {
        self.draft.clone()
    }
    pub(crate) fn selected(&self) -> &[Step] {
        &self.draft.selected
    }
    pub(crate) fn validate(&self, ctx: &Context) -> Result<(), String> {
        self.validate_context(ctx)?;
        let now = db::now();
        if now < self.draft.created_at
            || now >= self.draft.expires_at
            || self.issued.elapsed() >= Duration::from_secs(30)
        {
            return Err("维护确认已过期或时间变化，请重新检查。".into());
        }
        if digest(&(ctx, unsafe { libc::geteuid() }))? != self.context
            || policy(ctx)? != self.policy
            || Prepared::prepare(&ctx.home)?.binding()? != self.tool
        {
            return Err("规则、保护、账户或系统工具已变化，请重新检查。".into());
        }
        // I/O cannot renew either original clock bound.
        let now = db::now();
        if now < self.draft.created_at
            || now >= self.draft.expires_at
            || self.issued.elapsed() >= Duration::from_secs(30)
        {
            return Err("维护复查期间确认已过期。".into());
        }
        Ok(())
    }
    pub(crate) fn validate_context(&self, ctx: &Context) -> Result<(), String> {
        let now = db::now();
        if now < self.draft.created_at
            || now >= self.draft.expires_at
            || self.issued.elapsed() >= Duration::from_secs(30)
            || digest(&(ctx, unsafe { libc::geteuid() }))? != self.context
        {
            return Err("维护确认已失效或保护上下文已变化，请重新检查。".into());
        }
        Ok(())
    }
    pub(crate) fn journal_selection(&self) -> Result<journal::Selection, String> {
        journal::Selection::new(
            &self.draft.plan_id,
            &self.draft.id,
            self.draft.selected.clone(),
            &self.draft.snapshot_sha256,
            self.draft.created_at,
            self.draft.expires_at,
        )
    }
}
#[derive(Default)]
pub(crate) struct Store {
    generation: u64,
    pending: Option<u64>,
    entries: HashMap<String, Authorized>,
}
impl Store {
    pub(crate) fn begin(&mut self) -> Result<u64, String> {
        self.generation = self.generation.checked_add(1).ok_or("请重新打开应用。")?;
        self.entries.clear();
        self.pending = Some(self.generation);
        Ok(self.generation)
    }
    pub(crate) fn abort(&mut self, ticket: u64) {
        if self.pending == Some(ticket) {
            self.pending = None;
        }
    }
    pub(crate) fn publish(&mut self, ticket: u64, a: Authorized) -> Result<Draft, String> {
        if self.pending != Some(ticket) {
            return Err("此维护检查已被撤销或取代。".into());
        }
        self.pending = None;
        let draft = a.view();
        self.entries.insert(draft.id.clone(), a);
        Ok(draft)
    }
    pub(crate) fn discard(&mut self, id: &str) {
        self.entries.remove(id);
    }
    pub(crate) fn take(&mut self, id: &str, plan_id: &str) -> Result<Authorized, String> {
        let a = self
            .entries
            .remove(id)
            .ok_or("维护确认已使用、撤销或不存在，请重新检查。")?;
        if a.draft.plan_id != plan_id {
            return Err("维护确认不属于此计划。".into());
        }
        Ok(a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neither_wall_clock_nor_monotonic_confirmation_expiry_can_be_renewed_by_validation() {
        let root = tempfile::tempdir().unwrap();
        let ctx = Context {
            home: root.path().canonicalize().unwrap(),
            engine_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
            protected_paths: vec![],
            acknowledged: true,
        };
        let mut a = Authorized::prepare(&ctx, vec![Step::Generators]).unwrap();
        a.issued = Instant::now() - Duration::from_secs(31);
        assert!(a.validate(&ctx).is_err());
        a.issued = Instant::now();
        a.draft.created_at = db::now() + 1000;
        a.draft.expires_at = a.draft.created_at + 30_000;
        assert!(a.validate_context(&ctx).is_err());
    }
}
