use super::*;
use browser_profiles::{Browser, Selection};
fn job() -> Arc<Job> {
    Arc::new(Job::new(crate::engine::TaskKind::PersonalScan))
}
fn selection() -> Selection {
    Selection {
        browser: Browser::Firefox,
        profile: "abcd1234.default-release".into(),
    }
}
fn scan(home: &Path, rules: &[String]) -> Result<Scan, String> {
    super::super::chromium::collect_profile(home, selection(), rules, job())
}
#[test]
fn firefox_only_accepts_finished_local_file_uris_and_unambiguous_metadata() {
    assert_eq!(
        path(
            "file:///Users/test/a%20b.zip",
            Some(r#"{"state":1,"fileSize":12}"#)
        )
        .unwrap(),
        Some("/Users/test/a b.zip".into())
    );
    for meta in [
        r#"{"state":2}"#,
        r#"{"state":3}"#,
        r#"{"state":4}"#,
        r#"{"state":1,"deleted":true}"#,
    ] {
        assert_eq!(path("file:///Users/test/a.zip", Some(meta)).unwrap(), None);
    }
    for meta in [
        None,
        Some("{}"),
        Some("invalid"),
        Some(r#"{"state":"1"}"#),
        Some(r#"{"state":1,"state":2}"#),
        Some(r#"{"state":1,"deleted":null}"#),
    ] {
        assert!(path("file:///Users/test/a.zip", meta).is_err());
    }
    for uri in [
        "https://example.invalid/a",
        "file://remote/Users/test/a",
        "file:///Users/test/a?x=1",
        "file:///Users/test/a#fragment",
        "file:///Users/test/%0",
        "file:///Users/test/%zz",
        "file:///Users/test/%00",
        "file:///Users/test/%ff",
        "file:///Users/test/a\n",
    ] {
        assert!(path(uri, Some(r#"{"state":1}"#)).is_err(), "{uri}");
    }
}
#[test]
fn firefox_profile_discovery_observes_only_standard_places_file_metadata() {
    let tmp = tempfile::Builder::new()
        .prefix("MooMole-firefox-profiles-")
        .tempdir()
        .unwrap();
    let home = tmp.path().canonicalize().unwrap();
    for profile in ["abcd1234.default-release", "私人 工作", "work-profile"] {
        let s = Selection {
            browser: Browser::Firefox,
            profile: profile.into(),
        };
        let history = s.history(&home).unwrap();
        std::fs::create_dir_all(history.parent().unwrap()).unwrap();
        // Detection only checks metadata; even invalid SQLite bytes are not opened.
        std::fs::write(&history, "not a database").unwrap();
        std::fs::write(
            history.with_file_name("logins.json"),
            "credentials must stay unread",
        )
        .unwrap();
    }
    for profile in [
        "",
        ".hidden",
        "../outside",
        "bad/child",
        "bad\\child",
        "bad:",
        "bad\n",
        " bad",
        "bad.",
    ] {
        assert!(Selection {
            browser: Browser::Firefox,
            profile: profile.into()
        }
        .history(&home)
        .is_err());
    }
    assert!(Selection {
        browser: Browser::Firefox,
        profile: "界".repeat(43)
    }
    .history(&home)
    .is_err());
    let profiles = browser_profiles::discover(&home, Browser::Firefox, &[]).unwrap();
    assert!(profiles.complete);
    assert_eq!(profiles.profiles.len(), 3);
    assert!(profiles
        .profiles
        .iter()
        .all(|p| p.history_path.ends_with("/places.sqlite")));
    assert!(browser_profiles::discover(&home, Browser::Chrome, &[])
        .unwrap()
        .profiles
        .is_empty());
}
#[test]
fn firefox_places_wal_filters_downloads_and_preserves_every_source_byte() {
    let tmp = tempfile::Builder::new()
        .prefix("MooMole-firefox-records-")
        .tempdir()
        .unwrap();
    let home = tmp.path().canonicalize().unwrap();
    std::fs::create_dir(home.join("Downloads")).unwrap();
    let selected = home.join("Downloads/中文 sample.zip");
    std::fs::write(&selected, "owned download").unwrap();
    let history = selection().history(&home).unwrap();
    std::fs::create_dir_all(history.parent().unwrap()).unwrap();
    let db = fixture(&history, &selected);
    for (id, state, visit) in [
        (2, 2, 7),
        (3, 3, 7),
        (4, 1, 1),
        (5, 1, 7),
        (6, 1, 7),
        (7, 1, 7),
        (8, 1, 7),
    ] {
        let file = home.join(format!("Downloads/retained-{id}.zip"));
        std::fs::write(&file, "keep").unwrap();
        db.execute(
            "INSERT INTO moz_places VALUES(?1,'https://synthetic.invalid/private-page')",
            [id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO moz_historyvisits VALUES(?1,?1,?2)",
            rusqlite::params![id, visit],
        )
        .unwrap();
        db.execute(
            "INSERT INTO moz_annos VALUES(?1,?2,1,?3)",
            rusqlite::params![
                id * 10,
                id,
                reqwest::Url::from_file_path(&file).unwrap().to_string()
            ],
        )
        .unwrap();
        let meta = if id == 5 {
            r#"{"state":1,"deleted":true}"#.to_string()
        } else if id == 6 {
            "invalid".into()
        } else {
            format!(r#"{{"state":{state}}}"#)
        };
        if id != 8 {
            db.execute(
                "INSERT INTO moz_annos VALUES(?1,?2,2,?3)",
                rusqlite::params![id * 10 + 1, id, meta],
            )
            .unwrap();
        }
        if id == 7 {
            db.execute(
                "INSERT INTO moz_annos VALUES(?1,?2,2,?3)",
                rusqlite::params![id * 10 + 2, id, meta],
            )
            .unwrap();
        }
    }
    let wal = history.with_file_name("places.sqlite-wal");
    let before = (
        std::fs::read(&history).unwrap(),
        std::fs::read(&wal).unwrap(),
    );
    let result = scan(&home, &[]).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].path, selected.to_str().unwrap());
    assert!(!result.report.complete);
    assert_eq!(result.report.visited, 7); // ordinary visit is excluded by SQL
    assert!(!serde_json::to_string(&result.report)
        .unwrap()
        .contains("synthetic.invalid"));
    assert_eq!(
        (
            std::fs::read(&history).unwrap(),
            std::fs::read(&wal).unwrap()
        ),
        before
    );
    assert!(scan(&home, &[wal.to_str().unwrap().into()]).is_err());
    assert!(scan(&home, &[history.to_str().unwrap().into()]).is_err());
    let cancelled = job();
    cancelled
        .cancelled
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(super::super::chromium::collect_profile(&home, selection(), &[], cancelled).is_err());
    std::fs::write(
        history.with_file_name("places.sqlite-journal"),
        "unknown rollback",
    )
    .unwrap();
    assert!(scan(&home, &[]).is_err());
}
#[test]
fn firefox_ambiguous_destination_or_nonordinary_schema_never_produces_candidates() {
    let tmp = tempfile::Builder::new()
        .prefix("MooMole-firefox-unknown-")
        .tempdir()
        .unwrap();
    let home = tmp.path().canonicalize().unwrap();
    let file = home.join("owned.txt");
    std::fs::write(&file, "keep").unwrap();
    let history = selection().history(&home).unwrap();
    std::fs::create_dir_all(history.parent().unwrap()).unwrap();
    let db = fixture(&history, &file);
    db.execute("INSERT INTO moz_annos SELECT 3,place_id,anno_attribute_id,content FROM moz_annos WHERE id=1", []).unwrap();
    let result = scan(&home, &[]).unwrap();
    assert!(result.items.is_empty() && !result.report.complete);
    db.execute_batch("DROP TABLE moz_historyvisits; CREATE VIEW moz_historyvisits AS SELECT 1 AS place_id,7 AS visit_type").unwrap();
    assert!(scan(&home, &[]).is_err());
    assert_eq!(std::fs::read_to_string(file).unwrap(), "keep");
}
