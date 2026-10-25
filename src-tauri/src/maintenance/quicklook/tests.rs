use super::*;
use crate::engine::TaskKind;

fn job(cancelled: bool) -> Job {
    let job = Job::new(TaskKind::MaintenanceRefresh);
    if cancelled {
        job.cancel();
    }
    job
}

fn shell(script: &str) -> Command {
    // Test-only harmless processes. Production accepts only the Step enum.
    let mut command = Command::new("/bin/sh");
    command.args(["-c", script]);
    command
}
fn run(script: &str, cancelled: &Job) -> Attempt {
    supervise(
        Step::ThumbnailCache,
        shell(script),
        cancelled,
        Duration::from_secs(3),
        Duration::from_millis(40),
        |_| {},
    )
}

#[test]
fn only_two_pinned_argument_vectors_exist() {
    assert_eq!(Step::ALL[0].args(), ["-r", "cache"]);
    assert_eq!(Step::ALL[1].args(), ["-r"]);
    assert_eq!(
        serde_json::to_value(Step::ALL).unwrap(),
        serde_json::json!(["thumbnailCache", "generators"])
    );
}

#[test]
fn successful_exit_is_a_command_result_not_a_claim_of_empty_cache_or_fixed_previews() {
    let job = job(false);
    let result = run("exit 0", &job);
    assert_eq!(result.outcome, Outcome::CommandSucceeded);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.stop, None);
    assert_eq!(result.reason, "commandSucceeded");
    assert_eq!(job.process_id(), None);
}

#[test]
fn failure_after_a_real_side_effect_remains_uncertain() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("side-effect");
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "echo changed > \"$1\"; exit 7", "fixture"]);
    command.arg(&marker);
    let result = supervise(
        Step::Generators,
        command,
        &job(false),
        TIMEOUT,
        GRACE,
        |_| {},
    );
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "changed\n");
    assert_eq!(result.outcome, Outcome::Uncertain);
    assert_eq!(result.exit_code, Some(7));
    assert_eq!(result.reason, "commandFailedAfterStart");
}

#[test]
fn cancellation_before_spawn_never_runs_the_command() {
    let result = run("exit 0", &job(true));
    assert_eq!(result.outcome, Outcome::NotStarted);
    assert_eq!(result.stop, Some(Stop::Cancelled));
    assert_eq!(result.exit_code, None);
}

#[test]
fn spawn_failure_is_distinct_from_a_started_failure() {
    let result = supervise(
        Step::Generators,
        Command::new("/no-such-moomole-quicklook-fixture"),
        &job(false),
        TIMEOUT,
        GRACE,
        |_| panic!("must not report a spawned child"),
    );
    assert_eq!(result.outcome, Outcome::NotStarted);
    assert_eq!(result.reason, "spawnFailed");
}

#[test]
fn cancellation_after_spawn_is_uncertain_and_the_child_is_reaped() {
    let cancelled = job(false);
    let mut pid = 0;
    let result = supervise(
        Step::Generators,
        shell("sleep 30"),
        &cancelled,
        TIMEOUT,
        Duration::from_millis(40),
        |spawned| {
            pid = spawned;
            cancelled.cancel();
        },
    );
    assert_eq!(result.outcome, Outcome::Uncertain);
    assert_eq!(result.stop, Some(Stop::Cancelled));
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn timeout_does_not_pretend_no_change_and_kills_a_term_ignoring_child() {
    let started = Instant::now();
    let result = supervise(
        Step::ThumbnailCache,
        shell("trap '' TERM; while :; do sleep 1; done"),
        &job(false),
        Duration::from_millis(100),
        Duration::from_millis(40),
        |_| {},
    );
    assert_eq!(result.outcome, Outcome::Uncertain);
    assert_eq!(result.stop, Some(Stop::TimedOut));
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn a_stopped_command_remains_running_until_the_actual_timeout() {
    let job = job(false);
    let result = supervise(
        Step::Generators,
        shell("kill -STOP $$; exit 0"),
        &job,
        Duration::from_millis(100),
        Duration::from_millis(40),
        |_| {},
    );
    assert_eq!(result.outcome, Outcome::Uncertain);
    assert_eq!(result.stop, Some(Stop::TimedOut));
    assert_eq!(result.reason, "stoppedAfterStart");
    assert_eq!(job.process_id(), None);
}

#[test]
fn a_known_success_is_preserved_when_cancel_arrives_after_exit() {
    let cancelled = job(false);
    let result = supervise(
        Step::Generators,
        shell("exit 0"),
        &cancelled,
        TIMEOUT,
        GRACE,
        |pid| {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
                let rc = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        pid,
                        info.as_mut_ptr(),
                        libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                    )
                };
                assert_eq!(rc, 0);
                if crate::process::terminal_child_event(pid, &unsafe { info.assume_init() }) {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(POLL);
            }
            cancelled.cancel();
        },
    );
    assert_eq!(result.outcome, Outcome::CommandSucceeded);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.stop, None);
}

