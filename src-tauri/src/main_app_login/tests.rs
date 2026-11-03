use super::*;
use std::{cell::Cell, sync::Mutex};
const BUNDLE: &str = "io.github.rememberber.moomole";
fn system(status: Status) -> System {
    System {
        bundle_id: BUNDLE.into(),
        bundle_path: "/Applications/MooMole.app".into(),
        status,
    }
}
fn reply(status: Status) -> Result<Reply, String> {
    Ok(Reply {
        system: system(status),
        accepted: true,
        error: None,
        native_code: None,
    })
}
fn fixture() -> (tempfile::TempDir, Mutex<db::Database>) {
    let t = tempfile::Builder::new()
        .prefix("moomole-login-journal-owned-")
        .tempdir()
        .unwrap();
    let c = db::open_owned(&t.path().join("state.sqlite")).unwrap();
    db::save_settings(
        &c,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    (t, Mutex::new(c))
}
#[test]
fn defaults_read_without_writing_and_live_intent_precedes_only_the_own_app_request() {
    let (_t, c) = fixture();
    assert!(records(&c.lock().unwrap()).unwrap().is_empty());
    assert_eq!(
        c.lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='main_app_login_records'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    let calls = Cell::new(0);
    let id = uuid::Uuid::new_v4().to_string();
    let r = apply(
        &c,
        &id,
        BUNDLE,
        true,
        || Ok(()),
        |a| match a {
            Action::Read => reply(Status::NotFound),
            Action::Enable => {
                calls.set(calls.get() + 1);
                let rows = records(&c.lock().unwrap()).unwrap();
                assert_eq!(rows[0].state, "running");
                assert_eq!(rows[0].id, id);
                reply(Status::Enabled)
            }
            Action::Disable => panic!("wrong operation"),
        },
    )
    .unwrap();
    assert_eq!(r.state, "accepted");
    assert_eq!(calls.get(), 1);
    assert!(apply(
        &c,
        &id,
        BUNDLE,
        true,
        || Ok(()),
        |a| {
            assert!(matches!(a, Action::Read));
            reply(Status::Enabled)
        }
    )
    .is_err());
    let r = apply(
        &c,
        &uuid::Uuid::new_v4().to_string(),
        BUNDLE,
        false,
        || Ok(()),
        |a| match a {
            Action::Read => reply(Status::Enabled),
            Action::Disable => reply(Status::NotRegistered),
            _ => panic!(),
        },
    )
    .unwrap();
    assert_eq!(r.state, "accepted");
    assert!(db::settings(&c.lock().unwrap()).unwrap().acknowledged);
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/main-app-login-journal-native.json",serde_json::to_vec_pretty(&serde_json::json!({"records":records(&c.lock().unwrap()).unwrap(),"intentBeforeInjectedNative":true,"firstReadDidNotCreateTable":true,"scope":"real SQLite, injected system statuses; not OS registration evidence"})).unwrap()).unwrap();
}
#[test]
fn intent_failure_cancellation_or_unacknowledged_state_never_calls_a_mutator() {
    let (_t, c) = fixture();
    let count = Cell::new(0);
    let native = |a| {
        if !matches!(a, Action::Read) {
            count.set(count.get() + 1);
        }
        reply(Status::NotRegistered)
    };
    c.lock().unwrap().execute_batch(SCHEMA).unwrap();
    c.lock().unwrap().execute_batch("CREATE TRIGGER fail_login BEFORE INSERT ON main_app_login_records BEGIN SELECT RAISE(ABORT,'owned failure');END;").unwrap();
    assert!(apply(
        &c,
        &uuid::Uuid::new_v4().to_string(),
        BUNDLE,
        true,
        || Ok(()),
        native
    )
    .is_err());
    assert_eq!(count.get(), 0);
    c.lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_login")
        .unwrap();
    let checks = Cell::new(0);
    let r = apply(
        &c,
        &uuid::Uuid::new_v4().to_string(),
        BUNDLE,
        true,
        || {
            checks.set(checks.get() + 1);
            if checks.get() == 3 {
                Err("loginCancelledBeforeRequest".into())
            } else {
                Ok(())
            }
        },
        native,
    )
    .unwrap();
    assert_eq!(r.state, "notStarted");
    assert_eq!(count.get(), 0);
    db::save_settings(&*c.lock().unwrap(), &db::Settings::default()).unwrap();
    assert!(apply(
        &c,
        &uuid::Uuid::new_v4().to_string(),
        BUNDLE,
        true,
        || Ok(()),
        native
    )
    .is_err());
    assert_eq!(count.get(), 0);
}
#[test]
fn lost_completion_and_rejected_requests_keep_unknown_across_reopen_without_replay() {
    let (t, c) = fixture();
    c.lock().unwrap().execute_batch(SCHEMA).unwrap();
    c.lock().unwrap().execute_batch("CREATE TRIGGER lose_login BEFORE UPDATE ON main_app_login_records WHEN json_extract(NEW.body,'$.state')='accepted' BEGIN SELECT RAISE(ABORT,'owned loss');END;").unwrap();
    let calls = Cell::new(0);
    let id = uuid::Uuid::new_v4().to_string();
    assert!(apply(
        &c,
        &id,
        BUNDLE,
        true,
        || Ok(()),
        |a| if matches!(a, Action::Read) {
            reply(Status::NotRegistered)
        } else {
            calls.set(calls.get() + 1);
            reply(Status::Enabled)
        }
    )
    .is_err());
    assert_eq!(calls.get(), 1);
    drop(c);
    let mut c = db::open_owned(&t.path().join("state.sqlite")).unwrap();
    let rows = reconcile(&c, &[]).unwrap();
    assert_eq!(rows[0].state, "unknown");
    assert_eq!(calls.get(), 1);
    assert!(begin(
        &mut c,
        &uuid::Uuid::new_v4().to_string(),
        false,
        system(Status::Enabled)
    )
    .is_err());
    assert!(acknowledge(&c, &id, &system(Status::NotFound)).is_err());
    acknowledge(&c, &id, &system(Status::Enabled)).unwrap();
    assert_eq!(records(&c).unwrap()[0].state, "unknown");
    let row = begin(
        &mut c,
        &uuid::Uuid::new_v4().to_string(),
        false,
        system(Status::Enabled),
    )
    .unwrap();
    let row = finish(&c, row, Err("loginRequestRejected".into())).unwrap();
    assert_eq!(row.state, "unknown");
}
#[test]
fn history_is_bounded_and_future_or_mismatched_data_never_becomes_authority() {
    let (_t, c) = fixture();
    let mut c = c.lock().unwrap();
    for _ in 0..52 {
        let r = begin(
            &mut c,
            &uuid::Uuid::new_v4().to_string(),
            true,
            system(Status::Enabled),
        )
        .unwrap();
        assert_eq!(
            finish(&c, r, reply(Status::Enabled)).unwrap().state,
            "noChange"
        );
    }
    assert_eq!(records(&c).unwrap().len(), 50);
    let r = records(&c).unwrap()[0].clone();
    let mut json = serde_json::to_value(&r).unwrap();
    json["version"] = 2.into();
    c.execute(
        "UPDATE main_app_login_records SET body=?1 WHERE id=?2",
        params![json.to_string(), r.id],
    )
    .unwrap();
    assert!(records(&c).is_err());
    assert!(begin(
        &mut c,
        &uuid::Uuid::new_v4().to_string(),
        true,
        system(Status::Enabled)
    )
    .is_err());
    assert_eq!(
        c.query_row(
            "SELECT body FROM main_app_login_records WHERE id=?1",
            [r.id],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        json.to_string()
    );
    let mut other = system(Status::Enabled);
    other.bundle_id = "org.other.app".into();
    assert!(other.validate(BUNDLE).is_err());
}
#[test]
fn active_ownership_requires_the_same_registered_handle_not_another_job_reusing_an_id() {
    let id = uuid::Uuid::new_v4().to_string();
    let job = Arc::new(crate::engine::Job::new(crate::engine::TaskKind::AppControl));
    let owned = OwnedJob {
        id: id.clone(),
        job: Arc::downgrade(&job),
    };
    let mut jobs = std::collections::HashMap::from([(id.clone(), job)]);
    assert_eq!(active(Some(&owned), &jobs), vec![id.clone()]);
    jobs.insert(
        id.clone(),
        Arc::new(crate::engine::Job::new(crate::engine::TaskKind::AppControl)),
    );
    assert!(active(Some(&owned), &jobs).is_empty());
    assert!(active(None, &jobs).is_empty());
}
