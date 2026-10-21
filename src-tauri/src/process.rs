//! Owned process groups for the current read-only commands. The leader remains
//! unreaped until the last signal has been sent; inherited pipes have deadlines.
use crate::engine::{Job, OUTPUT_LIMIT};
use std::{
    mem::MaybeUninit,
    process::Stdio,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    task::JoinHandle,
    time::Instant,
};

const STOP_GRACE: Duration = Duration::from_secs(3);
const PIPE_GRACE: Duration = Duration::from_secs(1);
const REAP_GRACE: Duration = Duration::from_secs(1);

struct Group {
    job: Arc<Job>,
    pid: u32,
    lease: Arc<()>,
    armed: bool,
}
impl Group {
    fn release(&mut self, signal: Option<i32>) {
        if self.armed {
            self.job.release(self.pid, &self.lease, signal);
            self.armed = false;
        }
    }
}
impl Drop for Group {
    fn drop(&mut self) {
        self.release(Some(libc::SIGKILL));
    }
}

// This does not reap: the unreaped child pins its PID/PGID until Group releases
// the registration. No child.wait/try_wait runs while it is registered.
fn exited(pid: u32) -> Result<bool, String> {
    let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
    let rc = unsafe {
        libc::waitid(
            libc::P_PID,
            pid,
            info.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if rc != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EINTR) {
            return Ok(false);
        }
        return Err(format!(
            "无法确认任务主进程的所有权，已停止按 PID 发送信号：{error}"
        ));
    }
    Ok(unsafe { info.assume_init() }.si_pid == pid as i32)
}

struct Reader {
    handle: JoinHandle<Result<Vec<u8>, String>>,
    value: Option<Result<Vec<u8>, String>>,
    limit: u64,
}
impl Reader {
    fn new(stream: impl AsyncRead + Unpin + Send + 'static, limit: u64) -> Self {
        Self {
            handle: tokio::spawn(async move {
                let mut data = vec![];
                stream
                    .take(limit + 1)
                    .read_to_end(&mut data)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(data)
            }),
            value: None,
            limit,
        }
    }
    fn lines(
        mut stream: impl AsyncRead + Unpin + Send + 'static,
        mut receive: impl FnMut(&[u8]) -> Result<(), String> + Send + 'static,
    ) -> Self {
        Self {
            handle: tokio::spawn(async move {
                let mut pending = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let count =
                        tokio::time::timeout(Duration::from_secs(45), stream.read(&mut chunk))
                            .await
                            .map_err(|_| "状态流长时间没有输出，请重新开始采集。".to_string())?
                            .map_err(|e| e.to_string())?;
                    if count == 0 {
                        return if pending.is_empty() {
                            Ok(vec![])
                        } else {
                            Err("状态流最后一条记录不完整。".into())
                        };
                    }
                    for byte in &chunk[..count] {
                        if *byte == b'\n' {
                            receive(&pending)?;
                            pending.clear();
                        } else {
                            if pending.len() >= 512 * 1024 {
                                return Err("状态流单条记录超过上限。".into());
                            }
                            pending.push(*byte);
                        }
                    }
                }
            }),
            value: None,
            limit: 512 * 1024,
        }
    }
    async fn poll_finished(&mut self) {
        if self.value.is_none() && self.handle.is_finished() {
            self.value = Some(
                (&mut self.handle)
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.map_err(|e| e.to_string())),
            );
        }
    }
    fn problem(&self) -> Option<&str> {
        match &self.value {
            Some(Err(error)) => Some(error),
            Some(Ok(bytes)) if bytes.len() as u64 > self.limit => {
                Some("任务输出超过上限，请缩小扫描目录。")
            }
            _ => None,
        }
    }
    async fn finish(&mut self) -> Result<Vec<u8>, String> {
        if self.value.is_none() {
            self.value = Some(
                (&mut self.handle)
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.map_err(|e| e.to_string())),
            );
        }
        self.value.take().unwrap()
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

pub async fn output(command: Command, job: Arc<Job>, timeout: Duration) -> Result<Vec<u8>, String> {
    output_bounded(command, job, timeout, OUTPUT_LIMIT).await
}

pub async fn output_bounded(
    command: Command,
    job: Arc<Job>,
    timeout: Duration,
    limit: u64,
) -> Result<Vec<u8>, String> {
    supervise(command, job, timeout, limit, None).await
}

