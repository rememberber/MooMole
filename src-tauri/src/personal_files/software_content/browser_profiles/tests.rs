use super::*;
use rusqlite::Connection;
fn job() -> Arc<Job> {
    Arc::new(Job::new(crate::engine::TaskKind::PersonalScan))
}
fn fixture(home: &Path, selection: &Selection, file: &Path) -> Connection {
    let history = selection.history(home).unwrap();
    std::fs::create_dir_all(history.parent().unwrap()).unwrap();
    if selection.browser == Browser::Firefox {
        return super::super::firefox::fixture(&history, file);
    }
    let db = Connection::open(history).unwrap();
    db.pragma_update(None, "journal_mode", "WAL").unwrap();
    db.execute_batch("CREATE TABLE downloads(id INTEGER PRIMARY KEY,current_path TEXT,target_path TEXT,state INTEGER)").unwrap();
    db.execute(
        "INSERT INTO downloads VALUES(1,?1,?1,1)",
        [file.to_str().unwrap()],
    )
    .unwrap();
    db
}
#[test]
fn browser_profiles_discovery_is_bounded_scoped_and_keeps_unknown_unreadable_profiles() {
    let t = tempfile::Builder::new()
        .prefix("MooMole-profiles-owned-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    for name in ["Default", "Profile 0", "Profile 1", "Profile 4294967295"] {
        assert!(profile_name(name));
    }
    for name in [
        "",
        "../Default",
        "/tmp/Default",
        "Profile ../",
        "Profile 01",
        "Guest Profile",
        "System Profile",
        "Profile -1",
        "Profile １２",
        "Profile 1/History",
        "Profile 1\n",
    ] {
        assert!(!profile_name(name));
    }
    let missing = discover(&home, Browser::Edge, &[]).unwrap();
    assert!(missing.complete && missing.profiles.is_empty());
    let selection = Selection {
        browser: Browser::Brave,
        profile: "Profile 2".into(),
    };
    let _db = fixture(&home, &selection, &home.join("Downloads/a.txt"));
    let base = Browser::Brave.root(&home);
    std::fs::create_dir(base.join("Profile 3")).unwrap(); // no readable History is unknown
    std::os::unix::fs::symlink(base.join("Profile 2"), base.join("Profile 4")).unwrap();
    std::fs::create_dir(base.join("Guest Profile")).unwrap();
    let profiles = discover(&home, Browser::Brave, &[]).unwrap();
    assert_eq!(profiles.profiles.len(), 1);
    assert_eq!(profiles.profiles[0].selection, selection);
    assert!(!profiles.complete);
    assert_eq!(profiles.skipped, 2);
    assert!(discover(&home, Browser::Brave, &[base.to_str().unwrap().into()]).is_err());
    let protected = discover(
        &home,
        Browser::Brave,
        &[selection.history(&home).unwrap().to_str().unwrap().into()],
    );
    assert!(protected.is_err());
    for i in 5..2100 {
        std::fs::write(base.join(format!("ordinary-{i}")), "").unwrap();
    }
    assert!(!discover(&home, Browser::Brave, &[]).unwrap().complete);
}
#[test]
fn all_browser_profiles_preserve_wal_databases_and_bind_exact_persistent_source() {
    let t = tempfile::Builder::new()
        .prefix("MooMole-profiles-owned-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    std::fs::create_dir(home.join("Downloads")).unwrap();
    for (index, browser) in [
        Browser::Chrome,
        Browser::Chromium,
        Browser::Edge,
        Browser::Brave,
        Browser::Firefox,
    ]
    .into_iter()
    .enumerate()
    {
        let selection = Selection {
            browser,
            profile: format!("Profile {}", index + 1),
        };
        let selected = home.join(format!("Downloads/browser-{index}.txt"));
        std::fs::write(&selected, "owned").unwrap();
        let _db = fixture(&home, &selection, &selected);
        let other = Selection {
            browser,
            profile: "Default".into(),
        };
        let unselected = home.join(format!("Downloads/unselected-{index}.txt"));
        std::fs::write(&unselected, "keep").unwrap();
        let _other = fixture(&home, &other, &unselected);
        let history = selection.history(&home).unwrap();
        let wal = history.with_file_name(format!("{}-wal", browser.database_name()));
        let before = (
            std::fs::read(&history).unwrap(),
            std::fs::read(&wal).unwrap(),
        );
        let scan =
            super::super::chromium::collect_profile(&home, selection.clone(), &[], job()).unwrap();
        assert_eq!(scan.items.len(), 1);
        assert_eq!(scan.items[0].path, selected.to_str().unwrap());
        assert_eq!(
            scan.report
                .software_content
                .as_ref()
                .unwrap()
                .browser_profile
                .as_ref(),
            Some(&selection)
        );
        let id = scan.items[0].id.clone();
        let mut store = Store {
            scan: Some(scan),
            ..Default::default()
        };
        let offer = store.offer(&[id], db::now()).unwrap();
        let plan = store.take(&offer.id, db::now()).unwrap();
        plan.validate_record().unwrap();
        let value = serde_json::to_value(&plan).unwrap();
        let rebound: Plan = serde_json::from_value(value).unwrap();
        rebound.validate_record().unwrap();
        let mut binding = plan.software_content.clone().unwrap();
        binding.browser_profile = Some(other);
        assert!(binding
            .validate_record(home.to_str().unwrap(), &plan.items)
            .is_err());
        binding = plan.software_content.clone().unwrap();
        binding.browser_profile.as_mut().unwrap().profile = "../../Default".into();
        assert!(binding
            .validate_record(home.to_str().unwrap(), &plan.items)
            .is_err());
        assert_eq!(
            (
                std::fs::read(&history).unwrap(),
                std::fs::read(&wal).unwrap()
            ),
            before
        );
        let retained = home.join(format!("old-profile-{index}"));
        std::fs::rename(history.parent().unwrap(), &retained).unwrap();
        std::fs::create_dir(history.parent().unwrap()).unwrap();
        assert!(plan
            .software_content
            .as_ref()
            .unwrap()
            .validate(&plan.items[0], &home, db::now())
            .is_err());
    }
}
