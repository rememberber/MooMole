use super::*;
use crate::app_inspection::identity::fixture;
fn source(path: &Path, id: &str, index: usize) -> Source {
    Source::from_row(
        "62b0b6db-6a11-4879-97e7-bb33500caa67",
        index,
        &serde_json::json!({"name":"Owned","path":path,"bundle_id":id,"source":"App"}),
    )
    .unwrap()
}
#[test]
fn opening_dates_are_bounded_hints_and_missing_or_running_never_becomes_unused() {
    let now = crate::db::now();
    let old = now - 180 * 86400000;
    assert_eq!(
        opened(Date::Value(old as f64 / 1000. - 978307200.), now).unwrap(),
        old
    );
    for n in [
        f64::NAN,
        f64::INFINITY,
        -1e16,
        now as f64 / 1000. - 978307200. + 3600.,
    ] {
        assert!(opened(Date::Value(n), now).is_err());
    }
    assert_eq!(opened(Date::Missing, now).unwrap_err(), "notReported");
    let t = tempfile::Builder::new()
        .prefix("MooMole-app-usage-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let path = fixture(&home, "Owned.app", "org.moomole.usage");
    let s = source(&path, "org.moomole.usage", 2);
    let row = observe(&s, &[], |_, _| {
        (Date::Value(old as f64 / 1000. - 978307200.), Some(false))
    });
    assert_eq!(row.state, "observed");
    assert_eq!(row.index, 2);
    let row = observe(&s, &[], |_, _| (Date::Missing, Some(false)));
    assert_eq!(row.state, "unknown");
    assert_eq!(row.issue, Some("notReported"));
    assert_eq!(
        observe(&s, &[], |_, _| (Date::Missing, Some(true))).state,
        "running"
    );
    assert_eq!(
        observe(&s, &[], |_, _| (
            Date::Value(old as f64 / 1000. - 978307200.),
            None
        ))
        .issue,
        Some("runningUnknown")
    );
}
#[test]
fn usage_is_bound_to_one_copy_and_never_reads_a_protected_replaced_or_mismatched_target() {
    let t = tempfile::Builder::new()
        .prefix("MooMole-app-usage-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let a = fixture(&home, "One.app", "org.moomole.usage");
    let b = fixture(&home, "Two.app", "org.moomole.usage");
    let one = source(&a, "org.moomole.usage", 0);
    let two = source(&b, "org.moomole.usage", 1);
    assert_eq!(
        observe(&one, &[a.to_str().unwrap().into()], |_, _| panic!(
            "protected read"
        ))
        .state,
        "protected"
    );
    assert_eq!(
        observe(&source(&a, "org.moomole.wrong", 0), &[], |_, _| panic!(
            "mismatched read"
        ))
        .state,
        "unknown"
    );
    let row = observe(&two, &[], |path, _| {
        assert_eq!(path, b.to_str().unwrap());
        std::fs::write(b.join("Contents/Info.plist"), "changed").unwrap();
        (Date::Value(0.), Some(false))
    });
    assert_eq!(row.issue, Some("identityChanged"));
    assert!(a.exists());
}
#[test]
fn cancelled_usage_returns_partial_evidence_without_starting_a_read() {
    let j = Job::new(TaskKind::Apps);
    j.cancel();
    let r = inspect(
        "62b0b6db-6a11-4879-97e7-bb33500caa67".into(),
        uuid::Uuid::new_v4().to_string(),
        vec![serde_json::json!({"path":"/unknown.app"})],
        vec![0],
        vec![],
        &j,
    );
    assert!(!r.complete && r.cancelled && r.rows.is_empty());
}
#[test]
#[ignore = "explicit read-only Spotlight query on installed Apps and an owned automatically cleaned fixture; never launches or removes an App"]
fn real_system_usage_metadata_keeps_unknown_and_currently_running_apps_explicit() {
    let mut rows = vec![];
    for name in [
        "Safari",
        "Developer",
        "Cursor",
        "ChatGPT",
        "Docker",
        "MooMole",
    ] {
        let path = format!("/Applications/{name}.app");
        if !Path::new(&path).exists() {
            continue;
        }
        let (date, _) = native::read(&path, "");
        rows.push(
            serde_json::json!({"path":path,"lastOpenedAt":opened(date,crate::db::now()).ok()}),
        );
    }
    assert!(!rows.is_empty());
    let t = tempfile::Builder::new()
        .prefix("MooMole-app-usage-native-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let app = fixture(&home, "Owned.app", "org.moomole.usage.native");
    let original = std::fs::read(app.join("Contents/Info.plist")).unwrap();
    let row = observe(
        &source(&app, "org.moomole.usage.native", 0),
        &[],
        native::read,
    );
    // Spotlight may already provide a timestamp for a newly created bundle.
    // Creation alone proves neither "never opened" nor index absence.
    assert!(["observed", "unknown"].contains(&row.state));
    assert_eq!(row.last_opened_at.is_some(), row.state == "observed");
    assert_eq!(
        std::fs::read(app.join("Contents/Info.plist")).unwrap(),
        original
    );
    let task = "62b0b6db-6a11-4879-97e7-bb33500caa67";
    let report = inspect(
        task.into(),
        uuid::Uuid::new_v4().to_string(),
        vec![
            serde_json::json!({"name":"Owned","path":app,"bundle_id":"org.moomole.usage.native","source":"App"}),
        ],
        vec![0],
        vec![],
        &Job::new(TaskKind::Apps),
    );
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/app-usage-native.json",serde_json::to_vec_pretty(&serde_json::json!({"report":report,"systemRows":rows,"ownedTemporaryApp":row,"originalPreserved":true,"appWindowVerified":false})).unwrap()).unwrap();
}
