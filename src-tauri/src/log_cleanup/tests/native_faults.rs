//! Test-only checkpoints in the actual log loop. No hooks or environment switch
//! are present in the App build. Native operations use only fresh private logs.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    os::{
        macos::fs::MetadataExt as _,
        unix::{fs::PermissionsExt, process::ExitStatusExt},
    },
    process::{Child, Command, Stdio},
};
const ROOT: &str = "MOOMOLE_LOG_CRASH_ROOT";
const NONCE: &str = "MOOMOLE_LOG_CRASH_NONCE";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Control {
    nonce: String,
    plan: String,
    phase: Phase,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Rescue {
    source: String,
    receipt: execution::Receipt,
}
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn publish(path: &Path, value: &impl Serialize) {
    let temporary = path.with_extension("tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    file.sync_all().unwrap();
    std::fs::rename(temporary, path).unwrap();
}
fn restore(rescues: &[Rescue]) {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    for rescue in rescues {
        let metadata = std::fs::symlink_metadata(&rescue.receipt.destination).unwrap();
        assert!(metadata.is_file() && metadata.nlink() == 1);
        assert_eq!(
            (metadata.dev(), metadata.ino()),
            (rescue.receipt.device, rescue.receipt.inode)
        );
        assert!(
            !Path::new(&rescue.source).exists(),
            "never overwrite either copy"
        );
        objc2::rc::autoreleasepool(|_| {
            let from = NSURL::fileURLWithPath(&NSString::from_str(&rescue.receipt.destination));
            let to = NSURL::fileURLWithPath(&NSString::from_str(&rescue.source));
            NSFileManager::defaultManager()
                .moveItemAtURL_toURL_error(&from, &to)
                .unwrap();
        });
    }
}
fn assert_fixture_contents(home: &Path) {
    let mut contents = std::fs::read_dir(home.join("Library/Logs"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            assert!(path.is_file());
            std::fs::read_to_string(path).unwrap()
        })
        .collect::<Vec<_>>();
    contents.sort();
    assert_eq!(contents, ["first.log", "second.log", "unselected.log"]);
}

fn report(plan: &review::Plan) -> cleanup_preparation::Report {
    let now = db::now();
    cleanup_preparation::Report {
        version: 1,
        plan_id: plan.id.clone(),
        scope: "userLogFiles",
        method: "trash",
        read_only: true,
        checked_at: now,
        expires_at: now + 30_000,
        observed_count: 2,
        items: plan
            .items
            .iter()
            .map(|item| cleanup_preparation::Item {
                id: item.candidate.id.clone(),
                code: "observed",
                reason: "explicit isolated fixture".into(),
            })
            .collect(),
    }
}

#[tokio::test]
#[ignore = "Child of explicit native log crash matrix; environment/nonce/private root required"]
async fn worker() {
    let Some(value) = std::env::var_os(ROOT) else {
        return;
    };
    let home = PathBuf::from(value);
    assert_eq!(home, home.canonicalize().unwrap());
    assert!(home.starts_with(std::env::temp_dir().canonicalize().unwrap()));
    assert!(home
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("MooMole-log-executor-"));
    let metadata = std::fs::symlink_metadata(&home).unwrap();
    assert!(
        metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o077 == 0
    );
    let control: Control =
        serde_json::from_slice(&std::fs::read(home.join("control.json")).unwrap()).unwrap();
    assert_eq!(control.nonce, std::env::var(NONCE).unwrap());
    uuid::Uuid::parse_str(&control.nonce).unwrap();
    let database = Arc::new(Mutex::new(
        db::open_owned(&home.join("app.sqlite")).unwrap(),
    ));
    let plan = db::plan(&*database.lock().unwrap(), &control.plan).unwrap();
    assert_eq!(plan.items.len(), 2);
    for item in &plan.items {
        assert_eq!(
            Path::new(&item.candidate.path).parent(),
            Some(home.join("Library/Logs").as_path())
        );
    }
    let mut offers = Confirmations::default();
    let offer = offers
        .issue(&plan, &report(&plan), db::now())
        .unwrap()
        .unwrap();
    let authorized = offers.take(&offer.id, db::now()).unwrap();
    let root = home.clone();
    let rescues = Mutex::new(Vec::<Rescue>::new());
    let hook: Checkpoint = Arc::new(move |phase, run, receipt| {
        if let Some(receipt) = receipt {
            let item = plan
                .items
                .iter()
                .find(|item| {
                    item.witness.as_ref().is_some_and(|w| {
                        let node = w.nodes.last().unwrap();
                        node.dev == receipt.device && node.ino == receipt.inode
                    })
                })
                .expect("receipt must belong to a selected fixture inode");
            let mut rescues = rescues.lock().unwrap();
            rescues.push(Rescue {
                source: item.candidate.path.clone(),
                receipt: receipt.clone(),
            });
            publish(&root.join("rescue.json"), &*rescues);
        }
        if phase == control.phase {
            publish(&root.join("ready.json"), &run);
            let mut byte = [0];
            std::io::stdin().read_exact(&mut byte).unwrap();
            panic!("the parent must terminate this fixture worker");
        }
    });
    run_observed(
        database,
        home,
        authorized,
        Arc::new(Job::new(TaskKind::LogTrash)),
        |request, _| async move { Ok(clear(request)) },
        Some(hook),
    )
    .await
    .unwrap();
    panic!("fixture worker did not reach its checkpoint");
}

#[test]
#[ignore = "SIGKILL actual log loop at five boundaries with real Trash; exact rescue receipts restored; fixtures only"]
fn sigkill_native_log_loop_preserves_results_without_replay() {
    for (phase, expected, moved, uncertain) in [
        (Phase::Begun, ["notStarted", "notStarted"], 0, 0),
        (Phase::Intent, ["uncertain", "notStarted"], 0, 1),
        (Phase::NativeReturned, ["uncertain", "notStarted"], 0, 1),
        (Phase::ItemCommitted, ["moved", "notStarted"], 1, 0),
        (Phase::Finished, ["moved", "moved"], 2, 0),
    ] {
        let Fixture {
            mut root,
            home,
            plan,
            db: database,
        } = Fixture::new();
        root.disable_cleanup(true);
        eprintln!(
            "Native log crash fixture {phase:?} (preserved on failure): {}",
            home.display()
        );
        drop(database);
        let nonce = uuid::Uuid::new_v4().to_string();
        publish(
            &home.join("control.json"),
            &Control {
                nonce: nonce.clone(),
                plan: plan.id.clone(),
                phase,
            },
        );
        let mut child = ChildGuard(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "log_cleanup::tests::native_faults::worker",
                    "--nocapture",
                ])
                .env(ROOT, &home)
                .env(NONCE, nonce)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        while !home.join("ready.json").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "worker exited before {phase:?}"
            );
            assert!(Instant::now() < deadline, "worker did not reach {phase:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
        let run: String =
            serde_json::from_slice(&std::fs::read(home.join("ready.json")).unwrap()).unwrap();
        assert!(matches!(
            db::open_owned(&home.join("app.sqlite")),
            Err(db::OpenError::Instance(crate::instance::Error::Busy))
        ));
        child.0.kill().unwrap();
        assert_eq!(child.0.wait().unwrap().signal(), Some(libc::SIGKILL));
        let rescues: Vec<Rescue> = if home.join("rescue.json").exists() {
            serde_json::from_slice(&std::fs::read(home.join("rescue.json")).unwrap()).unwrap()
        } else {
            vec![]
        };
        let disk_state = || {
            plan.items
                .iter()
                .map(|item| std::fs::read(&item.candidate.path).ok())
                .collect::<Vec<_>>()
        };
        let before = disk_state();
        let mut database = db::open_owned(&home.join("app.sqlite")).unwrap();
        let recovered = execution::detail(&database, &run).unwrap();
        let unchanged = disk_state() == before;
        publish(&home.join("recovered.json"), &recovered);
        // Rescue only exact test receipts, including a receipt that SQLite
        // deliberately has not recorded. Startup itself must not infer it.
        restore(&rescues);
        assert!(unchanged, "startup must perform no target IO");
        assert_eq!(
            recovered.summary.state,
            if phase == Phase::Finished {
                "completed"
            } else {
                "interrupted"
            }
        );
        assert_eq!(
            recovered
                .items
                .iter()
                .map(|item| item.state.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(recovered.summary.moved_count, moved);
        assert_eq!(recovered.summary.uncertain_count, uncertain);
        if phase == Phase::NativeReturned {
            assert!(before[0].is_none());
            assert_eq!(rescues.len(), 1);
            assert!(
                recovered.items[0].receipt.is_none(),
                "no invented completion receipt"
            );
        }
        assert!(execution::begin(&mut database, &plan, db::now()).is_err());
        let once = serde_json::to_value(&recovered).unwrap();
        drop(database);
        let database = db::open_owned(&home.join("app.sqlite")).unwrap();
        assert_eq!(
            serde_json::to_value(execution::detail(&database, &run).unwrap()).unwrap(),
            once
        );
        assert_eq!(
            std::fs::read_dir(home.join("Library/Logs"))
                .unwrap()
                .count(),
            3
        );
        for (item, content) in plan.items.iter().zip(["first.log", "second.log"]) {
            assert_eq!(
                std::fs::read_to_string(&item.candidate.path).unwrap(),
                content
            );
        }
        assert_fixture_contents(&home);
        root.disable_cleanup(false);
    }
}

struct Restriction {
    path: PathBuf,
    kind: &'static str,
}
impl Restriction {
    fn set(path: PathBuf, kind: &'static str) -> Self {
        let result = Self { path, kind };
        match kind {
            "acl" => assert!(Command::new("/bin/chmod")
                .args(["+a", "everyone deny delete_child"])
                .arg(&result.path)
                .status()
                .unwrap()
                .success()),
            "flags" => {
                let path = std::ffi::CString::new(result.path.to_str().unwrap()).unwrap();
                assert_eq!(
                    unsafe { libc::chflags(path.as_ptr(), libc::UF_IMMUTABLE as u32) },
                    0
                );
            }
            "mode" => {
                std::fs::set_permissions(&result.path, std::fs::Permissions::from_mode(0o500))
                    .unwrap()
            }
            _ => panic!("unknown fixture restriction"),
        }
        result
    }
}
impl Drop for Restriction {
    fn drop(&mut self) {
        match self.kind {
            "acl" => assert!(Command::new("/bin/chmod")
                .arg("-N")
                .arg(&self.path)
                .status()
                .unwrap()
                .success()),
            "flags" => {
                let path = std::ffi::CString::new(self.path.to_str().unwrap()).unwrap();
                assert_eq!(unsafe { libc::chflags(path.as_ptr(), 0) }, 0);
            }
            "mode" => std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o755))
                .unwrap(),
            _ => unreachable!(),
        }
    }
}

#[tokio::test]
#[ignore = "Changes permissions on OWN fixture Logs only after durable intent; no user paths; restores restrictions and exact receipts"]
async fn permission_restrictions_after_intent_never_call_trash_or_clear_protection() {
    for kind in ["acl", "flags", "mode"] {
        let mut f = Fixture::new();
        f.root.disable_cleanup(true);
        eprintln!(
            "Native permission fixture {kind} (preserved on failure): {}",
            f.home.display()
        );
        let parent = f.home.join("Library/Logs");
        let restriction = Arc::new(Mutex::new(None));
        let held = restriction.clone();
        let job = Arc::new(Job::new(TaskKind::LogTrash));
        let stop = job.clone();
        let hook: Checkpoint = Arc::new(move |phase, run, _| {
            if phase == Phase::Intent {
                let mut value = held.lock().unwrap();
                if value.is_none() {
                    *value = Some(Restriction::set(parent.clone(), kind));
                }
            }
            if phase == Phase::ItemCommitted {
                assert!(stop.cancel_execution(run));
            }
        });
        let result = run_observed(
            f.db.clone(),
            f.home.clone(),
            f.authorize(),
            job,
            |request, _| async move { Ok(clear(request)) },
            Some(hook),
        )
        .await;
        // Capture evidence without asserting while our restriction is active;
        // always remove our own restriction before assertions or rescue.
        let path = f.home.join("Library/Logs");
        let still_restricted = match kind {
            "acl" => Command::new("/bin/ls")
                .arg("-lde")
                .arg(&path)
                .env("LC_ALL", "C")
                .output()
                .is_ok_and(|r| {
                    r.status.success()
                        && String::from_utf8_lossy(&r.stdout).contains("deny delete_child")
                }),
            "flags" => path
                .metadata()
                .is_ok_and(|m| m.st_flags() & libc::UF_IMMUTABLE != 0),
            "mode" => path.metadata().is_ok_and(|m| m.mode() & 0o777 == 0o500),
            _ => false,
        };
        drop(restriction.lock().unwrap().take());
        let records = execution::list(&*f.db.lock().unwrap()).unwrap();
        let details = records
            .iter()
            .map(|run| execution::detail(&*f.db.lock().unwrap(), &run.id).unwrap())
            .collect::<Vec<_>>();
        publish(&f.home.join("permission-results.json"), &details);
        let rescues: Vec<_> = details
            .iter()
            .flat_map(|run| run.items.iter())
            .filter_map(|item| {
                item.receipt.as_ref().map(|receipt| Rescue {
                    source: item.path.clone(),
                    receipt: receipt.clone(),
                })
            })
            .collect();
        restore(&rescues);
        let detail = result.unwrap();
        assert!(
            still_restricted,
            "executor must never remove the fixture restriction"
        );
        assert_eq!(detail.summary.state, "cancelled");
        assert_eq!(
            detail.items[0].state, "failed",
            "{kind}: {}",
            detail.items[0].reason
        );
        assert!(detail.items[0].reason.contains("未调用废纸篓接口"));
        if kind == "acl" {
            assert!(detail.items[0].reason.contains("ACL"));
            assert!(detail.items[0].reason.contains("重新核对权限"));
        }
        assert!(detail.items.iter().all(|item| item.receipt.is_none()));
        assert_eq!(detail.items[1].state, "cancelled");
        assert_eq!(f.original_files(), 3);
        assert_fixture_contents(&f.home);
        f.root.disable_cleanup(false);
    }
}
