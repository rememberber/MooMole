use super::*;
use std::path::PathBuf;
fn job() -> Arc<Job> {
    Arc::new(Job::new(TaskKind::PersonalScan))
}
#[test]
fn raw_source_is_exact_bounded_cancelled_and_changed_files_are_retained() {
    let t = tempfile::Builder::new()
        .prefix("MooMole-raw-source-")
        .tempdir()
        .unwrap();
    let root = t.path().canonicalize().unwrap();
    let path = root.join("owned.dng");
    std::fs::write(&path, b"owned bytes").unwrap();
    let j = job();
    let scan = super::super::scan::collect(&root, &[], j.clone()).unwrap();
    let item = &scan.items[0];
    let mut copy = tempfile::tempfile().unwrap();
    let mut read = 0;
    assert!(content(
        item,
        &[item.path.clone()],
        &j,
        Instant::now(),
        Some(&mut copy),
        &mut read
    )
    .is_err());
    assert_eq!(read, 0);
    j.cancel();
    assert!(content(item, &[], &j, Instant::now(), Some(&mut copy), &mut read).is_err());
    assert_eq!(copy.metadata().unwrap().len(), 0);
    let j = job();
    assert_eq!(
        content(item, &[], &j, Instant::now(), None, &mut read).unwrap(),
        format!("{:x}", Sha256::digest(b"owned bytes"))
    );
    std::fs::write(&path, b"changed now").unwrap();
    assert!(content(item, &[], &j, Instant::now(), None, &mut 0).is_err());
}
#[test]
fn ordinary_png_and_tiff_never_obtain_camera_raw_identity_from_an_extension_hint() {
    objc2::rc::autoreleasepool(|_| {
        let png = media::fixture_png(128, 96);
        assert!(media::Raster::decode(&png, 384).is_ok());
        let tiff = media::fixture_tiff();
        assert!(media::Raster::decode(&tiff, 384).is_ok());
        for bytes in [&png, &tiff] {
            for ext in ["dng", "nef", "cr2"] {
                assert!(
                    media::Raster::decode_raw(bytes, 384, ext).is_err(),
                    "ordinary image accepted as {ext}"
                );
            }
        }
    });
}
#[test]
fn worker_rejects_unbound_or_nonanonymous_inputs_before_native_decoding() {
    let mut f = tempfile::tempfile().unwrap();
    f.write_all(b"owned").unwrap();
    let m = f.metadata().unwrap();
    let input = worker::Input {
        id: uuid::Uuid::new_v4().to_string(),
        sha256: "a".repeat(64),
        bytes: 5,
        device: m.dev().to_string(),
        inode: m.ino().to_string(),
        max: 1024,
        extension: "dng".into(),
    };
    assert!(worker::decode_file(input, f).is_err());
    let mut f = tempfile::tempfile().unwrap();
    f.write_all(b"owned").unwrap();
    let m = f.metadata().unwrap();
    let input = worker::Input {
        id: uuid::Uuid::new_v4().to_string(),
        sha256: format!("{:x}", Sha256::digest(b"owned")),
        bytes: 5,
        device: m.dev().to_string(),
        inode: m.ino().to_string(),
        max: 4096,
        extension: "dng".into(),
    };
    assert!(worker::decode_file(input, f).is_err());
}

#[test]
fn named_files_and_changed_worker_results_are_refused() {
    objc2::rc::autoreleasepool(|_| {
        let png = media::fixture_png(128, 96);
        let mut named = tempfile::NamedTempFile::new().unwrap();
        named.write_all(&png).unwrap();
        let m = named.as_file().metadata().unwrap();
        let input = worker::Input {
            id: uuid::Uuid::new_v4().to_string(),
            sha256: format!("{:x}", Sha256::digest(&png)),
            bytes: png.len() as u64,
            device: m.dev().to_string(),
            inode: m.ino().to_string(),
            max: 1024,
            extension: "dng".into(),
        };
        assert!(worker::decode_file(
            serde_json::from_value(serde_json::to_value(&input).unwrap()).unwrap(),
            named.reopen().unwrap()
        )
        .is_err());
        let output = worker::Output {
            id: input.id.clone(),
            sha256: input.sha256.clone(),
            width: 128,
            height: 96,
            type_identifier: "com.adobe.raw-image".into(),
            png: media::Raster::decode(&png, 384).unwrap().png().unwrap(),
        };
        assert!(output.validate(&input).is_ok());
        for (key, value) in [
            ("id", serde_json::json!(uuid::Uuid::new_v4().to_string())),
            ("sha256", serde_json::json!("b".repeat(64))),
            ("width", serde_json::json!(0)),
            ("height", serde_json::json!(60001)),
            ("typeIdentifier", serde_json::json!("")),
            ("png", serde_json::json!("data:image/png;base64,bad")),
        ] {
            let mut changed = serde_json::to_value(&output).unwrap();
            changed[key] = value;
            let result: worker::Output = serde_json::from_value(changed).unwrap();
            assert!(
                result.validate(&input).is_err(),
                "accepted changed field: {key}"
            );
        }
    });
}

