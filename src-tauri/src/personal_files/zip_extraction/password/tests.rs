use super::*;
use crate::personal_files::zip_extraction::{
    tests::{config, job},
    Plan,
};
use std::{fs, path::PathBuf};
const SECRET: &str = "MooMole owned secret 8fd1 密码";
fn secret() -> Option<Password> {
    Some(Password::new(SECRET.into()).unwrap())
}
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let t = tempfile::Builder::new()
        .prefix("moomole-encrypted-zip-owned-")
        .tempdir()
        .unwrap();
    let root = t.path().canonicalize().unwrap();
    let zip = root.join("owned.zip");
    let out = root.join("Output");
    fs::create_dir(&out).unwrap();
    (t, zip, out)
}
fn independent(path: &Path) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures/archives/make_encrypted_zip.py");
    assert!(std::process::Command::new("/usr/bin/python3")
        .args(["-B"])
        .arg(script)
        .arg(path)
        .status()
        .unwrap()
        .success());
}
#[test]
fn independent_aes_and_system_legacy_zip_really_extract_and_keep_password_out_of_history() {
    let mut cases = vec![];
    for kind in ["independentAES256", "systemZipCrypto"] {
        let (_t, zip, out) = fixture();
        let root = zip.parent().unwrap();
        let cfg = config();
        if kind == "independentAES256" {
            independent(&zip);
        } else {
            fs::write(root.join("owned.txt"), b"owned legacy payload").unwrap();
            assert!(std::process::Command::new("/usr/bin/zip")
                .args(["-q", "-P", SECRET])
                .arg(&zip)
                .arg("owned.txt")
                .current_dir(root)
                .status()
                .unwrap()
                .success());
        }
        crate::update_quarantine::mark_from(
            &zip,
            "https://example.org/owned.zip",
            "https://example.org/",
        )
        .unwrap();
        let original = fs::read(&zip).unwrap();
        let keep = out.join("keep.txt");
        fs::write(&keep, b"unselected").unwrap();
        assert!(Plan::prepare(&zip, out.to_str().unwrap(), "Restored", &cfg, &job()).is_err());
        let wrong = Plan::prepare_password(
            &zip,
            out.to_str().unwrap(),
            "Restored",
            &cfg,
            &job(),
            false,
            Some(Password::new("wrong password".into()).unwrap()),
        );
        assert!(wrong.is_err());
        let p = Plan::prepare_password(
            &zip,
            out.to_str().unwrap(),
            "Restored",
            &cfg,
            &job(),
            false,
            secret(),
        )
        .unwrap();
        assert!(p.draft.encrypted);
        let draft = p.draft.clone();
        let database = Arc::new(Mutex::new(
            crate::db::open_owned(&root.join("history.sqlite")).unwrap(),
        ));
        crate::db::save_settings(&*database.lock().unwrap(), &cfg).unwrap();
        let r = super::super::run(&database, p, uuid::Uuid::new_v4().to_string(), &job()).unwrap();
        assert_eq!(r.state, "completed");
        if kind == "independentAES256" {
            assert_eq!(
                fs::read(out.join("Restored/Trip/秘密.txt")).unwrap(),
                "owned 独立 AES payload".as_bytes()
            );
            assert_eq!(fs::read(out.join("Restored/Trip/empty.txt")).unwrap(), b"");
            assert_eq!(
                fs::read(out.join("Restored/Trip/public.txt")).unwrap(),
                b"owned unencrypted companion"
            );
        } else {
            assert_eq!(
                fs::read(out.join("Restored/owned.txt")).unwrap(),
                b"owned legacy payload"
            );
        }
        assert_eq!(fs::read(&zip).unwrap(), original);
        assert_eq!(fs::read(&keep).unwrap(), b"unselected");
        assert!(!Path::new(&r.temporary_path).exists());
        let q = super::super::quarantine(&File::open(&zip).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            super::super::quarantine(&File::open(out.join("Restored")).unwrap())
                .unwrap()
                .unwrap(),
            q
        );
        let body = serde_json::to_string(&r).unwrap();
        assert!(!body.contains(SECRET));
        assert!(!body.contains("password"));
        drop(database);
        let mut db = crate::db::open_fixture(&root.join("history.sqlite")).unwrap();
        assert_eq!(super::super::journal::list(&db).unwrap().len(), 1);
        let archive_root = root.join("Archive");
        let plan = crate::execution_archive::prepare(&mut db).unwrap();
        let header = crate::execution_archive::archive(
            &mut db,
            &archive_root,
            &plan.fingerprint,
            crate::db::now(),
        )
        .unwrap();
        let view = crate::execution_archive::view(&archive_root, &header.id).unwrap();
        assert_eq!(serde_json::to_value(&header).unwrap()["version"], 7);
        let detail = crate::execution_archive::detail(
            &archive_root,
            &header.id,
            crate::execution_archive::Kind::Extraction,
            &r.id,
        )
        .unwrap();
        assert_eq!(detail, serde_json::to_value(&r).unwrap());
        let archive_file = fs::read_dir(&archive_root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|x| x == "moomole-history"))
            .unwrap();
        let original_archive = fs::read(&archive_file).unwrap();
        let end = original_archive.iter().position(|b| *b == b'\n').unwrap();
        let mut changed_header: serde_json::Value =
            serde_json::from_slice(&original_archive[..end]).unwrap();
        for version in 1..=6 {
            changed_header["version"] = version.into();
            let mut altered = serde_json::to_vec(&changed_header).unwrap();
            altered.push(b'\n');
            altered.extend_from_slice(&original_archive[end + 1..]);
            fs::write(&archive_file, altered).unwrap();
            assert!(crate::execution_archive::view(&archive_root, &header.id).is_err());
        }
        fs::write(&archive_file, &original_archive).unwrap();
        assert!(crate::execution_archive::view(&archive_root, &header.id).is_ok());
        cases.push(serde_json::json!({"kind":kind,"draft":draft,"record":r,"header":header,"view":view,"fixture":root,"sourceAndUnselectedPreserved":true,"passwordAbsentFromJournalAndArchive":true}));
    }
    fs::create_dir_all("../.cache").unwrap();
    fs::write("../.cache/encrypted-zip-native.json",serde_json::to_vec_pretty(&serde_json::json!({"cases":cases,"scope":"production core with independent CommonCrypto AE-2 and system zip, no user data or App window"})).unwrap()).unwrap();
}
#[test]
fn aes_authentication_tampering_scope_changes_and_password_limits_refuse_without_output() {
    let (_t, zip, out) = fixture();
    independent(&zip);
    let original = fs::read(&zip).unwrap();
    let cfg = config();
    let z = zip::ZipArchive::new(File::open(&zip).unwrap()).unwrap();
    let mut z = z;
    let start = z.by_index_raw(0).unwrap().data_start() as usize;
    drop(z);
    let mut altered = original.clone();
    altered[start + 18] ^= 1;
    fs::write(&zip, altered).unwrap();
    assert!(Plan::prepare_password(
        &zip,
        out.to_str().unwrap(),
        "Restored",
        &cfg,
        &job(),
        false,
        secret()
    )
    .is_err());
    assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
    fs::write(&zip, &original).unwrap();
    let p = Plan::prepare_password(
        &zip,
        out.to_str().unwrap(),
        "Restored",
        &cfg,
        &job(),
        false,
        secret(),
    )
    .unwrap();
    let j = job();
    j.cancel();
    assert!(p.validate(&cfg, &j, Instant::now()).is_err());
    assert!(Password::new(String::new()).is_err());
    assert!(Password::new("a".repeat(1025)).is_err());
    assert!(Password::new("  exact spaces  ".into()).is_ok());
    let tar = zip.with_extension("tar");
    fs::write(&tar, &original).unwrap();
    assert!(Plan::prepare_password(
        &tar,
        out.to_str().unwrap(),
        "Restored",
        &cfg,
        &job(),
        false,
        secret()
    )
    .is_err());
    assert_eq!(fs::read(&zip).unwrap(), original);
    assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
}
#[test]
fn encrypted_publication_with_lost_completion_reopens_unknown_without_credentials_or_replay() {
    let (_t, zip, out) = fixture();
    independent(&zip);
    let root = zip.parent().unwrap();
    let cfg = config();
    let p = Plan::prepare_password(
        &zip,
        out.to_str().unwrap(),
        "Restored",
        &cfg,
        &job(),
        false,
        secret(),
    )
    .unwrap();
    let db = crate::db::open_owned(&root.join("history.sqlite")).unwrap();
    crate::db::save_settings(&db, &cfg).unwrap();
    db.execute_batch(super::super::journal::TABLE).unwrap();
    db.execute_batch("CREATE TRIGGER lose_encrypted_completion BEFORE UPDATE ON zip_extractions WHEN json_extract(NEW.body,'$.state')='completed' BEGIN SELECT RAISE(ABORT,'owned lost completion'); END;").unwrap();
    let db = Mutex::new(db);
    assert!(super::super::run(&db, p, uuid::Uuid::new_v4().to_string(), &job()).is_err());
    drop(db);
    let db = crate::db::open_owned(&root.join("history.sqlite")).unwrap();
    let rows = super::super::journal::reconcile(&db, &[]).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, "unknown");
    assert!(rows[0].draft.encrypted);
    assert!(super::super::journal::admission(&db).is_err());
    assert!(!serde_json::to_string(&rows).unwrap().contains(SECRET));
    assert_eq!(
        fs::read(out.join("Restored/Trip/秘密.txt")).unwrap(),
        "owned 独立 AES payload".as_bytes()
    );
    fs::create_dir_all("../.cache").unwrap();
    fs::write("../.cache/encrypted-zip-lost-native.json",serde_json::to_vec_pretty(&serde_json::json!({"records":rows,"fixture":root,"unknownBlocksFurtherExecution":true,"passwordAbsent":true,"publishedContentPreserved":true,"replayed":false})).unwrap()).unwrap();
}
