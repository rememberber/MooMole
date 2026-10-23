//! Fixed, user-initiated read-only system-authorization diagnostic. This does
//! not grant cleanup authority, accept paths/commands, or install a helper.
use crate::engine::Job;
use std::sync::{atomic::Ordering, Arc};
use std::time::{Duration, Instant};

mod state;
// Staged backend building block. It has no IPC/action until the confirmation and
// ordinary-user executor can consume the same bound, fresh observation.
#[allow(dead_code)]
pub(crate) mod selected_logs;
pub(crate) use state::{Snapshot, Store};

const HEADER: &str = "MooMole.visibility.v1";
const END: &str = "MooMole.visibility.end.v1";
const OUTPUT_LIMIT: usize = 8192;
// The supervisor owns exactly one child. It only kills BEFORE reaping, without
// asynchronous signal handlers or a recycled-PID watchdog. System Perl and its
// core modules are required; missing support is not a PATH fallback opportunity.
const PROBE: &str = r#"use strict;
use warnings;
use IO::Select;
use POSIX qw(WNOHANG);
use Time::HiRes qw(clock_gettime CLOCK_MONOTONIC);
$| = 1;
my $nonce = shift @ARGV;
my $expires_ms = shift @ARGV;
defined($nonce) && $nonce =~ /\A[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\z/ && defined($expires_ms) && $expires_ms =~ /\A[0-9]{1,16}\z/ && !@ARGV or exit 126;
my $now_ms = int(clock_gettime(CLOCK_MONOTONIC) * 1000);
if ($expires_ms <= $now_ms || $expires_ms > $now_ms + 120000) {
    print "MooMole.visibility.v1\n$nonce\nuid=$>\nexpired\n";
    exit 0;
}
open STDIN, '<', '/dev/null' or exit 126;
pipe(my $reader, my $writer) or exit 126;
my $child = fork();
defined($child) or exit 126;
if (!$child) {
    close $reader;
    open STDOUT, '>&', $writer or exit 126;
    open STDERR, '>&', $writer or exit 126;
    close $writer;
    exec { '/usr/sbin/lsof' } '/usr/sbin/lsof', '-F', 'pu', '-p', '1';
    exit 126;
}
close $writer;
my $selector = IO::Select->new($reader);
my $deadline = clock_gettime(CLOCK_MONOTONIC) + 5.0;
my ($records, $reaped, $status, $eof, $failure) = ('', 0, 0, 0, 0);
while (1) {
    if (!$eof && $selector->can_read(0.02)) {
        my $read = sysread($reader, my $chunk, 4096);
        if (!defined($read)) { $failure = 125; last; }
        if ($read == 0) { $eof = 1; }
        else { $records .= $chunk; }
        if (length($records) > 4096) { $records = ''; $failure = 125; last; }
    }
    if (!$reaped) {
        my $waited = waitpid($child, WNOHANG);
        if ($waited == $child) { $status = $?; $reaped = 1; }
        elsif ($waited == -1) { $reaped = 1; $failure = 125; last; }
    }
    last if $eof && $reaped;
    if (clock_gettime(CLOCK_MONOTONIC) >= $deadline) { $failure = 124; last; }
    select(undef, undef, undef, 0.02) if $eof;
}
if (!$reaped) {
    kill 'KILL', $child;
    waitpid($child, 0);
}
close $reader;
print "MooMole.visibility.v1\n$nonce\nuid=$>\n";
print $records;
exit $failure if $failure;
exit 125 if $status & 127;
exit($status >> 8);
"#;

struct Probe {
    nonce: String,
    issued: Instant,
    deadline_ms: u64,
}

fn monotonic_ms() -> Option<u64> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) } != 0 || time.tv_sec < 0 {
        return None;
    }
    (time.tv_sec as u64)
        .checked_mul(1000)?
        .checked_add(time.tv_nsec as u64 / 1_000_000)
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Observation {
    CompleteViewObserved,
    IncompleteOrFailed,
    Cancelled,
    Expired,
    Unsupported,
}

// The private constructor accepts no externally supplied executable, source,
// arguments, or nonce. UUID v4 makes each authorization source distinct. System
// authentication/cache behavior still needs actual packaged-App verification.
impl Probe {
    fn new() -> Self {
        Self {
            nonce: uuid::Uuid::new_v4().to_string(),
            issued: Instant::now(),
            deadline_ms: monotonic_ms()
                .and_then(|time| time.checked_add(120_000))
                .unwrap_or(0),
        }
    }

