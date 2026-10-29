use super::*;
fn fixture() -> (tempfile::TempDir, Arc<Mutex<db::Database>>, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let database = db::open_owned(&home.join("app.sqlite")).unwrap();
    db::save_settings(
        &database,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    (temp, Arc::new(Mutex::new(database)), home)
}
#[test]
fn opt_in_persists_and_sample_capacity_is_bounded_without_touching_execution_history() {
    let (_temp, database, home) = fixture();
    let mut db = database.lock().unwrap();
    assert!(!load(&db).unwrap().config.enabled);
    assert!(!exists(&db, "storage_monitor_preferences").unwrap());
    let saved = save_config(
        &mut db,
        Config {
            enabled: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(due(&saved, None, 10));
    let sample = Sample {
        measured_at: 1_000_000,
        volume_id: "abc:123".into(),
        total_bytes: 100 * GIB,
        available_bytes: 20 * GIB,
    };
    assert!(!due(&saved, Some(&sample), sample.measured_at + 3_599_999));
    assert!(due(&saved, Some(&sample), sample.measured_at + 3_600_000));
    assert!(due(&saved, Some(&sample), sample.measured_at - 1));
    let tx = db.transaction().unwrap();
    for i in 0..725 {
        let mut s = sample.clone();
        s.measured_at += i;
        tx.execute(
            "INSERT INTO storage_monitor_samples(body) VALUES(?1)",
            [serde_json::to_string(&s).unwrap()],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    save_sample(&mut db, &sample).unwrap();
    assert_eq!(samples(&db).unwrap().len(), 720);
    assert_eq!(crate::execution::list(&db).unwrap().len(), 0);
    drop(db);
    drop(
        Arc::try_unwrap(database)
            .ok()
            .unwrap()
            .into_inner()
            .unwrap(),
    );
    let reopened = db::open_owned(&home.join("app.sqlite")).unwrap();
    assert_eq!(load(&reopened).unwrap().generation, saved.generation);
    assert_eq!(samples(&reopened).unwrap().len(), 720);
}
#[tokio::test]
async fn actual_capacity_observation_and_disabled_generation_do_not_modify_personal_files() {
    let (_temp, database, home) = fixture();
    let file = home.join("keep.txt");
    std::fs::write(&file, "owned unchanged").unwrap();
    let controller = Controller::default();
    let old = save_config(
        &mut database.lock().unwrap(),
        Config {
            enabled: true,
            ..Default::default()
        },
    )
    .unwrap();
    save_config(&mut database.lock().unwrap(), Config::default()).unwrap();
    controller
        .collect(&database, home.clone(), Some(&old.generation))
        .await
        .unwrap();
    assert!(samples(&database.lock().unwrap()).unwrap().is_empty());
    controller
        .collect(&database, home.clone(), None)
        .await
        .unwrap();
    let view = controller.view(&database.lock().unwrap()).unwrap();
    assert_eq!(view.samples.len(), 1);
    assert!(view.samples[0].valid());
    assert!(view.samples[0].total_bytes > 0);
    assert!(!view.config.enabled);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "owned unchanged");
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write(
        "../.cache/storage-monitor-native.json",
        serde_json::to_vec_pretty(&view).unwrap(),
    )
    .unwrap();
}
#[test]
fn invalid_future_preferences_are_preserved_and_never_enable_collection() {
    let (_temp, database, _home) = fixture();
    let mut db = database.lock().unwrap();
    db.execute_batch(SCHEMA).unwrap();
    db.execute(
        "INSERT INTO storage_monitor_preferences VALUES(1,2,'{}')",
        [],
    )
    .unwrap();
    assert!(load(&db).is_err());
    assert!(save_config(&mut db, Config::default()).is_err());
    assert_eq!(
        db.query_row::<u32, _, _>("SELECT version FROM storage_monitor_preferences", [], |r| r
            .get(0))
            .unwrap(),
        2
    );
}
