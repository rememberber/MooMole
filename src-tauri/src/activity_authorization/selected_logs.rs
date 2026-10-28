//! A sealed read-only cache-tree/exact-log observer used by confirmed cleanup.
//! PID-1 diagnostics cannot construct this request or supply its evidence.
//! No direct IPC, persistence, confirmation or mutation is provided here.
use super::{apple_literal, monotonic_ms, shell_literal};
use crate::{engine::Job, observation::AnchoredPath, provenance, review, rules};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    sync::Arc,
    time::Duration,
};

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

// Each result retains its ORIGINAL native time and target. Final Trash calls
// must revalidate it; conversion to a RuleCheck alone is not lasting authority.
pub(crate) struct Guard {
    source_digest: String,
    one: review::Plan,
    home: PathBuf,
    checked_ms: u64,
    expires_ms: u64,
    job: Arc<Job>,
    code: Code,
    visible: bool,
    diagnostic: Option<VisibilityDiagnostic>,
    query: Option<QueryDiagnostic>,
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
    query: Option<QueryDiagnostic>,
}

// Bounded protocol metadata only: no lsof output, process names, descriptors,
// passwords or file contents leave the privileged observer.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryDiagnostic {
    status: u16,
    bytes: u16,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct VisibilityDiagnostic {
    status: u16,
    bytes: u16,
    root_pair: bool,
    fields_known: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    version: u32,
    nonce: String,
    digest: String,
    uid: u32,
    visibility: String,
    diagnostic: Option<VisibilityDiagnostic>,
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
            || !crate::cleanup_preparation::supported(item, home)
        {
            return Err("此检查只支持来源明确的本账户缓存和普通日志文件。".into());
        }
        let expected = item.witness.as_ref().ok_or("缺少原计划文件身份。")?;
        let held = AnchoredPath::open(Path::new(&candidate.path))?;
        let target = held.witness.nodes.last().ok_or("缺少文件身份。")?;
        if expected.version != crate::observation::VERSION
            || held.witness != *expected
            || (!candidate.is_directory && expected.links != 1)
            || target.uid != uid
            || target.mode & libc::S_IFMT as u32
                != if candidate.is_directory {
                    libc::S_IFDIR as u32
                } else {
                    libc::S_IFREG as u32
                }
            || !held.local_materialized()
        {
            return Err("日志或祖先已变化，或不是本账户已落地的单链接普通文件。".into());
        }
        if candidate.is_directory {
            let tree = crate::tree::inspect(
                Path::new(&candidate.path),
                target.dev,
                target.ino,
                &mut crate::tree::Budget::new(),
            )?;
            if item.tree.as_ref() != Some(&tree) {
                return Err("缓存目录内容已变化。".into());
            }
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
        let framed = text
            .strip_suffix(&format!("\n{END}\n"))
            .ok_or_else(invalid)?;
        if framed.starts_with("error\n-128\n") {
            // Standard Additions' explicit user-cancel result. This withdraws
            // the same execution Job; it never establishes authorization.
            self.job.cancel();
            return Err("系统授权已取消，未取得当次占用观察。".into());
        }
        let body = framed.strip_prefix("ok\n").ok_or_else(invalid)?;
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
            || r.diagnostic.as_ref().is_some_and(|d| {
                d.status > 255
                    || d.bytes > 4096
                    || (d.fields_known && !d.root_pair)
                    || (r.visibility == "observed") != (d.status == 0 && d.fields_known)
            })
            || r.items.len() != p.items.len()
            || r.items.iter().zip(&p.items).any(|(a, b)| {
                a.id != b.id
                    || a.query
                        .as_ref()
                        .is_some_and(|d| d.status > 255 || d.bytes > 4096)
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

    #[cfg(test)]
    fn consume(
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

    pub(crate) fn into_guards(
        self,
        plan: &review::Plan,
        home: &Path,
        now: i64,
    ) -> Result<HashMap<String, Guard>, String> {
        self.validate(plan, home, now)?;
        let p = self.request.payload;
        let visible = self.report.visibility == "observed";
        let diagnostic = self.report.diagnostic;
        Ok(self
            .report
            .items
            .into_iter()
            .zip(&plan.items)
            .map(|(result, item)| {
                (
                    result.id,
                    Guard {
                        source_digest: p.digest.clone(),
                        one: review::Plan {
                            items: vec![item.clone()],
                            ..plan.clone()
                        },
                        home: home.to_path_buf(),
                        checked_ms: result.checked_ms,
                        expires_ms: p.expires_ms,
                        job: self.request.job.clone(),
                        code: result.code,
                        visible,
                        diagnostic: diagnostic.clone(),
                        query: result.query,
                    },
                )
            })
            .collect())
    }
}

impl Guard {
    fn current_time(&self) -> Result<(), String> {
        let now = monotonic_ms().ok_or("无法复核占用检查时间，保留文件。")?;
        if self.job.cancelled.load(Ordering::SeqCst)
            || now < self.checked_ms
            || now >= self.expires_ms
            || now.saturating_sub(self.checked_ms) >= FRESH_MS
        {
            return Err("当次占用观察已取消或过期，保留文件；请重新扫描并缩小选择。".into());
        }
        Ok(())
    }

    fn identity(&self) -> Result<(), String> {
        self.current_time()?;
        validate_plan(&self.one, &self.home, crate::db::now())?;
        self.current_time()
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.code != Code::NoOpenHandles {
            return Err(self.unconfirmed_reason());
        }
        self.identity()
    }

    pub(crate) fn apply(
        &self,
        plan: &review::Plan,
        checks: &mut HashMap<String, rules::RuleCheck>,
    ) -> Result<(), String> {
        if digest(plan)? != self.source_digest {
            return Err("占用检查不属于本次固定选择。".into());
        }
        self.identity()?;
        let rule = checks
            .get_mut(&self.one.items[0].candidate.id)
            .ok_or("缺少当次 Mole 规则，保留文件。")?;
        if rule.code != "passed"
            || rule.module.version != rules::MODULE_VERSION
            || rule.module.binding
                != provenance::binding(
                    self.one.items[0].candidate.provenance.as_ref(),
                    &self.one.items[0].candidate.path,
                )
            || rule.module.scope != rule.module.binding
            || rule.module.code != "checked"
            || rule.activity.version != rules::ACTIVITY_VERSION
        {
            return Err("当次 Mole 路径或日志模块条件未通过，保留文件。".into());
        }
        // Never erase a positive busy result or a failed/incomplete rule run.
        if !matches!(rule.activity.code.as_str(), "noOpenHandles" | "unknown") {
            return Ok(());
        }
        let code = match self.code {
            Code::NoOpenHandles => "noOpenHandles",
            Code::Busy => "busy",
            Code::BudgetExhausted => "budgetExhausted",
            Code::Changed => "checkFailed",
            Code::Unknown => "unknown",
        };
        let mut activity = rules::ActivityCheck::from_code(code)?;
        activity.source = "客户端当次授权观察 · Mole 占用检查语义".into();
        activity.reason = if self.code == Code::NoOpenHandles {
            "本次完整视图证明及精确文件查询未发现打开句柄；只适用于当前对象与短期观察，执行前仍会核对，不保证持续空闲。".into()
        } else {
            self.unconfirmed_reason()
        };
        rule.activity = activity;
        if self.code == Code::NoOpenHandles {
            Ok(())
        } else {
            Err(self.unconfirmed_reason())
        }
    }

    fn unconfirmed_reason(&self) -> String {
        let reason = match self.code {
            Code::Unknown if !self.visible => "当次授权检查仍未能证明完整进程视图，文件保留。",
            Code::Unknown => "进程视图已核对，但精确文件查询未能确认空闲，文件保留。",
            Code::Busy => "当次授权检查发现文件被占用，文件保留；请自行退出相关应用后重新扫描。",
            Code::BudgetExhausted => {
                "当次占用检查的 5 秒预算已用完，文件保留；请缩小选择后重新扫描。"
            }
            Code::Changed => "授权检查期间文件或祖先身份发生变化，文件保留；请重新扫描。",
            Code::NoOpenHandles => "仍需核对当次规则、身份和有效期，不能仅据占用观察处理。",
        };
        let mut detail = String::from(reason);
        if let Some(d) = &self.diagnostic {
            detail.push_str(&format!(
                " 诊断：视图状态 {}，{} 字节，root 字段 {}，字段格式 {}。",
                d.status, d.bytes, d.root_pair, d.fields_known
            ));
        }
        if let Some(d) = &self.query {
            detail.push_str(&format!(" 文件查询状态 {}，{} 字节。", d.status, d.bytes));
        }
        detail
    }
}

/// Only the existing confirmed worker calls this, owning its task/update lease.
#[cfg_attr(test, allow(dead_code))] // Unit regressions never request OS authorization.
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

#[cfg(test)]
pub(crate) fn fixture_evidence(
    plan: &review::Plan,
    home: &Path,
    job: Arc<Job>,
) -> Result<Evidence, String> {
    // Nonprivileged protocol fixture only. No production flag/IPC accepts this.
    let request = Request::new(plan, home, crate::db::now(), job)?;
    let items = request.payload.items.iter().map(|i| serde_json::json!({"id":i.id,"code":"noOpenHandles","checked_ms":monotonic_ms().unwrap()})).collect::<Vec<_>>();
    let body = serde_json::json!({"version":1,"nonce":request.payload.nonce,"digest":request.payload.digest,"uid":0,"visibility":"observed","items":items,"complete":"MooMole.logs.complete.v1"});
    request.finish(
        plan,
        home,
        format!("ok\n{body}\n{END}\n").as_bytes(),
        crate::db::now(),
    )
}
