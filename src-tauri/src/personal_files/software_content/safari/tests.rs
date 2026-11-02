use super::*;
fn job() -> Arc<Job> {
    Arc::new(Job::new(crate::engine::TaskKind::PersonalScan))
}
fn selection(profile: &str) -> Selection {
    Selection {
        browser: Browser::Safari,
        profile: profile.into(),
    }
}
fn entry(path: &Path) -> String {
    format!("<dict><key>DownloadEntryPath</key><string>{}</string><key>DownloadEntryProgressTotalToLoad</key><integer>24</integer><key>DownloadEntryProgressBytesSoFar</key><integer>24</integer><key>DownloadEntryDateFinished</key><date>2026-10-01T00:00:00Z</date><key>DownloadEntryURL</key><string>https://synthetic.invalid/never-open</string><key>DownloadEntryBookmarkBlob</key><data>AAEC</data></dict>",quick_xml::escape::escape(path.to_str().unwrap()))
}
fn bytes(rows: &str) -> Vec<u8> {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>DownloadHistory</key><array>{rows}</array></dict></plist>").into_bytes()
}
fn setup() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let t = tempfile::Builder::new()
        .prefix("MooMole-safari-owned-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let chosen = home.join("Downloads/中文 & sample.txt");
    std::fs::create_dir(chosen.parent().unwrap()).unwrap();
    std::fs::write(&chosen, b"owned completed download").unwrap();
    let history = selection("Standard").history(&home).unwrap();
    std::fs::create_dir_all(history.parent().unwrap()).unwrap();
    std::fs::write(&history, bytes(&entry(&chosen))).unwrap();
    (t, home, chosen, history)
}
fn binary(path: &Path) {
    let out = std::process::Command::new("/usr/bin/plutil")
        .args(["-convert", "binary1", "-o", "-"])
        .arg(path)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stdout.starts_with(b"bplist00"));
    std::fs::write(path, out.stdout).unwrap();
}
#[test]
fn safari_resolves_only_fixed_lists_and_discovery_does_not_parse_contents() {
    let (_t, home, _chosen, standard) = setup();
    std::fs::write(&standard, b"invalid plist; metadata only").unwrap();
    let sandbox = selection("Sandbox").history(&home).unwrap();
    std::fs::create_dir_all(sandbox.parent().unwrap()).unwrap();
    std::fs::write(&sandbox, b"invalid").unwrap();
    let result = browser_profiles::discover(&home, Browser::Safari, &[]).unwrap();
    assert!(result.complete);
    assert_eq!(result.profiles.len(), 2);
    for invalid in [
        "Default",
        "Profile 2",
        "../Standard",
        "Standard/child",
        "standard",
        "",
    ] {
        assert!(selection(invalid).history(&home).is_err());
    }
    assert!(
        !browser_profiles::discover(&home, Browser::Safari, &[standard.to_str().unwrap().into()])
            .unwrap()
            .complete
    );
    assert!(collect(&home, selection("Standard"), &[], job()).is_err());
}
#[test]
fn safari_xml_and_system_binary_keep_only_explicit_completed_files_without_urls_or_bookmarks() {
    let (_t, home, chosen, history) = setup();
    for as_binary in [false, true] {
        std::fs::write(&history, bytes(&entry(&chosen))).unwrap();
        if as_binary {
            binary(&history);
        }
        let before = std::fs::read(&history).unwrap();
        let scan = collect(&home, selection("Standard"), &[], job()).unwrap();
        assert!(scan.report.complete);
        assert_eq!(scan.items.len(), 1);
        assert_eq!(scan.items[0].path, chosen.to_str().unwrap());
        let serialized = serde_json::to_string(&scan.report).unwrap();
        assert!(!serialized.contains("synthetic.invalid"));
        assert!(!serialized.contains("BookmarkBlob"));
        assert_eq!(std::fs::read(&history).unwrap(), before);
        scan.software_content
            .as_ref()
            .unwrap()
            .validate_record(&scan.root, &scan.items)
            .unwrap();
    }
}
#[test]
fn safari_incomplete_failed_unknown_and_ambiguous_records_are_retained() {
    let (_t, home, chosen, history) = setup();
    let valid = entry(&chosen);
    for bad in [
        valid.replace("<integer>24</integer>", "<integer>25</integer>"),
        valid.replacen("<integer>24</integer>", "<integer>23</integer>", 1),
        valid.replace(
            "</dict>",
            "<key>DownloadEntryErrorCodeDictionaryKey</key><integer>-999</integer></dict>",
        ),
        valid.replace(
            "</dict>",
            "<key>DownloadEntryResumeData</key><data>AAEC</data></dict>",
        ),
        valid.replace(
            "<date>2026-10-01T00:00:00Z</date>",
            "<string>2026-10-01</string>",
        ),
        valid.replace(
            "<date>2026-10-01T00:00:00Z</date>",
            "<date>2099-10-01T00:00:00Z</date>",
        ),
        valid.replace("<integer>24</integer>", "<true/>"),
        valid.replace("<integer>24</integer>", "<real>24</real>"),
        valid.replace(
            "<date>2026-10-01T00:00:00Z</date>",
            "<date>1900-10-01T00:00:00Z</date>",
        ),
        format!("{valid}{valid}"),
        format!(
            "{valid}{}",
            valid.replace(
                "</dict>",
                "<key>DownloadEntryResumeData</key><data>AAEC</data></dict>"
            )
        ),
    ] {
        std::fs::write(&history, bytes(&bad)).unwrap();
        assert!(collect(&home, selection("Standard"), &[], job())
            .unwrap()
            .items
            .is_empty());
    }
    std::fs::write(&history, bytes(&valid.repeat(2001))).unwrap();
    let r = collect(&home, selection("Standard"), &[], job()).unwrap();
    assert!(!r.report.complete);
    assert!(r.items.is_empty());
    assert_eq!(r.report.visited, 2000);
}
#[test]
fn safari_protection_cancellation_links_changed_files_and_source_identity_do_not_authorize_actions()
{
    let (_t, home, chosen, history) = setup();
    assert!(collect(
        &home,
        selection("Standard"),
        &[history.to_str().unwrap().into()],
        job()
    )
    .is_err());
    assert!(collect(
        &home,
        selection("Standard"),
        &[chosen.to_str().unwrap().into()],
        job()
    )
    .unwrap()
    .items
    .is_empty());
    let stop = job();
    stop.cancel();
    assert!(collect(&home, selection("Standard"), &[], stop).is_err());
    let scan = collect(&home, selection("Standard"), &[], job()).unwrap();
    let binding = scan.software_content.as_ref().unwrap();
    let parent = history.parent().unwrap();
    std::fs::rename(parent, parent.with_file_name("Safari-old")).unwrap();
    std::fs::create_dir(parent).unwrap();
    std::fs::write(&history, bytes(&entry(&chosen))).unwrap();
    assert!(binding.validate(&scan.items[0], &home, db::now()).is_err());
    std::fs::write(&chosen, b"changed").unwrap();
    assert!(collect(&home, selection("Standard"), &[], job())
        .unwrap()
        .items
        .is_empty());
    std::fs::remove_file(&history).unwrap();
    std::os::unix::fs::symlink(home.join("Library/Safari-old/Downloads.plist"), &history).unwrap();
    assert!(collect(&home, selection("Standard"), &[], job()).is_err());
    std::fs::remove_file(&history).unwrap();
    std::fs::hard_link(home.join("Library/Safari-old/Downloads.plist"), &history).unwrap();
    assert!(collect(&home, selection("Standard"), &[], job()).is_err());
}

