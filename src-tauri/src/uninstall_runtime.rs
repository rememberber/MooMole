//! Read-only executable-path observation for a fixed App plan, not file-handle,
//! launch-service/management eligibility, user consent or execution permission.
use crate::uninstall_plan::{Context, Plan};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::OsStr,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MAX_PIDS: usize = 8192;
const MAX_MATCHES: usize = 32;
const BUDGET: Duration = Duration::from_secs(1);
const FRESH: Duration = Duration::from_secs(5);
mod entrypoint;

#[link(name = "proc")]
extern "C" {
    fn proc_listallpids(buffer: *mut libc::c_void, size: libc::c_int) -> libc::c_int;
    fn proc_pidpath(pid: libc::c_int, buffer: *mut libc::c_void, size: u32) -> libc::c_int;
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Code {
    NotObserved,
    Observed,
    Unknown,
    Unsupported,
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
    pub code: Code,
    pids: Vec<i32>,
    coverage: &'static str,
    issue: Option<&'static str>,
    can_prepare_confirmation: bool,
    can_execute: bool,
    next: &'static str,
}
pub(crate) struct Observation {
    report: Report,
    started: Instant,
}
impl Observation {
    pub(crate) fn report(&self) -> &Report {
        &self.report
    }
    pub(crate) fn validate_binding(&self, plan: &Plan) -> Result<(), String> {
        if self.report.plan_id != plan.id()
            || self.report.plan_snapshot_sha256 != fingerprint(plan)?
        {
            return Err("运行检查不属于当前卸载计划。".into());
        }
        Ok(())
    }
    pub(crate) fn permits_confirmation(&self, plan: &Plan, now: i64) -> Result<(), String> {
        self.validate_binding(plan)?;
        if self.report.code != Code::NotObserved
            || !self.report.can_prepare_confirmation
            || self.report.plan_id != plan.id()
            || self.report.plan_snapshot_sha256 != fingerprint(plan)?
            || now < self.report.checked_at
            || now >= self.report.expires_at
            || self.started.elapsed() >= FRESH
        {
            return Err("运行检查未知、已发现进程、失效或不属于此计划，请重新检查。".into());
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn fixture_observation(plan: &Plan, code: Code) -> Observation {
    let now = crate::db::now();
    Observation {
        report: Report {
            version: 1,
            plan_id: plan.id().into(),
            plan_snapshot_sha256: fingerprint(plan).unwrap(),
            checked_at: now,
            expires_at: plan.bundle_binding().3.min(now + 5000),
            scope: "bundleExecutablePaths",
            code,
            pids: vec![],
            coverage: "completePass",
            issue: None,
            can_prepare_confirmation: code == Code::NotObserved,
            can_execute: false,
            next: "test-only observation, not native execution evidence",
        },
        started: Instant::now(),
    }
}
pub(crate) fn fingerprint(plan: &Plan) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&plan.archive()).map_err(|_| "无法绑定卸载运行检查。")?)
    ))
}

trait Processes {
    fn pids(&mut self) -> Result<BTreeSet<i32>, &'static str>;
    fn executable(&mut self, pid: i32) -> Result<PathBuf, &'static str>;
}
struct Native;
impl Processes for Native {
    fn pids(&mut self) -> Result<BTreeSet<i32>, &'static str> {
        let mut pids = vec![0i32; MAX_PIDS];
        let count = unsafe {
            proc_listallpids(
                pids.as_mut_ptr().cast(),
                (pids.len() * std::mem::size_of::<i32>()) as i32,
            )
        };
        if count <= 0 || count as usize >= MAX_PIDS {
            return Err("processListUnavailable");
        }
        let mut set = BTreeSet::new();
        for &pid in &pids[..count as usize] {
            if pid < 0 || (pid > 0 && !set.insert(pid)) {
                return Err("processListInvalid");
            }
            // PID 0 is kernel_task, not a userspace App executable.
        }
        if !set.contains(&1) || !set.contains(&(std::process::id() as i32)) {
            return Err("processListIncomplete");
        }
        Ok(set)
    }
    fn executable(&mut self, pid: i32) -> Result<PathBuf, &'static str> {
        let mut raw = [0u8; 4096];
        let count = unsafe { proc_pidpath(pid, raw.as_mut_ptr().cast(), raw.len() as u32) };
        decode_path(&raw, count)
    }
}
fn decode_path(raw: &[u8], count: i32) -> Result<PathBuf, &'static str> {
    if count <= 0 || count as usize >= raw.len() {
        return Err("executablePathUnavailable");
    }
    let end = raw
        .iter()
        .position(|b| *b == 0)
        .ok_or("executablePathInvalid")?;
    if end == 0 || (count as usize != end && count as usize != end + 1) {
        return Err("executablePathInvalid");
    }
    let path = PathBuf::from(OsStr::from_bytes(&raw[..end]));
    if !path.is_absolute()
        || raw[..end]
            .split(|b| *b == b'/')
            .any(|s| s == b"." || s == b"..")
    {
        return Err("executablePathInvalid");
    }
    Ok(path)
}

