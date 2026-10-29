use super::*;
#[test]
fn native_visual_groups_are_not_byte_duplicates_and_keep_one_unchanged_original() {
    let temp = tempfile::Builder::new()
        .prefix("MooMole-similar-owned-")
        .tempdir()
        .unwrap();
    let root = temp.path().canonicalize().unwrap();
    let first = media::fixture_png(128, 96);
    let second = media::fixture_png(256, 192);
    assert_ne!(first, second);
    std::fs::write(root.join("first.png"), &first).unwrap();
    std::fs::write(root.join("resized.png"), &second).unwrap();
    std::fs::write(
        root.join("unrelated.png"),
        media::fixture_other_png(128, 96),
    )
    .unwrap();
    let job = Arc::new(Job::new(crate::engine::TaskKind::PersonalScan));
    let scan = super::super::scan::collect(&root, &[], job.clone()).unwrap();
    let scan = objc2::rc::autoreleasepool(|_| collect(scan, job.clone(), &[])).unwrap();
    let report = scan.report.similar_photos.as_ref().unwrap();
    assert_eq!(report.checked_files, 3);
    assert_eq!(scan.items.len(), 2);
    assert_eq!(
        report.groups.len(),
        1,
        "{}",
        serde_json::to_string(&scan.report).unwrap()
    );
    assert_ne!(report.images[0].digest, report.images[1].digest);
    let all = scan.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
    let one = all[0].clone();
    let now = crate::db::now();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    assert!(store.offer(&all, now).is_err());
    let confirmation = store.offer(&[one.clone()], now).unwrap();
    let plan = store.take(&confirmation.id, now).unwrap();
    assert_eq!(plan.origin(), MODULE);
    assert!(plan.duplicates.is_none());
    verify(&plan, &plan.items[0], &job, &[]).unwrap();
    let keeper = &plan.similar_photos.as_ref().unwrap()[0].keeper;
    std::fs::write(&keeper.path, "changed keeper").unwrap();
    assert!(verify(&plan, &plan.items[0], &job, &[]).is_err());
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/similar-photos-native-groups.json",serde_json::to_vec_pretty(&serde_json::json!({"confirmation":confirmation,"byteDifferent":true,"preserveOneEnforced":true,"changedKeeperRefused":true})).unwrap()).unwrap();
}
