use super::*;
use std::{fs, os::unix::fs::MetadataExt, process::Command};
fn job() -> Job {
    Job::new(TaskKind::PersonalArchive)
}
fn settings() -> db::Settings {
    db::Settings {
        acknowledged: true,
        ..Default::default()
    }
}
fn checksum(b: &mut [u8]) {
    b[148..156].fill(b' ');
    let sum = b.iter().map(|b| *b as u64).sum::<u64>();
    b[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
}
fn append(out: &mut Vec<u8>, name: &str, kind: u8, data: &[u8]) {
    let mut h = [0u8; 512];
    h[..name.len()].copy_from_slice(name.as_bytes());
    h[100..108].copy_from_slice(b"0000755\0");
    h[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
    h[156] = kind;
    h[257..265].copy_from_slice(b"ustar\0\x30\x30");
    checksum(&mut h);
    out.extend(h);
    out.extend(data);
    out.resize(out.len().div_ceil(512) * 512, 0);
}
fn archive(entries: &[(&str, u8, &[u8])]) -> Vec<u8> {
    let mut out = vec![];
    for (name, kind, data) in entries {
        append(&mut out, name, *kind, data)
    }
    out.extend([0; 1024]);
    out
}
fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let body = format!(" {key}={value}\n");
    let mut len = body.len() + 1;
    loop {
        let s = format!("{len}{body}");
        if s.len() == len {
            return s.into_bytes();
        }
        len = s.len();
    }
}
fn read_bytes(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    let mut f = tempfile::tempfile().unwrap();
    f.write_all(bytes).unwrap();
    read(&mut f, &job(), Instant::now())
}
fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut f = flate2::write::GzEncoder::new(vec![], flate2::Compression::fast());
    f.write_all(bytes).unwrap();
    f.finish().unwrap()
}
#[test]
fn ustar_pax_and_gnu_names_bind_exact_offsets_and_hashes() {
    let mut a = archive(&[
        ("./", b'5', b""),
        ("./Trip/", b'5', b""),
        ("Trip/照片.txt", b'0', b"owned unicode"),
        ("Trip/.hidden", b'0', b"hidden"),
        ("Trip/Empty/", b'5', b""),
    ]);
    let rows = read_bytes(&a).unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows.iter()
            .find(|e| e.name == "Trip/照片.txt")
            .unwrap()
            .sha256,
        format!("{:x}", Sha256::digest(b"owned unicode"))
    );
    let long = format!("Trip/{}/photo.txt", "a".repeat(110));
    let pax = pax_record("path", &long);
    a = archive(&[("PaxHeader", b'x', &pax), ("short", b'0', b"pax bytes")]);
    assert!(read_bytes(&a).unwrap().iter().any(|e| e.name == long));
    let mut name = long.as_bytes().to_vec();
    name.push(0);
    let mut a = archive(&[
        ("././@LongLink", b'L', &name),
        ("short", b'0', b"gnu bytes"),
    ]);
    a[257..265].copy_from_slice(b"ustar  \0");
    checksum(&mut a[..512]);
    assert!(read_bytes(&a).unwrap().iter().any(|e| e.name == long));
    let mut p = pax_record("path", "override.txt");
    p.extend(pax_record("size", "3"));
    let mut a = archive(&[("PaxHeader", b'x', &p), ("short", b'0', b"abc")]);
    a[1024 + 124..1024 + 136].copy_from_slice(b"00000000000\0");
    checksum(&mut a[1024..1536]);
    assert_eq!(read_bytes(&a).unwrap()[0].size, 3);
}
#[test]
fn tar_rejects_escape_aliases_links_special_sparse_and_ambiguous_extensions() {
    for name in [
        "/absolute",
        "/",
        "../escape",
        "a/../../escape",
        "a\\b",
        "a:b",
        "a/./b",
        "bad\nname",
    ] {
        assert!(
            read_bytes(&archive(&[(name, b'5', b"")])).is_err(),
            "{name}"
        )
    }
    for kind in [b'1', b'2', b'3', b'4', b'6', b'S', b'K', b'V'] {
        assert!(read_bytes(&archive(&[("unsupported", kind, b"")])).is_err())
    }
    for entries in [
        vec![("a", b'0', b"a".as_slice()), ("A", b'0', b"b")],
        vec![("a", b'0', b"a".as_slice()), ("./a", b'0', b"b")],
        vec![("a", b'0', b"a".as_slice()), ("a/b", b'0', b"b")],
    ] {
        assert!(read_bytes(&archive(&entries)).is_err())
    }
    for (key, value) in [
        ("path", "../outside"),
        ("GNU.sparse.map", "0,5"),
        ("SCHILY.realsize", "5"),
        ("linkpath", "other"),
        ("hdrcharset", "BINARY"),
        ("unknown-layout", "x"),
    ] {
        let p = pax_record(key, value);
        assert!(
            read_bytes(&archive(&[("Pax", b'x', &p), ("safe", b'0', b"owned")])).is_err(),
            "{key}"
        )
    }
    let p = pax_record("path", "a");
    assert!(read_bytes(&archive(&[("Pax", b'g', &p), ("safe", b'0', b"owned")])).is_err());
    let mut p = pax_record("path", "a");
    p.extend(pax_record("path", "b"));
    assert!(read_bytes(&archive(&[("Pax", b'x', &p), ("safe", b'0', b"owned")])).is_err());
}
#[test]
fn tar_checksum_end_markers_declared_sizes_and_extension_allocations_are_bounded() {
    let a = archive(&[("owned", b'0', b"bytes")]);
    for mode in [
        "checksum",
        "noEnd",
        "append",
        "hugeSize",
        "hugePax",
        "negative",
        "invalidUtf8",
    ] {
        let mut b = a.clone();
        match mode {
            "checksum" => b[0] ^= 1,
            "noEnd" => b.truncate(b.len() - 512),
            "append" => {
                b.extend(a.clone());
            }
            "hugeSize" => {
                b[124..136].copy_from_slice(b"77777777777\0");
                checksum(&mut b[..512]);
            }
            "hugePax" => {
                b[156] = b'x';
                b[124..136].copy_from_slice(b"00000200001\0");
                checksum(&mut b[..512]);
            }
            "negative" => {
                b[124] = 0xff;
                checksum(&mut b[..512]);
            }
            _ => {
                b[0] = 255;
                checksum(&mut b[..512]);
            }
        }
        assert!(read_bytes(&b).is_err(), "{mode}");
    }
    assert!(read_bytes(&archive(&[("Pax", b'x', b"20 path=oops\n")])).is_err());
    let j = job();
    j.cancel();
    let mut f = tempfile::tempfile().unwrap();
    f.write_all(&a).unwrap();
    assert!(read(&mut f, &j, Instant::now()).is_err());
}
#[test]
fn gzip_crc_trailing_members_and_mislabeled_tar_are_not_silently_accepted() {
    let a = archive(&[("owned", b'0', b"bytes")]);
    let good = gzip(&a);
    for mode in [
        "good",
        "crc",
        "secondMember",
        "trailing",
        "truncated",
        "notGzip",
    ] {
        let mut b = good.clone();
        match mode {
            "crc" => {
                let i = b.len() - 8;
                b[i] ^= 1
            }
            "secondMember" => b.extend(gzip(b"ignored")),
            "trailing" => b.extend(b"unknown"),
            "truncated" => {
                b.pop();
            }
            "notGzip" => b = a.clone(),
            _ => {}
        }
        let mut f = tempfile::tempfile().unwrap();
        f.write_all(&b).unwrap();
        assert_eq!(
            inflate(&mut f, &job(), Instant::now()).is_ok(),
            mode == "good",
            "{mode}"
        );
    }
    assert!(read_bytes(b"not a TAR").is_err());
}
#[test]
fn tar_preparation_binds_format_protection_source_and_single_use_lifetime() {
    let t = tempfile::Builder::new()
        .prefix("MooMole-tar-check-owned-")
        .tempdir()
        .unwrap();
    let root = t.path().canonicalize().unwrap();
    let p = root.join("owned.tar");
    fs::write(&p, archive(&[("owned", b'0', b"keep")])).unwrap();
    let cfg = settings();
    let j = job();
    let mut plan = Plan::prepare(&p, root.to_str().unwrap(), "Extracted", &cfg, &j).unwrap();
    assert!(plan.draft.format == Format::Tar);
    let mut protect = cfg.clone();
    protect.protected_paths.push(p.to_string_lossy().into());
    assert!(plan.validate(&protect, &j, Instant::now()).is_err());
    plan.issued = Instant::now() - Duration::from_secs(121);
    assert!(plan.time().is_err());
    fs::write(&p, b"changed").unwrap();
    assert!(plan.validate(&cfg, &j, Instant::now()).is_err());
    assert!(!root.join("Extracted").exists());
}
fn native_case(format: &str, lost: bool) -> serde_json::Value {
    let t = tempfile::Builder::new()
        .prefix("MooMole-tar-owned-")
        .tempdir()
        .unwrap();
    let root = t.path().canonicalize().unwrap();
    let original = root.join("Source");
    fs::create_dir(&original).unwrap();
    fs::create_dir(original.join("Empty")).unwrap();
    fs::write(original.join("照片.txt"), b"owned unicode").unwrap();
    fs::write(original.join(".hidden"), b"owned hidden").unwrap();
    fs::write(original.join("._sidecar"), b"raw sidecar").unwrap();
    let long = "a".repeat(if format == "ustar" { 90 } else { 110 });
    fs::create_dir(original.join(&long)).unwrap();
    fs::write(original.join(&long).join("long.txt"), b"long path bytes").unwrap();
    let suffix = if format == "ustar" || format == "pax" {
        "tar"
    } else {
        format
    };
    let source = root.join(format!("sample.{suffix}"));
    let mut c = Command::new("/usr/bin/tar");
    c.env("COPYFILE_DISABLE", "1").arg(if format == "ustar" {
        "--format=ustar"
    } else {
        "--format=pax"
    });
    if format == "tar.gz" || format == "tgz" {
        c.arg("-czf");
    } else {
        c.arg("-cf");
    }
    assert!(c
        .arg(&source)
        .arg("-C")
        .arg(&root)
        .arg("Source")
        .status()
        .unwrap()
        .success());
    let bytes = fs::read(&source).unwrap();
    let h = AnchoredPath::open(&source).unwrap();
    let q = b"0083;65aa0010;MooMoleOwnedTar;";
    assert_eq!(
        unsafe {
            libc::fsetxattr(
                h.target().as_raw_fd(),
                c"com.apple.quarantine".as_ptr(),
                q.as_ptr().cast(),
                q.len(),
                0,
                0,
            )
        },
        0
    );
    let cfg = settings();
    let j = job();
    let p = Plan::prepare(&source, root.to_str().unwrap(), "Extracted", &cfg, &j).unwrap();
    assert_eq!(p.draft.files, 4);
    let dbpath = root.join("state.sqlite");
    let database = db::open_owned(&dbpath).unwrap();
    db::save_settings(&database, &cfg).unwrap();
    if lost {
        database.execute_batch(journal::TABLE).unwrap();
        database.execute_batch("CREATE TRIGGER tar_completion_loss BEFORE UPDATE ON zip_extractions WHEN json_extract(NEW.body,'$.state')='completed' BEGIN SELECT RAISE(ABORT,'owned loss'); END;").unwrap();
    }
    let database = Mutex::new(database);
    let result = run(&database, p, uuid::Uuid::new_v4().to_string(), &j);
    if lost {
        assert!(result.is_err())
    } else {
        assert_eq!(result.unwrap().state, "completed")
    }
    for (name, expected) in [
        ("照片.txt", b"owned unicode".as_slice()),
        (".hidden", b"owned hidden"),
        ("._sidecar", b"raw sidecar"),
    ] {
        assert_eq!(
            fs::read(root.join("Extracted/Source").join(name)).unwrap(),
            expected
        );
        assert_eq!(fs::read(original.join(name)).unwrap(), expected);
    }
    assert_eq!(
        fs::read(root.join("Extracted/Source").join(&long).join("long.txt")).unwrap(),
        b"long path bytes"
    );
    assert!(root.join("Extracted/Source/Empty").is_dir());
    assert_eq!(fs::read(&source).unwrap(), bytes);
    let f = File::open(root.join("Extracted/Source/照片.txt")).unwrap();
    assert_eq!(quarantine(&f).unwrap().unwrap(), q);
    assert_eq!(f.metadata().unwrap().mode() & 0o777, 0o600);
    assert_eq!(
        fs::metadata(root.join("Extracted")).unwrap().mode() & 0o777,
        0o700
    );
    drop(database);
    let database = db::open_owned(&dbpath).unwrap();
    let row = journal::reconcile(&database, &[]).unwrap().remove(0);
    assert_eq!(row.state, if lost { "unknown" } else { "completed" });
    assert_eq!(journal::admission(&database).is_err(), lost);
    serde_json::json!({"record":row,"independentProducer":"system bsdtar","format":format,"lostCompletion":lost,"sourceAndOriginalFilesPreserved":true,"unicodeHiddenLongPathsEmptyDirectoriesAndRawSidecarVerified":true,"quarantineAndPrivatePermissionsVerified":true,"databaseReopenedWithoutReplay":true,"ownedTemporaryRoot":root,"ownedRootWillBeAutomaticallyRemoved":true})
}
#[test]
#[ignore = "real TAR/TAR.GZ extraction of independent system-tar packages and lost completion, only auto-cleaned system temporary fixtures"]
fn real_tar_ustar_pax_gzip_and_lost_completion_preserve_sources() {
    let mut rows = vec![];
    for format in ["ustar", "pax", "tar.gz", "tgz"] {
        rows.push(native_case(format, false));
    }
    rows.push(native_case("tar.gz", true));
    for row in &rows {
        assert!(!Path::new(row["ownedTemporaryRoot"].as_str().unwrap()).exists())
    }
    fs::create_dir_all("../.cache").unwrap();
    fs::write(
        "../.cache/tar-extraction-native.json",
        serde_json::to_vec(&serde_json::json!({"appWindowVerified":false,"reports":rows})).unwrap(),
    )
    .unwrap();
}
