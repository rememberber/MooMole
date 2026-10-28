use super::*;
use rusqlite::types::Value;

// Metadata-only fixtures: no target files or real application database.
fn fixture(saved: bool) -> (tempfile::TempDir, Connection) {
    let directory = tempfile::tempdir().unwrap();
    let db = open(&directory.path().join("app.sqlite")).unwrap();
    if saved {
        save_settings(
            &db,
            &Settings {
                language: "system".into(),
                theme: "dark".into(),
                retention_days: 90,
                protected_paths: vec!["/fixture/保留".into()],
                acknowledged: true,
            },
        )
        .unwrap();
    }
    let recent = now();
    let old = recent - 100 * 86_400_000;
    for (id, state, at) in [
        ("expired", "succeeded", old),
        ("recent", "succeeded", recent),
        ("queued", "queued", old),
        ("running", "running", old),
    ] {
        db.execute(
            "INSERT INTO tasks VALUES (?1,'status','/fixture',?2,?3,NULL,'metadata')",
            params![id, state, at],
        )
        .unwrap();
    }
    db.execute(
        "INSERT INTO clean_scans VALUES ('expired',?1,'old scan')",
        [old],
    )
    .unwrap();
    db.execute(
        "INSERT INTO review_plans VALUES ('expired','expired',?1,'old plan')",
        [old],
    )
    .unwrap();
    // Also exercise the independent count bounds, including scan cascades.
    for index in 0..12 {
        db.execute(
            "INSERT INTO clean_scans VALUES (?1,?2,?3)",
            params![
                format!("scan-{index:02}"),
                recent + index,
                format!("scan body {index}")
            ],
        )
        .unwrap();
    }
    for index in 0..25 {
        db.execute(
            "INSERT INTO review_plans VALUES (?1,'scan-11',?2,?3)",
            params![
                format!("plan-{index:02}"),
                recent + index,
                format!("plan body {index}")
            ],
        )
        .unwrap();
    }
    for (id, state, item, at) in [
        ("expired", "noChanges", "skipped", old),
        ("recent", "noChanges", "failed", recent),
        ("moved", "completed", "moved", old),
        ("uncertain", "partial", "uncertain", old),
        ("intent", "partial", "intent", old),
        ("running", "running", "pending", old),
        ("interrupted", "interrupted", "notStarted", old),
    ] {
        db.execute(
            "INSERT INTO execution_runs VALUES (?1,?1,'trash',?2,?3,?3,'snapshot','digest',32768)",
            params![id, state, at],
        )
        .unwrap();
        db.execute(
            "INSERT INTO execution_items VALUES (?1,'item',0,'/fixture',?2,'reason','receipt')",
            params![id, item],
        )
        .unwrap();
        db.execute("INSERT INTO execution_events (run_id,item_id,state,created_at,reason,receipt) VALUES (?1,'item',?2,?3,'reason','receipt')", params![id, item, at]).unwrap();
    }
    (directory, db)
}

