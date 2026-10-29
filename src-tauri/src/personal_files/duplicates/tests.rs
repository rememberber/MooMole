use super::*;
use crate::engine::TaskKind;
fn fixture() -> (tempfile::TempDir, Scan) {
    let t = tempfile::Builder::new()
        .prefix("MooMole-duplicates-")
        .tempdir()
        .unwrap();
    let root = t.path().canonicalize().unwrap();
    for (name, body) in [
        ("one.txt", "same"),
        ("two.txt", "same"),
        ("three.txt", "same"),
        ("different.txt", "DIFF"),
        ("only.txt", "unique size"),
    ] {
        std::fs::write(root.join(name), body).unwrap();
    }
    let job = Arc::new(Job::new(TaskKind::PersonalScan));
    let scan = super::super::scan::collect(&root, &[], job.clone()).unwrap();
    (t, collect(scan, job).unwrap())
}
#[test]
fn groups_match_content_and_selection_always_leaves_a_copy() {
    let (_t, scan) = fixture();
    let report = scan.report.duplicates.as_ref().unwrap();
    assert_eq!(report.groups.len(), 1);
    assert_eq!(scan.items.len(), 3);
    assert_eq!(report.extra_bytes, 8);
    let ids: Vec<_> = scan.items.iter().map(|i| i.id.clone()).collect();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    assert!(store.offer(&ids, crate::db::now()).is_err());
    let offer = store.offer(&ids[..2], crate::db::now()).unwrap();
    assert_eq!(offer.kept_files.as_ref().unwrap().len(), 1);
    let plan = store.take(&offer.id, crate::db::now()).unwrap();
    plan.validate_record().unwrap();
    assert_eq!(plan.module, MODULE);
    let job = Job::new(TaskKind::PersonalTrash);
    verify(&plan, &plan.items[0], &job).unwrap();
    let keeper = &plan.duplicates.as_ref().unwrap()[0].keeper;
    std::fs::write(&keeper.path, "changed keeper").unwrap();
    assert!(verify(&plan, &plan.items[0], &job).is_err());
    assert!(validate_keeper(&plan, &plan.items[0]).is_err());
}
#[test]
fn cancellation_retains_files_without_claiming_an_unchecked_group() {
    let (t, _) = fixture();
    let root = t.path().canonicalize().unwrap();
    let job = Arc::new(Job::new(TaskKind::PersonalScan));
    let scan = super::super::scan::collect(&root, &[], job.clone()).unwrap();
    job.cancel();
    let result = collect(scan, job).unwrap();
    assert!(result.report.duplicates.unwrap().groups.is_empty());
    assert!(result.report.cancelled && !result.report.complete);
    assert_eq!(
        std::fs::read_to_string(root.join("one.txt")).unwrap(),
        "same"
    );
}
