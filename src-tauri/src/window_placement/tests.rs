use super::*;

fn saved() -> Placement {
    Placement {
        width: 1200.0,
        height: 820.0,
        x: 100,
        y: 100,
        scale: 2.0,
    }
}
fn screen() -> WorkArea {
    WorkArea {
        x: 0,
        y: 48,
        width: 2880,
        height: 1752,
        scale: 2.0,
    }
}

#[test]
fn captures_only_ordinary_valid_sizes_and_does_not_grow_retina_windows() {
    assert_eq!(captured((2400, 1640), (100, 100), 2.0, true), Some(saved()));
    assert!(captured((2400, 1640), (100, 100), 2.0, false).is_none());
    for scale in [f64::NAN, f64::INFINITY, 0.0, -1.0, 0.49, 8.01] {
        assert!(captured((2400, 1640), (100, 100), scale, true).is_none());
    }
    for size in [(0, 0), (959, 820), (1200, 679), (8193, 820), (1200, 8193)] {
        assert!(captured(size, (0, 0), 1.0, true).is_none());
    }
    let mut value = saved();
    for _ in 0..10 {
        let restored = resolve(value, &[screen()], (0.0, 28.0)).unwrap();
        value = captured(
            (
                (restored.width * 2.0) as u32,
                (restored.height * 2.0) as u32,
            ),
            (restored.x, restored.y),
            2.0,
            true,
        )
        .unwrap();
        assert_eq!(value, saved());
    }
}

#[test]
fn missing_screens_and_large_windows_clamp_inside_the_current_work_area() {
    let area = screen();
    let remote = Placement {
        x: 9000,
        y: -9000,
        width: 8000.0,
        height: 8000.0,
        scale: 2.0,
    };
    let resolved = resolve(remote, &[area], (0.0, 28.0)).unwrap();
    assert_eq!(
        resolved,
        Placement {
            x: 0,
            y: 48,
            width: 1440.0,
            height: 848.0,
            scale: 2.0,
        }
    );
    for (x, y) in [(i32::MIN, i32::MAX), (-400, -700), (2800, 1700)] {
        let value = resolve(Placement { x, y, ..saved() }, &[area], (0.0, 28.0)).unwrap();
        assert!(value.x >= area.x && value.y >= area.y);
        assert!(
            f64::from(value.x) + value.width * area.scale
                <= f64::from(area.x) + f64::from(area.width)
        );
        assert!(
            f64::from(value.y) + (value.height + 28.0) * area.scale
                <= f64::from(area.y) + f64::from(area.height)
        );
    }
    assert!(resolve(saved(), &[], (0.0, 0.0)).is_none());
    assert!(resolve(
        saved(),
        &[WorkArea {
            width: 1000,
            height: 1000,
            ..area
        }],
        (0.0, 0.0)
    )
    .is_none());
    assert!(resolve(saved(), &[area], (f64::NAN, 0.0)).is_none());
    assert!(resolve(saved(), &[WorkArea { scale: 0.0, ..area }], (0.0, 0.0)).is_none());
}

#[test]
fn mixed_scale_negative_position_monitors_keep_logical_size_and_choose_overlap() {
    let secondary = WorkArea {
        x: -1920,
        y: 24,
        width: 1920,
        height: 1056,
        scale: 1.0,
    };
    let original = Placement {
        x: -1700,
        y: 100,
        scale: 1.0,
        ..saved()
    };
    assert_eq!(
        resolve(original, &[screen(), secondary], (0.0, 28.0)),
        Some(original)
    );
    // The old secondary is gone: the primary receives a bounded window.
    assert_eq!(
        resolve(original, &[screen()], (0.0, 28.0)),
        Some(Placement { x: 0, ..saved() })
    );
    // Stable global position but a new screen scale keeps logical dimensions.
    let changed = WorkArea {
        scale: 1.25,
        width: 2400,
        height: 1350,
        ..secondary
    };
    assert_eq!(
        resolve(original, &[changed], (0.0, 28.0)),
        Some(Placement {
            scale: 1.25,
            ..original
        })
    );
}

