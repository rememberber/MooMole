//! Test-only execution experiment. Every writable path belongs to a freshly
//! created TempDir. This module is excluded from application builds and has no
//! IPC entry point. Its recovery folder is NOT the macOS Trash.
//!
//! In particular, renameatx_np does not bind a source name to an inode. The race
//! test below deliberately demonstrates why this is not a production executor.
use crate::{
    clean_preview::{Candidate, Preview, PROTOCOL},
    review,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    ffi::CString,
    fs::{File, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum State {
    Intent,
    RestoreIntent,
    Moved,
    Skipped,
    Failed,
    Cancelled,
    Interrupted,
    NotStarted,
    Restored,
    RestoreFailed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Event {
    plan: String,
    item: String,
    state: State,
    reason: String,
    file: Option<Stamp>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Stamp {
    dev: u64,
    ino: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    size: u64,
    mtime: i64,
    mtime_ns: i64,
}

impl Stamp {
    fn read(meta: &std::fs::Metadata) -> Result<Self, String> {
        if !meta.is_file() || meta.nlink() != 1 {
            return Err("regular unshared file required".into());
        }
        Ok(Self {
            dev: meta.dev(),
            ino: meta.ino(),
            mode: meta.mode(),
            uid: meta.uid(),
            gid: meta.gid(),
            size: meta.size(),
            mtime: meta.mtime(),
            mtime_ns: meta.mtime_nsec(),
        })
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Fault {
    None,
    BeforeIntent,
    AfterIntent,
    AfterMove,
}

struct Sandbox {
    _temporary: tempfile::TempDir,
    source: PathBuf,
    recovery: PathBuf,
    journal: PathBuf,
    plan: Option<review::Plan>,
}

fn directory(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| e.to_string())
}

fn move_exclusive(from: &File, to: &File, name: &str) -> Result<(), String> {
    let name = CString::new(name).map_err(|e| e.to_string())?;
    // Only called with held descriptors to this fixture's two private folders.
    let result = unsafe {
        libc::renameatx_np(
            from.as_raw_fd(),
            name.as_ptr(),
            to.as_raw_fd(),
            name.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    matches!((Stamp::read(a), Stamp::read(b)), (Ok(a), Ok(b)) if a == b)
}

impl Sandbox {
    fn new(files: &[(&str, &str)]) -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let source = root.join("source");
        let recovery = root.join("recovery");
        for dir in [&source, &recovery] {
            std::fs::create_dir(dir).unwrap();
        }
        for (name, content) in files {
            assert_eq!(Path::new(name).components().count(), 1);
            assert!(matches!(
                Path::new(name).components().next(),
                Some(std::path::Component::Normal(_))
            ));
            std::fs::write(source.join(name), content).unwrap();
        }
        Self {
            _temporary: temporary,
            source,
            recovery,
            journal: root.join("events.ndjson"),
            plan: None,
        }
    }

    fn prepare(&mut self, names: &[&str], selected: &[usize]) -> Result<String, String> {
        let items = names
            .iter()
            .map(|name| {
                let path = self.source.join(name);
                let meta = std::fs::symlink_metadata(&path).unwrap();
                Candidate {
                    id: String::new(),
                    upstream_identity: format!("inode:{}:{}", meta.dev(), meta.ino()),
                    path: path.to_str().unwrap().into(),
                    section: "Isolated fixture".into(),
                    size: Some(meta.size()),
                    item_count: 1,
                    covered_by: None,
                    is_directory: meta.is_dir(),
                    resolved_path: None,
                    impact: None,
                    provenance: None,
                }
            })
            .collect();
        let scan = review::capture(
            uuid::Uuid::new_v4().to_string(),
            Preview {
                output: String::new(),
                dry_run: true,
                protocol: PROTOCOL,
                revision: crate::engine::REVISION.into(),
                system_included: false,
                items,
            },
            1000,
        )?;
        let ids = selected
            .iter()
            .map(|i| scan.items[*i].candidate.id.clone())
            .collect::<Vec<_>>();
        let plan = review::create(&scan, &ids, &[], 1001)?;
        let id = plan.id.clone();
        self.plan = Some(plan);
        Ok(id)
    }

    fn append(&self, event: &Event) -> Result<(), String> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.journal)
            .map_err(|e| e.to_string())?;
        let mut bytes = serde_json::to_vec(event).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())
    }

    fn events(&self) -> Result<Vec<Event>, String> {
        let bytes = match std::fs::read_to_string(&self.journal) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string()),
        };
        if !bytes.ends_with('\n') {
            return Err("incomplete journal; require manual inspection".into());
        }
        bytes
            .lines()
            .map(|line| serde_json::from_str(line).map_err(|e| e.to_string()))
            .collect()
    }

    fn recovered(&self) -> Result<HashMap<String, State>, String> {
        let mut states = HashMap::new();
        for event in self.events()? {
            states.insert(
                event.item,
                if matches!(event.state, State::Intent | State::RestoreIntent) {
                    State::Interrupted
                } else {
                    event.state
                },
            );
        }
        Ok(states)
    }

    // The hook injects cancellation/interruption/name replacement in tests. Rule
    // evidence is an explicit fixture code, never a claim about real Mole rules.
    fn run(
        &self,
        id: &str,
        now: i64,
        protected: &[String],
        rules: &HashMap<String, String>,
        cancelled: &AtomicBool,
        mut hook: impl FnMut(usize, State) -> Fault,
    ) -> Result<Vec<Event>, String> {
        let plan = self
            .plan
            .as_ref()
            .filter(|p| p.id == id)
            .ok_or("unknown plan")?;
        if self.events()?.iter().any(|e| e.plan == id) {
            return Err("attempt already recorded; do not replay".into());
        }
        let source = directory(&self.source)?;
        let recovery = directory(&self.recovery)?;
        let mut results = Vec::new();
        let mut interrupted = false;
        for (index, item) in plan.items.iter().enumerate() {
            let candidate = &item.candidate;
            let path = Path::new(&candidate.path);
            let mut event = Event {
                plan: id.into(),
                item: candidate.id.clone(),
                state: State::NotStarted,
                reason: String::new(),
                file: None,
            };
            if interrupted {
                results.push(event);
                continue;
            }
            if cancelled.load(Ordering::SeqCst) {
                event.state = State::Cancelled;
                event.reason = "cancelled before starting item".into();
            } else {
                let check = review::check(plan, protected, now).items.remove(index);
                if check.state != "unchanged"
                    || rules.get(&candidate.id).map(String::as_str) != Some("passed-fixture")
                {
                    event.state = State::Skipped;
                    event.reason = format!("{}; fixture rule evidence required", check.state);
                } else if path.parent() != Some(self.source.as_path()) || candidate.is_directory {
                    event.state = State::Skipped;
                    event.reason = "only fixture-owned direct regular files are supported".into();
                } else {
                    let observed = OpenOptions::new()
                        .read(true)
                        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                        .open(path)
                        .map_err(|e| e.to_string())?;
                    let before = observed.metadata().map_err(|e| e.to_string())?;
                    if !before.is_file() || before.nlink() != 1 {
                        event.state = State::Skipped;
                        event.reason = "regular unshared file required".into();
                    } else if hook(index, State::NotStarted) == Fault::BeforeIntent {
                        event.state = State::Failed;
                        event.reason = "injected journal failure; no operation attempted".into();
                        interrupted = true;
                    } else {
                        event.state = State::Intent;
                        event.file = Some(Stamp::read(&before)?);
                        self.append(&event)?;
                        if hook(index, State::Intent) == Fault::AfterIntent {
                            event.state = State::Interrupted;
                            interrupted = true;
                        } else {
                            let name = path.file_name().unwrap().to_str().unwrap();
                            match move_exclusive(&source, &recovery, name) {
                                Err(error) => {
                                    event.state = State::Failed;
                                    event.reason = error;
                                }
                                Ok(()) => {
                                    let after = std::fs::symlink_metadata(self.recovery.join(name))
                                        .map_err(|e| e.to_string())?;
                                    if hook(index, State::Moved) == Fault::AfterMove
                                        || !same_file(&before, &after)
                                    {
                                        // Preserve the uncertain target. Never auto-delete or undo it.
                                        event.state = State::Interrupted;
                                        interrupted = true;
                                    } else {
                                        event.state = State::Moved;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if event.state != State::Interrupted {
                self.append(&event)?;
            }
            hook(index, event.state.clone());
            results.push(event);
        }
        Ok(results)
    }

    fn restore(&self, item_id: &str) -> Result<(), String> {
        if !matches!(
            self.recovered()?.get(item_id),
            Some(State::Moved | State::RestoreFailed)
        ) {
            return Err("no completed move receipt".into());
        }
        let plan = self.plan.as_ref().unwrap();
        let item = plan
            .items
            .iter()
            .find(|i| i.candidate.id == item_id)
            .ok_or("unknown item")?;
        let original = Path::new(&item.candidate.path);
        let name = original.file_name().unwrap().to_str().unwrap();
        let receipt = self
            .events()?
            .into_iter()
            .rev()
            .find(|e| e.item == item_id && e.state == State::Moved)
            .ok_or("missing move receipt")?;
        let observed = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.recovery.join(name))
            .map_err(|e| e.to_string())?;
        if Some(Stamp::read(
            &observed.metadata().map_err(|e| e.to_string())?,
        )?) != receipt.file
        {
            return Err("recovery target changed; require manual inspection".into());
        }
        self.append(&Event {
            state: State::RestoreIntent,
            ..receipt.clone()
        })?;
        if let Err(error) =
            move_exclusive(&directory(&self.recovery)?, &directory(&self.source)?, name)
        {
            self.append(&Event {
                state: State::RestoreFailed,
                reason: error.clone(),
                ..receipt
            })?;
            return Err(error);
        }
        let after = std::fs::symlink_metadata(original).map_err(|e| e.to_string())?;
        if Some(Stamp::read(&after)?) != receipt.file {
            return Err("restore identity uncertain".into());
        }
        self.append(&Event {
            plan: plan.id.clone(),
            item: item_id.into(),
            state: State::Restored,
            reason: String::new(),
            file: receipt.file,
        })
    }

    fn rules(&self) -> HashMap<String, String> {
        self.plan
            .as_ref()
            .unwrap()
            .items
            .iter()
            .map(|i| (i.candidate.id.clone(), "passed-fixture".into()))
            .collect()
    }
}

#[test]
fn exact_selection_moves_and_restores_without_touching_unselected_files() {
    let names = ["缓存 空格\n[*].txt", "keep", "third"];
    let mut box_ = Sandbox::new(&[(names[0], "one"), (names[1], "two"), (names[2], "three")]);
    let id = box_.prepare(&names, &[0, 2]).unwrap();
    assert!(box_
        .run(
            "foreign-plan",
            1002,
            &[],
            &box_.rules(),
            &AtomicBool::new(false),
            |_, _| Fault::None
        )
        .is_err());
    let results = box_
        .run(
            &id,
            1002,
            &[],
            &box_.rules(),
            &AtomicBool::new(false),
            |_, _| Fault::None,
        )
        .unwrap();
    assert_eq!(
        results.iter().map(|e| &e.state).collect::<Vec<_>>(),
        [&State::Moved, &State::Moved]
    );
    assert_eq!(
        std::fs::read_to_string(box_.source.join("keep")).unwrap(),
        "two"
    );
    for (event, name) in results.iter().zip([names[0], names[2]]) {
        assert!(!box_.source.join(name).exists());
        assert!(box_.recovery.join(name).is_file());
        box_.restore(&event.item).unwrap();
        assert!(box_.source.join(name).is_file());
        assert!(box_.restore(&event.item).is_err());
    }
    assert_eq!(
        std::fs::read_to_string(box_.source.join(names[0])).unwrap(),
        "one"
    );
    assert!(box_
        .run(
            &id,
            1003,
            &[],
            &box_.rules(),
            &AtomicBool::new(false),
            |_, _| Fault::None
        )
        .is_err());
}

#[test]
fn collision_is_a_partial_failure_and_neither_move_nor_restore_overwrites() {
    let mut box_ = Sandbox::new(&[("a", "A"), ("b", "B"), ("c", "C")]);
    let id = box_.prepare(&["a", "b", "c"], &[0, 1, 2]).unwrap();
    std::fs::write(box_.recovery.join("b"), "existing recovery").unwrap();
    let results = box_
        .run(
            &id,
            1002,
            &[],
            &box_.rules(),
            &AtomicBool::new(false),
            |_, _| Fault::None,
        )
        .unwrap();
    assert_eq!(
        results.iter().map(|e| &e.state).collect::<Vec<_>>(),
        [&State::Moved, &State::Failed, &State::Moved]
    );
    assert_eq!(std::fs::read_to_string(box_.source.join("b")).unwrap(), "B");
    assert_eq!(
        std::fs::read_to_string(box_.recovery.join("b")).unwrap(),
        "existing recovery"
    );
    std::fs::write(box_.source.join("a"), "new file").unwrap();
    assert!(box_.restore(&results[0].item).is_err());
    assert_eq!(
        std::fs::read_to_string(box_.source.join("a")).unwrap(),
        "new file"
    );
    assert_eq!(
        std::fs::read_to_string(box_.recovery.join("a")).unwrap(),
        "A"
    );
}

#[test]
fn cancellation_keeps_completed_result_and_stops_subsequent_items() {
    let mut box_ = Sandbox::new(&[("a", "A"), ("b", "B")]);
    let id = box_.prepare(&["a", "b"], &[0, 1]).unwrap();
    let cancel = AtomicBool::new(false);
    let results = box_
        .run(&id, 1002, &[], &box_.rules(), &cancel, |i, state| {
            if i == 0 && state == State::Moved {
                cancel.store(true, Ordering::SeqCst);
            }
            Fault::None
        })
        .unwrap();
    assert_eq!(results[0].state, State::Moved);
    assert_eq!(results[1].state, State::Cancelled);
    assert!(box_.source.join("b").is_file());
    assert_eq!(
        box_.events()
            .unwrap()
            .iter()
            .filter(|e| e.state == State::Intent)
            .count(),
        1
    );
}

#[test]
fn current_protection_expiry_and_unknown_rules_prevent_changes() {
    for mode in ["protected", "expired", "unknown"] {
        let mut box_ = Sandbox::new(&[("a", "A")]);
        let id = box_.prepare(&["a"], &[0]).unwrap();
        let rules = if mode == "unknown" {
            HashMap::new()
        } else {
            box_.rules()
        };
        let protected = if mode == "protected" {
            vec![box_.source.to_str().unwrap().into()]
        } else {
            vec![]
        };
        let now = if mode == "expired" { 901000 } else { 1002 };
        let result = box_
            .run(
                &id,
                now,
                &protected,
                &rules,
                &AtomicBool::new(false),
                |_, _| Fault::None,
            )
            .unwrap();
        assert_eq!(result[0].state, State::Skipped);
        assert!(box_.source.join("a").is_file());
        assert!(box_.recovery.read_dir().unwrap().next().is_none());
    }
}

#[test]
fn changed_links_and_disappeared_files_never_follow_external_targets() {
    for mode in ["link", "hardlink", "missing", "directory"] {
        let mut box_ = Sandbox::new(&[("a", "A"), ("keep", "K")]);
        if mode == "directory" {
            std::fs::remove_file(box_.source.join("a")).unwrap();
            std::fs::create_dir(box_.source.join("a")).unwrap();
        }
        let id = box_.prepare(&["a"], &[0]).unwrap();
        match mode {
            "link" => {
                std::fs::remove_file(box_.source.join("a")).unwrap();
                std::os::unix::fs::symlink(box_.source.join("keep"), box_.source.join("a"))
                    .unwrap();
            }
            "hardlink" => {
                std::fs::hard_link(box_.source.join("a"), box_.source.join("shared")).unwrap()
            }
            "missing" => std::fs::remove_file(box_.source.join("a")).unwrap(),
            _ => (),
        }
        let result = box_
            .run(
                &id,
                1002,
                &[],
                &box_.rules(),
                &AtomicBool::new(false),
                |_, _| Fault::None,
            )
            .unwrap();
        assert_eq!(result[0].state, State::Skipped);
        assert_eq!(
            std::fs::read_to_string(box_.source.join("keep")).unwrap(),
            "K"
        );
        assert!(box_.recovery.read_dir().unwrap().next().is_none());
    }
}

#[test]
fn interruption_records_uncertainty_without_replay_or_claiming_success() {
    for fault in [Fault::BeforeIntent, Fault::AfterIntent, Fault::AfterMove] {
        let mut box_ = Sandbox::new(&[("a", "A"), ("b", "B")]);
        let id = box_.prepare(&["a", "b"], &[0, 1]).unwrap();
        let results = box_
            .run(
                &id,
                1002,
                &[],
                &box_.rules(),
                &AtomicBool::new(false),
                |i, state| {
                    if i == 0
                        && ((fault == Fault::BeforeIntent && state == State::NotStarted)
                            || (fault == Fault::AfterIntent && state == State::Intent)
                            || (fault == Fault::AfterMove && state == State::Moved))
                    {
                        fault
                    } else {
                        Fault::None
                    }
                },
            )
            .unwrap();
        assert_eq!(results[1].state, State::NotStarted);
        assert_eq!(box_.source.join("a").exists(), fault != Fault::AfterMove);
        assert_eq!(box_.recovery.join("a").exists(), fault == Fault::AfterMove);
        let recovered = box_.recovered().unwrap();
        assert_eq!(
            recovered.get(&results[0].item),
            Some(if fault == Fault::BeforeIntent {
                &State::Failed
            } else {
                &State::Interrupted
            })
        );
        assert!(box_.restore(&results[0].item).is_err());
        assert!(box_
            .run(
                &id,
                1003,
                &[],
                &box_.rules(),
                &AtomicBool::new(false),
                |_, _| Fault::None
            )
            .is_err());
    }
}

#[test]
fn late_name_substitution_demonstrates_the_remaining_source_identity_race() {
    let mut box_ = Sandbox::new(&[("a", "original"), ("b", "keep")]);
    let id = box_.prepare(&["a", "b"], &[0, 1]).unwrap();
    let result = box_
        .run(
            &id,
            1002,
            &[],
            &box_.rules(),
            &AtomicBool::new(false),
            |index, state| {
                if index == 0 && state == State::Intent {
                    std::fs::rename(box_.source.join("a"), box_.source.join("original")).unwrap();
                    std::fs::write(box_.source.join("a"), "replacement").unwrap();
                }
                Fault::None
            },
        )
        .unwrap();
    assert_eq!(result[0].state, State::Interrupted);
    assert_eq!(result[1].state, State::NotStarted);
    assert_eq!(
        std::fs::read_to_string(box_.recovery.join("a")).unwrap(),
        "replacement"
    );
    assert_eq!(
        std::fs::read_to_string(box_.source.join("original")).unwrap(),
        "original"
    );
    assert!(box_.restore(&result[0].item).is_err());
}

#[test]
fn changed_recovery_targets_and_truncated_journals_block_automatic_actions() {
    for mode in ["replacement", "modified", "symlink", "journal"] {
        let mut box_ = Sandbox::new(&[("a", "original"), ("keep", "K")]);
        let id = box_.prepare(&["a"], &[0]).unwrap();
        let result = box_
            .run(
                &id,
                1002,
                &[],
                &box_.rules(),
                &AtomicBool::new(false),
                |_, _| Fault::None,
            )
            .unwrap();
        let path = box_.recovery.join("a");
        match mode {
            "replacement" => {
                std::fs::rename(&path, box_.recovery.join("original")).unwrap();
                std::fs::write(&path, "replacement").unwrap();
            }
            "modified" => std::fs::write(&path, "changed content").unwrap(),
            "symlink" => {
                std::fs::rename(&path, box_.recovery.join("original")).unwrap();
                std::os::unix::fs::symlink(box_.source.join("keep"), &path).unwrap();
            }
            "journal" => {
                let mut journal = OpenOptions::new().append(true).open(&box_.journal).unwrap();
                journal.write_all(b"{\"partial\":").unwrap();
            }
            _ => unreachable!(),
        }
        assert!(box_.restore(&result[0].item).is_err());
        assert!(!box_.source.join("a").exists());
        assert_eq!(
            std::fs::read_to_string(box_.source.join("keep")).unwrap(),
            "K"
        );
        assert!(box_
            .run(
                &id,
                1003,
                &[],
                &box_.rules(),
                &AtomicBool::new(false),
                |_, _| Fault::None
            )
            .is_err());
    }
}
