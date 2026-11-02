use super::*;
const GIB: u64 = 1024 * 1024 * 1024;
fn fixture() -> (tempfile::TempDir, Connection) {
    let root = tempfile::tempdir().unwrap();
    let c = db::open_fixture(&root.path().join("notifications.sqlite")).unwrap();
    db::save_settings(
        &c,
        &db::Settings {
            acknowledged: true,
            ..Default::default()
        },
    )
    .unwrap();
    (root, c)
}
fn view(at: i64, available: u64) -> crate::storage_monitor::View {
    crate::storage_monitor::View {
        config: crate::storage_monitor::Config {
            enabled: true,
            interval_minutes: 15,
            warn_below_bytes: 10 * GIB,
        },
        samples: vec![Sample {
            measured_at: at,
            volume_id: "1:aabb".into(),
            total_bytes: 100 * GIB,
            available_bytes: available,
        }],
        collecting: false,
        error: None,
    }
}
fn allowed() -> native::Permission {
    native::Permission {
        authorization: "authorized".into(),
        alerts: true,
        center: true,
    }
}
#[test]
fn opt_in_is_separate_and_defaults_do_not_create_tables_or_request_permission() {
    let (_t, mut c) = fixture();
    assert!(!load(&c).unwrap().enabled);
    assert!(!exists(&c).unwrap());
    assert!(configure(&mut c, true, None).is_err());
    assert!(!exists(&c).unwrap());
    configure(&mut c, true, Some(&allowed())).unwrap();
    assert!(load(&c).unwrap().enabled);
    configure(&mut c, false, None).unwrap();
    assert!(!load(&c).unwrap().enabled);
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        8
    );
    assert_eq!(crate::execution::list(&c).unwrap().len(), 0);
}
#[test]
fn fresh_crossing_latches_until_recovery_and_does_not_replay_old_or_failed_samples() {
    let mut saved = Saved {
        enabled: true,
        ..Default::default()
    };
    let mut v = view(1_000_000, 9 * GIB);
    assert!(decision(&mut saved, &v, 1_000_001).is_some());
    assert!(decision(&mut saved, &v, 1_000_002).is_none());
    v.samples[0].measured_at += 1;
    assert!(decision(&mut saved, &v, 1_000_002).is_none());
    v.samples[0].available_bytes = 10 * GIB;
    v.samples[0].measured_at += 1;
    assert!(decision(&mut saved, &v, 1_000_003).is_none());
    assert!(saved.gate.latched);
    v.samples[0].available_bytes = 11 * GIB;
    v.samples[0].measured_at += 1;
    assert!(decision(&mut saved, &v, 1_000_004).is_none());
    assert!(!saved.gate.latched);
    v.samples[0].available_bytes = GIB;
    v.samples[0].measured_at += 1;
    assert!(decision(&mut saved, &v, 1_000_005).is_some());
    v.samples[0].measured_at = 900_000;
    assert!(decision(&mut saved, &v, 1_000_006).is_none());
}
#[test]
fn disabled_unknown_stale_future_in_progress_and_clock_rollback_cannot_alert() {
    for case in 0..7 {
        let mut saved = Saved {
            enabled: true,
            ..Default::default()
        };
        let mut v = view(1_000_000, GIB);
        let mut now = 1_000_001;
        match case {
            0 => saved.enabled = false,
            1 => v.config.enabled = false,
            2 => v.error = Some("measurement failed".into()),
            3 => v.collecting = true,
            4 => now = 3_000_001,
            5 => now = 999_999,
            _ => v.samples.clear(),
        };
        let before = saved.clone();
        assert!(decision(&mut saved, &v, now).is_none(), "case {case}");
        assert_eq!(saved, before);
    }
}
#[test]
fn durable_intent_survives_restart_without_duplicate_and_test_does_not_enable_monitoring() {
    let (root, mut c) = fixture();
    configure(&mut c, true, Some(&allowed())).unwrap();
    let v = view(1_000_000, GIB);
    let row = begin(&mut c, Some(&v), 1_000_001).unwrap().unwrap();
    assert_eq!(row.state, "submitting");
    drop(c);
    let mut c = db::open_fixture(&root.path().join("notifications.sqlite")).unwrap();
    recover(&mut c, 1_000_010).unwrap();
    assert_eq!(records(&c).unwrap()[0].state, "unknown");
    assert!(begin(&mut c, Some(&v), 1_000_020).unwrap().is_none());
    configure(&mut c, false, None).unwrap();
    let test = begin(&mut c, None, 1_000_030).unwrap().unwrap();
    assert_eq!(test.kind, "test");
    finish(&mut c, test, Ok(()), 1_000_031).unwrap();
    assert_eq!(records(&c).unwrap()[0].state, "submitted");
    assert!(!load(&c).unwrap().enabled);
}
#[test]
fn permission_failures_remain_blocked_unknown_stays_unknown_and_records_are_bounded() {
    let (_t, mut c) = fixture();
    for n in 0..55 {
        let row = begin(&mut c, None, 1000 + n).unwrap().unwrap();
        finish(
            &mut c,
            row,
            Err(if n % 2 == 0 {
                "permissionDenied"
            } else {
                "submissionTimeout"
            }
            .into()),
            1000 + n,
        )
        .unwrap();
    }
    let rows = records(&c).unwrap();
    assert_eq!(rows.len(), 50);
    assert_eq!(rows[0].state, "blocked");
    assert_eq!(rows[1].state, "unknown");
    assert!(!load(&c).unwrap().enabled);
    c.execute("UPDATE storage_notification_state SET version=2", [])
        .unwrap();
    assert!(configure(&mut c, false, None).is_err());
    assert!(begin(&mut c, None, 2000).is_err());
    assert!(recover(&mut c, 2001).is_err());
    assert_eq!(
        c.query_row("SELECT version FROM storage_notification_state", [], |r| {
            r.get::<_, u32>(0)
        })
        .unwrap(),
        2
    );
}
#[test]
fn failed_intent_transaction_cannot_enqueue_or_consume_episode_and_language_uses_no_paths() {
    let (_t, mut c) = fixture();
    configure(&mut c, true, Some(&allowed())).unwrap();
    let v = view(1_000_000, GIB);
    let before = load(&c).unwrap();
    c.execute_batch("CREATE TRIGGER owned_fail BEFORE INSERT ON storage_notification_records BEGIN SELECT RAISE(FAIL,'owned failure'); END;").unwrap();
    assert!(begin(&mut c, Some(&v), 1_000_001).is_err());
    assert_eq!(load(&c).unwrap(), before);
    assert!(records(&c).unwrap().is_empty());
    c.execute_batch("DROP TRIGGER owned_fail;").unwrap();
    let row = begin(&mut c, Some(&v), 1_000_002).unwrap().unwrap();
    for lang in ["zh", "en", "ja"] {
        let (title, body) = wording(&row, lang);
        assert!(!title.is_empty());
        assert!(body.contains("GiB"));
        assert!(!body.contains("/Users"));
    }
    c.pragma_update(None, "synchronous", "OFF").unwrap();
    assert!(begin(&mut c, None, 1_000_003).is_err());
}
#[test]
fn native_api_refuses_unbundled_test_process_without_permission_prompt_or_notification() {
    let result = std::thread::spawn(|| native::permission("io.github.rememberber.moomole", false))
        .join()
        .unwrap();
    assert_eq!(result.unwrap_err(), "requiresPackagedApp");
}
