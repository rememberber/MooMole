use super::*;
use crate::{
    clean_preview::{Candidate, Preview, PROTOCOL},
    execution::Outcome,
    review,
};
use objc2::rc::autoreleasepool;
use std::{cell::Cell, path::Path};

struct Fixture {
    root: tempfile::TempDir,
    path: PathBuf,
    item: ScanItem,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("MooMole-prepared-target-")
            .tempdir()
            .unwrap();
        let canonical = root.path().canonicalize().unwrap();
        std::fs::create_dir(canonical.join("source")).unwrap();
        let path = canonical
            .join("source")
            .join(format!("MooMole-{}-缓存\n#%*.log", uuid::Uuid::new_v4()));
        std::fs::write(&path, "selected original fixture").unwrap();
        std::fs::write(canonical.join("source/unselected"), "unselected fixture").unwrap();
        let meta = path.metadata().unwrap();
        let scan = review::capture(
            uuid::Uuid::new_v4().to_string(),
            Preview {
                dry_run: true,
                protocol: PROTOCOL,
                revision: crate::engine::REVISION.into(),
                output: String::new(),
                system_included: false,
                items: vec![Candidate {
                    id: String::new(),
                    upstream_identity: format!("inode:{}:{}", meta.dev(), meta.ino()),
                    path: path.to_str().unwrap().into(),
                    section: "isolated reference fixture".into(),
                    size: Some(meta.len()),
                    item_count: 1,
                    covered_by: None,
                    is_directory: false,
                    resolved_path: None,
                    impact: None,
                    provenance: None,
                }],
            },
            crate::db::now(),
        )
        .unwrap();
        Self {
            root,
            path,
            item: scan.items[0].clone(),
        }
    }

    fn root(&self) -> PathBuf {
        self.path.parent().unwrap().parent().unwrap().to_owned()
    }

    fn change(&self, kind: &str) -> PathBuf {
        match kind {
            "name" => {
                let moved = self.root().join("selected-relocated.log");
                std::fs::rename(&self.path, &moved).unwrap();
                std::fs::write(&self.path, "unselected replacement fixture").unwrap();
                moved
            }
            "parent" => {
                let moved = self.root().join("parent-relocated");
                std::fs::rename(self.root().join("source"), &moved).unwrap();
                std::fs::create_dir(self.root().join("source")).unwrap();
                std::fs::write(&self.path, "unselected replacement fixture").unwrap();
                moved.join(self.path.file_name().unwrap())
            }
            "content" => {
                std::fs::write(&self.path, "changed selected fixture content").unwrap();
                self.path.clone()
            }
            _ => panic!("unknown fixture change"),
        }
    }
}

#[test]
fn prepared_reference_is_read_only_and_bound_to_full_planned_identity() {
    autoreleasepool(|_| {
        let f = Fixture::new();
        let prepared = PreparedFile::prepare(&f.item).unwrap();
        assert!(prepared.reference.isFileReferenceURL());
        prepared.validate().unwrap();
        assert_eq!(
            std::fs::read_to_string(&f.path).unwrap(),
            "selected original fixture"
        );
        assert_eq!(
            std::fs::read_to_string(f.root().join("source/unselected")).unwrap(),
            "unselected fixture"
        );
        let mut old = f.item.clone();
        old.witness.as_mut().unwrap().version = 0;
        assert!(PreparedFile::prepare(&old).is_err());
        old = f.item.clone();
        old.witness = None;
        assert!(PreparedFile::prepare(&old).is_err());
        old = f.item.clone();
        old.candidate.is_directory = true;
        assert!(PreparedFile::prepare(&old).is_err());
    });
}

#[test]
fn replacement_during_reference_acquisition_never_binds_the_new_file() {
    autoreleasepool(|_| {
        for phase in [Preparation::Anchored, Preparation::Referenced] {
            let f = Fixture::new();
            let result = PreparedFile::prepare_with(&f.item, |current| {
                if current == phase {
                    f.change("name");
                }
            });
            assert!(result.is_err());
            assert_eq!(
                std::fs::read_to_string(&f.path).unwrap(),
                "unselected replacement fixture"
            );
            assert_eq!(
                std::fs::read_to_string(f.root().join("selected-relocated.log")).unwrap(),
                "selected original fixture"
            );
        }
    });
}

#[test]
fn reference_following_a_renamed_object_does_not_grant_new_path_scope() {
    autoreleasepool(|_| {
        for kind in ["name", "parent", "content"] {
            let f = Fixture::new();
            let prepared = PreparedFile::prepare(&f.item).unwrap();
            let relocated = f.change(kind);
            assert!(prepared.validate().is_err(), "{kind}");
            if kind != "content" {
                let resolved = prepared
                    .reference
                    .filePathURL()
                    .unwrap()
                    .path()
                    .unwrap()
                    .to_string();
                assert_eq!(Path::new(&resolved), relocated);
                assert_eq!(
                    std::fs::read_to_string(&f.path).unwrap(),
                    "unselected replacement fixture"
                );
            }
        }
    });
}