#[tokio::test]
#[ignore = "real formal Mole + native Trash on owned Safari-schema XML/binary samples; exact receipt return, DB reopen and archive; no real Safari files"]
async fn real_safari_lists_preserve_source_and_unlisted_files_through_exact_confirmed_trash() {
    use crate::{
        engine::TaskKind,
        personal_files::{writer, Store},
        rules,
    };
    let mut temp = tempfile::Builder::new()
        .prefix("MooMole-safari-native-")
        .tempdir()
        .unwrap();
    let root = temp.path().canonicalize().unwrap();
    let mut evidence = vec![];
    for (profile, as_binary) in [("Standard", false), ("Sandbox", true)] {
        let home = root.join(profile);
        std::fs::create_dir(&home).unwrap();
        let chosen = home.join("Downloads/owned.txt");
        std::fs::create_dir(chosen.parent().unwrap()).unwrap();
        std::fs::write(&chosen, b"owned completed download").unwrap();
        let keep = home.join("Downloads/unlisted.txt");
        std::fs::write(&keep, b"unlisted keep").unwrap();
        let history = selection(profile).history(&home).unwrap();
        std::fs::create_dir_all(history.parent().unwrap()).unwrap();
        std::fs::write(&history, bytes(&entry(&chosen))).unwrap();
        if as_binary {
            binary(&history);
        }
        let source_bytes = std::fs::read(&history).unwrap();
        let scan = collect(&home, selection(profile), &[], job()).unwrap();
        assert_eq!(scan.items.len(), 1);
        let report = serde_json::to_value(&scan.report).unwrap();
        let ids = vec![scan.items[0].id.clone()];
        let mut store = Store {
            scan: Some(scan),
            ..Default::default()
        };
        let offer = store.offer(&ids, db::now()).unwrap();
        let confirmation = serde_json::to_value(&offer).unwrap();
        let plan = store.take(&offer.id, db::now()).unwrap();
        let dbpath = home.join("state.sqlite");
        let database = db::open_owned(&dbpath).unwrap();
        db::save_settings(
            &database,
            &db::Settings {
                acknowledged: true,
                ..Default::default()
            },
        )
        .unwrap();
        let database = Arc::new(std::sync::Mutex::new(database));
        let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        let rule_home = home.clone();
        temp.disable_cleanup(true);
        let rescue_root = root.clone();
        let result = writer::run_with(
            database.clone(),
            home.clone(),
            plan,
            Arc::new(Job::new(TaskKind::PersonalTrash)),
            move |requested, control| {
                let (engine, home) = (engine.clone(), rule_home.clone());
                async move { rules::run(&engine, &home, &requested, control).await }
            },
            move |id| {
                std::fs::write(
                    "../.cache/safari-native-rescue.json",
                    serde_json::to_vec(&serde_json::json!({"ownedRoot":rescue_root,"runId":id}))
                        .unwrap(),
                )
                .unwrap();
            },
        )
        .await
        .unwrap();
        std::fs::write(
            "../.cache/safari-native-rescue.json",
            serde_json::to_vec(&serde_json::json!({"ownedRoot":root,"result":result})).unwrap(),
        )
        .unwrap();
        for row in &result.items {
            if let Some(r) = &row.receipt {
                let m = std::fs::symlink_metadata(&r.destination).unwrap();
                assert!(m.is_file());
                assert_eq!((m.dev(), m.ino()), (r.device, r.inode));
                assert_eq!(
                    std::fs::read(&r.destination).unwrap(),
                    b"owned completed download"
                );
                assert!(!Path::new(&row.path).exists());
                std::fs::rename(&r.destination, &row.path).unwrap();
                assert_eq!(
                    std::fs::read(&row.path).unwrap(),
                    b"owned completed download"
                );
            }
        }
        temp.disable_cleanup(false);
        std::fs::remove_file("../.cache/safari-native-rescue.json").unwrap();
        assert_eq!(
            result.summary.moved_count,
            1,
            "{}",
            serde_json::to_string(&result).unwrap()
        );
        assert_eq!(std::fs::read(&history).unwrap(), source_bytes);
        assert_eq!(std::fs::read(&keep).unwrap(), b"unlisted keep");
        let expected = serde_json::to_value(&result).unwrap();
        drop(database);
        let mut reopened = db::open_owned(&dbpath).unwrap();
        assert_eq!(
            serde_json::to_value(crate::execution::detail(&reopened, &result.summary.id).unwrap())
                .unwrap(),
            expected
        );
        let draft = crate::execution_archive::prepare(&mut reopened).unwrap();
        let archive_dir = home.join("Archives");
        let header = crate::execution_archive::archive(
            &mut reopened,
            &archive_dir,
            &draft.fingerprint,
            db::now(),
        )
        .unwrap();
        assert_eq!(
            crate::execution_archive::detail(
                &archive_dir,
                &header.id,
                crate::execution_archive::Kind::Cleanup,
                &result.summary.id
            )
            .unwrap(),
            expected
        );
        evidence.push(serde_json::json!({"profile":profile,"binary":as_binary,"report":report,"confirmation":confirmation,"result":expected,"sourceAndUnlistedPreserved":true,"exactOwnedReceiptReturned":true,"databaseReopenAndArchiveVerified":true}));
    }
    let owned = temp.path().to_path_buf();
    drop(temp);
    assert!(!owned.exists());
    std::fs::write("../.cache/safari-downloads-native.json",serde_json::to_vec(&serde_json::json!({"scope":"owned source-schema core acceptance, not real Safari or App window compatibility","cases":evidence,"ownedTemporaryRootRemoved":true})).unwrap()).unwrap();
}
