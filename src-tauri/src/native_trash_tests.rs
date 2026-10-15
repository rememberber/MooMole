//! Explicit Foundation compatibility checks, not a production executor. Regular
//! tests only observe file-reference URLs. The ignored smoke check trashes ONLY
//! unique fixtures it creates, then restores each exact returned item.
use objc2::rc::{autoreleasepool, Retained};
use objc2_foundation::{NSFileManager, NSString, NSURL};
use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

fn url(path: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(
        path.to_str().expect("fixture paths are UTF-8"),
    ))
}

fn path(url: &NSURL) -> PathBuf {
    assert!(url.isFileURL());
    PathBuf::from(
        url.path()
            .expect("local fixture URL has a path")
            .to_string(),
    )
}

fn identity(path: &Path) -> (u64, u64, u32) {
    try_identity(path).unwrap()
}

fn try_identity(path: &Path) -> Result<(u64, u64, u32), String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    Ok((meta.dev(), meta.ino(), meta.mode()))
}

#[test]
fn reference_urls_follow_fixture_identity_while_path_urls_keep_the_old_name() {
    autoreleasepool(|_| {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().canonicalize().unwrap();
        let original = root.join("缓存 空格\n#%.txt");
        let renamed = root.join("renamed");
        std::fs::write(&original, "original").unwrap();
        let original_id = identity(&original);
        let path_url = url(&original);
        let reference = path_url
            .fileReferenceURL()
            .expect("reference URL supported on test volume");
        assert!(reference.isFileReferenceURL());
        std::fs::rename(&original, &renamed).unwrap();
        std::fs::write(&original, "replacement").unwrap();
        assert_eq!(path(&path_url), original);
        let resolved = path(&reference.filePathURL().unwrap());
        assert_eq!(resolved, renamed);
        assert_eq!(identity(&resolved), original_id);
        assert_ne!(identity(&original), original_id);
    });
}

#[test]
fn reference_urls_may_follow_an_item_outside_its_initial_parent() {
    autoreleasepool(|_| {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().canonicalize().unwrap();
        let first = root.join("selected");
        let second = root.join("outside-selected");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let file = first.join("item");
        std::fs::write(&file, "fixture").unwrap();
        let reference = url(&file).fileReferenceURL().unwrap();
        std::fs::rename(&file, second.join("item")).unwrap();
        let resolved = path(&reference.filePathURL().unwrap());
        assert!(!resolved.starts_with(&first));
        assert_eq!(resolved, second.join("item"));
        // Reference identity alone cannot authorize acting outside a reviewed scope.
    });
}

struct Receipt {
    original: PathBuf,
    returned: PathBuf,
    identity: (u64, u64, u32),
    restored: bool,
}

impl Receipt {
    fn restore(&mut self) -> Result<(), String> {
        if try_identity(&self.returned)? != self.identity {
            return Err("returned Trash item no longer matches this fixture".into());
        }
        NSFileManager::defaultManager()
            .moveItemAtURL_toURL_error(&url(&self.returned), &url(&self.original))
            .map_err(|e| {
                format!(
                    "{} ({}, {})",
                    e.localizedDescription(),
                    e.domain(),
                    e.code()
                )
            })?;
        self.restored = true;
        if try_identity(&self.original)? != self.identity {
            return Err("restored fixture identity mismatch".into());
        }
        Ok(())
    }
}

impl Drop for Receipt {
    fn drop(&mut self) {
        if !self.restored {
            if let Err(error) = self.restore() {
                eprintln!("Fixture remains at {}: {error}", self.returned.display());
            }
        }
    }
}

#[test]
#[ignore = "Explicit native Trash check: creates unique fixtures, trashes them via Foundation, then restores only returned items"]
fn native_trash_round_trip_smoke() {
    autoreleasepool(|_| {
        let scratch = tempfile::Builder::new()
            .prefix("MooMole-native-trash-")
            .tempdir()
            .unwrap();
        // Keep the root on a failed check so exact receipts can still be restored.
        let root = scratch.keep().canonicalize().unwrap();
        let name = format!("MooMole-fixture-{}-缓存 空格\n#%.txt", uuid::Uuid::new_v4());
        let mut receipts: Vec<Receipt> = Vec::new();
        let report =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.cache/native-trash-smoke.json");
        std::fs::create_dir_all(report.parent().unwrap()).unwrap();
        let save_report = |receipts: &[Receipt]| {
            let value = serde_json::json!({"fixtureRoot": root, "receipts": receipts.iter().map(|r|
                serde_json::json!({"original": r.original, "returned": r.returned, "identity": r.identity, "restored": r.restored})).collect::<Vec<_>>()});
            std::fs::write(&report, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        };
        let manager = NSFileManager::defaultManager();
        for parent in ["first", "second", "directory"] {
            let folder = root.join(parent);
            std::fs::create_dir(&folder).unwrap();
            let item = folder.join(&name);
            if parent == "directory" {
                std::fs::create_dir(&item).unwrap();
                std::fs::write(item.join("child"), "directory fixture").unwrap();
            } else {
                std::fs::write(&item, parent).unwrap();
            }
            let before = identity(&item);
            let source_url = url(&item);
            let reference = source_url.fileReferenceURL().unwrap();
            let mut resulting = None;
            manager
                .trashItemAtURL_resultingItemURL_error(&reference, Some(&mut resulting))
                .unwrap_or_else(|e| {
                    panic!(
                        "native Trash refused fixture: {} ({}, {})",
                        e.localizedDescription(),
                        e.domain(),
                        e.code()
                    )
                });
            let returned =
                path(&resulting.expect("Trash operation must return actual destination"));
            eprintln!("Trash fixture destination: {}", returned.display());
            let receipt = Receipt {
                original: item.clone(),
                returned: returned.clone(),
                identity: before,
                restored: false,
            };
            // Push before assertions so unwinding still attempts exact fixture recovery.
            receipts.push(receipt);
            save_report(&receipts);
            assert!(!item.exists());
            assert_eq!(identity(&returned), before);
        }
        assert_ne!(receipts[0].returned, receipts[1].returned);
        assert_eq!(
            std::fs::read_to_string(&receipts[0].returned).unwrap(),
            "first"
        );
        assert_eq!(
            std::fs::read_to_string(&receipts[1].returned).unwrap(),
            "second"
        );
        assert_eq!(
            std::fs::read_to_string(receipts[2].returned.join("child")).unwrap(),
            "directory fixture"
        );
        // A replacement at the source is preserved; retry only after moving that
        // new fixture aside. No Trash enumeration or permanent deletion is used.
        std::fs::write(&receipts[0].original, "replacement").unwrap();
        assert!(receipts[0].restore().is_err());
        assert_eq!(
            std::fs::read_to_string(&receipts[0].original).unwrap(),
            "replacement"
        );
        std::fs::rename(&receipts[0].original, root.join("replacement-kept")).unwrap();
        for receipt in &mut receipts {
            receipt.restore().unwrap();
            assert!(!receipt.returned.exists());
        }
        save_report(&receipts);
        assert_eq!(
            std::fs::read_to_string(&receipts[0].original).unwrap(),
            "first"
        );
        eprintln!(
            "Verified: 3 native Trash fixtures restored; collision preserved; no Trash purge."
        );
        std::fs::remove_dir_all(&root).unwrap();
    });
}
