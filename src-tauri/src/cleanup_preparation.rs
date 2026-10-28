//! Read-only cache and single-log preparation from THIS rule review. Never an execution
//! ticket: no handles, approvals or rule evidence are persisted for later use.
use crate::{cleanup_target::PreparedFile, permissions, provenance, review, rules};
use serde::Serialize;
use std::{
    collections::HashSet,
    path::Path,
    time::{Duration, Instant},
};

pub const VERSION: u64 = 2;
pub const LIFETIME_MS: i64 = 30_000;
const MAX_NATIVE: usize = 20;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub code: &'static str,
    pub reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub version: u64,
    pub plan_id: String,
    pub scope: &'static str,
    pub method: &'static str,
    pub read_only: bool,
    pub checked_at: i64,
    pub expires_at: i64,
    pub observed_count: usize,
    pub authorization_count: usize,
    pub items: Vec<Item>,
}
impl Report {
    pub fn withdraw(&mut self, reason: &str) {
        self.observed_count = 0;
        self.authorization_count = 0;
        for item in &mut self.items {
            item.code = "notChecked";
            item.reason = reason.into();
        }
    }
}

/// Execution scope comes from captured Mole provenance and physical path boundaries.
pub(crate) fn supported(item: &review::ScanItem, home: &Path) -> bool {
    let binding = provenance::binding(item.candidate.provenance.as_ref(), &item.candidate.path);
    rules::module_scope(Path::new(&item.candidate.path), home) == binding
        && (binding == "userCache" || (binding == "userLogs" && !item.candidate.is_directory))
}

pub fn observe(
    plan: &review::Plan,
    view: &review::PlanView,
    home: &Path,
    now: i64,
    cancelled: impl Fn() -> bool,
) -> Report {
    observe_with(plan, view, home, now, cancelled, |item| {
        PreparedFile::prepare(item)?.validate()
    })
}

pub(crate) fn prepare_one(
    plan: &review::Plan,
    view: &review::PlanView,
    home: &Path,
    now: i64,
    cancelled: impl Fn() -> bool,
) -> Result<PreparedFile, String> {
    if plan.items.len() != 1 {
        return Err("执行复核必须逐项进行。".into());
    }
    let mut target = None;
    let report = observe_with(plan, view, home, now, cancelled, |item| {
        let prepared = PreparedFile::prepare(item)?;
        prepared.validate()?;
        target = Some(prepared);
        Ok(())
    });
    if report.observed_count != 1 {
        return Err(report
            .items
            .first()
            .map(|i| i.reason.clone())
            .unwrap_or("文件准备未完成。".into()));
    }
    target.ok_or("缺少本次原生对象引用。".into())
}

