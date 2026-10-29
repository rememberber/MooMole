use super::*;
#[test]
fn exported_uris_keep_manifest_streams_unlisted_and_changed_sources() {
    let temp = tempfile::Builder::new()
        .prefix("MooMole-playlist-owned-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let media = home.join("Movies");
    std::fs::create_dir(&media).unwrap();
    let chosen = media.join("owned clip.mov");
    let unlisted = media.join("unlisted.mp4");
    std::fs::write(&chosen, "owned media").unwrap();
    std::fs::write(&unlisted, "unlisted keep").unwrap();
    std::fs::write(media.join("notes.txt"), "not media").unwrap();
    std::os::unix::fs::symlink(&chosen, media.join("alias.mov")).unwrap();
    let manifest = media.join("owned.m3u8");
    let original = format!("\u{feff}#EXTM3U\r\n#EXTINF:1,owned\r\nowned%20clip.mov\r\n{}\r\nhttps://example.invalid/stream.mp4\r\nnotes.txt\r\nalias.mov\r\nmissing.mp4\r\n#EXTVLCOPT:never-execute-any-option\r\n", reqwest::Url::from_file_path(&chosen).unwrap());
    std::fs::write(&manifest, &original).unwrap();
    let scan = collect(
        &home,
        &manifest,
        &[],
        Arc::new(Job::new(crate::engine::TaskKind::PersonalScan)),
    )
    .unwrap();
    assert_eq!(scan.items.len(), 1);
    assert_eq!(scan.items[0].path, chosen.to_str().unwrap());
    assert!(!scan.report.complete);
    assert_eq!(scan.report.skipped, 5);
    assert_eq!(std::fs::read_to_string(&manifest).unwrap(), original);
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
    std::fs::write(&manifest, "#EXTM3U\nchanged.mp4\n").unwrap();
    assert!(binding.validate(&plan.items[0], &home, db::now()).is_err());
    assert_eq!(std::fs::read_to_string(&unlisted).unwrap(), "unlisted keep");
    assert!(collect(
        &home,
        &manifest,
        &[manifest.to_str().unwrap().into()],
        Arc::new(Job::new(crate::engine::TaskKind::PersonalScan))
    )
    .is_err());
    std::fs::write(
        &manifest,
        "#EXTM3U\n#EXT-X-TARGETDURATION:10\nsegment.mp4\n",
    )
    .unwrap();
    assert!(collect(
        &home,
        &manifest,
        &[],
        Arc::new(Job::new(crate::engine::TaskKind::PersonalScan))
    )
    .is_err());
}
