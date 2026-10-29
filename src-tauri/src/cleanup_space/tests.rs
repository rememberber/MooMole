use super::*;

fn database() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE execution_runs(id TEXT PRIMARY KEY,state TEXT); INSERT INTO execution_runs VALUES ('run','running');").unwrap();
    db
}
fn sample(at: i64, available: u64) -> Sample {
    Sample {
        measured_at: at,
        volume_id: "1:aabbcc".into(),
        total_bytes: 1000,
        available_bytes: available,
    }
}

#[test]
fn optional_record_is_bounded_one_time_and_follows_run_lifetime() {
    let db = database();
    assert!(!has_rows(&db).unwrap());
    assert_eq!(read(&db, "run", 10, None), None);
    let before = sample(11, 100);
    begin(&db, "run", Some(&before)).unwrap();
    assert!(has_rows(&db).unwrap());
    assert!(begin(&db, "run", Some(&before)).is_err());
    assert!(begin(&db, "different-run", Some(&before)).is_err());
    assert!(finish(&db, "run", Some(&sample(14, 200))).is_err());
    assert_eq!(
        read(&db, "run", 10, None).unwrap().before,
        Some(before.clone())
    );
    db.execute("UPDATE execution_runs SET state='completed'", [])
        .unwrap();
    finish(&db, "run", Some(&sample(14, 200))).unwrap();
    assert!(finish(&db, "run", Some(&sample(15, 300))).is_err());
    let report = read(&db, "run", 10, Some(13)).unwrap();
    assert_eq!(report.before, Some(before));
    assert_eq!(report.after, Some(sample(14, 200)));
    db.execute("DELETE FROM execution_runs", []).unwrap();
    assert!(!has_rows(&db).unwrap());
}

#[test]
fn missing_and_corrupt_observations_remain_unknown_without_changing_results() {
    let db = database();
    begin(&db, "run", None).unwrap();
    assert_eq!(read(&db, "run", 10, None).unwrap().before, None);
    db.execute("UPDATE execution_runs SET state='interrupted'", [])
        .unwrap();
    finish(&db, "run", None).unwrap();
    db.execute(
        "UPDATE cleanup_space_observations SET before_sample='invalid json',after_sample=?1",
        [serde_json::to_string(&sample(9, 100)).unwrap()],
    )
    .unwrap();
    let report = read(&db, "run", 10, Some(12)).unwrap();
    assert!(report.before.is_none() && report.after.is_none());
    let state: String = db
        .query_row("SELECT state FROM execution_runs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(state, "interrupted");
    for bad in [
        Sample {
            available_bytes: 1001,
            ..sample(10, 0)
        },
        Sample {
            total_bytes: MAX_EXACT + 1,
            ..sample(10, 0)
        },
        Sample {
            measured_at: -1,
            ..sample(10, 0)
        },
        Sample {
            volume_id: "../path".into(),
            ..sample(10, 0)
        },
    ] {
        assert!(encode(Some(&bad)).is_err());
    }
}

#[cfg(target_os = "macos")]
#[test]
fn actual_local_volume_read_is_bounded_and_does_not_follow_a_symlink() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let result = observe(&root).expect("owned local directory has statfs evidence");
    assert!(result.valid());
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    std::os::unix::fs::symlink(&root, root.join("alias")).unwrap();
    assert!(observe(&root.join("alias")).is_none());
}