fn observe_with(
    plan: &review::Plan,
    view: &review::PlanView,
    home: &Path,
    now: i64,
    cancelled: impl Fn() -> bool,
    mut native: impl FnMut(&review::ScanItem) -> Result<(), String>,
) -> Report {
    let started = Instant::now();
    let mut attempts = 0;
    let bound = !plan.items.is_empty()
        && plan.items.len() <= review::MAX_SELECTION
        && plan
            .items
            .iter()
            .map(|i| &i.candidate.id)
            .collect::<HashSet<_>>()
            .len()
            == plan.items.len()
        && view.summary.id == plan.id
        && view.summary.scan_id == plan.scan_id
        && view.summary.item_count == plan.items.len()
        && view.items.len() == plan.items.len()
        && view
            .items
            .iter()
            .zip(&plan.items)
            .all(|(a, b)| a.id == b.candidate.id && a.path == b.candidate.path)
        && view.read_only
        && view.requires_mole_recheck
        && plan.protocol == crate::clean_preview::PROTOCOL
        && plan.revision == crate::engine::REVISION
        && view.rule_checked_at.is_some_and(|at| {
            at >= plan.created_at && at <= now && now.saturating_sub(at) < LIFETIME_MS
        })
        && now >= plan.created_at
        && now >= view.checked_at
        && now < plan.expires_at;
    let mut report = Report {
        version: VERSION,
        plan_id: plan.id.clone(),
        scope: if plan.items.iter().any(|i| {
            provenance::binding(i.candidate.provenance.as_ref(), &i.candidate.path) == "userCache"
        }) {
            "userCachesAndLogs"
        } else {
            "userLogFiles"
        },
        method: "trash",
        read_only: true,
        checked_at: now,
        expires_at: plan.expires_at.min(now.saturating_add(LIFETIME_MS)).min(
            view.rule_checked_at
                .unwrap_or(now)
                .saturating_add(LIFETIME_MS),
        ),
        observed_count: 0,
        authorization_count: 0,
        items: vec![],
    };
    for (index, item) in plan.items.iter().enumerate() {
        let current = view.items.get(index);
        let refusal = if !bound {
            Some((
                "notChecked",
                "缺少本次完整规则结果或计划已失效，请重新复查。".into(),
            ))
        } else if cancelled() {
            Some(("notChecked", "准备已取消，之前的准备结果不再适用。".into()))
        } else if !matches!(
            current.unwrap().state.as_str(),
            "unchanged" | "activityUnknown"
        ) {
            Some(("retained", current.unwrap().reason.clone()))
        } else if !supported(item, home) {
            Some((
                "retained",
                "仅支持来源明确的用户缓存和普通日志文件；此项目保留。".into(),
            ))
        } else if item.candidate.is_directory
            && item
                .candidate
                .size
                .is_none_or(|size| size > 9_007_199_254_740_991)
        {
            Some(("retained", "缓存目录缺少可核对的大小，保留。".into()))
        } else if item.candidate.is_directory
            && !current.unwrap().descendant_rules.as_ref().is_some_and(|d| {
                d.version == crate::descendant_rules::VERSION && d.code == "checked"
            })
        {
            Some(("retained", "缓存目录的后代保护尚未全部核对，保留。".into()))
        } else if current.unwrap().permissions.version != permissions::VERSION
            || current.unwrap().permissions.code != permissions::Code::NoRestrictionObserved
        {
            Some(("retained", "文件权限仍受限或未知，不能继续准备。".into()))
        } else if !current.unwrap().mole_rule.as_ref().is_some_and(|rule| {
            rule.code == "passed"
                && rule.module.version == rules::MODULE_VERSION
                && rule.module.binding
                    == provenance::binding(item.candidate.provenance.as_ref(), &item.candidate.path)
                && rule.module.scope == rule.module.binding
                && rule.module.code == "checked"
                && rule.activity.version == rules::ACTIVITY_VERSION
                && ((rule.activity.code == "noOpenHandles"
                    && current.unwrap().state == "unchanged")
                    || (rule.activity.code == "unknown"
                        && current.unwrap().state == "activityUnknown"))
        }) {
            Some((
                "retained",
                "本次 Mole 模块、保护或占用条件尚未全部确认，保留此项。".into(),
            ))
        } else if attempts >= MAX_NATIVE || started.elapsed() > Duration::from_millis(500) {
            Some((
                "notChecked",
                "本轮准备预算已用完，请缩小选择后重新复查。".into(),
            ))
        } else {
            attempts += 1;
            match native(item) {
                Ok(()) if started.elapsed() <= Duration::from_millis(500) => None,
                Ok(()) => Some((
                    "notChecked",
                    "文件准备超时，未发布通过结论，请重新复查。".into(),
                )),
                Err(reason) => Some(("retained", reason)),
            }
        };
        let (code, reason) = refusal.unwrap_or_else(|| {
            if current.unwrap().mole_rule.as_ref().unwrap().activity.code == "unknown" {
                report.authorization_count += 1;
                return (
                    "authorizationRequired",
                    "占用尚未确认。可先核对固定范围，再由本人授权当次只读检查；未确认空闲则保留。"
                        .into(),
                );
            }
            report.observed_count += 1;
            (
                "observed",
                "本轮清理规则、权限及原生对象准备已核对；执行前仍需重新验证。".into(),
            )
        });
        report.items.push(Item {
            id: item.candidate.id.clone(),
            code,
            reason,
        });
    }
    if cancelled() {
        report.withdraw("准备已取消，之前的准备结果不再适用。");
    }
    report
}

#[cfg(test)]
pub(crate) mod tests;
