//! Fresh, bounded *retention* checks for ordinary whole-App uninstall. These
//! observations never stop services, infer ownership for deletion, or authorize
//! a native call. Custom managers and opaque external registrations are outside
//! this declared scope, not proved absent by a negative result.
use crate::{
    uninstall_plan::{Context, Plan},
    uninstall_runtime,
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

mod plist;
mod scan;

const FRESH: Duration = Duration::from_secs(5);
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Code {
    NotDetected,
    Detected,
    Unknown,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Finding {
    pub code: Code,
    reason: Option<&'static str>,
    checked_entries: usize,
}
impl Finding {
    fn clear(entries: usize) -> Self {
        Self {
            code: Code::NotDetected,
            reason: None,
            checked_entries: entries,
        }
    }
    fn detected(reason: &'static str, entries: usize) -> Self {
        Self {
            code: Code::Detected,
            reason: Some(reason),
            checked_entries: entries,
        }
    }
    fn unknown(reason: &'static str, entries: usize) -> Self {
        Self {
            code: Code::Unknown,
            reason: Some(reason),
            checked_entries: entries,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    version: u32,
    plan_id: String,
    plan_snapshot_sha256: String,
    checked_at: i64,
    expires_at: i64,
    scope: &'static str,
    pub background: Finding,
    pub management: Finding,
    can_prepare_confirmation: bool,
    can_execute: bool,
    next: &'static str,
}
pub(crate) struct Observation {
    report: Report,
    started: Instant,
    home: PathBuf,
    dependencies: scan::Scan,
}
impl Observation {
    pub(crate) fn report(&self) -> &Report {
        &self.report
    }
    pub(crate) fn validate_binding(&self, plan: &Plan, ctx: &Context) -> Result<(), String> {
        if self.report.plan_id != plan.id()
            || self.report.plan_snapshot_sha256 != uninstall_runtime::fingerprint(plan)?
            || self.home != ctx.home
        {
            return Err("后台/管理方式检查不属于当前计划或账户。".into());
        }
        Ok(())
    }
    pub(crate) fn permits_confirmation(
        &self,
        plan: &Plan,
        ctx: &Context,
        now: i64,
    ) -> Result<(), String> {
        self.validate_binding(plan, ctx)?;
        if !self.report.can_prepare_confirmation
            || self.report.background.code != Code::NotDetected
            || self.report.management.code != Code::NotDetected
            || now < self.report.checked_at
            || now >= self.report.expires_at
            || self.started.elapsed() >= FRESH
        {
            return Err(
                "发现后台/管理线索，或检查未知/过期，请重新检查并使用相应卸载方式。".into(),
            );
        }
        self.dependencies
            .verify()
            .map_err(|_| "后台或管理方式检查的目录/配置已变化，请重新检查。".to_string())?;
        // Verification does not renew the original observation.
        if crate::db::now() < now
            || crate::db::now() >= self.report.expires_at
            || self.started.elapsed() >= FRESH
        {
            return Err("复核期间后台/管理方式观察已失效。".into());
        }
        Ok(())
    }
}

struct Scope {
    launch: Vec<PathBuf>,
    rooms: Vec<PathBuf>,
    helpers: PathBuf,
}
impl Scope {
    fn system(home: &Path) -> Self {
        Self {
            launch: vec![
                home.join("Library/LaunchAgents"),
                "/Library/LaunchAgents".into(),
                "/Library/LaunchDaemons".into(),
            ],
            rooms: vec![
                "/opt/homebrew/Caskroom".into(),
                "/usr/local/Caskroom".into(),
            ],
            helpers: "/Library/PrivilegedHelperTools".into(),
        }
    }
}
fn related_id(value: &str, id: &str) -> bool {
    value == id || value.strip_prefix(id).is_some_and(|v| v.starts_with('.'))
}

fn background(plan: &Plan, scope: &Scope, scan: &mut scan::Scan) -> Finding {
    let result = (|| {
        let (raw, snapshot, _, _) = plan.bundle_binding();
        let app = Path::new(raw);
        // Modern SMAppService definitions and bundled login/privileged helpers
        // stay unsupported even if disabled or not currently running.
        for relative in [
            "Contents/Library/LoginItems",
            "Contents/Library/LaunchAgents",
            "Contents/Library/LaunchDaemons",
            "Contents/Library/HelperTools",
            "Contents/Library/SystemExtensions",
        ] {
            if scan.exists(&app.join(relative))? {
                return Ok(Some("embeddedBackground"));
            }
        }
        let info = scan.read(&app.join("Contents/Info.plist"))?;
        if plist::privileged(&info)? {
            return Ok(Some("privilegedDeclaration"));
        }
        let id = &snapshot.bundle_id;
        for root in &scope.launch {
            for entry in scan.directory(root)? {
                let name = entry.file_name().ok_or("entryUnavailable")?;
                let Some(name) = name.to_str() else {
                    return Err("entryUnavailable");
                };
                if !name.ends_with(".plist") {
                    continue;
                }
                // A matching name is a reason to retain, not ownership proof.
                if name
                    .strip_suffix(".plist")
                    .is_some_and(|v| related_id(v, id))
                {
                    return Ok(Some("launchConfiguration"));
                }
                let bytes = scan.read(&entry)?;
                if plist::launch_reference(&bytes, id, app)? {
                    return Ok(Some("launchConfiguration"));
                }
            }
        }
        for entry in scan.directory(&scope.helpers)? {
            let name = entry
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or("entryUnavailable")?;
            if related_id(name, id) {
                return Ok(Some("externalHelper"));
            }
        }
        Ok(None)
    })();
    match result {
        Ok(Some(reason)) => Finding::detected(reason, scan.entries()),
        Ok(None) => Finding::clear(scan.entries()),
        Err(issue) => Finding::unknown(issue, scan.entries()),
    }
}

// Mole searches default Caskrooms through depth 3 and also falls back to the
// exact app-stem token. We deliberately retain *any* such clue, including stale
// versions, ambiguity and unrelated same-name copies; no brew info prose is
// parsed and no cask hook or Ruby source is executed.
fn management(plan: &Plan, scope: &Scope, scan: &mut scan::Scan) -> Finding {
    let result = (|| {
        let app = Path::new(plan.bundle_binding().0);
        let name = app
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("entryUnavailable")?;
        let stem = name.strip_suffix(".app").ok_or("entryUnavailable")?;
        for room in &scope.rooms {
            for token in scan.directory(room)? {
                let token_name = token
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or("entryUnavailable")?;
                if token_name.eq_ignore_ascii_case(stem) {
                    return Ok(Some("homebrewToken"));
                }
                if !scan.is_directory(&token)? {
                    continue;
                }
                for version in scan.directory(&token)? {
                    if version
                        .file_name()
                        .and_then(|s| s.to_str())
                        .is_some_and(|v| v.eq_ignore_ascii_case(name))
                    {
                        return Ok(Some("homebrewBundle"));
                    }
                    if !scan.is_directory(&version)? {
                        continue;
                    }
                    for candidate in scan.directory(&version)? {
                        let leaf = candidate
                            .file_name()
                            .and_then(|s| s.to_str())
                            .ok_or("entryUnavailable")?;
                        if leaf.eq_ignore_ascii_case(name) {
                            return Ok(Some("homebrewBundle"));
                        }
                    }
                }
            }
        }
        Ok(None)
    })();
    match result {
        Ok(Some(reason)) => Finding::detected(reason, scan.entries()),
        Ok(None) => Finding::clear(scan.entries()),
        Err(issue) => Finding::unknown(issue, scan.entries()),
    }
}

pub(crate) fn observe(plan: &Plan, ctx: &Context) -> Result<Observation, String> {
    observe_with(plan, ctx, &Scope::system(&ctx.home))
}
fn observe_with(plan: &Plan, ctx: &Context, scope: &Scope) -> Result<Observation, String> {
    plan.recheck(ctx, crate::db::now())?;
    let checked = crate::db::now();
    let started = Instant::now();
    let mut bg = scan::Scan::new();
    let mut manager = scan::Scan::new();
    let mut b = background(plan, scope, &mut bg);
    let mut m = management(plan, scope, &mut manager);
    if bg.finish().is_err() && b.code != Code::Detected {
        b = Finding::unknown("changed", bg.entries());
    }
    if manager.finish().is_err() && m.code != Code::Detected {
        m = Finding::unknown("changed", manager.entries());
    }
    bg.append(manager);
    plan.recheck(ctx, crate::db::now())?;
    let now = crate::db::now();
    if now < checked {
        return Err("后台/管理方式检查期间系统时间回退。".into());
    }
    let expires = plan.bundle_binding().3.min(checked.saturating_add(5000));
    let clear = b.code == Code::NotDetected
        && m.code == Code::NotDetected
        && now < expires
        && started.elapsed() < FRESH;
    Ok(Observation {
        report: Report {
            version: 1,
            plan_id: plan.id().into(),
            plan_snapshot_sha256: uninstall_runtime::fingerprint(plan)?,
            checked_at: checked,
            expires_at: expires,
            scope: "ordinaryAppDefaultLaunchAndCaskroom",
            background: b,
            management: m,
            can_prepare_confirmation: clear,
            can_execute: false,
            next: if clear {
                "声明范围内未发现后台配置或 Homebrew 管理线索；包外数据保留。仍需最新运行/权限检查和明确提交，当前不能卸载。"
            } else {
                "后台或管理方式存在限制/未知，本次保留；请使用对应管理工具或专用卸载方式。"
            },
        },
        started,
        home: ctx.home.clone(),
        dependencies: bg,
    })
}

#[cfg(test)]
pub(crate) fn fixture_observation(plan: &Plan, ctx: &Context) -> Observation {
    let now = crate::db::now();
    Observation {
        report: Report {
            version: 1,
            plan_id: plan.id().into(),
            plan_snapshot_sha256: uninstall_runtime::fingerprint(plan).unwrap(),
            checked_at: now,
            expires_at: now + 5000,
            scope: "testOnly",
            background: Finding::clear(0),
            management: Finding::clear(0),
            can_prepare_confirmation: true,
            can_execute: false,
            next: "test-only",
        },
        started: Instant::now(),
        home: ctx.home.clone(),
        dependencies: scan::Scan::new(),
    }
}
#[cfg(test)]
mod tests;