    fn command(&self) -> String {
        format!(
            "cd / && /usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin LC_ALL=C /usr/bin/perl -e {} {} {}",
            shell_literal(PROBE),
            shell_literal(&self.nonce),
            self.deadline_ms,
        )
    }

    fn authorization_source(&self) -> String {
        // No tell/application block: this is Standard Additions system
        // authentication, not automation of Terminal, Finder, or another app.
        format!(
            "try\nset observed to do shell script {} with administrator privileges without altering line endings\nset reportText to \"ok\" & linefeed & observed\non error messageText number errorNumber\nset reportText to \"error\" & linefeed & (errorNumber as text) & linefeed & messageText\nend try\nreturn reportText & linefeed & \"{END}\"",
            apple_literal(&self.command()),
        )
    }

    fn observe(&self, bytes: &[u8], privileged: bool, success: bool) -> Observation {
        if self.issued.elapsed() >= Duration::from_secs(120) {
            return Observation::Expired;
        }
        if bytes.len() > OUTPUT_LIMIT {
            return Observation::IncompleteOrFailed;
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Observation::IncompleteOrFailed;
        };
        let body = if privileged {
            // osascript appends one newline to a returned string. Bind an end
            // marker so we never trim away diagnostic/unknown field content.
            let Some(text) = text
                .strip_suffix(&format!("\n{END}\n"))
                .or_else(|| text.strip_suffix(&format!("\n{END}")))
            else {
                return Observation::IncompleteOrFailed;
            };
            if text.starts_with("error\n-128\n") {
                return Observation::Cancelled;
            }
            let Some(body) = text.strip_prefix("ok\n") else {
                return Observation::IncompleteOrFailed;
            };
            body
        } else {
            text
        };
        let uid = if privileged {
            0
        } else {
            unsafe { libc::geteuid() }
        };
        // lsof's local man page: f is ALWAYS selected, including -F pu. Keep
        // PID/UID in the same process set, allowing only known descriptor fields
        // after it; warnings, a second process, ferr/NOFD, and other fields fail.
        let prefix = format!("{HEADER}\n{}\nuid={uid}\n", self.nonce);
        if success && body == format!("{HEADER}\n{}\nuid={uid}\nexpired\n", self.nonce) {
            return Observation::Expired;
        }
        let records = body.strip_prefix(&prefix).unwrap_or("");
        let mut fields = records.split_terminator('\n');
        if success
            && fields.next() == Some("p1")
            && fields.next() == Some("u0")
            && fields.all(file_descriptor)
        {
            Observation::CompleteViewObserved
        } else {
            Observation::IncompleteOrFailed
        }
    }

    fn diagnostic(&self, bytes: &[u8]) -> String {
        // Only fixed-marker/numeric presence and framing metadata leave this
        // module. Never expose native error prose or arbitrary process output.
        let Ok(text) = std::str::from_utf8(bytes) else {
            return "原生响应不是有效 UTF-8，无法核对协议。".into();
        };
        let status = if text.starts_with("ok\n") {
            "0".into()
        } else if text.starts_with("error\n") {
            text.lines()
                .nth(1)
                .and_then(|s| s.parse::<i32>().ok())
                .map(|n| n.to_string())
                .unwrap_or("未知".into())
        } else {
            "未知".into()
        };
        let found = |field: &str| {
            if text.lines().any(|s| s == field) {
                "有"
            } else {
                "未见"
            }
        };
        format!("脚本状态：{status}；固定探针标记：{}；有效用户 root：{}；PID 1 字段：{}；root 字段：{}；文件描述符字段：{}；结束标记：{}；{} 字节（LF {} / CR {}）。", found(HEADER), found("uid=0"), found("p1"), found("u0"), text.lines().filter(|line| file_descriptor(line)).count(), found(END), bytes.len(), bytes.iter().filter(|&&b| b == b'\n').count(), bytes.iter().filter(|&&b| b == b'\r').count())
    }
}

fn file_descriptor(field: &str) -> bool {
    let Some(value) = field.strip_prefix('f') else {
        return false;
    };
    matches!(value, "cwd" | "rtd" | "txt" | "mem" | "mmap" | "pd")
        || (!value.is_empty()
            && value.len() <= 10
            && value.bytes().all(|c| c.is_ascii_digit())
            && value.parse::<u32>().is_ok())
}

