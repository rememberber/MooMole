//! A sealed, read-only exact-log observer, staged before executor integration.
//! PID-1 diagnostics cannot construct this request or supply its evidence.
//! No IPC, persistence, rule override, confirmation or mutation is provided here.
use super::{apple_literal, monotonic_ms, shell_literal};
use crate::{engine::Job, observation::AnchoredPath, provenance, review, rules};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, path::Path, sync::atomic::Ordering, sync::Arc, time::Duration};

const PROGRAM: &str = include_str!("selected_logs.pl");
const END: &str = "MooMole.logs.end.v1";
const FRESH_MS: u64 = 5_000;
const MAX_PAYLOAD: usize = 24_576;
const OUTPUT_LIMIT: usize = 8192;

#[derive(Serialize)]
struct Target {
    id: String,
    path: String,
    // Decimal strings preserve full device/inode integers through JSON/Perl.
    nodes: Vec<[String; 6]>,
    size: String,
    links: String,
    mtime: String,
    ctime: String,
}

#[derive(Serialize)]
struct Payload {
    nonce: String,
    digest: String,
    uid: u32,
    home: String,
    expires_ms: u64,
    items: Vec<Target>,
}

// Neither request nor evidence is Clone/Deserialize/Serialize. These values
// belong to one backend call, not renderer input, a cache or a persisted grant.
struct Request {
    payload: Payload,
    issued_ms: u64,
    job: Arc<Job>,
}

pub(crate) struct Evidence {
    request: Request,
    report: Report,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Code {
    NoOpenHandles,
    Busy,
    Unknown,
    BudgetExhausted,
    Changed,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    id: String,
    code: Code,
    checked_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    version: u32,
    nonce: String,
    digest: String,
    uid: u32,
    visibility: String,
    items: Vec<Item>,
    complete: String,
}

fn digest(plan: &review::Plan) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(plan).map_err(|_| "无法绑定日志计划。")?)
    ))
}

fn validate_plan(plan: &review::Plan, home: &Path, now: i64) -> Result<(), String> {
    let uid = unsafe { libc::geteuid() };
    if uid == 0
        || plan.protocol != crate::clean_preview::PROTOCOL
        || plan.revision != crate::engine::REVISION
        || now < plan.created_at
        || now >= plan.expires_at
        || plan.items.is_empty()
        || plan.items.len() > 20
        || plan
            .items
            .iter()
            .map(|i| &i.candidate.id)
            .collect::<HashSet<_>>()
            .len()
            != plan.items.len()
        || plan
            .items
            .iter()
            .map(|i| &i.candidate.path)
            .collect::<HashSet<_>>()
            .len()
            != plan.items.len()
    {
        return Err("日志计划已失效、重复或超出单次 20 个文件范围。".into());
    }
    for item in &plan.items {
        let candidate = &item.candidate;
        if candidate.id.is_empty()
            || candidate.id.len() > 128
            || candidate.path.len() > 4096
            || candidate.is_directory
            || rules::module_scope(Path::new(&candidate.path), home) != "userLogs"
            || provenance::binding(candidate.provenance.as_ref(), &candidate.path) != "userLogs"
        {
            return Err("此检查只支持来源明确的本账户普通日志文件。".into());
        }
        let expected = item.witness.as_ref().ok_or("缺少原计划文件身份。")?;
        let held = AnchoredPath::open(Path::new(&candidate.path))?;
        let target = held.witness.nodes.last().ok_or("缺少文件身份。")?;
        if expected.version != crate::observation::VERSION
            || held.witness != *expected
            || expected.links != 1
            || target.uid != uid
            || target.mode & libc::S_IFMT as u32 != libc::S_IFREG as u32
            || !held.local_materialized()
        {
            return Err("日志或祖先已变化，或不是本账户已落地的单链接普通文件。".into());
        }
        held.validate()?;
        // DROP every O_EVTONLY target descriptor BEFORE lsof. Retaining one
        // here could make the observer itself appear to occupy the target.
    }
    Ok(())
}

impl Request {
    fn new(plan: &review::Plan, home: &Path, now: i64, job: Arc<Job>) -> Result<Self, String> {
        if job.cancelled.load(Ordering::SeqCst) {
            return Err("日志检查已取消。".into());
        }
        validate_plan(plan, home, now)?;
        let issued_ms = monotonic_ms().ok_or("无法读取系统单调时钟。")?;
        let items = plan
            .items
            .iter()
            .map(|item| {
                let w = item.witness.as_ref().unwrap();
                Ok(Target {
                    id: item.candidate.id.clone(),
                    path: item.candidate.path.clone(),
                    nodes: w
                        .nodes
                        .iter()
                        .map(|n| {
                            Ok([
                                n.path.to_str().ok_or("祖先路径无法编码。")?.to_string(),
                                n.dev.to_string(),
                                n.ino.to_string(),
                                n.mode.to_string(),
                                n.uid.to_string(),
                                n.gid.to_string(),
                            ])
                        })
                        .collect::<Result<_, String>>()?,
                    size: w.size.to_string(),
                    links: w.links.to_string(),
                    mtime: w.mtime.to_string(),
                    ctime: w.ctime.to_string(),
                })
            })
            .collect::<Result<_, String>>()?;
        let request = Self {
            payload: Payload {
                nonce: uuid::Uuid::new_v4().to_string(),
                digest: digest(plan)?,
                uid: unsafe { libc::geteuid() },
                home: home.to_str().ok_or("用户目录无法编码。")?.into(),
                expires_ms: issued_ms
                    .saturating_add(plan.expires_at.saturating_sub(now).min(120_000) as u64),
                items,
            },
            issued_ms,
            job,
        };
        // Fail before any system prompt, not after passing an oversized arg.
        request.source()?;
        Ok(request)
    }

