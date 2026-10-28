//! The exact exit controller used by both Tauri paths, with the actual log loop.
//! Positive rules remain fixtures. Native operations touch only our fresh logs.
use super::*;
use crate::shutdown::{Controller, Outcome, Request};
use std::sync::atomic::AtomicBool;

#[tokio::test]
#[ignore = "Exit drain + actual Foundation Trash on private HOME; restores exact receipts before assertions"]
async fn native_exit_collects_inflight_result_and_cancels_later_items_before_drain() {
    exit_matrix(false).await;
}

#[tokio::test]
#[ignore = "Exit drain + native cache-directory Trash on private HOME; exact receipts restored before assertions"]
async fn native_cache_exit_collects_inflight_result_and_cancels_later_items_before_drain() {
    exit_matrix(true).await;
}

async fn exit_matrix(directory: bool) {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    for phase in [
        Phase::Begun,
        Phase::Intent,
        Phase::NativeReturned,
        Phase::ItemCommitted,
    ] {
        let mut f = if directory {
            Fixture::new_cache()
        } else {
            Fixture::new()
        };
        f.root.disable_cleanup(true);
        eprintln!(
            "Native exit fixture {phase:?} (preserved on failure): {}",
            f.home.display()
        );
        let controller = Arc::new(Controller::default());
        let checker = Arc::new(crate::updates::Checker::default());
        let operation = checker.installation.enter().unwrap();
        let job = Arc::new(Job::new(TaskKind::LogTrash));
        let jobs = Arc::new(Mutex::new(HashMap::from([(
            "own-request".into(),
            job.clone(),
        )])));
        let observations = Arc::new(Mutex::new(Vec::new()));
        let returned = Arc::new(Mutex::new(Vec::<execution::Receipt>::new()));
        let root = f.home.clone();
        let seen = AtomicBool::new(false);
        let (c, u, j, o, r) = (
            controller.clone(),
            checker.clone(),
            jobs.clone(),
            observations.clone(),
            returned.clone(),
        );
        let hook: Checkpoint = Arc::new(move |at, _, receipt| {
            if let Some(receipt) = receipt {
                let mut values = r.lock().unwrap();
                values.push(receipt.clone());
                std::fs::write(
                    root.join("exit-rescue.json"),
                    serde_json::to_vec_pretty(&*values).unwrap(),
                )
                .unwrap();
            }
            if at == phase && !seen.swap(true, Ordering::SeqCst) {
                let request = c.request(&j, &u);
                let duplicate = c.request(&j, &u);
                let poll = c.poll(&j, &u);
                o.lock().unwrap().push((request, duplicate, poll));
            }
        });
        let result = run_observed(
            f.db.clone(),
            f.home.clone(),
            f.authorize(),
            job.clone(),
            |request, _| async move { Ok(clear(request)) },
            Some(hook),
        )
        .await;
        let before_remove = controller.poll(&jobs, &checker);
        let records = execution::list(&*f.db.lock().unwrap()).unwrap();
        let details = records
            .iter()
            .map(|r| execution::detail(&*f.db.lock().unwrap(), &r.id).unwrap())
            .collect::<Vec<_>>();
        std::fs::write(
            f.home.join("exit-results.json"),
            serde_json::to_vec_pretty(&details).unwrap(),
        )
        .unwrap();
        // Restore every exact native return, even if an earlier assertion would
        // otherwise fail. Source is found only among this fixture's selections.
        for receipt in returned.lock().unwrap().iter() {
            let metadata = std::fs::symlink_metadata(&receipt.destination).unwrap();
            assert_eq!(
                (metadata.dev(), metadata.ino()),
                (receipt.device, receipt.inode)
            );
            let item = f
                .plan
                .items
                .iter()
                .find(|item| {
                    item.witness.as_ref().is_some_and(|w| {
                        let node = w.nodes.last().unwrap();
                        node.dev == receipt.device && node.ino == receipt.inode
                    })
                })
                .unwrap();
            assert!(!Path::new(&item.candidate.path).exists());
            objc2::rc::autoreleasepool(|_| {
                NSFileManager::defaultManager()
                    .moveItemAtURL_toURL_error(
                        &NSURL::fileURLWithPath(&NSString::from_str(&receipt.destination)),
                        &NSURL::fileURLWithPath(&NSString::from_str(&item.candidate.path)),
                    )
                    .unwrap();
            });
        }
        let detail = result.unwrap();
        assert_eq!(
            *observations.lock().unwrap(),
            vec![(Request::Drain, Request::Waiting, Outcome::Waiting)]
        );
        assert!(controller.closing());
        assert!(job.cancelled.load(Ordering::SeqCst));
        assert_eq!(
            before_remove,
            Outcome::Waiting,
            "results persisted does not yet remove the worker handle"
        );
        assert_eq!(detail.summary.state, "cancelled");
        assert_eq!(
            detail.summary.moved_count,
            if phase == Phase::Begun { 0 } else { 1 }
        );
        assert_eq!(detail.items[1].state, "cancelled");
        assert!(
            checker.installation.begin().is_err(),
            "worker must retain its admission during result collection"
        );
        jobs.lock().unwrap().remove("own-request");
        drop(operation);
        assert_eq!(controller.poll(&jobs, &checker), Outcome::Drained);
        assert_eq!(controller.request(&jobs, &checker), Request::Exit);
        let mut values = std::fs::read_dir(f.home.join(if directory {
            "Library/Caches"
        } else {
            "Library/Logs"
        }))
        .unwrap()
        .map(|e| {
            let path = e.unwrap().path();
            std::fs::read_to_string(if directory {
                path.join("deep/payload")
            } else {
                path
            })
            .unwrap()
        })
        .collect::<Vec<_>>();
        values.sort();
        assert_eq!(values, ["first.log", "second.log", "unselected.log"]);
        assert_eq!(
            serde_json::to_value(
                execution::detail(&*f.db.lock().unwrap(), &detail.summary.id).unwrap()
            )
            .unwrap(),
            serde_json::to_value(detail).unwrap()
        );
        f.root.disable_cleanup(false);
    }
}