struct Finding {
    code: Code,
    pids: Vec<i32>,
    issue: Option<&'static str>,
}
fn contains_executable(
    executable: &Path,
    bundle: &Path,
    root: Option<(u64, u64)>,
) -> Result<bool, &'static str> {
    if executable.starts_with(bundle) {
        return Ok(true);
    }
    // A system Data-volume/firmlink spelling may differ while referring to the
    // same physical App. Compare only same-basename candidate roots, never a
    // shared executable name or a substring, and never grant source authority.
    if let Some(candidate) = executable
        .ancestors()
        .skip(1)
        .find(|p| p.file_name() == bundle.file_name())
    {
        let expected = root.ok_or("bundleAliasUnknown")?;
        let meta = std::fs::symlink_metadata(candidate).map_err(|_| "bundleAliasUnknown")?;
        if !meta.is_dir() {
            return Err("bundleAliasUnknown");
        }
        return Ok((meta.dev(), meta.ino()) == expected);
    }
    Ok(false)
}
#[cfg(test)]
fn sample(source: &mut impl Processes, path: &Path, started: Instant) -> Finding {
    sample_bound(source, path, started, None)
}
fn sample_bound(
    source: &mut impl Processes,
    path: &Path,
    started: Instant,
    root: Option<(u64, u64)>,
) -> Finding {
    let mut found = BTreeSet::new();
    let mut failure = None;
    // Two complete, equal PID inventories surround each path pass. A bounded
    // second attempt tolerates ordinary process churn without treating gaps as
    // absence; it never erases a positive match observed in either attempt.
    for _ in 0..2 {
        if started.elapsed() >= BUDGET {
            failure = Some("timeBudget");
            break;
        }
        let before = match source.pids() {
            Ok(pids) => pids,
            Err(e) => {
                failure = Some(e);
                break;
            }
        };
        if before.len() >= MAX_PIDS {
            failure = Some("processLimit");
            break;
        }
        let mut pass_error = None;
        for &pid in &before {
            if started.elapsed() >= BUDGET {
                pass_error = Some("timeBudget");
                break;
            }
            match source
                .executable(pid)
                .and_then(|executable| contains_executable(&executable, path, root))
            {
                Ok(true) => {
                    found.insert(pid);
                }
                Ok(false) => {}
                Err(e) => pass_error = Some(e),
            }
        }
        let after = match source.pids() {
            Ok(pids) => pids,
            Err(e) => {
                failure = Some(e);
                break;
            }
        };
        if pass_error.is_none() && before == after && started.elapsed() < BUDGET {
            return Finding {
                code: if found.is_empty() {
                    Code::NotObserved
                } else {
                    Code::Observed
                },
                pids: found.into_iter().take(MAX_MATCHES).collect(),
                issue: None,
            };
        }
        failure = pass_error.or(Some("processListChanged"));
    }
    Finding {
        code: if found.is_empty() {
            Code::Unknown
        } else {
            Code::Observed
        },
        pids: found.into_iter().take(MAX_MATCHES).collect(),
        issue: failure,
    }
}

