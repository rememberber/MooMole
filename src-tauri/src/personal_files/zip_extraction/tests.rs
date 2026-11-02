use super::*;
use std::{fs, os::unix::fs::symlink};
fn config() -> db::Settings {
    db::Settings {
        acknowledged: true,
        ..Default::default()
    }
}
fn job() -> Job {
    Job::new(TaskKind::PersonalArchive)
}
fn write_zip(path: &Path, names: &[(&str, &[u8])]) {
    let f = File::create(path).unwrap();
    let mut z = zip::ZipWriter::new(f);
    for (name, data) in names {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        if name.ends_with('/') {
            z.add_directory(*name, options).unwrap();
        } else {
            z.start_file(*name, options).unwrap();
            z.write_all(data).unwrap();
        }
    }
    z.finish().unwrap();
}
fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let t = tempfile::Builder::new()
        .prefix("MooMole-zip-extract-owned-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let zip = home.join("sample.zip");
    let out = home.join("Output");
    fs::create_dir(&out).unwrap();
    write_zip(
        &zip,
        &[
            ("Trip/Empty/", b""),
            ("Trip/照片.txt", b"owned unicode"),
            ("Trip/.hidden", b"owned hidden"),
            ("__MACOSX/Trip/._photo", b"raw AppleDouble kept"),
        ],
    );
    (t, zip, out)
}
#[test]
fn zip_inventory_rejects_traversal_duplicates_links_corruption_and_allocation_bombs() {
    let (_t, zip, out) = fixture();
    let cfg = config();
    let j = job();
    let p = Plan::prepare(&zip, out.to_str().unwrap(), "Extracted", &cfg, &j).unwrap();
    assert_eq!(p.draft.files, 3);
    assert_eq!(p.draft.directories, 5);
    assert!(p.draft.metadata_bytes > 0);
    for names in [
        vec![("../outside", b"owned".as_slice())],
        vec![("/absolute", b"owned".as_slice())],
        vec![("a/../../outside", b"owned".as_slice())],
        vec![("a", b"1".as_slice()), ("A", b"2")],
        vec![("a", b"1".as_slice()), ("a/file", b"2")],
        vec![("bad\\name", b"1".as_slice())],
    ] {
        write_zip(&zip, &names);
        assert!(Plan::prepare(&zip, out.to_str().unwrap(), "Extracted", &cfg, &j).is_err());
    }
    let f = File::create(&zip).unwrap();
    let mut z = zip::ZipWriter::new(f);
    z.add_symlink(
        "link",
        "../outside",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    z.finish().unwrap();
    assert!(Plan::prepare(&zip, out.to_str().unwrap(), "Extracted", &cfg, &j).is_err());
    write_zip(&zip, &[("data", b"own")]);
    let mut bytes = fs::read(&zip).unwrap();
    let pos = bytes.windows(4).position(|x| x == b"PK\x01\x02").unwrap();
    bytes[pos + 24..pos + 28].copy_from_slice(&u32::MAX.to_le_bytes());
    fs::write(&zip, &bytes).unwrap();
    assert!(Plan::prepare(&zip, out.to_str().unwrap(), "Extracted", &cfg, &j).is_err());
    let mut payload = [0u8; 56];
    payload[..4].copy_from_slice(b"PK\x06\x06");
    payload[4..12].copy_from_slice(&44u64.to_le_bytes());
    payload[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
    let f = File::create(&zip).unwrap();
    let mut z = zip::ZipWriter::new(f);
    z.start_file(
        "embedded",
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .unwrap();
    z.write_all(&payload).unwrap();
    z.finish().unwrap();
    assert!(Plan::prepare(&zip, out.to_str().unwrap(), "Extracted", &cfg, &j).is_err());
    assert!(!out.join("Extracted").exists());
}
#[test]
fn plans_bind_original_target_protection_expiry_and_stop_without_output() {
    let (t, zip, out) = fixture();
    let cfg = config();
    let j = job();
    let mut p = Plan::prepare(&zip, out.to_str().unwrap(), "Extracted", &cfg, &j).unwrap();
    p.issued = Instant::now() - Duration::from_secs(121);
    assert!(p.time().is_err());
    let mut protect = cfg.clone();
    protect.protected_paths.push(zip.to_string_lossy().into());
    assert!(Plan::prepare(&zip, out.to_str().unwrap(), "New", &protect, &j).is_err());
    protect.protected_paths = vec![out.join("New/private").to_string_lossy().into()];
    assert!(Plan::prepare(&zip, out.to_str().unwrap(), "New", &protect, &j).is_err());
    fs::create_dir(out.join("Existing")).unwrap();
    assert!(Plan::prepare(&zip, out.to_str().unwrap(), "Existing", &cfg, &j).is_err());
    symlink(&zip, t.path().join("link.zip")).unwrap();
    assert!(Plan::prepare(
        &t.path().join("link.zip"),
        out.to_str().unwrap(),
        "Linked",
        &cfg,
        &j
    )
    .is_err());
    let p = Plan::prepare(&zip, out.to_str().unwrap(), "Extracted", &cfg, &j).unwrap();
    fs::write(&zip, b"changed").unwrap();
    assert!(p.validate(&cfg, &j, Instant::now()).is_err());
    write_zip(&zip, &[("file", b"owned")]);
    let p = Plan::prepare(&zip, out.to_str().unwrap(), "Extracted", &cfg, &j).unwrap();
    let db = db::open_owned(&t.path().join("state.sqlite")).unwrap();
    db::save_settings(&db, &cfg).unwrap();
    let j = job();
    j.cancel();
    let r = run(&Mutex::new(db), p, uuid::Uuid::new_v4().to_string(), &j).unwrap();
    assert_eq!(r.state, "cancelled");
    assert!(!out.join("Extracted").exists());
    assert_eq!(fs::read_dir(&out).unwrap().count(), 1);
}
#[test]
fn staging_never_removes_unexpected_content_or_replaced_objects() {
    let (_t, zip, out) = fixture();
    let cfg = config();
    let mut p = Plan::prepare(&zip, out.to_str().unwrap(), "Extracted", &cfg, &job()).unwrap();
    let parent = movement::directory(out.to_str().unwrap(), &cfg).unwrap();
    let mut s = tree::Stage::create(&parent, &p.draft.id).unwrap();
    s.extract(
        &mut p.snapshot,
        p.draft.format,
        &p.entries,
        None,
        &job(),
        Instant::now(),
    )
    .unwrap();
    let path = out.join(s.name.to_str().unwrap());
    fs::write(path.join("user-added"), b"retain").unwrap();
    assert!(s.verify(&p.entries, None, &job(), Instant::now()).is_err());
    assert!(!s.cleanup());
    assert_eq!(fs::read(path.join("user-added")).unwrap(), b"retain");
    drop(s);
}
#[test]
fn a_destination_created_after_confirmation_is_preserved_and_owned_staging_is_cleaned() {
    let (t, zip, out) = fixture();
    let cfg = config();
    let p = Plan::prepare(&zip, out.to_str().unwrap(), "Extracted", &cfg, &job()).unwrap();
    fs::create_dir(out.join("Extracted")).unwrap();
    fs::write(out.join("Extracted/user.txt"), b"user keep").unwrap();
    let c = db::open_owned(&t.path().join("state.sqlite")).unwrap();
    db::save_settings(&c, &cfg).unwrap();
    let r = run(&Mutex::new(c), p, uuid::Uuid::new_v4().to_string(), &job()).unwrap();
    assert_eq!(r.state, "unknown");
    assert_eq!(
        fs::read(out.join("Extracted/user.txt")).unwrap(),
        b"user keep"
    );
    assert!(!Path::new(&r.temporary_path).exists());
}
#[test]
fn bounded_zip64_end_records_and_empty_content_are_read_without_truncating_names() {
    let (_t, zip, out) = fixture();
    write_zip(&zip, &[("empty.txt", b"")]);
    let mut b = fs::read(&zip).unwrap();
    let end = b.len() - 22;
    let count = u16::from_le_bytes(b[end + 10..end + 12].try_into().unwrap()) as u64;
    let size = u32::from_le_bytes(b[end + 12..end + 16].try_into().unwrap()) as u64;
    let offset = u32::from_le_bytes(b[end + 16..end + 20].try_into().unwrap()) as u64;
    let mut footer = b.split_off(end);
    let mut z = Vec::from(b"PK\x06\x06".as_slice());
    z.extend(44u64.to_le_bytes());
    z.extend([45, 0, 45, 0]);
    z.extend([0; 8]);
    z.extend(count.to_le_bytes());
    z.extend(count.to_le_bytes());
    z.extend(size.to_le_bytes());
    z.extend(offset.to_le_bytes());
    assert_eq!(z.len(), 56);
    b.extend(z);
    b.extend(b"PK\x06\x07");
    b.extend([0; 4]);
    b.extend((end as u64).to_le_bytes());
    b.extend(1u32.to_le_bytes());
    footer[8..12].fill(255);
    footer[12..20].fill(255);
    b.extend(footer);
    fs::write(&zip, &b).unwrap();
    let p = Plan::prepare(&zip, out.to_str().unwrap(), "Extracted", &config(), &job()).unwrap();
    assert_eq!(p.draft.bytes, 0);
    assert_eq!(p.draft.files, 1);
}
fn native(lost: bool) {
    let (t, zip, out) = fixture();
    let cfg = config();
    let held = source(&zip, &cfg).unwrap();
    let q = b"0083;65aa0010;MooMoleAcceptance;";
    assert_eq!(
        unsafe {
            libc::fsetxattr(
                held.target().as_raw_fd(),
                c"com.apple.quarantine".as_ptr(),
                q.as_ptr().cast(),
                q.len(),
                0,
                0,
            )
        },
        0
    );
    let original = fs::read(&zip).unwrap();
    let p = Plan::prepare(&zip, out.to_str().unwrap(), "Extracted", &cfg, &job()).unwrap();
    let dbpath = t.path().join("state.sqlite");
    let c = db::open_owned(&dbpath).unwrap();
    db::save_settings(&c, &cfg).unwrap();
    if lost {
        c.execute_batch(journal::TABLE).unwrap();
        c.execute_batch("CREATE TRIGGER lost_extraction_result BEFORE UPDATE ON zip_extractions WHEN json_extract(NEW.body,'$.state')='completed' BEGIN SELECT RAISE(ABORT,'owned lost result'); END;").unwrap();
    }
    let database = Mutex::new(c);
    let result = run(&database, p, uuid::Uuid::new_v4().to_string(), &job());
    if lost {
        assert!(result.is_err());
    } else {
        assert_eq!(result.unwrap().state, "completed");
    }
    assert_eq!(fs::read(&zip).unwrap(), original);
    assert_eq!(quarantine(held.target()).unwrap().unwrap(), q);
    assert_eq!(
        fs::read(out.join("Extracted/Trip/照片.txt")).unwrap(),
        b"owned unicode"
    );
    assert_eq!(
        fs::read(out.join("Extracted/Trip/.hidden")).unwrap(),
        b"owned hidden"
    );
    assert!(out.join("Extracted/Trip/Empty").is_dir());
    assert_eq!(
        fs::read(out.join("Extracted/__MACOSX/Trip/._photo")).unwrap(),
        b"raw AppleDouble kept"
    );
    let extracted = File::open(out.join("Extracted/Trip/照片.txt")).unwrap();
    assert_eq!(quarantine(&extracted).unwrap().unwrap(), q);
    assert_eq!(extracted.metadata().unwrap().mode() & 0o777, 0o600);
    drop(database);
    let c = db::open_owned(&dbpath).unwrap();
    let rows = journal::reconcile(&c, &[]).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, if lost { "unknown" } else { "completed" });
    assert_eq!(journal::admission(&c).is_err(), lost);
    let tx = c.unchecked_transaction().unwrap();
    assert_eq!(
        crate::execution_admission::reserve(&tx, 131072).is_err(),
        lost
    );
    drop(tx);
    fs::create_dir_all("../.cache").unwrap();
    fs::write(format!("../.cache/zip-extraction-native-{}.json",if lost{"lost-completion"}else{"completed"}),serde_json::to_vec_pretty(&serde_json::json!({"records":rows,"originalZipAndQuarantinePreserved":true,"unicodeHiddenDataEmptyDirectoriesAndRawAppleDoubleVerified":true,"extractedQuarantineAndPrivatePermissionsVerified":true,"databaseReopenedWithoutReplay":true,"appWindowVerified":false})).unwrap()).unwrap();
}
#[test]
#[ignore = "real ZIP extraction and lost durable result on automatically cleaned system fixtures only"]
fn real_zip_extraction_and_lost_completion_preserve_source_and_never_replay() {
    native(false);
    native(true);
    // Independent macOS ZIP producer, including raw AppleDouble files.
    let (t, zip, out) = fixture();
    let root = t.path().canonicalize().unwrap().join("Source");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("照片.txt"), b"independent unicode").unwrap();
    fs::write(root.join(".hidden"), b"independent hidden").unwrap();
    fs::create_dir(root.join("Empty")).unwrap();
    let held = AnchoredPath::open(&root.join("照片.txt")).unwrap();
    let resource = b"owned resource fork";
    assert_eq!(
        unsafe {
            libc::fsetxattr(
                held.target().as_raw_fd(),
                c"com.apple.ResourceFork".as_ptr(),
                resource.as_ptr().cast(),
                resource.len(),
                0,
                0,
            )
        },
        0
    );
    fs::remove_file(&zip).unwrap();
    assert!(std::process::Command::new("/usr/bin/ditto")
        .args(["-c", "-k", "--sequesterRsrc", "--keepParent"])
        .arg(&root)
        .arg(&zip)
        .status()
        .unwrap()
        .success());
    let p = Plan::prepare(
        &zip,
        out.to_str().unwrap(),
        "Independent",
        &config(),
        &job(),
    )
    .unwrap();
    assert!(p.draft.metadata_bytes > 0);
    let c = db::open_owned(&t.path().join("independent.sqlite")).unwrap();
    db::save_settings(&c, &config()).unwrap();
    let r = run(&Mutex::new(c), p, uuid::Uuid::new_v4().to_string(), &job()).unwrap();
    assert_eq!(r.state, "completed");
    assert_eq!(
        fs::read(out.join("Independent/Source/照片.txt")).unwrap(),
        b"independent unicode"
    );
    assert_eq!(
        fs::read(out.join("Independent/Source/.hidden")).unwrap(),
        b"independent hidden"
    );
    assert!(out.join("Independent/Source/Empty").is_dir());
    fs::write("../.cache/zip-extraction-native-independent.json",serde_json::to_vec_pretty(&serde_json::json!({"records":[r],"independentDittoZipWithRawAppleDoubleExtracted":true,"originalSourceAndZipRetained":true,"appWindowVerified":false})).unwrap()).unwrap();
}