#[test]
fn optional_storage_roundtrips_without_changing_settings_history_or_schema_version() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("app.sqlite");
    let db = crate::db::open_fixture(&path).unwrap();
    let schema_before: u32 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    crate::db::save_settings(&db, &crate::db::Settings::default()).unwrap();
    crate::db::start(&db, "test", "status", Some("/fixture")).unwrap();
    let before: String = db
        .query_row("SELECT value FROM settings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(load(&db).unwrap(), None);
    assert_eq!(
        db.query_row::<i64, _, _>(
            "SELECT count(*) FROM sqlite_master WHERE name='window_preferences'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        0
    );
    save_without_wait(&db, saved()).unwrap();
    assert_eq!(load(&db).unwrap(), Some(saved()));
    assert_eq!(
        db.query_row::<String, _, _>("SELECT value FROM settings", [], |r| r.get(0))
            .unwrap(),
        before
    );
    assert_eq!(crate::db::history(&db).unwrap()[0].state, "running");
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        schema_before
    );
    assert_eq!(
        db.pragma_query_value::<u64, _>(None, "busy_timeout", |r| r.get(0))
            .unwrap(),
        5000
    );
    drop(db);
    let reopened = crate::db::open_fixture(&path).unwrap();
    assert_eq!(load(&reopened).unwrap(), Some(saved()));
}

#[test]
fn malformed_oversize_and_future_records_fall_back_without_overwriting_future_versions() {
    let directory = tempfile::tempdir().unwrap();
    let db = crate::db::open_fixture(&directory.path().join("app.sqlite")).unwrap();
    save(&db, saved()).unwrap();
    for body in [
        "{broken".into(),
        "x".repeat(BODY_LIMIT + 1),
        "{\"width\":0,\"height\":820,\"x\":0,\"y\":0}".into(),
        "{\"width\":1200,\"height\":820,\"x\":0,\"y\":0,\"extra\":true}".into(),
    ] {
        db.execute("UPDATE window_preferences SET body=?1", [body])
            .unwrap();
        assert!(load(&db).is_err());
        save(&db, saved()).unwrap();
        assert_eq!(load(&db).unwrap(), Some(saved()));
    }
    db.execute("UPDATE window_preferences SET version=2,body='future'", [])
        .unwrap();
    assert!(load(&db).is_err());
    assert!(save_without_wait(&db, saved())
        .unwrap_err()
        .contains("更新版本"));
    assert_eq!(
        db.query_row::<String, _, _>("SELECT body FROM window_preferences", [], |r| r.get(0))
            .unwrap(),
        "future"
    );
    assert!(db.is_autocommit());
    assert_eq!(
        db.pragma_query_value::<u64, _>(None, "busy_timeout", |r| r.get(0))
            .unwrap(),
        5000
    );
}

#[test]
fn readonly_write_lock_and_update_failure_preserve_old_value_and_busy_timeout() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("app.sqlite");
    let db = crate::db::open_fixture(&path).unwrap();
    save(&db, saved()).unwrap();
    let next = Placement { x: 200, ..saved() };
    let reader =
        Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert!(save_without_wait(&reader, next).is_err());
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(save_without_wait(&db, next).unwrap_err().contains("locked"));
    assert_eq!(
        db.pragma_query_value::<u64, _>(None, "busy_timeout", |r| r.get(0))
            .unwrap(),
        5000
    );
    writer.execute_batch("ROLLBACK").unwrap();
    assert_eq!(load(&db).unwrap(), Some(saved()));
    db.execute_batch("CREATE TRIGGER fail_window_update BEFORE UPDATE ON window_preferences BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(save_without_wait(&db, next).is_err());
    assert_eq!(load(&db).unwrap(), Some(saved()));
    assert!(db.is_autocommit());
    db.execute_batch("DROP TRIGGER fail_window_update").unwrap();
    save_without_wait(&db, next).unwrap();
    assert_eq!(load(&db).unwrap(), Some(next));
}
