use super::*;
use std::fs;
fn job() -> Arc<Job> {
    Arc::new(Job::new(crate::engine::TaskKind::PersonalScan))
}
fn export(tracks: &str) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>Major Version</key><integer>1</integer><key>Minor Version</key><integer>1</integer><key>Application Version</key><string>1.6</string><key>Tracks</key><dict>{tracks}</dict><key>Playlists</key><array><dict><key>Name</key><string>PRIVATE PLAYLIST</string></dict></array></dict></plist>")
}
fn track(id: u64, path: &Path, extra: &str) -> String {
    format!("<key>{id}</key><dict><key>Track ID</key><integer>{id}</integer><key>Name</key><string>PRIVATE TRACK</string><key>Artist</key><string>PRIVATE ARTIST</string><key>Track Type</key><string>File</string><key>Size</key><integer>{}</integer><key>Location</key><string>{}</string>{extra}</dict>",fs::metadata(path).unwrap().len(),reqwest::Url::from_file_path(path).unwrap().as_str().replace('&',"&amp;"))
}
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let t = tempfile::Builder::new()
        .prefix("moomole-music-owned-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    fs::create_dir(home.join("Music")).unwrap();
    let chosen = home.join("Music/owned 中文 & song.wav");
    fs::write(&chosen, b"owned music media").unwrap();
    let xml = home.join("Library.xml");
    fs::write(&xml, export(&track(1, &chosen, ""))).unwrap();
    (t, home, xml, chosen)
}
#[test]
fn music_export_finds_only_explicit_local_files_binds_source_and_keeps_private_fields_out_of_reports(
) {
    let (_t, home, xml, chosen) = fixture();
    let unlisted = home.join("Music/unlisted.mp3");
    fs::write(&unlisted, b"keep unlisted").unwrap();
    let protected = home.join("Music/owned.m4p");
    fs::write(&protected, b"encrypted").unwrap();
    let excluded = home.join("Music/protected.wav");
    fs::write(&excluded, b"keep protected").unwrap();
    let original = export(
        &(track(1, &chosen, "")
            + &track(2, &excluded, "<key>Protected</key><true/>")
            + &track(3, &excluded, "<key>Apple Music</key><true/>")
            + &track(4, &protected, "")),
    );
    fs::write(&xml, &original).unwrap();
    let scan = collect(&home, &xml, &[], job()).unwrap();
    assert_eq!(scan.items.len(), 1);
    assert_eq!(scan.items[0].path, chosen.to_str().unwrap());
    assert_eq!(scan.report.skipped, 3);
    assert!(!scan.report.complete);
    let report = serde_json::to_string(&scan.report).unwrap();
    assert!(!report.contains("PRIVATE"));
    assert!(!report.contains("file://"));
    let id = scan.items[0].id.clone();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = store.offer(&[id], db::now()).unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    plan.validate_record().unwrap();
    let binding = plan.software_content.as_ref().unwrap();
    binding.validate(&plan.items[0], &home, db::now()).unwrap();
    assert_eq!(fs::read_to_string(&xml).unwrap(), original);
    fs::write(&xml, export("")).unwrap();
    assert!(binding.validate(&plan.items[0], &home, db::now()).is_err());
    assert_eq!(fs::read(&unlisted).unwrap(), b"keep unlisted");
}
#[test]
fn music_ambiguous_duplicates_missing_size_links_protection_and_unknown_structures_are_retained() {
    let (_t, home, xml, chosen) = fixture();
    fs::write(
        &xml,
        export(&(track(1, &chosen, "") + &track(2, &chosen, ""))),
    )
    .unwrap();
    assert!(collect(&home, &xml, &[], job()).unwrap().items.is_empty());
    let valid = export(&track(1, &chosen, ""));
    for bad in [
        valid.replace("<key>Size</key>", "<key>Unknown Size</key>"),
        valid.replace("<string>File</string>", "<string>URL</string>"),
        valid
            .replace("<key>Protected</key>", "<key>No Protected</key>")
            .replace(
                "<key>Size</key><integer>17</integer>",
                "<key>Size</key><true/>",
            ),
        valid.replace("file://", "https://"),
        valid.replace("<integer>17</integer>", "<integer>18</integer>"),
    ] {
        fs::write(&xml, bad).unwrap();
        assert!(collect(&home, &xml, &[], job()).unwrap().items.is_empty());
    }
    fs::write(&xml, &valid).unwrap();
    assert!(
        collect(&home, &xml, &[chosen.to_str().unwrap().into()], job())
            .unwrap()
            .items
            .is_empty()
    );
    fs::hard_link(&chosen, home.join("Music/shared.wav")).unwrap();
    assert!(collect(&home, &xml, &[], job()).unwrap().items.is_empty());
    fs::remove_file(home.join("Music/shared.wav")).unwrap();
    fs::rename(&chosen, home.join("Music/original.wav")).unwrap();
    std::os::unix::fs::symlink(home.join("Music/original.wav"), &chosen).unwrap();
    assert!(collect(&home, &xml, &[], job()).unwrap().items.is_empty());
    assert!(collect(&home, &xml, &[xml.to_str().unwrap().into()], job()).is_err());
    let stopped = job();
    stopped.cancel();
    assert!(collect(&home, &xml, &[], stopped).is_err());
    for bad in [valid.replace("<integer>1</integer><key>Minor", "<integer>2</integer><key>Minor"),valid.replace("<key>Tracks</key>","<key>Tracks</key><dict/><key>Tracks</key>"),valid.replace("<key>Location</key>","<key>Location</key><string>file:///unknown</string><key>Location</key>"),valid.replace("<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">","<!DOCTYPE plist [<!ENTITY leak SYSTEM \"file:///etc/passwd\">]>"),"<plist version=\"1.0\"><dict>".into()] {
        fs::write(&xml,bad).unwrap();assert!(collect(&home,&xml,&[],job()).is_err());
    }
}

#[test]
fn music_export_bounds_track_count_depth_size_and_duplicate_keys_even_in_unused_metadata() {
    let (_t, home, xml, chosen) = fixture();
    let many = (1..=2001)
        .map(|id| track(id, &chosen, ""))
        .collect::<String>();
    fs::write(&xml, export(&many)).unwrap();
    assert!(collect(&home, &xml, &[], job()).is_err());
    let valid = export(&track(1, &chosen, ""));
    let duplicate = valid.replace(
        "<key>Name</key><string>PRIVATE PLAYLIST</string>",
        "<key>Name</key><string>one</string><key>Name</key><string>two</string>",
    );
    fs::write(&xml, duplicate).unwrap();
    assert!(collect(&home, &xml, &[], job()).is_err());
    let nested = format!(
        "{}<string>unused</string>{}",
        "<array>".repeat(34),
        "</array>".repeat(34)
    );
    fs::write(
        &xml,
        valid.replace("<string>PRIVATE PLAYLIST</string>", &nested),
    )
    .unwrap();
    assert!(collect(&home, &xml, &[], job()).is_err());
    fs::write(&xml, vec![b' '; 8 * 1024 * 1024 + 1]).unwrap();
    assert!(collect(&home, &xml, &[], job()).is_err());
}
