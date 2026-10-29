use super::*;
use crate::engine::TaskKind;
fn selected(root: &Path) -> super::super::Plan {
    let scan =
        super::super::scan::collect(root, &[], Arc::new(Job::new(TaskKind::PersonalScan))).unwrap();
    let chosen = scan
        .items
        .iter()
        .find(|i| i.path.ends_with("owned.txt"))
        .unwrap()
        .id
        .clone();
    let mut store = super::super::Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = store.offer(&[chosen], db::now()).unwrap();
    store.take(&offer.id, db::now()).unwrap()
}
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::Builder::new()
        .prefix("MooMole-transfer-owned-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let source = home.join("Downloads");
    let target = home.join("Target");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&target).unwrap();
    std::fs::write(source.join("owned.txt"), "owned transfer data\n").unwrap();
    std::fs::write(source.join("unselected.txt"), "unselected keep\n").unwrap();
    let path = source.join("owned.txt");
    use std::ffi::CString;
    let raw = CString::new(path.to_str().unwrap()).unwrap();
    for (name, value) in [
        ("user.moomole-owned", b"owned attribute".as_slice()),
        ("com.apple.ResourceFork", b"owned resource fork".as_slice()),
        (
            "com.apple.quarantine",
            b"0081;00000000;MooMoleOwnedTest;owned".as_slice(),
        ),
    ] {
        let key = CString::new(name).unwrap();
        assert_eq!(
            unsafe {
                libc::setxattr(
                    raw.as_ptr(),
                    key.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    0,
                    0,
                )
            },
            0
        );
    }
    (temp, source, target)
}
#[test]
fn copy_building_block_preserves_data_metadata_and_never_overwrites_or_removes_source() {
    let (_temp, source, target) = fixture();
    let path = source.join("owned.txt");
    let p = selected(&source);
    let settings = db::Settings {
        acknowledged: true,
        ..Default::default()
    };
    let movement = movement::prepare(
        &p.items,
        target.to_str().unwrap(),
        movement::Conflict::Skip,
        &settings,
    )
    .unwrap();
    let output = target.join("owned.txt");
    let temporary = target.join(".owned.partial");
    let job = Job::new(TaskKind::PersonalTransfer);
    let copy = native::copy(
        &p.items[0],
        &movement,
        output.to_str().unwrap(),
        &temporary,
        &job,
        Instant::now(),
        || Ok(()),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        std::fs::read(&output).unwrap()
    );
    assert!(!temporary.exists());
    native::verify_copy(&copy, &job, Instant::now()).unwrap();
    assert!(native::copy(
        &p.items[0],
        &movement,
        output.to_str().unwrap(),
        &temporary,
        &job,
        Instant::now(),
        || Ok(())
    )
    .is_err());
    assert!(path.exists());
    assert_eq!(
        std::fs::read_to_string(source.join("unselected.txt")).unwrap(),
        "unselected keep\n"
    );
    assert!(prepare(
        p,
        target.to_str().unwrap(),
        movement::Conflict::Skip,
        &settings
    )
    .is_err());
}
#[test]
fn ctime_only_source_change_still_refuses_publication_and_keeps_original() {
    let (_temp, source, target) = fixture();
    let p = selected(&source);
    let path = source.join("owned.txt");
    let settings = db::Settings {
        acknowledged: true,
        ..Default::default()
    };
    let movement = movement::prepare(
        &p.items,
        target.to_str().unwrap(),
        movement::Conflict::Skip,
        &settings,
    )
    .unwrap();
    let output = target.join("owned.txt");
    let temporary = target.join(".owned.partial");
    let count = std::sync::atomic::AtomicUsize::new(0);
    let job = Job::new(TaskKind::PersonalTransfer);
    let result = native::copy(
        &p.items[0],
        &movement,
        output.to_str().unwrap(),
        &temporary,
        &job,
        Instant::now(),
        || {
            if count.fetch_add(1, Ordering::SeqCst) > 0 {
                let permissions = std::fs::metadata(&path).unwrap().permissions();
                std::fs::set_permissions(&path, permissions).unwrap();
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert!(!output.exists());
    assert!(!temporary.exists());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "owned transfer data\n"
    );
}
#[test]
fn cancelling_before_copy_publication_cleans_only_the_owned_temporary_and_keeps_source() {
    let (_temp, source, target) = fixture();
    let p = selected(&source);
    let settings = db::Settings {
        acknowledged: true,
        ..Default::default()
    };
    let movement = movement::prepare(
        &p.items,
        target.to_str().unwrap(),
        movement::Conflict::Skip,
        &settings,
    )
    .unwrap();
    let output = target.join("owned.txt");
    let temporary = target.join(".owned.partial");
    let job = Job::new(TaskKind::PersonalTransfer);
    let count = std::sync::atomic::AtomicUsize::new(0);
    assert!(native::copy(
        &p.items[0],
        &movement,
        output.to_str().unwrap(),
        &temporary,
        &job,
        Instant::now(),
        || {
            if count.fetch_add(1, Ordering::SeqCst) > 0 {
                job.cancel();
            }
            stopped(&job, Instant::now())
        }
    )
    .is_err());
    assert!(!output.exists());
    assert!(!temporary.exists());
    assert!(source.join("owned.txt").exists());
}
#[tokio::test]
#[ignore = "formal Mole rules, two actual APFS volumes and native Trash on unique self-created data only"]
async fn real_cross_volume_move_keeps_both_receipts_metadata_unselected_files_and_reopen_evidence()
{
    let (temp, source, _) = fixture();
    let external = tempfile::Builder::new()
        .prefix("MooMole-cross-volume-owned-")
        .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap())
        .unwrap();
    let target = external.path().canonicalize().unwrap();
    assert_ne!(
        std::fs::metadata(&source).unwrap().dev(),
        std::fs::metadata(&target).unwrap().dev(),
        "needs two real volumes"
    );
    let home = temp.path().canonicalize().unwrap();
    let settings = db::Settings {
        acknowledged: true,
        ..Default::default()
    };
    let database = db::open_owned(&home.join("database.sqlite")).unwrap();
    db::save_settings(&database, &settings).unwrap();
    let database = Arc::new(Mutex::new(database));
    let p = prepare(
        selected(&source),
        target.to_str().unwrap(),
        movement::Conflict::Skip,
        &settings,
    )
    .unwrap();
    let context = Context {
        database: database.clone(),
        home: home.clone(),
        engine_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
    };
    let record = run(
        context,
        p,
        uuid::Uuid::new_v4().to_string(),
        Arc::new(Job::new(TaskKind::PersonalTransfer)),
    )
    .await
    .unwrap();
    let entry = &record.items[0];
    if let Some(r) = &entry.source_trash_receipt {
        let m = std::fs::symlink_metadata(&r.destination).unwrap();
        assert_eq!((m.dev(), m.ino()), (r.device, r.inode));
        objc2::rc::autoreleasepool(|_| {
            use objc2_foundation::{NSFileManager, NSString, NSURL};
            NSFileManager::defaultManager()
                .moveItemAtURL_toURL_error(
                    &NSURL::fileURLWithPath(&NSString::from_str(&r.destination)),
                    &NSURL::fileURLWithPath(&NSString::from_str(&entry.path)),
                )
                .unwrap();
        });
    }
    assert_eq!(
        record.state,
        "completed",
        "{}",
        serde_json::to_string_pretty(&record).unwrap()
    );
    assert_eq!(entry.state, "moved");
    let copy = entry.copy_receipt.as_ref().unwrap();
    let original = entry.source_trash_receipt.as_ref().unwrap();
    assert_ne!(copy.device, original.device);
    assert_eq!(
        std::fs::read(&entry.path).unwrap(),
        std::fs::read(&copy.destination).unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(source.join("unselected.txt")).unwrap(),
        "unselected keep\n"
    );
    let context = || Context {
        database: database.clone(),
        home: home.clone(),
        engine_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
    };
    // Cancellation and process interruption are independent operations. Keep
    // the just-retrieved original untouched: Foundation restoration can be
    // followed by a ctime-only change, which must still invalidate a scan.
    // Use identical fresh data + resource fork + quarantine fixtures, rather
    // than relaxing the production witness after restoring the first sample.
    let (_cancel_temp, cancel_source, _) = fixture();
    let p = prepare(
        selected(&cancel_source),
        target.to_str().unwrap(),
        movement::Conflict::KeepBoth,
        &settings,
    )
    .unwrap();
    let cancelled = run_with(
        context(),
        p,
        uuid::Uuid::new_v4().to_string(),
        Arc::new(Job::new(TaskKind::PersonalTransfer)),
        |_, job| {
            job.cancel();
            Ok(())
        },
    )
    .await
    .unwrap();
    assert_eq!(
        cancelled.state,
        "partial",
        "{}",
        serde_json::to_string_pretty(&cancelled).unwrap()
    );
    assert_eq!(cancelled.items[0].state, "copyOnly");
    assert!(cancelled.items[0].source_trash_receipt.is_none());
    assert!(cancel_source.join("owned.txt").exists());
    assert!(cancelled.items[0].copy_receipt.is_some());
    journal::acknowledge(&*database.lock().unwrap(), &cancelled.id).unwrap();
    let (_fault_temp, fault_source, _) = fixture();
    let p = prepare(
        selected(&fault_source),
        target.to_str().unwrap(),
        movement::Conflict::KeepBoth,
        &settings,
    )
    .unwrap();
    assert!(run_with(
        context(),
        p,
        uuid::Uuid::new_v4().to_string(),
        Arc::new(Job::new(TaskKind::PersonalTransfer)),
        |_, _| Err("owned injected interruption after copy evidence".into())
    )
    .await
    .is_err());
    assert!(fault_source.join("owned.txt").exists());
    drop(database);
    let reopened = db::open_owned(&home.join("database.sqlite")).unwrap();
    let rows = journal::list(&reopened).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].state, "interrupted");
    assert!(rows[0].items[0].copy_receipt.is_some());
    assert!(rows[0].items[0].source_trash_receipt.is_none());
    assert_eq!(
        serde_json::to_value(&rows[2]).unwrap(),
        serde_json::to_value(&record).unwrap()
    );
    assert_eq!(
        std::fs::read(source.join("owned.txt")).unwrap(),
        std::fs::read(&copy.destination).unwrap()
    );
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/cross-volume-native.json",serde_json::to_vec_pretty(&serde_json::json!({"record":record,"copyThenCancel":cancelled,"reopened":rows,"twoActualVolumes":true,"ownedOriginalRetrievedByReceipt":true,"reopenVerified":true,"interruptionDidNotReplay":true})).unwrap()).unwrap();
}