    fn command(&self) -> Result<String, String> {
        let json = serde_json::to_vec(&self.payload).map_err(|_| "无法编码日志检查请求。")?;
        if json.len() > MAX_PAYLOAD {
            return Err("日志身份请求超过单次大小限制。".into());
        }
        let hex = json.iter().map(|b| format!("{b:02x}")).collect::<String>();
        Ok(format!("cd / && /usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin LC_ALL=C /usr/bin/perl -e {} {}", shell_literal(PROGRAM), shell_literal(&hex)))
    }

    fn source(&self) -> Result<String, String> {
        let source = format!("try\nset observed to do shell script {} with administrator privileges without altering line endings\nset reportText to \"ok\" & linefeed & observed\non error messageText number errorNumber\nset reportText to \"error\" & linefeed & (errorNumber as text) & linefeed & messageText\nend try\nreturn reportText & linefeed & \"{END}\"", apple_literal(&self.command()?));
        if source.len() > 96 * 1024 {
            return Err("日志检查程序超过系统参数限制。".into());
        }
        Ok(source)
    }

    fn finish(
        self,
        plan: &review::Plan,
        home: &Path,
        bytes: &[u8],
        now: i64,
    ) -> Result<Evidence, String> {
        let invalid = || "未取得与当次日志及计划匹配的完整检查结果，保留文件。".to_string();
        if bytes.len() > OUTPUT_LIMIT {
            return Err(invalid());
        }
        let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
        let body = text
            .strip_prefix("ok\n")
            .and_then(|s| s.strip_suffix(&format!("\n{END}\n")))
            .ok_or_else(invalid)?;
        let report: Report = serde_json::from_str(body).map_err(|_| invalid())?;
        let evidence = Evidence {
            request: self,
            report,
        };
        evidence.validate(plan, home, now)?;
        Ok(evidence)
    }
}

impl Evidence {
    fn validate(&self, plan: &review::Plan, home: &Path, now: i64) -> Result<(), String> {
        let p = &self.request.payload;
        let r = &self.report;
        if self.request.job.cancelled.load(Ordering::SeqCst) {
            return Err("日志检查已取消。".into());
        }
        let mono = monotonic_ms().ok_or("无法复核检查时间。")?;
        if digest(plan)? != p.digest
            || home.to_str() != Some(&p.home)
            || unsafe { libc::geteuid() } != p.uid
            || mono < self.request.issued_ms
            || mono >= p.expires_ms
            || r.version != 1
            || r.uid != 0
            || r.nonce != p.nonce
            || r.digest != p.digest
            || r.complete != "MooMole.logs.complete.v1"
            || !matches!(r.visibility.as_str(), "observed" | "unknown")
            || r.items.len() != p.items.len()
            || r.items.iter().zip(&p.items).any(|(a, b)| {
                a.id != b.id
                    || a.checked_ms < self.request.issued_ms
                    || a.checked_ms > mono
                    || mono.saturating_sub(a.checked_ms) >= FRESH_MS
                    || (r.visibility != "observed" && a.code == Code::NoOpenHandles)
            })
        {
            return Err("日志检查已过期、不完整或不属于当前计划；保留文件。".into());
        }
        // Includes nanosecond content metadata, ownership, every ancestor and
        // local/materialized status; root Perl's second-granularity stat alone
        // is never sufficient. Changed objects withdraw the whole observation.
        validate_plan(plan, home, now)?;
        // File-system observation itself can take time. Check freshness again
        // AFTER it, rather than accepting a timestamp sampled before it began.
        let after = monotonic_ms().ok_or("无法复核检查时间。")?;
        if after < mono
            || after >= p.expires_ms
            || r.items
                .iter()
                .any(|i| after.saturating_sub(i.checked_ms) >= FRESH_MS)
        {
            return Err("核对文件期间检查已过期，保留文件。".into());
        }
        if self.request.job.cancelled.load(Ordering::SeqCst) {
            return Err("日志检查已取消。".into());
        }
        Ok(())
    }

    pub(crate) fn consume(
        self,
        plan: &review::Plan,
        home: &Path,
        now: i64,
    ) -> Result<Vec<(String, Code)>, String> {
        self.validate(plan, home, now)?;
        Ok(self
            .report
            .items
            .into_iter()
            .map(|i| (i.id, i.code))
            .collect())
    }
}

/// Not exposed to IPC yet: integration must hold the task/update lease, obtain
/// explicit user intent, and preserve fresh Mole rules and final user-mode Trash.
pub(crate) async fn observe(
    plan: &review::Plan,
    home: &Path,
    job: Arc<Job>,
) -> Result<Evidence, String> {
    if job.cancelled.load(Ordering::SeqCst) {
        return Err("日志检查已取消。".into());
    }
    let request = Request::new(plan, home, crate::db::now(), job.clone())?;
    let mut command = tokio::process::Command::new("/bin/sh");
    command
        .args([
            "-c",
            &format!(
                "exec /usr/bin/osascript -e {} 2>&1",
                shell_literal(&request.source()?)
            ),
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("LC_ALL", "C")
        .current_dir("/");
    let result = crate::process::output_bounded(
        command,
        job.clone(),
        Duration::from_secs(120),
        OUTPUT_LIMIT as u64,
    )
    .await;
    if job.cancelled.load(Ordering::SeqCst) || result.is_err() {
        // Cancelling our interpreter does not establish that a system-owned
        // dialog/worker has closed. Keep the normal read-only budget reserved.
        tokio::time::sleep(Duration::from_secs(6)).await;
        return Err("检查已取消或通道未完成，保留文件；仍显示的系统窗口需本人取消。".into());
    }
    request.finish(plan, home, &result.unwrap(), crate::db::now())
}

#[cfg(test)]
mod tests;