fn snapshot(db: &Connection) -> Vec<Vec<Vec<Value>>> {
    [
        "settings",
        "tasks",
        "clean_scans",
        "review_plans",
        "execution_runs",
        "execution_items",
        "execution_events",
    ]
    .into_iter()
    .map(|table| {
        let mut query = db
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = query.column_count();
        query
            .query_map([], |row| {
                (0..columns)
                    .map(|i| row.get(i))
                    .collect::<Result<Vec<Value>, _>>()
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    })
    .collect()
}

fn ids(db: &Connection, table: &str) -> Vec<String> {
    db.prepare(&format!("SELECT id FROM {table} ORDER BY id"))
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

const DELETE_FAULTS: &[(&str, &str)] = &[
    ("tasks", "OLD.id='expired'"),
    ("clean_scans", "OLD.id='expired'"),
    ("review_plans", "OLD.id='expired'"),
    ("review_plans", "OLD.id='plan-00'"),
    ("clean_scans", "OLD.id='scan-00'"),
    ("execution_runs", "OLD.id='expired'"),
    ("execution_items", "OLD.run_id='expired'"),
    ("execution_events", "OLD.run_id='expired'"),
];

fn fail_delete(db: &Connection, table: &str, condition: &str) {
    db.execute_batch(&format!("CREATE TRIGGER retention_fault BEFORE DELETE ON {table} WHEN {condition} BEGIN SELECT RAISE(ABORT,'injected retention failure'); END;")).unwrap();
}

#[test]
fn settings_and_all_retention_stages_roll_back_together_on_failure() {
    for saved in [false, true] {
        for (table, condition) in DELETE_FAULTS {
            let (directory, db) = fixture(saved);
            let before = snapshot(&db);
            fail_delete(&db, table, condition);
            let error = save_settings(
                &db,
                &Settings {
                    theme: "light".into(),
                    retention_days: 7,
                    ..Settings::default()
                },
            )
            .unwrap_err();
            assert!(
                error.contains("injected retention failure"),
                "{table}: {error}"
            );
            assert!(db.is_autocommit());
            assert_eq!(snapshot(&db), before, "{saved}/{table}/{condition}");
            // Independent SQLite reader observes the same committed state.
            let reader = Connection::open(directory.path().join("app.sqlite")).unwrap();
            assert_eq!(snapshot(&reader), before);
        }
    }
}

#[test]
fn startup_retention_wrapper_rolls_back_all_prior_deletions() {
    for (table, condition) in DELETE_FAULTS {
        let (_directory, db) = fixture(true);
        let before = snapshot(&db);
        fail_delete(&db, table, condition);
        assert!(prune(&db, 7)
            .unwrap_err()
            .contains("injected retention failure"));
        assert_eq!(snapshot(&db), before, "{table}/{condition}");
        assert!(db.is_autocommit());
    }
}

#[test]
fn settings_commit_failure_rolls_back_preference_and_retention() {
    let (_directory, db) = fixture(true);
    let before = snapshot(&db);
    db.execute_batch("CREATE TABLE commit_fault (parent TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED);
        CREATE TRIGGER fail_commit AFTER UPDATE ON settings BEGIN INSERT INTO commit_fault VALUES ('missing'); END;").unwrap();
    assert!(save_settings(&db, &Settings::default())
        .unwrap_err()
        .contains("FOREIGN KEY"));
    assert!(db.is_autocommit());
    assert_eq!(snapshot(&db), before);
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM commit_fault", [], |r| r.get(0))
            .unwrap(),
        0
    );
}

#[test]
fn successful_settings_prune_keeps_live_tasks_and_review_receipts() {
    let (directory, db) = fixture(true);
    let next = Settings {
        theme: "light".into(),
        retention_days: 7,
        ..Settings::default()
    };
    save_settings(&db, &next).unwrap();
    assert_eq!(settings(&db).unwrap().theme, "light");
    assert_eq!(settings(&db).unwrap().retention_days, 7);
    assert_eq!(ids(&db, "tasks"), ["queued", "recent", "running"]);
    assert_eq!(
        ids(&db, "clean_scans"),
        (2..12).map(|i| format!("scan-{i:02}")).collect::<Vec<_>>()
    );
    assert_eq!(
        ids(&db, "review_plans"),
        (5..25).map(|i| format!("plan-{i:02}")).collect::<Vec<_>>()
    );
    let retained = [
        "intent",
        "interrupted",
        "moved",
        "recent",
        "running",
        "uncertain",
    ];
    assert_eq!(ids(&db, "execution_runs"), retained);
    for table in ["execution_items", "execution_events"] {
        let mut query = db
            .prepare(&format!("SELECT run_id FROM {table} ORDER BY run_id"))
            .unwrap();
        let runs = query
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(runs, retained);
    }
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get(0))
            .unwrap(),
        0
    );
    let reader = Connection::open(directory.path().join("app.sqlite")).unwrap();
    assert_eq!(snapshot(&reader), snapshot(&db));
}

#[test]
fn readonly_and_locked_settings_save_leave_all_records_intact() {
    let (directory, db) = fixture(true);
    let before = snapshot(&db);
    let path = directory.path().join("app.sqlite");
    let reader =
        Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert!(save_settings(&reader, &Settings::default()).is_err());
    assert_eq!(snapshot(&db), before);
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    db.busy_timeout(std::time::Duration::ZERO).unwrap();
    assert!(save_settings(&db, &Settings::default())
        .unwrap_err()
        .contains("locked"));
    assert!(db.is_autocommit());
    writer.execute_batch("ROLLBACK").unwrap();
    assert_eq!(snapshot(&db), before);
    save_settings(&db, &Settings::default()).expect("retry works after lock release");
}
