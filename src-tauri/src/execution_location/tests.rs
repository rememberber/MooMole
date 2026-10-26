use super::*;
use std::os::unix::fs::{symlink, MetadataExt};

struct Fixture {
    _root: tempfile::TempDir,
    path: PathBuf,
    detail: execution::Detail,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().canonicalize().unwrap().join("receipt-dir");
        std::fs::create_dir(&parent).unwrap();
        let path = parent.join("receipt.log");
        std::fs::write(&path, "own receipt fixture").unwrap();
        let m = path.metadata().unwrap();
        let detail = execution::Detail {
            summary: execution::Summary {
                id: uuid::Uuid::new_v4().to_string(),
                plan_id: uuid::Uuid::new_v4().to_string(),
                method: "trash".into(),
                state: "completed".into(),
                created_at: 1,
                finished_at: Some(2),
                item_count: 1,
                moved_count: 1,
                uncertain_count: 0,
                reviewed_at: None,
                retained_for_review: true,
            },
            revision: crate::engine::REVISION.into(),
            protocol: 2,
            snapshot_sha256: "a".repeat(64),
            items: vec![execution::Item {
                id: "selected".into(),
                path: "/fixture/original.log".into(),
                state: "moved".into(),
                reason: "fixture".into(),
                receipt: Some(execution::Receipt {
                    destination: path.to_str().unwrap().into(),
                    device: m.dev(),
                    inode: m.ino(),
                }),
            }],
            events: vec![],
        };
        Self {
            _root: root,
            path,
            detail,
        }
    }
}
#[test]
fn only_exact_receipted_items_can_be_located_without_changing_files_or_history() {
    let mut f = Fixture::new();
    let before = serde_json::to_value(&f.detail).unwrap();
    let location = Location::prepare(&f.detail, "selected").unwrap();
    location.validate().unwrap();
    assert_eq!(
        std::fs::read_to_string(&f.path).unwrap(),
        "own receipt fixture"
    );
    assert_eq!(serde_json::to_value(&f.detail).unwrap(), before);
    assert!(Location::prepare(&f.detail, "other").is_err());
    for state in ["intent", "pending", "failed", "cancelled", "unknown"] {
        f.detail.items[0].state = state.into();
        assert!(Location::prepare(&f.detail, "selected").is_err());
    }
    f.detail.items[0].state = "uncertain".into();
    assert!(Location::prepare(&f.detail, "selected").is_ok());
    f.detail.items[0].receipt = None;
    assert!(Location::prepare(&f.detail, "selected").is_err());
}
#[test]
fn moved_replaced_linked_and_changed_objects_are_never_followed_or_opened() {
    for mode in [
        "missing",
        "replacement",
        "symlink",
        "hardlink",
        "directory",
        "after-prepare",
        "parent-replaced",
        "ancestor-link",
    ] {
        let f = Fixture::new();
        let prepared = Location::prepare(&f.detail, "selected").unwrap();
        let other = f.path.with_file_name("original-relocated.log");
        if mode == "parent-replaced" || mode == "ancestor-link" {
            let parent = f.path.parent().unwrap();
            let relocated = parent.with_file_name("relocated-dir");
            std::fs::rename(parent, &relocated).unwrap();
            if mode == "ancestor-link" {
                symlink(&relocated, parent).unwrap();
            } else {
                std::fs::create_dir(parent).unwrap();
                std::fs::write(&f.path, "replacement").unwrap();
            }
        } else if mode == "hardlink" {
            std::fs::hard_link(&f.path, &other).unwrap();
        } else if mode == "after-prepare" {
            std::fs::write(&f.path, "changed contents").unwrap();
        } else {
            std::fs::rename(&f.path, &other).unwrap();
            match mode {
                "replacement" => std::fs::write(&f.path, "replacement").unwrap(),
                "symlink" => symlink(&other, &f.path).unwrap(),
                "directory" => std::fs::create_dir(&f.path).unwrap(),
                _ => {}
            }
        }
        assert!(prepared.validate().is_err(), "{mode}");
        if mode != "after-prepare" {
            assert!(Location::prepare(&f.detail, "selected").is_err(), "{mode}");
        }
        if mode == "replacement" {
            assert_eq!(std::fs::read_to_string(&f.path).unwrap(), "replacement");
        }
    }
}
