use super::*;
use crate::engine::TaskKind;
use std::{
    os::unix::process::ExitStatusExt,
    process::{Command, Stdio},
    sync::atomic::Ordering,
};

#[test]
fn both_exit_paths_share_one_deadline_and_never_claim_a_native_job_finished() {
    let shutdown = Controller::default();
    let checker = updates::Checker::default();
    let job = Arc::new(Job::new(TaskKind::LogTrash));
    let jobs = Mutex::new(HashMap::from([("request".into(), job.clone())]));
    let now = Instant::now();
    assert_eq!(shutdown.request_at(&jobs, &checker, now), Request::Drain);
    assert!(shutdown.closing());
    assert!(job.cancelled.load(Ordering::SeqCst));
    assert_eq!(
        shutdown.request_at(&jobs, &checker, now + Duration::from_secs(5)),
        Request::Waiting
    );
    assert_eq!(
        shutdown.poll_at(&jobs, &checker, now + Duration::from_secs(5)),
        Outcome::Waiting
    );
    assert_eq!(
        shutdown.poll_at(&jobs, &checker, now + GRACE),
        Outcome::TimedOut
    );
    assert_eq!(
        shutdown.request_at(&jobs, &checker, now + Duration::from_secs(7)),
        Request::Exit
    );
    assert_eq!(
        shutdown.poll_at(&jobs, &checker, now + Duration::from_secs(100)),
        Outcome::TimedOut
    );
    assert_eq!(
        jobs.lock().unwrap().len(),
        1,
        "timeout must not erase a live task or invent completion"
    );
}
#[test]
fn idle_exit_closes_admission_and_a_completed_worker_is_drained_before_deadline() {
    for active in [false, true] {
        let controller = Controller::default();
        let jobs = Mutex::new(HashMap::new());
        let checker = updates::Checker::default();
        assert!(!controller.closing());
        if active {
            jobs.lock()
                .unwrap()
                .insert("job".into(), Arc::new(Job::new(TaskKind::Status)));
        }
        assert_eq!(
            controller.request(&jobs, &checker),
            if active {
                Request::Drain
            } else {
                Request::Exit
            }
        );
        assert!(controller.closing());
        if active {
            assert_eq!(controller.poll(&jobs, &checker), Outcome::Waiting);
            jobs.lock().unwrap().remove("job");
        }
        assert_eq!(controller.poll(&jobs, &checker), Outcome::Drained);
        assert_eq!(controller.request(&jobs, &checker), Request::Exit);
    }
}
struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn expiry_stops_only_the_registered_child_group_and_releases_its_lease() {
    use std::os::unix::process::CommandExt;
    let mut child = ChildGuard(
        Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let job = Arc::new(Job::new(TaskKind::Status));
    let lease = job.register(child.0.id()).unwrap();
    let jobs = Mutex::new(HashMap::from([("job".into(), job.clone())]));
    let checker = updates::Checker::default();
    let controller = Controller::default();
    let now = Instant::now();
    assert_eq!(controller.request_at(&jobs, &checker, now), Request::Drain);
    assert_eq!(
        controller.poll_at(&jobs, &checker, now + GRACE),
        Outcome::TimedOut
    );
    assert_eq!(child.0.wait().unwrap().signal(), Some(libc::SIGKILL));
    assert_eq!(job.process_id(), None);
    job.signal(child.0.id(), &lease, libc::SIGKILL); // stale lease does nothing.
    assert_eq!(controller.poll(&jobs, &checker), Outcome::TimedOut);
}
