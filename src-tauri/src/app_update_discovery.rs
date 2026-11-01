//! User-requested vendor feed discovery. Feed claims never grant install rights.
use crate::{
    app_inspection::identity,
    db,
    engine::{Job, TaskKind},
    uninstall_plan::{Context, Source},
    AppState,
};
use serde::Serialize;
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
pub(crate) mod commands;
mod feed;
mod network;
#[cfg(test)]
mod tests;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Probe {
    id: String,
    task_id: String,
    index: usize,
    path: String,
    bundle_id: String,
    current_version: Option<String>,
    current_build: Option<String>,
    feed_url: Option<String>,
    state: &'static str,
    reason: String,
    created_at: i64,
    expires_at: i64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Release {
    id: String,
    version: String,
    build: String,
    url: String,
    kind: &'static str,
    bytes: Option<u64>,
    requires_review: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    probe_id: String,
    request_id: String,
    checked_at: i64,
    expires_at: i64,
    state: &'static str,
    complete: bool,
    inspected: usize,
    skipped: usize,
    releases: Vec<Release>,
}
#[derive(Clone)]
struct Plan {
    probe: Probe,
    source: Source,
    identity: identity::ControlIdentity,
    context: Context,
    issued: Instant,
}
#[derive(Default)]
pub(crate) struct Store {
    plan: Option<Plan>,
    report: Option<Report>,
}
fn check(job: &Job) -> Result<(), String> {
    if job.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
        Err("应用更新来源检查已停止。".into())
    } else {
        Ok(())
    }
}
impl Plan {
    fn prepare(source: Source, ctx: Context, job: &Job) -> Result<Self, String> {
        check(job)?;
        let (path, id, _, kind) = source.control_target();
        if kind != "App"
            || !crate::app_protection::check(
                path,
                Some(id),
                ctx.current_app.as_deref(),
                &ctx.current_id,
                &ctx.protected_paths,
            )?
            .reasons
            .is_empty()
        {
            return Err("请选择未受保护的普通应用；其他管理来源请使用原有更新方式。".into());
        }
        let before = identity::capture_control(Path::new(path), id)?;
        let (feed_url, current_version, current_build) = identity::update_feed(Path::new(path))?;
        if identity::capture_control(Path::new(path), id)? != before {
            return Err("应用身份或更新来源变化，请刷新列表。".into());
        }
        let numeric = current_build
            .as_deref()
            .is_some_and(|v| crate::application_update::version(v).is_ok());
        let url = feed_url
            .as_deref()
            .and_then(|v| network::url(v).ok())
            .map(|v| v.to_string());
        let now = db::now();
        let (task_id, index) = source.address();
        let supported = numeric && url.is_some();
        let probe = Probe {
            id: uuid::Uuid::new_v4().to_string(),
            task_id,
            index,
            path: path.into(),
            bundle_id: id.into(),
            current_version,
            current_build,
            feed_url: url,
            state: if supported { "ready" } else { "unsupported" },
            reason: if !numeric {
                "应用构建号无法按本轮规则比较，请使用应用自身更新器。"
            } else if !supported {
                "未发现本轮支持的 HTTPS 更新地址，请使用应用自身更新器或已下载应用包。"
            } else {
                "更新地址来自此应用的 Info.plist，尚未访问网络或验证发布者。"
            }
            .into(),
            created_at: now,
            expires_at: now + 300000,
        };
        Ok(Self {
            probe,
            source,
            identity: before,
            context: ctx,
            issued: Instant::now(),
        })
    }
    fn validate(&self, ctx: &Context, job: &Job) -> Result<(), String> {
        check(job)?;
        let now = db::now();
        if self.issued.elapsed() >= Duration::from_secs(300)
            || now < self.probe.created_at
            || now >= self.probe.expires_at
            || ctx.protected_paths != self.context.protected_paths
            || ctx.home != self.context.home
            || ctx.current_id != self.context.current_id
            || ctx.current_app != self.context.current_app
            || !crate::app_protection::check(
                &self.probe.path,
                Some(&self.probe.bundle_id),
                ctx.current_app.as_deref(),
                &ctx.current_id,
                &ctx.protected_paths,
            )?
            .reasons
            .is_empty()
            || identity::capture_control(Path::new(&self.probe.path), &self.probe.bundle_id)?
                != self.identity
        {
            return Err("应用更新来源已变化、受保护或过期，请重新读取。".into());
        }
        Ok(())
    }
}
fn system_version() -> Result<String, String> {
    let mut b = [0u8; 64];
    let mut n = b.len();
    if unsafe {
        libc::sysctlbyname(
            c"kern.osproductversion".as_ptr(),
            b.as_mut_ptr().cast(),
            &mut n,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || n == 0
        || n > b.len()
    {
        return Err("当前系统版本无法核对，未判断更新适用范围。".into());
    }
    let s = std::str::from_utf8(&b[..n])
        .map_err(|_| "系统版本无法编码。")?
        .trim_end_matches('\0')
        .to_string();
    crate::application_update::version(&s)?;
    Ok(s)
}
