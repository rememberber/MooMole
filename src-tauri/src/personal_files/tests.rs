use super::*;
use crate::engine::{Job, TaskKind};
use std::sync::Arc;

fn fixture() -> (tempfile::TempDir, Scan) {
    let t = tempfile::Builder::new()
        .prefix("MooMole-personal-files-")
        .tempdir()
        .unwrap();
    let root = t.path().canonicalize().unwrap();
    std::fs::write(root.join("chosen.pdf"), "owned original").unwrap();
    let scan = scan::collect(&root, &[], Arc::new(Job::new(TaskKind::PersonalScan))).unwrap();
    (t, scan)
}
#[test]
fn independent_confirmation_binds_exact_scan_ids_and_expires_without_replay() {
    let (_t, scan) = fixture();
    let now = crate::db::now();
    let ids = vec![scan.items[0].id.clone()];
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    assert!(store.offer(&["foreign".into()], now).is_err());
    assert!(store.offer(&[ids[0].clone(), ids[0].clone()], now).is_err());
    let offer = store.offer(&ids, now).unwrap();
    let plan = store.take(&offer.id, now).unwrap();
    assert_eq!(plan.module, MODULE);
    assert_eq!(plan.items[0].id, ids[0]);
    assert!(store.take(&offer.id, now).is_err());
    let offer = store.offer(&ids, now).unwrap();
    assert!(store.take(&offer.id, offer.expires_at).is_err());
    assert!(store.take(&offer.id, now).is_err());
}
#[test]
fn scanner_avoids_links_shared_files_secrets_protected_paths_and_packages() {
    let (t, _) = fixture();
    let root = t.path().canonicalize().unwrap();
    std::fs::write(root.join(".env"), "secret").unwrap();
    std::fs::write(root.join("private.pem"), "secret").unwrap();
    std::fs::create_dir(root.join("Something.app")).unwrap();
    std::fs::write(root.join("Something.app/payload"), "keep").unwrap();
    std::fs::write(root.join("shared"), "shared").unwrap();
    std::fs::hard_link(root.join("shared"), root.join("shared-again")).unwrap();
    std::os::unix::fs::symlink(root.join("chosen.pdf"), root.join("alias")).unwrap();
    let scan = scan::collect(&root, &[], Arc::new(Job::new(TaskKind::PersonalScan))).unwrap();
    assert_eq!(scan.items.len(), 1);
    assert!(scan.report.skipped >= 5);
    let rules = vec![root.join("chosen.pdf").to_str().unwrap().into()];
    let scan = scan::collect(&root, &rules, Arc::new(Job::new(TaskKind::PersonalScan))).unwrap();
    assert!(scan.items.is_empty());
    let job = Arc::new(Job::new(TaskKind::PersonalScan));
    job.cancel();
    let scan = scan::collect(&root, &[], job).unwrap();
    assert!(scan.report.cancelled && !scan.report.complete);
}