#[test]
fn missing_links_and_new_hard_links_invalidate_prepared_targets() {
    autoreleasepool(|_| {
        for kind in ["missing", "symlink", "hardlink"] {
            let f = Fixture::new();
            let prepared = PreparedFile::prepare(&f.item).unwrap();
            match kind {
                "missing" => std::fs::remove_file(&f.path).unwrap(),
                "symlink" => {
                    std::fs::remove_file(&f.path).unwrap();
                    std::os::unix::fs::symlink(f.root().join("source/unselected"), &f.path)
                        .unwrap();
                }
                _ => std::fs::hard_link(&f.path, f.root().join("extra-hardlink")).unwrap(),
            }
            assert!(prepared.validate().is_err());
            assert!(PreparedFile::prepare(&f.item).is_err());
            assert_eq!(
                std::fs::read_to_string(f.root().join("source/unselected")).unwrap(),
                "unselected fixture"
            );
        }
    });
}

#[test]
#[ignore = "Explicit native reference race check: only fresh fixtures; restores each exact returned Trash item"]
fn native_prepared_reference_gate_and_final_window() {
    use objc2_foundation::NSFileManager;
    autoreleasepool(|_| {
        for (timing, kind) in [
            ("stable", "none"),
            ("beforeGate", "name"),
            ("beforeGate", "parent"),
            ("beforeGate", "content"),
            ("afterGate", "name"),
            ("afterGate", "parent"),
            ("afterGate", "content"),
        ] {
            let mut f = Fixture::new();
            f.root.disable_cleanup(true);
            let root = f.root();
            eprintln!(
                "Prepared native fixture {timing}/{kind}: {}",
                root.display()
            );
            let prepared = PreparedFile::prepare(&f.item).unwrap();
            let expected = f.path.metadata().unwrap();
            let mut original_location = f.path.clone();
            if timing == "beforeGate" {
                original_location = f.change(kind);
            }
            let called = Cell::new(false);
            let result = prepared.trash_fixture(&root, || {
                called.set(true);
                if timing == "afterGate" {
                    original_location = f.change(kind);
                }
            });
            if timing == "beforeGate" {
                assert!(result.is_err());
                assert!(!called.get());
                assert!(original_location.is_file());
                eprintln!("Rejected before native API; all fixture data retained.");
                f.root.disable_cleanup(false);
                continue;
            }
            let attempt = result.unwrap();
            let (state, receipt) = match &attempt.outcome {
                Outcome::Moved(receipt) => ("moved", Some(receipt.clone())),
                Outcome::Uncertain(receipt) => ("uncertain", receipt.clone()),
                Outcome::Failed => ("failed", None),
            };
            let report = root.join("native-prepared-rescue.json");
            let mut record = serde_json::json!({
                "fixtureRoot": root, "timing": timing, "change": kind, "state": state,
                "reason": attempt.reason, "receipt": receipt, "originalLocation": original_location,
                "selectedDevice": expected.dev().to_string(), "selectedInode": expected.ino().to_string(), "restored": false,
            });
            std::fs::write(&report, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
            let receipt =
                receipt.expect("exact native return required; fixture evidence preserved");
            let returned_selected =
                (receipt.device, receipt.inode) == (expected.dev(), expected.ino());
            // Restore ONLY a positively identified fixture, even if the native
            // experiment unexpectedly moved the replacement. Never overwrite.
            let restore_to = if returned_selected {
                original_location.clone()
            } else {
                let replacement = std::fs::read_to_string(&receipt.destination).unwrap();
                assert_eq!(replacement, "unselected replacement fixture");
                f.path.clone()
            };
            assert!(restore_to.starts_with(&root));
            let from = NSURL::fileURLWithPath(&NSString::from_str(&receipt.destination));
            let to = NSURL::fileURLWithPath(&NSString::from_str(restore_to.to_str().unwrap()));
            NSFileManager::defaultManager()
                .moveItemAtURL_toURL_error(&from, &to)
                .unwrap();
            let restored = std::fs::symlink_metadata(&restore_to).unwrap();
            assert_eq!(
                (restored.dev(), restored.ino()),
                (receipt.device, receipt.inode)
            );
            record["restored"] = true.into();
            record["restoredTo"] = restore_to.to_str().unwrap().into();
            std::fs::write(&report, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
            eprintln!("Native {timing}/{kind}: state={state}, selectedIdentity={returned_selected}, exact item restored.");
            assert!(
                returned_selected,
                "native reference moved a replacement; root retained for inspection"
            );
            assert_eq!(
                state,
                if timing == "stable" {
                    "moved"
                } else {
                    "uncertain"
                }
            );
            assert_eq!(
                std::fs::read_to_string(&original_location).unwrap(),
                if kind == "content" {
                    "changed selected fixture content"
                } else {
                    "selected original fixture"
                }
            );
            if timing == "afterGate" && kind != "content" {
                assert_eq!(
                    std::fs::read_to_string(&f.path).unwrap(),
                    "unselected replacement fixture"
                );
            }
            let unselected = if kind == "parent" {
                root.join("parent-relocated/unselected")
            } else {
                root.join("source/unselected")
            };
            assert_eq!(
                std::fs::read_to_string(unselected).unwrap(),
                "unselected fixture"
            );
            assert!(!Path::new(&receipt.destination).exists());
            f.root.disable_cleanup(false);
        }
    });
}