#[test]
fn discarded_output_has_no_pipe_or_buffer_budget() {
    let result = run(
        "dd if=/dev/zero bs=65536 count=40; echo diagnostic >&2",
        &job(false),
    );
    assert_eq!(result.outcome, Outcome::CommandSucceeded);
}

#[test]
fn readonly_system_tool_preparation_accepts_apples_symlink_without_running_reset() {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
    let prepared = Prepared::prepare(&home).unwrap();
    prepared.validate().unwrap();
    let cancelled = job(false);
    let result = prepared.run(Step::ThumbnailCache, &cancelled, || {
        Err("no durable intent".into())
    });
    assert_eq!(result.outcome, Outcome::NotStarted);
    assert_eq!(result.reason, "gateRefused");
    let result = prepared.run(Step::Generators, &cancelled, || {
        cancelled.cancel();
        Ok(())
    });
    assert_eq!(result.outcome, Outcome::NotStarted);
    assert_eq!(result.stop, Some(Stop::Cancelled));
}

#[test]
fn unwinding_collects_only_its_own_unreaped_child() {
    let job = job(false);
    let mut pid = 0;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        supervise(
            Step::Generators,
            shell("sleep 30"),
            &job,
            TIMEOUT,
            GRACE,
            |spawned| {
                pid = spawned;
                panic!("fixture failure after spawn")
            },
        );
    }));
    assert!(result.is_err());
    assert_eq!(job.process_id(), None);
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn lost_child_ownership_is_uncertain_and_not_signalled_or_reaped_again() {
    let job = job(false);
    let result = supervise(
        Step::Generators,
        shell("exit 0"),
        &job,
        TIMEOUT,
        GRACE,
        |pid| {
            // Deliberately simulate an unexpected reaper. It is now unsafe to
            // signal the numeric PID/PGID, even though its exit was successful.
            let mut status = 0;
            assert_eq!(
                unsafe { libc::waitpid(pid as i32, &mut status, 0) },
                pid as i32
            );
        },
    );
    assert_eq!(result.outcome, Outcome::Uncertain);
    assert_eq!(result.reason, "childOwnershipUnknown");
    assert_eq!(result.exit_code, None);
    assert_eq!(job.process_id(), None);
}

#[test]
fn registration_conflict_collects_only_the_new_child_and_keeps_the_original_lease() {
    use std::os::unix::process::ExitStatusExt;
    struct Guard(Child);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut first = Guard(
        Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap(),
    );
    let job = job(false);
    let lease = job.register(first.0.id()).unwrap();
    let result = supervise(
        Step::Generators,
        shell("exec sleep 30"),
        &job,
        TIMEOUT,
        GRACE,
        |_| panic!("conflicting registration must not enter the supervisor"),
    );
    assert_eq!(result.outcome, Outcome::Uncertain);
    assert_eq!(result.reason, "childRegistrationFailed");
    assert_eq!(result.stop, Some(Stop::SupervisionFailed));
    assert_eq!(job.process_id(), Some(first.0.id()));
    assert!(first.0.try_wait().unwrap().is_none());
    job.release(first.0.id(), &lease, Some(libc::SIGKILL));
    assert_eq!(first.0.wait().unwrap().signal(), Some(libc::SIGKILL));
    assert_eq!(job.process_id(), None);
}

#[test]
fn pre_cancelled_preparation_does_not_enter_the_intent_gate() {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
    let prepared = Prepared::prepare(&home).unwrap();
    let result = prepared.run(Step::Generators, &job(true), || {
        panic!("cancelled work must not create an intent")
    });
    assert_eq!(result.outcome, Outcome::NotStarted);
    assert_eq!(result.stop, Some(Stop::Cancelled));
}

#[test]
fn shared_exit_force_stop_reaches_owned_child_before_supervisor_resumes() {
    let job = job(false);
    let mut pid = 0;
    let result = supervise(
        Step::Generators,
        shell("exec sleep 30"),
        &job,
        TIMEOUT,
        GRACE,
        |spawned| {
            pid = spawned;
            assert_eq!(job.process_id(), Some(spawned));
            // The same operation used by shutdown expiry must work while this
            // supervisor has not resumed polling cancellation.
            job.kill_group();
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
                let rc = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        spawned,
                        info.as_mut_ptr(),
                        libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                    )
                };
                assert_eq!(rc, 0);
                if crate::process::terminal_child_event(spawned, &unsafe { info.assume_init() }) {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(POLL);
            }
            assert_eq!(job.process_id(), None);
        },
    );
    assert_eq!(result.outcome, Outcome::Uncertain);
    assert_eq!(job.process_id(), None);
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}