#[tokio::test]
#[ignore = "requires built owned worker; proves independent 20-second watchdog without a supervising GUI"]
async fn owned_worker_stops_waiting_for_input_without_a_supervisor() {
    use std::process::Stdio;
    let began = Instant::now();
    let mut child = tokio::process::Command::new(executable().unwrap())
        .arg("--moomole-raw-worker-v1")
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let input = child.stdin.take().unwrap();
    let result = tokio::time::timeout(Duration::from_secs(25), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    drop(input);
    assert_eq!(result.status.code(), Some(3));
    assert!(result.stdout.is_empty());
    assert!(began.elapsed() >= Duration::from_secs(19));
}

#[tokio::test]
#[ignore = "real supervised RAW ImageIO/Vision on transient CC0 camera samples; own copy Trash/receipt return, SQLite reopen and archive, all temp roots cleaned"]
async fn real_camera_raw_previews_comparison_and_separate_native_trash_preserve_originals() {
    use crate::rules;
    let sample_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.cache/raw-samples");
    let sources: serde_json::Value = serde_json::from_slice(
        &std::fs::read(sample_root.join("source-manifest.json"))
            .expect("download only the documented CC0 samples into controlled project scratch"),
    )
    .unwrap();
    let mut temp = tempfile::Builder::new()
        .prefix("MooMole-raw-owned-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let root = home.join("Pictures");
    std::fs::create_dir(&root).unwrap();
    for name in ["ricoh.dng", "nikon.nef", "canon.cr2"] {
        let bytes = std::fs::read(sample_root.join(name)).unwrap();
        let entry = sources
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["file"] == name)
            .unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            entry["sha256"].as_str().unwrap()
        );
        std::fs::write(root.join(name), bytes).unwrap();
    }
    std::fs::write(root.join("ricoh.xmp"), b"owned sidecar must stay").unwrap();
    let scan = super::super::scan::collect(&root, &[], job()).unwrap();
    let mut previews = vec![];
    let mut ricoh_png = None;
    for name in ["ricoh.dng", "nikon.nef", "canon.cr2"] {
        eprintln!("Checking public RAW sample: {name}");
        let item = scan
            .items
            .iter()
            .find(|i| i.path.ends_with(name))
            .unwrap()
            .clone();
        let selected = item.clone();
        let out = tokio::task::spawn_blocking(move || {
            objc2::rc::autoreleasepool(|_| {
                analyze(&selected, &[], job(), Instant::now(), 1024, &mut 0)
            })
        })
        .await
        .unwrap()
        .unwrap();
        assert!(out.width > 1000 && out.height > 1000);
        assert_eq!(
            out.sha256,
            format!("{:x}", Sha256::digest(std::fs::read(&item.path).unwrap()))
        );
        if name == "ricoh.dng" {
            ricoh_png = Some(png_bytes(&out.png).unwrap())
        }
        previews.push(serde_json::json!({"scanId":scan.id,"itemId":item.id,"path":item.path,"content":{"kind":"rawImage","dataUrl":out.png,"width":out.width,"height":out.height,"typeIdentifier":out.type_identifier}}));
    }
    // Cancel the actual owned decoder after it starts, before any file action.
    let selected = scan
        .items
        .iter()
        .find(|i| i.path.ends_with("nikon.nef"))
        .unwrap()
        .clone();
    let control = job();
    let child_job = control.clone();
    let task = tokio::task::spawn_blocking(move || {
        objc2::rc::autoreleasepool(|_| {
            analyze(&selected, &[], child_job, Instant::now(), 1024, &mut 0)
        })
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while control.process_id().is_none() {
            assert!(
                !task.is_finished(),
                "decoder finished before cancellation was exercised"
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    control.cancel();
    assert!(task.await.unwrap().is_err());
    assert_eq!(control.process_id(), None);
    let group_root = home.join("Compare");
    std::fs::create_dir(&group_root).unwrap();
    let original = std::fs::read(root.join("ricoh.dng")).unwrap();
    std::fs::write(group_root.join("a-selected.dng"), &original).unwrap();
    let mut variant = original.clone();
    variant.extend(b"MooMole owned trailing annotation");
    std::fs::write(group_root.join("b-keeper.dng"), &variant).unwrap();
    std::fs::write(group_root.join("c-preview.png"), ricoh_png.unwrap()).unwrap();
    std::fs::write(
        group_root.join("a-selected.xmp"),
        b"owned adjustment sidecar keep",
    )
    .unwrap();
    std::fs::write(group_root.join("unselected.txt"), b"unselected keep").unwrap();
    let root_copy = group_root.clone();
    let grouped = tokio::task::spawn_blocking(move || {
        objc2::rc::autoreleasepool(|_| {
            let scan = super::super::scan::collect(&root_copy, &[], job())?;
            super::super::similar_photos::collect_with_sequence(scan, job(), &[], false)
        })
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        grouped.report.similar_photos.as_ref().unwrap().groups.len(),
        1,
        "{}",
        serde_json::to_string(&grouped.report).unwrap()
    );
    assert_eq!(grouped.items.len(), 3);
    let public_report = serde_json::to_value(&grouped.report).unwrap();
    let selected = grouped
        .items
        .iter()
        .find(|i| i.path.ends_with("a-selected.dng"))
        .unwrap()
        .clone();
    let mut store = Store {
        scan: Some(grouped),
        ..Default::default()
    };
    let all = store
        .scan
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| i.id.clone())
        .collect::<Vec<_>>();
    assert!(store.offer(&all, db::now()).is_err());
    let offer = store.offer(&[selected.id.clone()], db::now()).unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    let keeper = plan.similar_photos.as_ref().unwrap()[0].keeper.clone();
    assert!(keeper.path.ends_with("b-keeper.dng"));
    let path = home.join("state.sqlite");
    let database = db::open_owned(&path).unwrap();
    db::save_settings(
        &database,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    let database = Arc::new(std::sync::Mutex::new(database));
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
    let rule_home = home.clone();
    temp.disable_cleanup(true);
    eprintln!("Owned RAW acceptance rescue root: {}", home.display());
    let rescue_home = home.clone();
    let result = super::super::writer::run_with(
        database.clone(),
        home.clone(),
        plan,
        Arc::new(Job::new(TaskKind::PersonalTrash)),
        move |requested, control| {
            let (engine, home) = (engine.clone(), rule_home.clone());
            async move { rules::run(&engine, &home, &requested, control).await }
        },
        move |id| {
            std::fs::write(
                "../.cache/raw-photo-native-rescue.json",
                serde_json::to_vec(&serde_json::json!({"ownedRoot":rescue_home,"runId":id}))
                    .unwrap(),
            )
            .unwrap();
        },
    )
    .await
    .unwrap();
    std::fs::write(
        "../.cache/raw-photo-native-rescue.json",
        serde_json::to_vec(&serde_json::json!({"ownedRoot":home,"result":result})).unwrap(),
    )
    .unwrap();
    for row in &result.items {
        if let Some(receipt) = &row.receipt {
            let m = std::fs::symlink_metadata(&receipt.destination).unwrap();
            assert!(m.is_file());
            assert_eq!((m.dev(), m.ino()), (receipt.device, receipt.inode));
            assert_eq!(std::fs::read(&receipt.destination).unwrap(), original);
            assert!(!Path::new(&row.path).exists());
            std::fs::rename(&receipt.destination, &row.path).unwrap();
            assert_eq!(std::fs::read(&row.path).unwrap(), original);
        }
    }
    temp.disable_cleanup(false);
    std::fs::remove_file("../.cache/raw-photo-native-rescue.json").unwrap();
    assert_eq!(
        result.summary.moved_count,
        1,
        "{}",
        serde_json::to_string(&result).unwrap()
    );
    assert_eq!(std::fs::read(&keeper.path).unwrap(), variant);
    assert_eq!(
        std::fs::read(group_root.join("a-selected.xmp")).unwrap(),
        b"owned adjustment sidecar keep"
    );
    assert_eq!(
        std::fs::read(group_root.join("unselected.txt")).unwrap(),
        b"unselected keep"
    );
    assert_eq!(
        std::fs::read(root.join("ricoh.xmp")).unwrap(),
        b"owned sidecar must stay"
    );
    let expected = serde_json::to_value(&result).unwrap();
    drop(database);
    let mut reopened = db::open_owned(&path).unwrap();
    assert_eq!(
        serde_json::to_value(crate::execution::detail(&reopened, &result.summary.id).unwrap())
            .unwrap(),
        expected
    );
    let draft = crate::execution_archive::prepare(&mut reopened).unwrap();
    let archive_root = home.join("Archive");
    let h = crate::execution_archive::archive(
        &mut reopened,
        &archive_root,
        &draft.fingerprint,
        db::now(),
    )
    .unwrap();
    assert_eq!(
        crate::execution_archive::detail(
            &archive_root,
            &h.id,
            crate::execution_archive::Kind::Cleanup,
            &result.summary.id
        )
        .unwrap(),
        expected
    );
    drop(reopened);
    let temp_path = temp.path().to_path_buf();
    drop(temp);
    assert!(!temp_path.exists());
    std::fs::write("../.cache/raw-photo-native.json",serde_json::to_vec(&serde_json::json!({"scope":"native core, not packaged App window acceptance","cameraSources":sources,"previews":previews,"report":public_report,"result":expected,"anonymousSnapshotWorker":true,"actualWorkerCancellationAndReapVerified":true,"sourceFilesAndSidecarsPreserved":true,"exactOwnedReceiptReturned":true,"databaseReopenAndArchiveVerified":true,"ownedTemporaryRootRemoved":true})).unwrap()).unwrap();
}
