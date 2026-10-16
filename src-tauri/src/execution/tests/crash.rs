//! Real SIGKILL at committed boundaries. All paths/DBs belong to this test's
//! TempDir; this driver is compiled only into the Rust test binary, never App.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    ffi::CString,
    fs::OpenOptions,
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{fs::OpenOptionsExt, process::ExitStatusExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const ROOT_ENV: &str = "MOOMOLE_EXECUTION_CRASH_ROOT";
const NONCE_ENV: &str = "MOOMOLE_EXECUTION_CRASH_NONCE";

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
enum Point {
    Begun,
    Intent,
    Moved,
    Result,
    NextIntent,
    AllResults,
    Finished,
    Cancelled,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Control {
    nonce: String,
    plan_id: String,
    point: Point,
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn barrier(root: &Path, run: &str) {
    let mut marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("ready.tmp"))
        .unwrap();
    marker.write_all(run.as_bytes()).unwrap();
    marker.sync_all().unwrap();
    std::fs::rename(root.join("ready.tmp"), root.join("ready")).unwrap();
    // Parent owns the write end. EOF also terminates a stranded fixture child.
    let mut byte = [0];
    std::io::stdin().read_exact(&mut byte).unwrap();
}

fn move_fixture(root: &Path, item: &review::ScanItem) -> Receipt {
    let source = Path::new(&item.candidate.path);
    assert_eq!(source.parent(), Some(root.join("source").as_path()));
    let before = std::fs::symlink_metadata(source).unwrap();
    assert!(before.is_file() && before.nlink() == 1);
    let directory = |path: PathBuf| {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .unwrap()
    };
    let from = directory(root.join("source"));
    let to = directory(root.join("recovery"));
    let name = CString::new(source.file_name().unwrap().to_str().unwrap()).unwrap();
    // Only these private, direct-child regular files are ever moved. This does
    // NOT solve the source-name race or implement native Trash authorization.
    assert_eq!(
        unsafe {
            libc::renameatx_np(
                from.as_raw_fd(),
                name.as_ptr(),
                to.as_raw_fd(),
                name.as_ptr(),
                libc::RENAME_EXCL,
            )
        },
        0
    );
    let destination = root.join("recovery").join(source.file_name().unwrap());
    let after = std::fs::symlink_metadata(&destination).unwrap();
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    Receipt {
        destination: destination.to_str().unwrap().into(),
        device: after.dev(),
        inode: after.ino(),
    }
}

#[test]
fn worker() {
    let Some(root) = std::env::var_os(ROOT_ENV) else {
        return;
    };
    let root = PathBuf::from(root);
    assert!(root.is_absolute() && root == root.canonicalize().unwrap());
    let metadata = std::fs::symlink_metadata(&root).unwrap();
    assert!(
        metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o077 == 0
    );
    let control: Control =
        serde_json::from_slice(&std::fs::read(root.join("control.json")).unwrap()).unwrap();
    assert_eq!(control.nonce, std::env::var(NONCE_ENV).unwrap());
    uuid::Uuid::parse_str(&control.nonce).unwrap();
    let mut database = db::open_owned(&root.join("app.sqlite")).unwrap();
    let plan = db::plan(&database, &control.plan_id).unwrap();
    assert_eq!(plan.items.len(), 2);
    for item in &plan.items {
        assert_eq!(
            Path::new(&item.candidate.path).parent(),
            Some(root.join("source").as_path())
        );
    }
    let at = db::now();
    let run = begin(&mut database, &plan, at).unwrap();
    if matches!(control.point, Point::Begun) {
        barrier(&root, &run);
        return;
    }
    intent(&mut database, &run, &plan.items[0].candidate.id, at + 1).unwrap();
    if matches!(control.point, Point::Intent) {
        barrier(&root, &run);
        return;
    }
    let receipt = move_fixture(&root, &plan.items[0]);
    if matches!(control.point, Point::Moved) {
        barrier(&root, &run);
        return;
    }
    complete_item(
        &mut database,
        &run,
        &plan.items[0].candidate.id,
        Outcome::Moved(receipt),
        "private fixture identity verified",
        at + 2,
    )
    .unwrap();
    if matches!(control.point, Point::Result) {
        barrier(&root, &run);
        return;
    }
    if matches!(control.point, Point::Cancelled) {
        finish(&mut database, &run, true, at + 3).unwrap();
        barrier(&root, &run);
        return;
    }
    intent(&mut database, &run, &plan.items[1].candidate.id, at + 3).unwrap();
    if matches!(control.point, Point::NextIntent) {
        assert!(
            finish(&mut database, &run, true, at + 4).is_err(),
            "in-flight intent must be collected before cancellation"
        );
        barrier(&root, &run);
        return;
    }
    let receipt = move_fixture(&root, &plan.items[1]);
    complete_item(
        &mut database,
        &run,
        &plan.items[1].candidate.id,
        Outcome::Moved(receipt),
        "private fixture identity verified",
        at + 4,
    )
    .unwrap();
    if matches!(control.point, Point::Finished) {
        finish(&mut database, &run, false, at + 5).unwrap();
    }
    barrier(&root, &run);
}

struct Crashed {
    // TempDir outlives both process and database. ChildGuard kills/reaps even if
    // an assertion fails, preventing any worker from outliving its fixture.
    _root: tempfile::TempDir,
    root: PathBuf,
    plan: Plan,
    run: String,
    before: serde_json::Value,
    lock_inode: u64,
}

fn crash(point: Point) -> Crashed {
    let Fixture {
        _root,
        root,
        db,
        plan,
        ..
    } = Fixture::new();
    drop(db);
    let nonce = uuid::Uuid::new_v4().to_string();
    let control = Control {
        nonce: nonce.clone(),
        plan_id: plan.id.clone(),
        point,
    };
    std::fs::write(
        root.join("control.json"),
        serde_json::to_vec(&control).unwrap(),
    )
    .unwrap();
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "execution::tests::crash::worker", "--nocapture"])
            .env(ROOT_ENV, &root)
            .env(NONCE_ENV, nonce)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !root.join("ready").exists() {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "fixture worker exited before {point:?}"
        );
        assert!(
            Instant::now() < deadline,
            "fixture worker did not reach {point:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let run = std::fs::read_to_string(root.join("ready")).unwrap();
    let lock_inode = std::fs::metadata(root.join(crate::instance::LOCK_NAME))
        .unwrap()
        .ino();
    uuid::Uuid::parse_str(&run).unwrap();
    assert!(matches!(
        db::open_owned(&root.join("app.sqlite")),
        Err(db::OpenError::Instance(crate::instance::Error::Busy))
    ));
    let reader = Connection::open_with_flags(
        root.join("app.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let before = serde_json::to_value(detail(&reader, &run).unwrap()).unwrap();
    drop(reader);
    child.0.kill().unwrap();
    assert_eq!(child.0.wait().unwrap().signal(), Some(libc::SIGKILL));
    Crashed {
        _root,
        root,
        plan,
        run,
        before,
        lock_inode,
    }
}

fn file_state(root: &Path) -> Vec<(bool, String)> {
    ["中文\ncache *", "second", "unselected"]
        .iter()
        .flat_map(|name| {
            ["source", "recovery"].iter().map(move |parent| {
                let path = root.join(parent).join(name);
                (
                    path.exists(),
                    std::fs::read_to_string(path).unwrap_or_default(),
                )
            })
        })
        .collect()
}

#[test]
fn sigkill_recovery_never_replays_and_preserves_committed_receipts() {
    for (point, states, moved, uncertain, events_before) in [
        (Point::Begun, ["notStarted", "notStarted"], 0, 0, 0),
        (Point::Intent, ["uncertain", "notStarted"], 0, 1, 1),
        (Point::Moved, ["uncertain", "notStarted"], 0, 1, 1),
        (Point::Result, ["moved", "notStarted"], 1, 0, 2),
        (Point::NextIntent, ["moved", "uncertain"], 1, 1, 3),
        (Point::AllResults, ["moved", "moved"], 2, 0, 4),
    ] {
        let f = crash(point);
        assert_eq!(f.before["state"], "running", "{point:?}");
        assert_eq!(f.before["events"].as_array().unwrap().len(), events_before);
        let files = file_state(&f.root);
        let moved_on_disk = match point {
            Point::Begun | Point::Intent => 0,
            Point::AllResults => 2,
            _ => 1,
        };
        for (i, item) in f.plan.items.iter().enumerate() {
            let source = Path::new(&item.candidate.path);
            let target = f.root.join("recovery").join(source.file_name().unwrap());
            assert_eq!(source.exists(), i >= moved_on_disk, "source at {point:?}");
            assert_eq!(
                target.exists(),
                i < moved_on_disk,
                "destination at {point:?}"
            );
            let remaining = if i < moved_on_disk {
                target.as_path()
            } else {
                source
            };
            assert_eq!(
                std::fs::read_to_string(remaining).unwrap(),
                source.file_name().unwrap().to_str().unwrap()
            );
        }
        assert_eq!(
            std::fs::read_to_string(f.root.join("source/unselected")).unwrap(),
            "unselected"
        );
        let mut database = db::open_owned(&f.root.join("app.sqlite")).unwrap();
        assert_eq!(
            std::fs::metadata(f.root.join(crate::instance::LOCK_NAME))
                .unwrap()
                .ino(),
            f.lock_inode
        );
        let recovered = detail(&database, &f.run).unwrap();
        assert_eq!(recovered.summary.state, "interrupted", "{point:?}");
        assert_eq!(
            recovered
                .items
                .iter()
                .map(|i| i.state.as_str())
                .collect::<Vec<_>>(),
            states
        );
        assert_eq!(recovered.summary.moved_count, moved);
        assert_eq!(recovered.summary.uncertain_count, uncertain);
        assert!(recovered.summary.retained_for_review);
        assert_eq!(
            file_state(&f.root),
            files,
            "startup must perform no target IO"
        );
        // A moved-but-unrecorded item stays uncertain with NO invented receipt.
        if matches!(point, Point::Moved) {
            assert!(!Path::new(&f.plan.items[0].candidate.path).exists());
            assert!(recovered.items[0].receipt.is_none());
        }
        let before_events = f.before["events"].as_array().unwrap();
        let value = serde_json::to_value(&recovered).unwrap();
        assert_eq!(
            &value["events"].as_array().unwrap()[..before_events.len()],
            before_events.as_slice()
        );
        for item in recovered.items.iter().filter(|i| i.state == "moved") {
            let receipt = item.receipt.as_ref().unwrap();
            let metadata = std::fs::symlink_metadata(&receipt.destination).unwrap();
            assert_eq!(
                (metadata.dev(), metadata.ino()),
                (receipt.device, receipt.inode)
            );
        }
        assert!(
            begin(&mut database, &f.plan, db::now()).is_err(),
            "same plan must not replay"
        );
        let mut other = f.plan.clone();
        other.id = uuid::Uuid::new_v4().to_string();
        if uncertain > 0 {
            assert!(begin(&mut database, &other, db::now())
                .unwrap_err()
                .contains("待核实"));
        }
        drop(database);
        let database = db::open_owned(&f.root.join("app.sqlite")).unwrap();
        assert_eq!(
            serde_json::to_value(detail(&database, &f.run).unwrap()).unwrap(),
            value,
            "reconciliation must be idempotent"
        );
        assert_eq!(file_state(&f.root), files);
    }
}

#[test]
fn committed_completion_and_cancellation_survive_sigkill_without_new_recovery_events() {
    for (point, state, moved, item_states) in [
        (Point::Finished, "completed", 2, ["moved", "moved"]),
        (Point::Cancelled, "cancelled", 1, ["moved", "cancelled"]),
    ] {
        let f = crash(point);
        let files = file_state(&f.root);
        let mut database = db::open_owned(&f.root.join("app.sqlite")).unwrap();
        let recovered = detail(&database, &f.run).unwrap();
        assert_eq!(recovered.summary.state, state);
        assert_eq!(recovered.summary.moved_count, moved);
        assert_eq!(
            recovered
                .items
                .iter()
                .map(|i| i.state.as_str())
                .collect::<Vec<_>>(),
            item_states
        );
        assert_eq!(serde_json::to_value(&recovered).unwrap(), f.before);
        assert_eq!(file_state(&f.root), files);
        assert!(begin(&mut database, &f.plan, db::now()).is_err());
    }
}
