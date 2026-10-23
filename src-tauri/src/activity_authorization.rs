//! Feasibility experiment, compiled only in tests. The sole operation is the
//! pinned Mole PID 1/root visibility probe. No paths, commands, passwords,
//! deletion, daemon installation, or production authorization tokens.
use std::time::{Duration, Instant};

const HEADER: &str = "MooMole.visibility.v1";
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
defined($nonce) && $nonce =~ /\A[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\z/ && !@ARGV or exit 126;
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
}

#[derive(Debug, PartialEq)]
enum Observation {
    CompleteViewObserved,
    IncompleteOrFailed,
    Cancelled,
    Expired,
}

// The private constructor accepts no externally supplied executable, source,
// arguments, or nonce. UUID v4 makes each authorization source distinct. System
// authentication/cache behavior still needs actual packaged-App verification.
impl Probe {
    fn new() -> Self {
        Self {
            nonce: uuid::Uuid::new_v4().to_string(),
            issued: Instant::now(),
        }
    }

    fn command(&self) -> String {
        format!(
            "cd / && /usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin LC_ALL=C /usr/bin/perl -e {} {}",
            shell_literal(PROBE),
            shell_literal(&self.nonce)
        )
    }

    fn authorization_source(&self) -> String {
        // No tell/application block: this is Standard Additions system
        // authentication, not automation of Terminal, Finder, or another app.
        format!(
            "try\nset observed to do shell script {} with administrator privileges without altering line endings\nreturn \"ok\" & linefeed & observed\non error messageText number errorNumber\nreturn \"error\" & linefeed & (errorNumber as text) & linefeed & messageText\nend try",
            apple_literal(&self.command())
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
        // A PID 1 record and its uid=0 must belong to the SAME record. Warnings,
        // extra/truncated fields, status 1 and success without proof are unknown.
        let expected = format!("{HEADER}\n{}\nuid={uid}\np1\nu0\n", self.nonce);
        if success && (body == expected || body == expected.trim_end_matches('\n')) {
            Observation::CompleteViewObserved
        } else {
            Observation::IncompleteOrFailed
        }
    }
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
        let valid = format!("ok\n{HEADER}\n{}\nuid=0\np1\nu0\n", probe.nonce);
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
            probe.observe(b"error\n-128\nUser canceled.", true, true),
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
            .args(["-e", PROBE, &probe.nonce])
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
            .args(["-e", &source, &probe.nonce])
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
}
