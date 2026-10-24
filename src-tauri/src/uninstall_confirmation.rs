//! One-use confirmation of immutable scope, not lasting execution authority.
//! Submission retrieves the still-published plan and rechecks every gate.
#![allow(dead_code)]
use crate::{
    uninstall_plan::{Context, Plan},
    uninstall_runtime::{self, Observation},
    uninstall_support,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const LIFETIME: Duration = Duration::from_secs(30);
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Confirmation {
    version: u32,
    pub id: String,
    pub plan_id: String,
    plan_snapshot_sha256: String,
    created_at: i64,
    expires_at: i64,
    name: String,
    path: String,
    bundle_id: String,
    method: &'static str,
    scope: &'static str,
    related_data: &'static str,
    support_scope: &'static str,
    requires_ordinary_app_confirmation: bool,
    can_execute: bool,
    next: &'static str,
}
pub(crate) struct Selection {
    confirmation: Confirmation,
    context_sha256: String,
    issued: Instant,
    plan: Plan,
}
#[derive(Default)]
pub(crate) struct Confirmations {
    entries: HashMap<String, Selection>,
    pending: HashMap<String, u64>,
    generation: u64,
}
fn context_digest(ctx: &Context) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(
        &ctx.home,
        &ctx.engine_root,
        &ctx.current_app,
        &ctx.current_id,
        &ctx.protected_paths,
    ))
    .map_err(|_| "无法固定卸载保护上下文。")?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
impl Selection {
    pub(crate) fn plan_id(&self) -> &str {
        self.plan.id()
    }
    pub(crate) fn validate(&self, plan: &Plan, ctx: &Context, now: i64) -> Result<(), String> {
        let c = &self.confirmation;
        if now < c.created_at
            || now >= c.expires_at
            || self.issued.elapsed() >= LIFETIME
            || plan.id() != c.plan_id
            || uninstall_runtime::fingerprint(plan)? != c.plan_snapshot_sha256
            || uninstall_runtime::fingerprint(&self.plan)? != c.plan_snapshot_sha256
            || context_digest(ctx)? != self.context_sha256
        {
            return Err("卸载确认已失效、计划/保护发生变化或时间回退，请重新检查。".into());
        }
        Ok(())
    }
}
impl Confirmations {
    fn prune(&mut self, now: i64) {
        self.entries.retain(|_, s| {
            now >= s.confirmation.created_at
                && now < s.confirmation.expires_at
                && s.issued.elapsed() < LIFETIME
        });
    }
    pub(crate) fn revoke_plan(&mut self, plan_id: &str) {
        self.entries.retain(|_, s| s.plan.id() != plan_id);
        self.pending.remove(plan_id);
    }
    pub(crate) fn discard(&mut self, id: &str) {
        self.entries.remove(id);
    }
    pub(crate) fn revoke_path(&mut self, path: &str) {
        self.entries
            .retain(|_, s| s.plan.bundle_binding().0 != path);
    }
    pub(crate) fn begin_review(&mut self, plan_id: &str, now: i64) -> Result<u64, String> {
        self.prune(now);
        self.revoke_plan(plan_id);
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("请重新打开应用后复查。")?;
        self.pending.insert(plan_id.into(), self.generation);
        Ok(self.generation)
    }
    pub(crate) fn finish_review(&mut self, plan_id: &str, generation: u64) {
        if self.pending.get(plan_id) == Some(&generation) {
            self.pending.remove(plan_id);
        }
    }
    pub(crate) fn publish(
        &mut self,
        generation: u64,
        plan: &Plan,
        ctx: &Context,
        observation: &Observation,
        support: &uninstall_support::Observation,
        now: i64,
    ) -> Result<Option<Confirmation>, String> {
        if self.pending.get(plan.id()) != Some(&generation) {
            return Err("此卸载复查已撤销或被新检查取代。".into());
        }
        observation.validate_binding(plan)?;
        support.validate_binding(plan, ctx)?;
        self.finish_review(plan.id(), generation);
        if observation.report().code == uninstall_runtime::Code::NotObserved
            && support.report().background.code == uninstall_support::Code::NotDetected
            && support.report().management.code == uninstall_support::Code::NotDetected
        {
            self.issue(plan, ctx, observation, support, now).map(Some)
        } else {
            Ok(None)
        }
    }
    fn issue(
        &mut self,
        plan: &Plan,
        ctx: &Context,
        observation: &Observation,
        support: &uninstall_support::Observation,
        now: i64,
    ) -> Result<Confirmation, String> {
        self.prune(now);
        self.revoke_plan(plan.id());
        // A newly reviewed plan for this exact location also withdraws older
        // drafts. Merely opening/closing does not submit a destructive action.
        self.entries
            .retain(|_, s| s.plan.bundle_binding().0 != plan.bundle_binding().0);
        observation.permits_confirmation(plan, now)?;
        support.permits_confirmation(plan, ctx, now)?;
        // Support dependency revalidation performs bounded I/O. Do not issue
        // a draft using a process pass that expired while that I/O ran.
        let latest = crate::db::now();
        if latest < now {
            return Err("准备确认期间系统时间回退，请重新检查。".into());
        }
        observation.permits_confirmation(plan, latest)?;
        let now = latest;
        plan.archive().live_at(now)?;
        if self.entries.len() >= 8 {
            return Err("待确认卸载已满，请关闭旧确认后重试。".into());
        }
        let archive = plan.archive();
        let confirmation=Confirmation {version:2,id:uuid::Uuid::new_v4().to_string(),plan_id:plan.id().into(),plan_snapshot_sha256:uninstall_runtime::fingerprint(plan)?,created_at:now,expires_at:plan.bundle_binding().3.min(now.saturating_add(30_000)),
            name:archive.name().into(),path:archive.path().into(),bundle_id:archive.bundle_id().into(),method:"appBundleTrash",scope:"appBundleOnly",related_data:"retainAll",support_scope:"ordinaryAppDefaultLaunchAndCaskroom",requires_ordinary_app_confirmation:true,can_execute:true,
            next:"仅支持普通 App；受管理或带专用后台服务的应用请使用对应卸载方式。仅移入此 App 包，设置、缓存和个人数据保留；提交后重新检查，条件不足时保留。"};
        self.entries.insert(
            confirmation.id.clone(),
            Selection {
                confirmation: confirmation.clone(),
                context_sha256: context_digest(ctx)?,
                issued: Instant::now(),
                plan: plan.clone(),
            },
        );
        Ok(confirmation)
    }
    pub(crate) fn take(
        &mut self,
        id: &str,
        plan: &Plan,
        ctx: &Context,
        now: i64,
    ) -> Result<Selection, String> {
        let selection = self
            .entries
            .remove(id)
            .ok_or("卸载确认已撤销、过期或使用，请重新检查。")?;
        selection.validate(plan, ctx, now)?;
        Ok(selection)
    }
}

#[cfg(test)]
mod tests;
