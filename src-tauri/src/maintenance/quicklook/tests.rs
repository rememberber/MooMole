use super::*;

fn shell(script: &str) -> Command {
    // Test-only harmless processes. Production accepts only the Step enum.
    let mut command = Command::new("/bin/sh");
    command.args(["-c", script]);
    command
}
fn run(script: &str, cancelled: &AtomicBool) -> Attempt {
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
    let result = run("exit 0", &AtomicBool::new(false));
    assert_eq!(result.outcome, Outcome::CommandSucceeded);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.stop, None);
    assert_eq!(result.reason, "commandSucceeded");
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
        &AtomicBool::new(false),
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
    let result = run("exit 0", &AtomicBool::new(true));
    assert_eq!(result.outcome, Outcome::NotStarted);
    assert_eq!(result.stop, Some(Stop::Cancelled));
    assert_eq!(result.exit_code, None);
}

#[test]
fn spawn_failure_is_distinct_from_a_started_failure() {
    let result = supervise(
        Step::Generators,
        Command::new("/no-such-moomole-quicklook-fixture"),
        &AtomicBool::new(false),
        TIMEOUT,
        GRACE,
        |_| panic!("must not report a spawned child"),
    );
    assert_eq!(result.outcome, Outcome::NotStarted);
    assert_eq!(result.reason, "spawnFailed");
}

#[test]
fn cancellation_after_spawn_is_uncertain_and_the_child_is_reaped() {
    let cancelled = AtomicBool::new(false);
    let mut pid = 0;
    let result = supervise(
        Step::Generators,
        shell("sleep 30"),
        &cancelled,
        TIMEOUT,
        Duration::from_millis(40),
        |spawned| {
            pid = spawned;
            cancelled.store(true, Ordering::SeqCst);
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
        &AtomicBool::new(false),
        Duration::from_millis(100),
        Duration::from_millis(40),
        |_| {},
    );
    assert_eq!(result.outcome, Outcome::Uncertain);
    assert_eq!(result.stop, Some(Stop::TimedOut));
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn a_known_success_is_preserved_when_cancel_arrives_after_exit() {
    let cancelled = AtomicBool::new(false);
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
                if unsafe { info.assume_init() }.si_pid == pid as i32 {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(POLL);
            }
            cancelled.store(true, Ordering::SeqCst);
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
        &AtomicBool::new(false),
    );
    assert_eq!(result.outcome, Outcome::CommandSucceeded);
}

#[test]
fn readonly_system_tool_preparation_accepts_apples_symlink_without_running_reset() {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
    let prepared = Prepared::prepare(&home).unwrap();
    prepared.validate().unwrap();
    let cancelled = AtomicBool::new(false);
    let result = prepared.run(Step::ThumbnailCache, &cancelled, || {
        Err("no durable intent".into())
    });
    assert_eq!(result.outcome, Outcome::NotStarted);
    assert_eq!(result.reason, "gateRefused");
    let result = prepared.run(Step::Generators, &cancelled, || {
        cancelled.store(true, Ordering::SeqCst);
        Ok(())
    });
    assert_eq!(result.outcome, Outcome::NotStarted);
    assert_eq!(result.stop, Some(Stop::Cancelled));
}

#[test]
fn unwinding_collects_only_its_own_unreaped_child() {
    let mut pid = 0;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        supervise(
            Step::Generators,
            shell("sleep 30"),
            &AtomicBool::new(false),
            TIMEOUT,
            GRACE,
            |spawned| {
                pid = spawned;
                panic!("fixture failure after spawn")
            },
        );
    }));
    assert!(result.is_err());
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
    let result = supervise(
        Step::Generators,
        shell("exit 0"),
        &AtomicBool::new(false),
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
}

#[test]
fn pre_cancelled_preparation_does_not_enter_the_intent_gate() {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
    let prepared = Prepared::prepare(&home).unwrap();
    let result = prepared.run(Step::Generators, &AtomicBool::new(true), || {
        panic!("cancelled work must not create an intent")
    });
    assert_eq!(result.outcome, Outcome::NotStarted);
    assert_eq!(result.stop, Some(Stop::Cancelled));
}
