//! Fixed ordinary-user Quick Look operations from Mole's cache_refresh handler.
//! This is a native building block, NOT selection/confirmation or IPC authority.
//! The caller must commit durable intent and recheck policy before each spawn.
#![allow(dead_code)]
use crate::observation::AnchoredPath;
use serde::{Deserialize, Serialize};
use std::{
    mem::MaybeUninit,
    os::{
        fd::AsRawFd,
        unix::{fs::MetadataExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const TOOL: &str = "/usr/bin/qlmanage";
const TIMEOUT: Duration = Duration::from_secs(15);
const GRACE: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(20);

fn readonly_system_tool(tool: &AnchoredPath) -> bool {
    let mut fs = MaybeUninit::<libc::statfs>::zeroed();
    // Root ownership/mode alone would not exclude an extra writable ACL.
    // macOS 14+ supplies this tool on the read-only system volume. Refuse a
    // writable/remounted or unsupported replacement rather than assume SIP.
    unsafe {
        libc::fstatfs(tool.target().as_raw_fd(), fs.as_mut_ptr()) == 0
            && fs.assume_init().f_flags & libc::MNT_RDONLY as u32 != 0
    }
}

// No domain, path, executable, shell or user-supplied argv can enter this API.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Step {
    ThumbnailCache,
    Generators,
}
impl Step {
    pub(crate) const ALL: [Self; 2] = [Self::ThumbnailCache, Self::Generators];
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::ThumbnailCache => "thumbnailCache",
            Self::Generators => "generators",
        }
    }
    fn args(self) -> &'static [&'static str] {
        match self {
            Self::ThumbnailCache => &["-r", "cache"],
            Self::Generators => &["-r"],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Outcome {
    NotStarted,
    CommandSucceeded,
    Uncertain,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Stop {
    Cancelled,
    TimedOut,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Attempt {
    pub step: Step,
    pub outcome: Outcome,
    pub exit_code: Option<i32>,
    pub stop: Option<Stop>,
    pub reason: &'static str,
}
impl Attempt {
    fn not_started(step: Step, reason: &'static str, stop: Option<Stop>) -> Self {
        Self {
            step,
            outcome: Outcome::NotStarted,
            exit_code: None,
            stop,
            reason,
        }
    }
}

pub(crate) struct Prepared {
    executable: PathBuf,
    tool: AnchoredPath,
    home: PathBuf,
    home_handle: AnchoredPath,
    uid: u32,
}
impl Prepared {
    pub(super) fn binding(&self) -> Result<String, String> {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(&(
            &self.executable,
            &self.tool.witness,
            &self.home_handle.witness,
            self.uid,
        ))
        .map_err(|_| "无法固定系统工具身份。")?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
    pub(crate) fn prepare(home: &Path) -> Result<Self, String> {
        let uid = unsafe { libc::geteuid() };
        if uid == 0 || uid != unsafe { libc::getuid() } {
            return Err("Quick Look 刷新仅支持当前普通账户。".into());
        }
        // macOS ships /usr/bin/qlmanage as a symlink into QuickLook.framework.
        // Resolve only that fixed system name, then hold/recheck the physical
        // root-owned path. Do not reject Apple's symlink or trust a PATH lookup.
        let executable =
            std::fs::canonicalize(TOOL).map_err(|_| "系统 Quick Look 工具不可用。".to_string())?;
        if executable != Path::new(TOOL)
            && !(executable.starts_with("/System/Library/Frameworks/QuickLook.framework")
                && executable.file_name().is_some_and(|n| n == "qlmanage"))
        {
            return Err("系统 Quick Look 工具位置无法核对。".into());
        }
        let tool = AnchoredPath::open(&executable)?;
        let m = tool.target().metadata().map_err(|e| e.to_string())?;
        if !m.is_file()
            || m.mode() & 0o111 == 0
            || m.mode() & 0o6000 != 0
            || !tool.local_materialized()
            || !readonly_system_tool(&tool)
            || tool
                .witness
                .nodes
                .iter()
                .any(|n| n.uid != 0 || n.mode & 0o022 != 0)
        {
            return Err("系统 Quick Look 工具身份或访问条件不支持。".into());
        }
        let home_handle = AnchoredPath::open(home)?;
        let m = home_handle.target().metadata().map_err(|e| e.to_string())?;
        if !m.is_dir() || m.uid() != uid || !home_handle.local_materialized() {
            return Err("当前账户目录无法核对。".into());
        }
        let prepared = Self {
            executable,
            tool,
            home: home.to_owned(),
            home_handle,
            uid,
        };
        prepared.validate()?;
        Ok(prepared)
    }
    fn validate(&self) -> Result<(), String> {
        if self.uid != unsafe { libc::geteuid() }
            || self.uid != unsafe { libc::getuid() }
            || std::fs::canonicalize(TOOL).ok().as_ref() != Some(&self.executable)
            || !readonly_system_tool(&self.tool)
        {
            return Err("账户或 Quick Look 工具已变化。".into());
        }
        self.tool.validate()?;
        self.home_handle.validate()
    }
    // gate is supplied ONLY by the sealed executor. It must check the original
    // confirmation, latest rules/protection, exclusion lease and FULL intent.
    // No caller may retry an Uncertain attempt automatically.
    pub(crate) fn run(
        &self,
        step: Step,
        cancelled: &AtomicBool,
        gate: impl FnOnce() -> Result<(), String>,
    ) -> Attempt {
        if cancelled.load(Ordering::SeqCst) {
            return Attempt::not_started(step, "cancelledBeforeStart", Some(Stop::Cancelled));
        }
        if self.validate().is_err() {
            return Attempt::not_started(step, "identityChanged", None);
        }
        if gate().is_err() {
            return Attempt::not_started(step, "gateRefused", None);
        }
        if self.validate().is_err() {
            return Attempt::not_started(step, "identityChanged", None);
        }
        let mut command = Command::new(&self.executable);
        command
            .args(step.args())
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("LANG", "C")
            .current_dir(&self.home);
        supervise(step, command, cancelled, TIMEOUT, GRACE, |_| {})
    }
}

struct OwnedChild {
    child: Child,
    pid: u32,
    owned: bool,
}
impl OwnedChild {
    // WNOWAIT pins our unreaped PID/PGID through the last signal. try_wait()
    // would reap early and permit signalling a reused process-group number.
    fn exited(&self) -> std::io::Result<bool> {
        let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
        let rc = unsafe {
            libc::waitid(
                libc::P_PID,
                self.pid,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if rc == 0 {
            Ok(unsafe { info.assume_init() }.si_pid == self.pid as i32)
        } else {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                Ok(false)
            } else {
                Err(error)
            }
        }
    }
    fn signal(&self, signal: i32) {
        if self.owned {
            unsafe { libc::kill(-(self.pid as i32), signal) };
        }
    }
    fn collect(&mut self) -> std::io::Result<ExitStatus> {
        self.signal(libc::SIGKILL);
        // Deliberately keep the blocking worker/its operation lease until the
        // child is reaped. A kernel-stalled reap has no promised hard deadline.
        let status = self.child.wait();
        self.owned = false;
        status
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.owned {
            // Unwinding must not leave this operation running after its caller
            // releases the lease. First verify we still own the unreaped PID.
            if self.exited().is_ok() {
                let _ = self.collect();
            }
        }
    }
}

fn supervise(
    step: Step,
    mut command: Command,
    cancelled: &AtomicBool,
    timeout: Duration,
    grace: Duration,
    after_spawn: impl FnOnce(u32),
) -> Attempt {
    if cancelled.load(Ordering::SeqCst) {
        return Attempt::not_started(step, "cancelledBeforeStart", Some(Stop::Cancelled));
    }
    // No terminal prose is collected or interpreted. Discarded output cannot
    // exhaust memory or keep an inherited pipe open after the leader exits.
    command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return Attempt::not_started(step, "spawnFailed", None),
    };
    let mut owned = OwnedChild {
        pid: child.id(),
        child,
        owned: true,
    };
    after_spawn(owned.pid);
    let started = Instant::now();
    let mut stop = None;
    let mut stopped_at = None;
    loop {
        match owned.exited() {
            Ok(true) => break,
            Ok(false) => (),
            Err(_) => {
                // Ownership was lost; never signal this PID/PGID again.
                owned.owned = false;
                return Attempt {
                    step,
                    outcome: Outcome::Uncertain,
                    exit_code: None,
                    stop,
                    reason: "childOwnershipUnknown",
                };
            }
        }
        if stop.is_none() {
            stop = if cancelled.load(Ordering::SeqCst) {
                Some(Stop::Cancelled)
            } else if started.elapsed() >= timeout {
                Some(Stop::TimedOut)
            } else {
                None
            };
            if stop.is_some() {
                owned.signal(libc::SIGTERM);
                stopped_at = Some(Instant::now());
            }
        }
        if stopped_at.is_some_and(|at| at.elapsed() >= grace) {
            break;
        }
        std::thread::sleep(POLL);
    }
    let status = owned.collect();
    let exit_code = status.as_ref().ok().and_then(|s| s.code());
    let succeeded = stop.is_none() && status.as_ref().is_ok_and(|s| s.success());
    Attempt {
        step,
        outcome: if succeeded {
            Outcome::CommandSucceeded
        } else {
            Outcome::Uncertain
        },
        exit_code,
        stop,
        reason: if succeeded {
            "commandSucceeded"
        } else if status.is_err() {
            "collectionFailed"
        } else if stop.is_some() {
            "stoppedAfterStart"
        } else {
            "commandFailedAfterStart"
        },
    }
}

#[cfg(test)]
mod tests;
