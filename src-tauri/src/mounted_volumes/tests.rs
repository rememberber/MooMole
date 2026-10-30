use super::*;
#[test]
fn mounted_scope_and_capacity_never_treat_unknown_as_free_or_hidden_system_mounts_as_drives() {
    for p in ["/Volumes/Owned disk", "/Volumes/中文盘"] {
        assert!(external_root(Path::new(p)));
    }
    for p in [
        "/",
        "/Volumes",
        "/Volumes/.hidden",
        "/Volumes/Disk/sub",
        "/System/Volumes/Data",
        "/Volumes/name\n",
    ] {
        assert!(!external_root(Path::new(p)));
    }
    let mut s: libc::statfs = unsafe { std::mem::zeroed() };
    assert_eq!(capacity(&s), (None, None));
    s.f_bsize = 4096;
    s.f_blocks = 100;
    s.f_bavail = 30;
    assert_eq!(capacity(&s), (Some(409600), Some(122880)));
    s.f_bavail = 101;
    assert_eq!(capacity(&s), (None, None));
    s.f_blocks = u64::MAX;
    assert_eq!(capacity(&s), (None, None));
}
#[test]
fn fresh_owned_locator_is_one_use_and_changed_directories_or_old_lists_never_resolve() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let path = home.join("Owned");
    std::fs::create_dir(&path).unwrap();
    let created = crate::db::now();
    let (row, entry) = observe(&path, Kind::Account, created).unwrap();
    assert!(row.can_analyze);
    assert!(row.total_bytes.is_some());
    let entry = entry.unwrap();
    assert_eq!(entry.resolve().unwrap(), path.to_str().unwrap());
    let mut store = Store::default();
    let ticket = store.begin().unwrap();
    let report = Report {
        version: 1,
        created_at: created,
        expires_at: created + TTL,
        complete: true,
        volumes: vec![row.clone()],
    };
    store
        .publish(ticket, &report, HashMap::from([(row.id.clone(), entry)]))
        .unwrap();
    let entry = store.take(&row.id).unwrap();
    assert!(store.take(&row.id).is_err());
    std::fs::rename(&path, home.join("Old")).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(entry.resolve().is_err());
    let (_, entry) = observe(&path, Kind::Account, created).unwrap();
    let mut entry = entry.unwrap();
    entry.issued = Instant::now() - Duration::from_secs(121);
    assert!(entry.fresh().is_err());
    let old = store.begin().unwrap();
    store.begin().unwrap();
    assert!(store.publish(old, &report, HashMap::new()).is_err());
    let (row, entry) = observe(&path, Kind::Mounted, created).unwrap();
    assert!(!row.can_analyze);
    assert!(entry.is_none());
}
#[test]
fn actual_mount_inventory_is_bounded_and_does_not_change_owned_files() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().canonicalize().unwrap();
    let sentinel = home.join("kept.txt");
    std::fs::write(&sentinel, b"owned kept").unwrap();
    let (report, entries) = collect(&home).unwrap();
    assert!(report.volumes.len() <= MAX);
    assert!(report
        .volumes
        .iter()
        .any(|v| v.kind == Kind::Account && v.can_analyze));
    assert!(report
        .volumes
        .iter()
        .any(|v| v.kind == Kind::Startup && v.path == "/" && v.can_analyze));
    for row in report.volumes.iter().filter(|v| v.can_analyze) {
        assert_eq!(entries.get(&row.id).unwrap().resolve().unwrap(), row.path);
    }
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"owned kept");
}
