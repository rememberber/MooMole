use super::*;
#[test]
fn preference_is_explicit_persistent_and_does_not_change_sampling_or_files() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("data.sqlite");
    let mut db = db::open_fixture(&path).unwrap();
    assert_eq!(load(&db).unwrap(), Config::default());
    assert!(save(&mut db, Config { enabled: true }).is_err());
    db::save_settings(
        &db,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    let owned = t.path().join("owned.txt");
    std::fs::write(&owned, b"owned keep").unwrap();
    save(&mut db, Config { enabled: true }).unwrap();
    assert!(load(&db).unwrap().enabled);
    let sampling: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='storage_monitor_preferences'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sampling, 0);
    drop(db);
    let mut db = db::open_fixture(&path).unwrap();
    assert!(load(&db).unwrap().enabled);
    save(&mut db, Config { enabled: false }).unwrap();
    assert!(!load(&db).unwrap().enabled);
    assert_eq!(std::fs::read(&owned).unwrap(), b"owned keep");
    db.execute("UPDATE menu_bar_preferences SET version=99", [])
        .unwrap();
    assert!(load(&db).is_err());
    assert!(save(&mut db, Config { enabled: true }).is_err());
    assert_eq!(
        db.query_row::<u32, _, _>("SELECT version FROM menu_bar_preferences", [], |r| r.get(0))
            .unwrap(),
        99
    );
}
#[test]
fn historical_failed_and_future_capacity_never_show_a_current_low_space_alarm() {
    let mut view = crate::storage_monitor::View {
        config: crate::storage_monitor::Config {
            enabled: false,
            interval_minutes: 15,
            warn_below_bytes: 10 * 1024 * 1024 * 1024,
        },
        samples: vec![],
        collecting: false,
        error: None,
    };
    assert_eq!(model(&view, "en", 100_000).title, "");
    view.samples.push(crate::cleanup_space::Sample {
        measured_at: 100_000,
        volume_id: "123:456".into(),
        total_bytes: 100 * 1024 * 1024 * 1024,
        available_bytes: 2 * 1024 * 1024 * 1024,
    });
    assert_eq!(model(&view, "en", 100_000).title, "! 2.0 GiB");
    assert_eq!(model(&view, "en", 2_000_001).title, "");
    assert!(model(&view, "en", 2_000_001).status.contains("Historical"));
    assert!(!model(&view, "en", 99_999).title.starts_with('!'));
    view.error = Some("owned failed sample".into());
    assert!(!model(&view, "en", 100_000).title.starts_with('!'));
    assert!(model(&view, "en", 100_000).status.contains("failed"));
    assert!(!Controller::default().keeps_running());
}