pub(crate) async fn run(job: Arc<Job>) -> (Observation, String) {
    // System paths only; don't download/install interpreters or search PATH.
    if ["/usr/bin/osascript", "/usr/bin/perl", "/usr/sbin/lsof"]
        .iter()
        .any(|path| !std::path::Path::new(path).is_file())
    {
        return (Observation::Unsupported, "固定系统检查工具缺失。".into());
    }
    let probe = Probe::new();
    if probe.deadline_ms == 0 {
        return (Observation::Unsupported, "系统单调时钟不可用。".into());
    }
    let mut command = tokio::process::Command::new("/bin/sh");
    command
        // Merge OUTER interpreter diagnostics too. process::output_bounded
        // otherwise returns stdout on status 0 even with stderr warnings.
        .args([
            "-c",
            &format!(
                "exec /usr/bin/osascript -e {} 2>&1",
                shell_literal(&probe.authorization_source())
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
    let (observation, diagnostic) = match result {
        Ok(bytes) => (probe.observe(&bytes, true, true), probe.diagnostic(&bytes)),
        Err(error) => (
            Observation::IncompleteOrFailed,
            format!(
                "原生进程或通道未完成；{}",
                probe.diagnostic(error.as_bytes())
            ),
        ),
    };
    let observation = if job.cancelled.load(Ordering::SeqCst) {
        Observation::Cancelled
    } else {
        observation
    };
    if observation == Observation::Cancelled || observation == Observation::IncompleteOrFailed {
        // Killing our osascript group cannot prove a system-authorized process
        // stopped. Keep admission/installation lease through its normal probe
        // budget. A surviving system dialog must be cancelled by the user; no
        // returned result after cancellation can authorize anything.
        tokio::time::sleep(Duration::from_secs(6)).await;
    }
    (
        if job.cancelled.load(Ordering::SeqCst) {
            Observation::Cancelled
        } else {
            observation
        },
        diagnostic,
    )
}

fn shell_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn apple_literal(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::{rc::autoreleasepool, AnyThread};
    use objc2_foundation::{NSAppleScript, NSString};
    use std::process::{Command, Stdio};

    #[test]
    fn proof_binds_nonce_effective_user_exit_status_and_root_record() {
        let probe = Probe::new();
        let valid = format!("ok\n{HEADER}\n{}\nuid=0\np1\nu0\n\n{END}\n", probe.nonce);
        assert_eq!(
            probe.observe(valid.as_bytes(), true, true),
            Observation::CompleteViewObserved
        );
        assert_eq!(
            probe.observe(valid.trim_end().as_bytes(), true, true),
            Observation::CompleteViewObserved
        );
        for invalid in [
            valid.replace(&probe.nonce, &uuid::Uuid::new_v4().to_string()),
            valid.replace("uid=0", "uid=501"),
            valid.replace("p1\nu0", "p1\np2\nu0"),
            valid.replace("p1\nu0", "p1\nu501"),
            valid.replace("p1\nu0", ""),
            format!("{valid}lsof: permission denied\n"),
            format!("{valid}{valid}"),
            valid.replace("visibility.v1", "visibility.v2"),
            "ok\n".into(),
            "error\n1\n".into(),
            "error\n-60005\nnot authorized".into(),
            "error\n142\nalarm".into(),
            "x".repeat(OUTPUT_LIMIT + 1),
        ] {
            assert_eq!(
                probe.observe(invalid.as_bytes(), true, true),
                Observation::IncompleteOrFailed,
                "{invalid}"
            );
        }
        assert_eq!(
            probe.observe(valid.as_bytes(), true, false),
            Observation::IncompleteOrFailed
        );
        assert_eq!(
            probe.observe(b"\xff", true, true),
            Observation::IncompleteOrFailed
        );
        assert_eq!(
            probe.observe(
                format!("error\n-128\nUser canceled.\n{END}\n").as_bytes(),
                true,
                true
            ),
            Observation::Cancelled
        );
        for end in 0..valid.len() - 1 {
            assert_ne!(
                probe.observe(&valid.as_bytes()[..end], true, true),
                Observation::CompleteViewObserved
            );
        }
        let expired = Probe {
            nonce: probe.nonce,
            issued: Instant::now() - Duration::from_secs(120),
            deadline_ms: probe.deadline_ms,
        };
        assert_eq!(
            expired.observe(valid.as_bytes(), true, true),
            Observation::Expired
        );
    }

    #[test]
    fn system_compiles_fixed_authorization_source_without_executing_it() {
        let probe = Probe::new();
        autoreleasepool(|_| {
            let script = NSAppleScript::initWithSource(
                NSAppleScript::alloc(),
                &NSString::from_str(&probe.authorization_source()),
            )
            .unwrap();
            let mut error = None;
            assert!(
                unsafe { script.compileAndReturnError(Some(&mut error)) },
                "{error:?}"
            );
            assert!(script.isCompiled());
        });
    }

    #[test]
    fn literal_quoting_survives_two_languages_without_running_interpolations() {
        let literal = "Unicode 中文 ' \" \\ $(false) `false`\nline";
        let output = Command::new("/bin/sh")
            .args(["-c", &format!("printf %s {}", shell_literal(literal))])
            .env_clear()
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, literal.as_bytes());
        let source = Probe::new().authorization_source();
        assert!(source.contains("with administrator privileges without altering line endings"));
        assert!(!source.contains("password "));
        assert!(!source.contains("tell application"));
        assert_ne!(source, Probe::new().authorization_source());
    }

    #[test]
    fn actual_unprivileged_probe_never_prompts_or_borrows_sudo_credentials() {
        let probe = Probe::new();
        let output = Command::new("/usr/bin/perl")
            .args(["-e", PROBE, &probe.nonce, &probe.deadline_ms.to_string()])
            .env_clear()
            .env("LC_ALL", "C")
            .current_dir("/")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let observation = probe.observe(&output.stdout, false, output.status.success());
        // Host-dependent observation, not a fixture claim of complete visibility.
        eprintln!(
            "actual ordinary-account PID 1 visibility: {observation:?}; exit={:?}",
            output.status.code()
        );
        assert!(matches!(
            observation,
            Observation::CompleteViewObserved | Observation::IncompleteOrFailed
        ));
        assert!(output
            .stdout
            .starts_with(format!("{HEADER}\n{}\nuid=", probe.nonce).as_bytes()));
    }

    // Replace the fixed child only inside nonprivileged tests. There is no such
    // switch in the authorization source, an environment flag, or a request.
    fn fixture(probe: &Probe, child: &str) -> std::process::Output {
        const EXEC: &str = "exec { '/usr/sbin/lsof' } '/usr/sbin/lsof', '-F', 'pu', '-p', '1';";
        assert_eq!(PROBE.matches(EXEC).count(), 1);
        let child = child.replace('\\', "\\\\").replace('\'', "\\'");
        let source = PROBE
            .replace(
                EXEC,
                &format!("exec {{ '/usr/bin/perl' }} '/usr/bin/perl', '-e', '{child}';"),
            )
            .replace("+ 5.0", "+ 0.2");
        Command::new("/usr/bin/perl")
            .args(["-e", &source, &probe.nonce, &probe.deadline_ms.to_string()])
            .env_clear()
            .env("LC_ALL", "C")
            .current_dir("/")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    #[test]
    fn supervisor_collects_exit_status_warnings_and_reaps_its_timed_out_child() {
        let probe = Probe::new();
        let output = fixture(&probe, "print qq(p1\\nu0\\n); exit 0;");
        assert_eq!(
            probe.observe(&output.stdout, false, output.status.success()),
            Observation::CompleteViewObserved
        );
        for child in [
            "print qq(p1\\nu0\\n); exit 1;",
            "print qq(p1\\nu0\\n); print STDERR qq(warning\\n);",
        ] {
            let output = fixture(&probe, child);
            assert_eq!(
                probe.observe(&output.stdout, false, output.status.success()),
                Observation::IncompleteOrFailed
            );
        }
        // Child is real but unprivileged and only sleeps; timeout owns and reaps
        // it before returning. No detached watchdog later signals a reused PID.
        let started = Instant::now();
        let output = fixture(&probe, "$|=1; print qq(child=$$\\n); sleep 30;");
        assert_eq!(output.status.code(), Some(124));
        assert!(started.elapsed() < Duration::from_secs(4));
        let text = std::str::from_utf8(&output.stdout).unwrap();
        let child: i32 = text
            .lines()
            .find_map(|s| s.strip_prefix("child="))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(unsafe { libc::kill(child, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        assert_eq!(
            probe.observe(&output.stdout, false, output.status.success()),
            Observation::IncompleteOrFailed
        );
    }

    #[test]
    fn supervisor_limits_output_and_waits_for_exit_even_after_stdout_closes() {
        let probe = Probe::new();
        let output = fixture(&probe, "print q(x) x 8192; sleep 30;");
        assert_eq!(output.status.code(), Some(125));
        assert!(output.stdout.len() <= OUTPUT_LIMIT);
        assert_eq!(
            probe.observe(&output.stdout, false, output.status.success()),
            Observation::IncompleteOrFailed
        );
        let output = fixture(&probe, "close STDOUT; close STDERR; sleep 30;");
        assert_eq!(output.status.code(), Some(124));
        assert_eq!(
            probe.observe(&output.stdout, false, output.status.success()),
            Observation::IncompleteOrFailed
        );
    }

    #[test]
    fn expired_authorization_does_not_launch_even_the_fixed_probe() {
        let probe = Probe {
            deadline_ms: 0,
            ..Probe::new()
        };
        let output = fixture(&probe, "print qq(child_must_not_run\\n); exit 0;");
        assert!(output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("child_must_not_run"));
        assert_eq!(
            probe.observe(&output.stdout, false, true),
            Observation::Expired
        );
    }

    #[test]
    fn native_interpreter_roundtrips_fixed_response_without_authorization_or_app_events() {
        let probe = Probe::new();
        let records = format!("{HEADER}\n{}\nuid=0\np1\nu0\n", probe.nonce);
        // Pure AppleScript string evaluation: no do shell script, authorization,
        // filesystem operation, tell/application block or UI interaction.
        let source = format!(
            "return \"ok\" & linefeed & {} & linefeed & \"{END}\"",
            apple_literal(&records)
        );
        let output = Command::new("/usr/bin/osascript")
            .args(["-e", &source])
            .env_clear()
            .current_dir("/")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            probe.observe(&output.stdout, true, true),
            Observation::CompleteViewObserved,
            "{:?}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    #[test]
    fn diagnostics_expose_only_fixed_protocol_metadata_not_native_error_prose() {
        let probe = Probe::new();
        let input = format!(
            "error\n1\nprivate/path secret=password\n{HEADER}\n{}\nuid=0\n\n{END}\n",
            probe.nonce
        );
        let diagnostic = probe.diagnostic(input.as_bytes());
        assert!(diagnostic.contains("脚本状态：1"));
        assert!(diagnostic.contains("有效用户 root：有"));
        assert!(diagnostic.contains("PID 1 字段：未见"));
        for private in ["private", "password", "secret", probe.nonce.as_str()] {
            assert!(!diagnostic.contains(private));
        }
    }

    #[test]
    fn native_shell_transport_runs_fixed_probe_without_administrator_privileges() {
        let probe = Probe::new();
        // Only remove elevation inside this test. It executes the SAME fixed
        // read-only probe as the ordinary-account direct test, with no UI or
        // app automation events and no system authentication.
        let source = probe
            .authorization_source()
            .replace(" with administrator privileges", "");
        let output = Command::new("/usr/bin/osascript")
            .args(["-e", &source])
            .env_clear()
            .current_dir("/")
            .output()
            .unwrap();
        assert!(output.status.success());
        let diagnostic = probe.diagnostic(&output.stdout);
        eprintln!("ordinary-account native shell transport: {diagnostic}");
        assert!(diagnostic.contains("固定探针标记：有"), "{diagnostic}");
    }

    #[test]
    fn root_process_proof_accepts_mandatory_file_descriptor_sets_but_not_warnings() {
        let probe = Probe::new();
        let frame =
            |records: &str| format!("ok\n{HEADER}\n{}\nuid=0\n{records}\n{END}\n", probe.nonce);
        for records in [
            "p1\nu0\nfcwd\nfrtd\nftxt\nftxt\nf0\nf1\nf25\n",
            "p1\nu0\n",
            "p1\nu0\nf0",
        ] {
            assert_eq!(
                probe.observe(frame(records).as_bytes(), true, true),
                Observation::CompleteViewObserved
            );
        }
        for records in [
            "p1\nu0\nferr\n",
            "p1\nu0\nfNOFD\n",
            "p1\nu0\nfcwd\nwarning\n",
            "p1\nu0\nf\n",
            "p1\nu0\nf4294967296\n",
            "p1\nu0\nf0\nu0\n",
            "p1\nfcwd\nu0\n",
            "p1\nu0\nf0\np2\nu0\n",
            "p1\nu0\nf0\n\n",
        ] {
            assert_eq!(
                probe.observe(frame(records).as_bytes(), true, true),
                Observation::IncompleteOrFailed,
                "{records:?}"
            );
        }
        // Observe real descriptor records from this test process, never root.
        let output = Command::new("/usr/sbin/lsof")
            .args(["-F", "pu", "-p", &std::process::id().to_string()])
            .env_clear()
            .output()
            .unwrap();
        assert!(output.status.success());
        let own = String::from_utf8(output.stdout).unwrap();
        let fields: Vec<_> = own.lines().skip(2).collect();
        assert!(!fields.is_empty());
        assert!(
            fields.iter().all(|line| file_descriptor(line)),
            "{fields:?}"
        );
    }
}
