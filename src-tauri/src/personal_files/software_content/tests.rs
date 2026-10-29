use super::*;
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let t = tempfile::Builder::new()
        .prefix("MooMole-mail-owned-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let root = Source::MailSandboxDownloads.path(&home);
    std::fs::create_dir_all(&root).unwrap();
    (t, home, root)
}
pub(crate) fn age(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    let old = crate::db::now() / 1000 - 40 * 86400;
    let times = [libc::timespec {
        tv_sec: old,
        tv_nsec: 0,
    }; 2];
    assert_eq!(
        unsafe {
            libc::utimensat(
                libc::AT_FDCWD,
                path.as_ptr(),
                times.as_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        },
        0
    );
}
#[test]
fn mail_roots_are_explicit_and_old_files_keep_body_databases_recent_and_protected_files() {
    let (_t, home, root) = fixture();
    let file = root.join("old.txt");
    std::fs::write(&file, vec![b'x'; 6 * 1024 * 1024]).unwrap();
    age(&file);
    std::fs::write(root.join("recent.txt"), "keep").unwrap();
    std::fs::write(root.join("message.emlx"), "mail body").unwrap();
    let database = home.join("Library/Mail/Envelope Index");
    std::fs::create_dir_all(database.parent().unwrap()).unwrap();
    std::fs::write(&database, "keep database").unwrap();
    let job = Arc::new(Job::new(crate::engine::TaskKind::PersonalScan));
    let scan = collect(&home, Source::MailSandboxDownloads, &[], job.clone()).unwrap();
    assert_eq!(scan.items.len(), 1);
    let id = scan.items[0].id.clone();
    let binding = scan.software_content.as_ref().unwrap().clone();
    assert!(!binding.allows(&database));
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    assert!(store.offer(&[id], db::now()).is_err());
    let legacy = Source::MailDownloads.path(&home);
    std::fs::create_dir_all(&legacy).unwrap();
    let legacy_file = legacy.join("old.txt");
    std::fs::write(&legacy_file, vec![b'x'; 6 * 1024 * 1024]).unwrap();
    age(&legacy_file);
    let scan = collect(&home, Source::MailDownloads, &[], job.clone()).unwrap();
    let id = scan.items[0].id.clone();
    let binding = scan.software_content.as_ref().unwrap().clone();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offered = store.offer(&[id], db::now()).unwrap();
    let plan = store.take(&offered.id, db::now()).unwrap();
    assert_eq!(plan.origin(), MODULE);
    plan.validate_record().unwrap();
    binding.validate(&plan.items[0], &home, db::now()).unwrap();
    let protected = vec![file.to_str().unwrap().into()];
    assert!(
        collect(&home, Source::MailSandboxDownloads, &protected, job)
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(std::fs::read_to_string(database).unwrap(), "keep database");
    std::fs::rename(&legacy, legacy.with_file_name("old-root")).unwrap();
    std::fs::create_dir(&legacy).unwrap();
    assert!(binding.validate(&plan.items[0], &home, db::now()).is_err());
}