type LineReceiver = Box<dyn FnMut(&[u8]) -> Result<(), String> + Send>;
pub(crate) async fn stream_lines(
    command: Command,
    job: Arc<Job>,
    receive: impl FnMut(&[u8]) -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    // One session is deliberately bounded; continuing after 24h is explicit.
    supervise(
        command,
        job,
        Duration::from_secs(24 * 3600),
        64 * 1024,
        Some(Box::new(receive)),
    )
    .await
    .map(|_| ())
}
async fn supervise(
    mut command: Command,
    job: Arc<Job>,
    timeout: Duration,
    limit: u64,
    receive: Option<LineReceiver>,
) -> Result<Vec<u8>, String> {
    if limit == 0 || limit > OUTPUT_LIMIT {
        return Err("输出边界无法识别。".into());
    }
    if job.cancelled.load(Ordering::SeqCst) {
        return Err("任务已取消。".into());
    }
    use std::os::unix::process::CommandExt;
    command.as_std_mut().process_group(0);
    // Cleanup belongs to Group. Tokio's implicit PID signal on drop would not
    // share the registration lock or the unreaped-leader ownership boundary.
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    let mut child = command.spawn().map_err(|e| format!("无法启动 Mole：{e}"))?;
    let pid = child.id().ok_or("无法取得任务进程。")?;
    let lease = match job.register(pid) {
        Ok(lease) => lease,
        Err(error) => {
            // This is our just-spawned, unreaped child, not the other registration.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
            let _ = tokio::time::timeout(REAP_GRACE, child.wait()).await;
            return Err(error);
        }
    };
    let mut group = Group {
        job: job.clone(),
        pid,
        lease,
        armed: true,
    };
    let streaming = receive.is_some();
    let stdout = child.stdout.take().ok_or("无法读取任务输出。")?;
    let mut out = match receive {
        Some(receive) => Reader::lines(stdout, receive),
        None => Reader::new(stdout, limit),
    };
    let mut err = Reader::new(child.stderr.take().ok_or("无法读取任务诊断。")?, limit);
    let started = Instant::now();
    let mut exited_at = None;
    let mut stopping: Option<(String, Instant)> = None;
    loop {
        out.poll_finished().await;
        err.poll_finished().await;
        match exited(pid) {
            Ok(true) => {
                exited_at.get_or_insert_with(Instant::now);
            }
            Ok(false) => (),
            Err(error) => {
                // Do not signal a PID whose unreaped-child ownership was lost.
                group.release(None);
                return Err(error);
            }
        }
        if stopping.is_none() {
            let reason = if job.cancelled.load(Ordering::SeqCst) {
                Some("任务已取消。".to_string())
            } else if started.elapsed() >= timeout {
                Some("任务超时，可缩小扫描范围后重试。".to_string())
            } else if let Some(problem) = out.problem().or(err.problem()) {
                Some(problem.to_string())
            } else if streaming && out.value.is_some() && exited_at.is_none() {
                Some("状态流输出通道已结束，请重新开始采集。".to_string())
            } else if exited_at.is_some_and(|at| at.elapsed() >= PIPE_GRACE)
                && (out.value.is_none() || err.value.is_none())
            {
                Some("主进程已结束，但输出通道未关闭。".to_string())
            } else {
                None
            };
            if let Some(reason) = reason {
                job.signal(pid, &group.lease, libc::SIGTERM);
                stopping = Some((reason, Instant::now()));
            } else if exited_at.is_some() && out.value.is_some() && err.value.is_some() {
                break;
            }
        }
        // A stopped leader with closed pipes no longer needs the grace period.
        // It stays unreaped until the final group signal below pins its PGID.
        if stopping.is_some() && exited_at.is_some() && out.value.is_some() && err.value.is_some() {
            break;
        }
        if stopping
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= STOP_GRACE)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    // Last signal happens under the registration lock and BEFORE reaping.
    group.release(Some(libc::SIGKILL));
    let status = tokio::time::timeout(REAP_GRACE, child.wait())
        .await
        .map_err(|_| "进程回收未在期限内完成，已停止等待，状态需核实。")?
        .map_err(|e| e.to_string())?;
    let pipes = tokio::time::timeout(PIPE_GRACE, async {
        tokio::join!(out.finish(), err.finish())
    })
    .await;
    let (stdout, stderr) = match pipes {
        Ok((out, err)) => (out?, err?),
        Err(_) => {
            return Err(format!(
                "{}输出通道仍未关闭，可能有脱离进程组的子进程；请重新检查。",
                stopping.as_ref().map_or("", |(reason, _)| reason.as_str())
            ))
        }
    };
    if let Some((reason, _)) = stopping {
        return Err(reason.into());
    }
    if job.cancelled.load(Ordering::SeqCst) {
        return Err("任务已取消。".into());
    }
    if !status.success() {
        let source = if stderr.is_empty() { &stdout } else { &stderr };
        let diagnostic: String =
            crate::engine::plain_terminal_text(&String::from_utf8_lossy(source))
                .chars()
                .rev()
                .take(1600)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
        return Err(if diagnostic.trim().is_empty() {
            "Mole 未能完成任务。".into()
        } else {
            diagnostic
        });
    }
    Ok(stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::TaskKind;
    use std::path::Path;

    async fn ready(path: &Path) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !path.is_file() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture did not become ready");
    }
    fn shell(script: &str, path: &Path) -> Command {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", script, "fixture"]).arg(path);
        c
    }
    #[tokio::test]
    async fn exited_leader_is_not_reaped_or_signalled_after_release() {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("/usr/bin/true")
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();
        let job = Arc::new(Job::new(TaskKind::Analyze));
        let lease = job.register(pid).unwrap();
        let mut group = Group {
            job: job.clone(),
            pid,
            lease: lease.clone(),
            armed: true,
        };
        tokio::time::timeout(Duration::from_secs(2), async {
            while !exited(pid).unwrap() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(exited(pid).unwrap()); // WNOWAIT preserves ownership on repeated observations.
        group.release(None);
        assert!(child.wait().unwrap().success());
        assert!(exited(pid).is_err());
        let mut next = std::process::Command::new("/bin/sleep")
            .arg("5")
            .process_group(0)
            .spawn()
            .unwrap();
        let next_pid = next.id();
        job.register(next_pid).unwrap();
        // Releasing an old registration must never clear or signal a later one.
        job.release(pid, &lease, Some(libc::SIGKILL));
        // A stale lease is refused even if a numeric PID were reused.
        job.release(next_pid, &lease, Some(libc::SIGKILL));
        drop(group);
        assert_eq!(job.process_id(), Some(next_pid));
        assert!(!exited(next_pid).unwrap());
        job.kill_group();
        next.wait().unwrap();
        assert_eq!(job.process_id(), None);
    }
    #[tokio::test]
    async fn pre_cancelled_work_never_spawns_and_duplicate_registration_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let marker = t.path().join("spawned");
        let job = Arc::new(Job::new(TaskKind::Analyze));
        job.cancel();
        assert!(output(
            shell("echo spawned > \"$1\"", &marker),
            job,
            Duration::from_secs(2)
        )
        .await
        .unwrap_err()
        .contains("取消"));
        assert!(!marker.exists());
        let active = Arc::new(Job::new(TaskKind::Analyze));
        let command = shell("echo ready > \"$1\"; exec sleep 10", &marker);
        let a = active.clone();
        let running =
            tokio::spawn(async move { output(command, a, Duration::from_secs(20)).await });
        ready(&marker).await;
        let original = active.process_id();
        assert!(output(
            Command::new("/usr/bin/true"),
            active.clone(),
            Duration::from_secs(2)
        )
        .await
        .unwrap_err()
        .contains("已有"));
        assert_eq!(active.process_id(), original);
        active.cancel();
        assert!(running.await.unwrap().unwrap_err().contains("取消"));
    }
    #[tokio::test]
    async fn term_grace_lets_a_watchdog_reap_its_separate_process_group() {
        let t = tempfile::tempdir().unwrap();
        let marker = t.path().join("state");
        let mut c = Command::new("/usr/bin/perl");
        c.args([
            "-e",
            r#"
            use POSIX qw(setpgid); use Time::HiRes qw(sleep);
            my $stop=0; $SIG{TERM}=sub{$stop=1};
            my $pid=fork(); defined $pid or die;
            if (!$pid) { setpgid(0,0); $SIG{TERM}='DEFAULT'; sleep 10; exit; }
            setpgid($pid,$pid);
            open(my $f,'>',$ARGV[0]) or die; print $f 'ready'; close $f;
            while (!$stop) { sleep 0.01; }
            kill 'TERM', -$pid; waitpid($pid,0);
            open($f,'>',$ARGV[0]) or die; print $f 'reaped'; close $f;
        "#,
        ])
        .arg(&marker);
        let job = Arc::new(Job::new(TaskKind::Review));
        let a = job.clone();
        let running = tokio::spawn(async move { output(c, a, Duration::from_secs(20)).await });
        ready(&marker).await;
        job.cancel();
        let result = tokio::time::timeout(Duration::from_secs(6), running)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(result.contains("取消"));
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "reaped");
        assert_eq!(job.process_id(), None);
    }
    #[tokio::test]
    async fn ignored_term_is_escalated_and_an_aborted_waiter_releases_its_group() {
        for abort in [false, true] {
            let t = tempfile::tempdir().unwrap();
            let marker = t.path().join("ready");
            let job = Arc::new(Job::new(TaskKind::Analyze));
            let a = job.clone();
            let c = shell("trap '' TERM; echo ready > \"$1\"; exec sleep 10", &marker);
            let running = tokio::spawn(async move { output(c, a, Duration::from_secs(20)).await });
            ready(&marker).await;
            if abort {
                running.abort();
                assert!(running.await.unwrap_err().is_cancelled());
            } else {
                job.cancel();
                assert!(tokio::time::timeout(Duration::from_secs(6), running)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap_err()
                    .contains("取消"));
            }
            assert_eq!(job.process_id(), None);
        }
    }
    #[tokio::test]
    async fn escaped_writer_cannot_keep_a_finished_task_waiting_forever() {
        let t = tempfile::tempdir().unwrap();
        let marker = t.path().join("escaped-finished");
        let mut c = Command::new("/usr/bin/perl");
        c.args(["-e", r#"
            use POSIX qw(setpgid); use Time::HiRes qw(sleep);
            my $pid=fork(); defined $pid or die;
            if (!$pid) { setpgid(0,0); sleep 7; open(my $f,'>',$ARGV[0]) or die; print $f 'done'; close $f; exit; }
        "#]).arg(&marker);
        let job = Arc::new(Job::new(TaskKind::Analyze));
        let result = tokio::time::timeout(
            Duration::from_secs(6),
            output(c, job.clone(), Duration::from_secs(20)),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(result.contains("输出通道"));
        assert!(result.contains("脱离进程组"));
        assert_eq!(job.process_id(), None);
        assert!(!marker.exists());
        // The escaped fixture is deliberately bounded; do not signal a saved PID.
        ready(&marker).await;
    }
    #[tokio::test]
    async fn caller_output_bounds_stop_a_small_stream_and_release_the_owned_group() {
        let started = Instant::now();
        let mut command = Command::new("/usr/bin/perl");
        command.args(["-e", "$|=1; print 'x' x 4096; sleep 10"]);
        let job = Arc::new(Job::new(TaskKind::Apps));
        let result = tokio::time::timeout(
            Duration::from_secs(6),
            output_bounded(command, job.clone(), Duration::from_secs(20), 1024),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(result.contains("超过上限"));
        assert_eq!(job.process_id(), None);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    #[tokio::test]
    async fn excessive_output_interrupts_without_waiting_for_the_command_deadline() {
        let mut c = Command::new("/usr/bin/perl");
        c.args(["-e", "print 'x' x (33*1024*1024); sleep 10"]);
        let job = Arc::new(Job::new(TaskKind::Analyze));
        let result = tokio::time::timeout(
            Duration::from_secs(6),
            output(c, job.clone(), Duration::from_secs(20)),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(result.contains("超过上限"));
        assert_eq!(job.process_id(), None);
    }
    #[tokio::test]
    #[ignore = "Explicit pinned Mole timeout helper check; only a unique temporary PID marker and bounded sleep are used"]
    async fn pinned_mole_perl_timeout_cleanup_survives_normal_cancellation() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        crate::engine_manifest::verify(&root).unwrap();
        let t = tempfile::tempdir().unwrap();
        let marker = t.path().join("pid");
        let mut c = Command::new("/bin/bash");
        c.args([
            "-c",
            r#"
            export MO_TIMEOUT_INITIALIZED=1 MO_TIMEOUT_BIN='' MO_TIMEOUT_PERL_BIN=/usr/bin/perl
            source "$1"
            run_with_timeout 20 /bin/sh -c 'echo $$ > "$1"; exec /bin/sleep 10' fixture "$2"
        "#,
            "fixture",
        ])
        .arg(root.join("lib/core/timeout.sh"))
        .arg(&marker);
        let job = Arc::new(Job::new(TaskKind::Clean));
        let a = job.clone();
        let running = tokio::spawn(async move { output(c, a, Duration::from_secs(30)).await });
        ready(&marker).await;
        let pid: i32 = std::fs::read_to_string(&marker)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        job.cancel();
        assert!(tokio::time::timeout(Duration::from_secs(6), running)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err()
            .contains("取消"));
        assert_eq!(job.process_id(), None);
        tokio::time::timeout(Duration::from_secs(2), async {
            while unsafe { libc::kill(pid, 0) } == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("pinned helper left its bounded payload running");
    }
}
