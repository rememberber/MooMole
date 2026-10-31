use super::*;
#[test]
fn fixed_download_table_and_wal_snapshot_find_only_completed_local_files_without_writes() {
    let temp = tempfile::Builder::new()
        .prefix("MooMole-browser-owned-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let history = browser_profiles::Selection::default()
        .history(&home)
        .unwrap();
    std::fs::create_dir_all(history.parent().unwrap()).unwrap();
    let downloads = home.join("Downloads");
    std::fs::create_dir(&downloads).unwrap();
    let complete = downloads.join("complete.txt");
    let pending = downloads.join("pending.txt");
    std::fs::write(&complete, "owned download").unwrap();
    std::fs::write(&pending, "unfinished").unwrap();
    std::fs::write(downloads.join("unlisted.txt"), "not a recorded download").unwrap();
    let db = Connection::open(&history).unwrap();
    db.pragma_update(None, "journal_mode", "WAL").unwrap();
    db.execute_batch("CREATE TABLE downloads(id INTEGER PRIMARY KEY,current_path TEXT,target_path TEXT,state INTEGER)").unwrap();
    for (id, path, state) in [(1, &complete, 1), (2, &pending, 0), (3, &complete, 1)] {
        db.execute(
            "INSERT INTO downloads VALUES(?1,?2,?2,?3)",
            rusqlite::params![id, path.to_str().unwrap(), state],
        )
        .unwrap();
    }
    let before = std::fs::read(&history).unwrap();
    let wal = history.with_file_name("History-wal");
    let before_wal = std::fs::read(&wal).unwrap();
    let scan = collect(
        &home,
        Source::ChromeDownloads,
        &[],
        Arc::new(Job::new(crate::engine::TaskKind::PersonalScan)),
    )
    .unwrap();
    assert_eq!(scan.items.len(), 1);
    assert_eq!(scan.items[0].path, complete.to_str().unwrap());
    assert!(!scan.report.complete);
    assert!(scan.software_content.as_ref().unwrap().history.is_some());
    assert_eq!(std::fs::read(&history).unwrap(), before);
    assert_eq!(std::fs::read(&wal).unwrap(), before_wal);
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM downloads", [], |r| r.get(0))
            .unwrap(),
        3
    );
    let id = scan.items[0].id.clone();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = store.offer(&[id], db::now()).unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    assert_eq!(plan.origin(), MODULE);
    plan.validate_record().unwrap();
    plan.software_content
        .as_ref()
        .unwrap()
        .validate(&plan.items[0], &home, db::now())
        .unwrap();
}