// Scripts can run under an interpreter outside the bundle, so an empty native
// executable-path view is NOT a usable prerequisite for those entrypoints.
fn macho_entrypoint(plan: &Plan) -> Result<bool, String> {
    let (raw, snapshot, _, _) = plan.bundle_binding();
    let path = Path::new(raw)
        .join("Contents/MacOS")
        .join(&snapshot.executable_name);
    let held = crate::observation::AnchoredPath::open(&path)?;
    if held.witness != snapshot.executable {
        return Err("App 主程序已变化。".into());
    }
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW_ANY | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(&path)
        .map_err(|_| "无法读取主程序格式，本次保留。")?;
    let before = file.metadata().map_err(|_| "无法核对主程序。")?;
    let read = entrypoint::native_executable(&mut file, before.size());
    let after = file.metadata().map_err(|_| "无法复核主程序。")?;
    let node = snapshot.executable.nodes.last().ok_or("缺少主程序身份。")?;
    let fields = |m: &std::fs::Metadata| {
        (
            m.dev(),
            m.ino(),
            m.mode(),
            m.uid(),
            m.gid(),
            m.size(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
            m.nlink(),
        )
    };
    held.validate()?;
    if fields(&before) != fields(&after)
        || before.dev() != node.dev
        || before.ino() != node.ino
        || before.mode() != node.mode
        || before.size() != snapshot.executable.size
        || before.mtime() != snapshot.executable.mtime
        || before.mtime_nsec() != snapshot.executable.mtime_ns
        || before.ctime() != snapshot.executable.ctime
        || before.ctime_nsec() != snapshot.executable.ctime_ns
    {
        return Err("读取格式期间 App 主程序变化，本次检查撤回。".into());
    }
    match read {
        Ok(native) => Ok(native),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(_) => Err("主程序格式读取未完成，本次检查撤回。".into()),
    }
}

pub(crate) fn observe(plan: &Plan, ctx: &Context) -> Result<Observation, String> {
    plan.recheck(ctx, crate::db::now())?;
    let checked = crate::db::now();
    let started = Instant::now();
    let finding = if macho_entrypoint(plan)? {
        let root = plan
            .bundle_binding()
            .1
            .root
            .nodes
            .last()
            .ok_or("缺少 App 根身份。")?;
        sample_bound(
            &mut Native,
            Path::new(plan.bundle_binding().0),
            started,
            Some((root.dev, root.ino)),
        )
    } else {
        Finding {
            code: Code::Unsupported,
            pids: vec![],
            issue: Some("entrypointUnsupported"),
        }
    };
    // Changes while collecting system metadata revoke the entire observation.
    plan.recheck(ctx, crate::db::now())?;
    let now = crate::db::now();
    if now < checked {
        return Err("运行检查期间系统时间回退，请重新检查。".into());
    }
    let expires = plan.bundle_binding().3.min(checked.saturating_add(5000));
    Ok(Observation {
        report: Report {
            version: 1,
            plan_id: plan.id().into(),
            plan_snapshot_sha256: fingerprint(plan)?,
            checked_at: checked,
            expires_at: expires,
            scope: "bundleExecutablePaths",
            code: finding.code,
            pids: finding.pids,
            coverage: if finding.issue.is_none() {
                "completePass"
            } else {
                "incomplete"
            },
            issue: finding.issue,
            can_prepare_confirmation: finding.code == Code::NotObserved
                && now < expires
                && started.elapsed() < FRESH,
            can_execute: false,
            next: match finding.code {
                Code::Observed => {
                    "发现此 App 包内的运行程序，请自行退出后重新检查；不会自动结束进程。"
                }
                Code::NotObserved => {
                    "本次未发现包内可执行进程；这不代表后台服务和管理方式已确认，当前不能卸载。"
                }
                Code::Unknown => "进程视图不完整或变化，本次保留；请稍后重新检查。",
                Code::Unsupported => {
                    "主程序不是支持的原生可执行格式，脚本或其他入口暂不提供卸载确认。"
                }
            },
        },
        started,
    })
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::Fixture;
