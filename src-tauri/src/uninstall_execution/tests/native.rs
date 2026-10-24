//! Explicit own-fixture integration, never an installed/user App target.
use super::*;
use crate::{app_inspection::identity, tasks, uninstall_plan::Store};
use std::{io::Write, path::Path};
fn persist(path: &Path, value: &serde_json::Value) {
    let mut file = fs::File::create(path).unwrap();
    file.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    file.sync_all().unwrap();
    fs::File::open(path.parent().unwrap())
        .unwrap()
        .sync_all()
        .unwrap();
}

#[tokio::test]
#[ignore = "explicit owned App: real Mole list, fresh native checks, FULL intent and system Trash, exact no-overwrite return; saves rescue evidence"]
async fn owned_app_mole_list_to_production_native_executor_and_exact_return() {
    let mut t = Test::new();
    t.f._temp.disable_cleanup(true);
    fs::create_dir_all(t.f.home.join("Library/LaunchAgents")).unwrap();
    let retained = t.f.home.join("personal-retained");
    fs::write(&retained, "outside retained").unwrap();
    let report_path = t.f.home.join("native-executor-rescue.json");
    let mut report = serde_json::json!({"fixtureRoot":t.f.home,"source":t.f.app,"requestId":t.id,"snapshot":t.f.plan.bundle_binding().1,"state":"prepared","restored":false});
    persist(&report_path, &report);
    eprintln!("Owned native executor fixture: {}", t.f.home.display());
    let list = crate::engine::run(
        &t.f.ctx.engine_root,
        &t.f.home,
        Arc::new(Job::new(TaskKind::Apps)),
        None,
        &[],
    )
    .await
    .unwrap();
    let index = list
        .as_array()
        .unwrap()
        .iter()
        .position(|row| row["path"] == t.f.app.to_str().unwrap())
        .expect("own App must occur in actual Mole list");
    let task_id = uuid::Uuid::new_v4().to_string();
    let mut results = tasks::Results::default();
    results.remember(&tasks::Snapshot {
        id: task_id.clone(),
        kind: "apps".into(),
        state: "succeeded".into(),
        result: Some(list),
        error: None,
        result_unavailable: None,
    });
    let source = results.uninstall_target(&task_id, index).unwrap();
    let plan = Plan::prepare(source, &t.f.ctx).unwrap();
    let expected_snapshot = plan.bundle_binding().1.clone();
    report["snapshot"] = serde_json::to_value(&expected_snapshot).unwrap();
    persist(&report_path, &report);
    let mut store = Store::default();
    let generation = store.begin(plan.bundle_binding().0, None).unwrap();
    store.publish(generation, plan.clone(), db::now()).unwrap();
    // No fixture-positive runtime/support evidence in this explicit path.
    let support = uninstall_support::observe(&plan, &t.f.ctx).unwrap();
    let runtime = uninstall_runtime::observe(&plan, &t.f.ctx).unwrap();
    let mut confirmations = Confirmations::default();
    let generation = confirmations.begin_review(plan.id(), db::now()).unwrap();
    let confirmation = confirmations
        .publish(generation, &plan, &t.f.ctx, &runtime, &support, db::now())
        .unwrap()
        .expect("fresh real prerequisites must permit this owned ordinary App");
    let selection = confirmations
        .take(&confirmation.id, &plan, &t.f.ctx, db::now())
        .unwrap();
    let database = t.db.clone();
    let ctx = t.f.ctx.clone();
    let original = plan.clone();
    let store = Mutex::new(store);
    let results = Mutex::new(results);
    let result = run(
        t.db.clone(),
        t.id.clone(),
        plan,
        selection,
        t.job.clone(),
        move || {
            let settings = db::settings(&*database.lock().unwrap())?;
            if !settings.acknowledged {
                return Err("首次说明未确认".into());
            }
            let latest = store.lock().unwrap().get(original.id(), db::now())?;
            if uninstall_runtime::fingerprint(&latest)?
                != uninstall_runtime::fingerprint(&original)?
            {
                return Err("计划变化".into());
            }
            if !original.source_matches(&results.lock().unwrap().uninstall_target(&task_id, index)?)
            {
                return Err("来源变化".into());
            }
            Ok(Context {
                protected_paths: settings.protected_paths,
                ..ctx.clone()
            })
        },
    )
    .await;
    report["response"] = serde_json::to_value(&result).unwrap();
    if let Ok(detail) = &result {
        report["record"] = serde_json::to_value(detail).unwrap();
    }
    persist(&report_path, &report);
    let detail =
        result.expect("native executor must return a recorded result; rescue report preserved");
    let receipt = detail
        .receipt
        .as_ref()
        .expect("actual exact receipt required; owned fixture/report retained");
    let actual = Path::new(&receipt.actual.destination);
    let id = t.f.plan.archive().bundle_id().to_owned();
    let before = identity::capture_returned(actual, &id).unwrap();
    let root = before.root.nodes.last().unwrap();
    let expected = &expected_snapshot;
    let original_root = expected.root.nodes.last().unwrap();
    assert_eq!((root.dev, root.ino), (original_root.dev, original_root.ino));
    assert_eq!(
        (root.dev, root.ino),
        (receipt.actual.device, receipt.actual.inode)
    );
    assert_eq!(before.inventory, expected.inventory);
    assert_eq!(before.info_sha256, expected.info_sha256);
    assert!(fs::symlink_metadata(&t.f.app).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound));
    // System move refuses an existing destination. Verify exact returned object
    // and original absence before this no-overwrite test-only restoration.
    objc2::rc::autoreleasepool(|_| {
        use objc2_foundation::{NSFileManager, NSString, NSURL};
        NSFileManager::defaultManager()
            .moveItemAtURL_toURL_error(
                &NSURL::fileURLWithPath(&NSString::from_str(&receipt.actual.destination)),
                &NSURL::fileURLWithPath(&NSString::from_str(t.f.app.to_str().unwrap())),
            )
            .unwrap();
    });
    let restored = identity::capture(&t.f.app, &id).unwrap();
    let restored_root = restored.root.nodes.last().unwrap();
    assert_eq!((restored_root.dev, restored_root.ino), (root.dev, root.ino));
    assert_eq!(restored.inventory, expected.inventory);
    assert_eq!(restored.info_sha256, expected.info_sha256);
    assert!(fs::symlink_metadata(actual).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound));
    assert_eq!(fs::read_to_string(&retained).unwrap(), "outside retained");
    report["restored"] = true.into();
    persist(&report_path, &report);
    assert_eq!(detail.summary.item_state, "moved");
    assert_eq!(
        detail
            .events
            .iter()
            .map(|e| e.state.as_str())
            .collect::<Vec<_>>(),
        ["pending", "intent", "moved"]
    );
    eprintln!("Real Mole → immutable selection → fresh native prerequisites → FULL intent → system Trash → moved receipt passed; exact owned App returned, external data retained.");
    // Keep this owned report/database as explicit evidence; no live authority
    // is recovered from them and no user's database has been opened or edited.
}
