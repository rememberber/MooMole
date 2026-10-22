use super::*;
use crate::engine::TaskKind;

#[test]
fn sampling_intervals_are_fixed_and_bound_to_each_session() {
    for (seconds, argument) in [(1, "1s"), (2, "2s"), (5, "5s")] {
        let interval = Interval::try_from(seconds).unwrap();
        assert_eq!(interval.argument(), argument);
        let mut store = Store::default();
        store.begin("sample", interval).unwrap();
        assert_eq!(store.read("sample").unwrap().interval_seconds, seconds);
    }
    for invalid in [0, 3, 10, 60, u64::MAX] {
        assert!(Interval::try_from(invalid).is_err());
    }
}

#[test]
fn io_and_network_fields_preserve_zero_missing_and_limits_without_exposing_ip_or_history() {
    let value = validate(br#"{"collected_at":"2026-10-21T00:00:00Z","cpu":{"usage":0,"per_core_estimated":true},"disk_io":{"read_rate":0,"write_rate":1.5},"network":[{"name":"utun7","rx_rate_mbs":2.5,"tx_rate_mbs":null,"ip":"fixture-private"}],"network_history":{"rx_history":[1,2,3]}}"#).unwrap();
    assert_eq!(value["disk_io"]["read_rate"], 0.0);
    assert_eq!(value["cpu"]["per_core_estimated"], true);
    assert_eq!(value["network"][0]["rx_rate_mbs"], 2.5);
    assert!(value["network"][0]["tx_rate_mbs"].is_null());
    assert!(value["network"][0].get("ip").is_none());
    assert!(value.get("network_history").is_none());
    assert!(validate(br#"{"collected_at":"t"}"#).unwrap()["disk_io"].is_null());
    assert_eq!(
        validate(br#"{"collected_at":"t","network":[]}"#).unwrap()["network"],
        serde_json::json!([])
    );
    for bad in [
        r#"{"collected_at":"t","disk_io":{"read_rate":"invalid"}}"#,
        r#"{"collected_at":"t","network":[{"name":""}]}"#,
        r#"{"collected_at":"t","network":[{"name":"en0"},{"name":"en0"}]}"#,
        r#"{"collected_at":"t","network":[{"name":"a"},{"name":"b"},{"name":"c"},{"name":"d"}]}"#,
    ] {
        assert!(validate(bad.as_bytes()).is_err());
    }
}

#[test]
fn memory_annotations_preserve_reported_counters_and_unknowns_without_inventing_freshness() {
    let raw = br#"{"collected_at":"2026-10-22T00:00:00Z","memory":{"pressure":"warn","available":1024,"swap_used":0,"swap_total":2048,"pressure_collected_at":"not-a-supported-field","private_detail":"omit"}}"#;
    let report = validate(raw).unwrap();
    let memory = &report["memory"];
    assert_eq!(memory["pressure"], "warn");
    assert_eq!(memory["available"], 1024);
    assert_eq!(memory["swap_used"], 0);
    assert_eq!(memory["swap_total"], 2048);
    assert!(memory.get("pressure_collected_at").is_none());
    assert!(memory.get("private_detail").is_none());
    for pressure in ["normal", "critical", "", "unsupported"] {
        let raw = serde_json::json!({"collected_at":"2026-10-22T00:00:00Z", "memory":{"pressure":pressure}});
        let report = validate(&serde_json::to_vec(&raw).unwrap()).unwrap();
        assert_eq!(report["memory"]["pressure"], pressure);
        for key in ["available", "swap_used", "swap_total"] {
            assert!(report["memory"][key].is_null());
        }
    }
    let mut store = Store::default();
    store.begin("memory", Interval::Two).unwrap();
    store.receive("memory", raw).unwrap();
    for field in ["available", "swap_used", "swap_total"] {
        for invalid in [
            serde_json::json!(-1),
            serde_json::json!(0.5),
            serde_json::json!("0"),
        ] {
            let mut value = serde_json::json!({"collected_at":"2026-10-22T00:00:00Z","memory":{}});
            value["memory"][field] = invalid;
            assert!(store
                .receive("memory", &serde_json::to_vec(&value).unwrap())
                .is_err());
            assert_eq!(store.read("memory").unwrap().sequence, 1);
            assert_eq!(
                store.read("memory").unwrap().data.unwrap()["memory"]["pressure"],
                "warn"
            );
        }
    }
    let oversized = serde_json::json!({"collected_at":"t", "memory":{"pressure":"x".repeat(4097)}});
    assert!(validate(&serde_json::to_vec(&oversized).unwrap()).is_err());
}

#[test]
fn hardware_subset_is_bounded_keeps_unknowns_and_does_not_publish_fake_temperature_or_timestamps() {
    let raw = br#"{"collected_at":"2026-10-22T00:00:00Z","gpu":[{"name":"Apple Fixture","usage":-1,"core_count":10,"note":"omit"}],"batteries":[{"percent":0,"status":"discharging","capacity":85,"cycle_count":42,"time_left":"omit"}],"thermal":{"cpu_temp":99,"gpu_temp":88,"battery_temp":35.5,"system_power":12},"hardware_collected_at":"omit"}"#;
    let value = validate(raw).unwrap();
    assert_eq!(value["gpu"][0]["usage"], -1.0);
    assert_eq!(value["gpu"][0]["core_count"], 10);
    assert_eq!(value["batteries"][0]["percent"], 0.0);
    assert_eq!(value["batteries"][0]["capacity"], 85);
    assert_eq!(value["thermal"]["battery_temp"], 35.5);
    assert!(value["gpu"][0].get("note").is_none());
    assert!(value["batteries"][0].get("time_left").is_none());
    assert!(value["thermal"].get("cpu_temp").is_none());
    assert!(value["thermal"].get("gpu_temp").is_none());
    assert!(value["thermal"].get("system_power").is_none());
    assert!(value.get("hardware_collected_at").is_none());
    let missing = validate(br#"{"collected_at":"t"}"#).unwrap();
    for field in ["gpu", "batteries", "thermal"] {
        assert!(missing[field].is_null());
    }
    let empty = validate(br#"{"collected_at":"t","gpu":[],"batteries":[],"thermal":{}}"#).unwrap();
    assert_eq!(empty["gpu"], serde_json::json!([]));
    assert_eq!(empty["batteries"], serde_json::json!([]));
    assert!(empty["thermal"]["battery_temp"].is_null());
    for (field, limit) in [("gpu", 8), ("batteries", 4)] {
        let mut value = serde_json::json!({"collected_at":"t"});
        value[field] = serde_json::json!(vec![serde_json::json!({}); limit]);
        assert!(validate(&serde_json::to_vec(&value).unwrap()).is_ok());
        value[field]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({}));
        assert!(validate(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    for raw in [
        serde_json::json!({"collected_at":"t","gpu":[{"name":"x".repeat(4097)}]}),
        serde_json::json!({"collected_at":"t","batteries":[{"status":"x".repeat(4097)}]}),
        serde_json::json!({"collected_at":"t","gpu":[{"usage":"0"}]}),
        serde_json::json!({"collected_at":"t","batteries":[{"percent":"0"}]}),
        serde_json::json!({"collected_at":"t","thermal":{"battery_temp":"35"}}),
    ] {
        assert!(validate(&serde_json::to_vec(&raw).unwrap()).is_err());
    }
    let mut store = Store::default();
    store.begin("hardware", Interval::Two).unwrap();
    store.receive("hardware", raw).unwrap();
    assert!(store
        .receive("hardware", br#"{"collected_at":"t","gpu":"invalid"}"#)
        .is_err());
    assert_eq!(store.read("hardware").unwrap().sequence, 1);
    assert_eq!(
        store.read("hardware").unwrap().data.unwrap()["thermal"]["battery_temp"],
        35.5
    );
}

#[test]
fn disk_reports_follow_pinned_three_record_limit_and_reject_bad_updates_without_losing_last_snapshot(
) {
    let disk =
        serde_json::json!({"mount":"/", "total":4096, "used":0,"used_percent":0,"external":false});
    let raw = serde_json::json!({"collected_at":"2026-10-22T00:00:00Z","disks":[disk.clone(),disk.clone(),disk.clone()]});
    let mut store = Store::default();
    store.begin("disks", Interval::Two).unwrap();
    store
        .receive("disks", &serde_json::to_vec(&raw).unwrap())
        .unwrap();
    let value = store.read("disks").unwrap().data.unwrap();
    assert_eq!(value["disks"].as_array().unwrap().len(), 3);
    assert_eq!(value["disks"][0]["used"], 0);
    let mut too_many = raw.clone();
    too_many["disks"].as_array_mut().unwrap().push(disk.clone());
    assert!(store
        .receive("disks", &serde_json::to_vec(&too_many).unwrap())
        .is_err());
    assert_eq!(store.read("disks").unwrap().sequence, 1);
    assert_eq!(store.read("disks").unwrap().data.unwrap(), value);
    for invalid in [
        serde_json::json!(-1),
        serde_json::json!(0.5),
        serde_json::json!("0"),
    ] {
        let mut malformed = disk.clone();
        malformed["used"] = invalid;
        assert!(validate(
            &serde_json::to_vec(&serde_json::json!({"collected_at":"t","disks":[malformed]}))
                .unwrap()
        )
        .is_err());
    }
    assert!(validate(br#"{"collected_at":"t"}"#).unwrap()["disks"].is_null());
    assert_eq!(
        validate(br#"{"collected_at":"t","disks":[]}"#).unwrap()["disks"],
        serde_json::json!([])
    );
}

#[test]
fn latest_only_binding_expiry_and_bad_records_preserve_last_evidence() {
    let mut store = Store::default();
    store.begin("first", Interval::Two).unwrap();
    assert!(store.begin("second", Interval::Five).is_err());
    assert!(store.read("wrong").is_err());
    for n in 0..200 {
        store
            .receive(
                "first",
                format!(r#"{{"collected_at":"2026-10-20T00:00:00Z","cpu":{{"usage":{n}}}}}"#)
                    .as_bytes(),
            )
            .unwrap();
    }
    assert_eq!(store.read("first").unwrap().sequence, 200);
    assert_eq!(
        store.read("first").unwrap().data.unwrap()["cpu"]["usage"],
        199.0
    );
    for bad in [
        r#"{"collected_at":""}"#,
        r#"{"collected_at":"t","cpu":[]}"#,
        r#"{"collected_at":"t","host":{}}"#,
        r#"{"collected_at":"t","top_processes":[{"pid":"bad","name":"x"}]}"#,
    ] {
        assert!(store.receive("first", bad.as_bytes()).is_err());
        assert_eq!(store.read("first").unwrap().sequence, 200);
    }
    store.0.as_mut().unwrap().touched = Instant::now() - LEASE;
    assert!(store.read("first").is_err()); // read cannot revive expired lease
    assert!(!store.alive("first"));
    store.finish("first", None);
    assert_eq!(store.read("first").unwrap().state, "stopped");
    store.begin("second", Interval::Five).unwrap();
    store.finish("first", Some("late old response".into()));
    assert!(store.read("second").unwrap().error.is_none());
    assert!(store.receive("first", br#"{"collected_at":"t"}"#).is_err());
}

#[tokio::test]
async fn stream_bounds_bad_framing_eof_and_cancellation_reap_owned_process() {
    for script in [
        "$|=1; print qq({bad}\\n); sleep 20",
        "$|=1; print 'x' x (512*1024+1); sleep 20",
        "$|=1; print qq({\"collected_at\":\"t\"}); exit",
        "close STDOUT; sleep 20",
    ] {
        let mut command = tokio::process::Command::new("/usr/bin/perl");
        command.args(["-e", script]);
        let job = Arc::new(Job::new(TaskKind::StatusWatch));
        let result = tokio::time::timeout(
            Duration::from_secs(6),
            process::stream_lines(command, job.clone(), |bytes| validate(bytes).map(|_| ())),
        )
        .await
        .unwrap();
        assert!(result.is_err());
        assert_eq!(job.process_id(), None);
    }
    let mut command = tokio::process::Command::new("/usr/bin/perl");
    command.args([
        "-e",
        "$|=1; print qq({\"collected_at\":\"t\"}\\n); sleep 20",
    ]);
    let job = Arc::new(Job::new(TaskKind::StatusWatch));
    let stop = job.clone();
    let result = process::stream_lines(command, job.clone(), move |bytes| {
        validate(bytes)?;
        stop.cancel();
        Ok(())
    })
    .await;
    assert!(result.unwrap_err().contains("取消"));
    assert_eq!(job.process_id(), None);
}

#[tokio::test]
async fn split_and_joined_records_are_delivered_without_accumulating_history() {
    let mut command = tokio::process::Command::new("/usr/bin/perl");
    command.args(["-e", "$|=1; print '{\"collected_'; select(undef,undef,undef,0.05); print qq(at\":\"t\"}\\n{\"collected_at\":\"t\"}\\n); sleep 20"]);
    let job = Arc::new(Job::new(TaskKind::StatusWatch));
    let stop = job.clone();
    let store = Arc::new(Mutex::new(Store::default()));
    store.lock().unwrap().begin("s", Interval::Two).unwrap();
    let target = store.clone();
    process::stream_lines(command, job.clone(), move |bytes| {
        target.lock().unwrap().receive("s", bytes)?;
        if target.lock().unwrap().read("s")?.sequence == 2 {
            stop.cancel();
        }
        Ok(())
    })
    .await
    .unwrap_err();
    assert_eq!(store.lock().unwrap().read("s").unwrap().sequence, 2);
    assert_eq!(job.process_id(), None);
}

#[tokio::test]
#[ignore = "Explicit real pinned Mole warm collector; reads system metrics, no user files changed"]
async fn pinned_warm_collector_delivers_multiple_snapshots_and_stops() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
    let home = tempfile::tempdir().unwrap();
    let id = "pinned".to_string();
    let store = Arc::new(Mutex::new(Store::default()));
    store.lock().unwrap().begin(&id, Interval::Two).unwrap();
    let job = Arc::new(Job::new(TaskKind::StatusWatch));
    let running_job = job.clone();
    let running_store = store.clone();
    let running = tokio::spawn(async move {
        run(
            &root,
            home.path(),
            id,
            running_job,
            running_store,
            Interval::Two,
        )
        .await
    });
    let mut pid = None;
    let observed = tokio::time::timeout(Duration::from_secs(35), async {
        loop {
            let snapshot = store.lock().unwrap().read("pinned").unwrap();
            if let Some(current) = job.process_id() {
                if let Some(previous) = pid {
                    assert_eq!(previous, current, "collector restarted");
                }
                pid = Some(current);
            }
            if snapshot.sequence >= 3 {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    job.cancel();
    let result = tokio::time::timeout(Duration::from_secs(6), running)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.process_id(), None);
    assert!(result.unwrap_err().contains("取消"));
    let snapshot = observed.unwrap();
    assert!(pid.is_some());
    assert_eq!(snapshot.state, "running");
    eprintln!(
        "Pinned status watch: {} snapshots; single PID {:?}; stopped and reaped",
        snapshot.sequence, pid
    );
}
