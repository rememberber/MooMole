use super::*;
use crate::{
    clean_preview::{Candidate, Preview, PROTOCOL},
    engine::TaskKind,
    provenance::{Origin, Provenance},
};
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

struct Fixture {
    root: tempfile::TempDir,
    home: PathBuf,
    plan: review::Plan,
    db: Arc<Mutex<db::Database>>,
}
impl Fixture {
    fn new() -> Self {
        Self::with_cache_directories(false)
    }
    fn new_cache() -> Self {
        Self::with_cache_directories(true)
    }
    fn with_cache_directories(directory: bool) -> Self {
        let root = tempfile::Builder::new()
            .prefix("MooMole-log-executor-")
            .tempdir()
            .unwrap();
        let home = root.path().canonicalize().unwrap();
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
        let parent = home.join(if directory {
            "Library/Caches"
        } else {
            "Library/Logs"
        });
        std::fs::create_dir_all(&parent).unwrap();
        let items = ["first.log", "second.log", "unselected.log"].iter().map(|name| {
            let path = parent.join(format!("{}-{name}", uuid::Uuid::new_v4()));
            if directory {
                std::fs::create_dir_all(path.join("deep")).unwrap();
                std::fs::write(path.join("deep/payload"), name).unwrap();
            } else { std::fs::write(&path, name).unwrap(); }
            let m = path.metadata().unwrap();
            let path = path.to_str().unwrap().to_string();
            let (stack, guard) = if directory { ("record_dry_run_cleanup_target,_safe_clean_impl,safe_clean_guarded,clean_user_essentials", "_user_cache_deno_delete_guard") } else { ("record_dry_run_cleanup_target,_safe_clean_impl,safe_clean_guarded,_clean_user_log_directory,clean_user_essentials", "_user_log_delete_guard") };
            Candidate { id: String::new(), upstream_identity: format!("inode:{}:{}", m.dev(), m.ino()), path: path.clone(), section: "isolated cleanup fixture".into(), size: Some(name.len() as u64), item_count: 1, is_directory: directory, resolved_path: None, covered_by: None, impact: None,
                provenance: Some(Provenance { version: 1, observations: vec![Origin::from_fields(&path, stack, guard, guard).unwrap()] }) }
        }).collect();
        let at = db::now();
        let scan = review::capture(
            uuid::Uuid::new_v4().to_string(),
            Preview {
                output: String::new(),
                dry_run: true,
                protocol: PROTOCOL,
                revision: crate::engine::REVISION.into(),
                system_included: false,
                items,
            },
            at,
        )
        .unwrap();
        let plan = review::create(
            &scan,
            &scan.items[..2]
                .iter()
                .map(|i| i.candidate.id.clone())
                .collect::<Vec<_>>(),
            &[],
            at,
        )
        .unwrap();
        let db = db::open_owned(&home.join("app.sqlite")).unwrap();
        db::save_settings(
            &db,
            &db::Settings {
                acknowledged: true,
                ..Default::default()
            },
        )
        .unwrap();
        db::save_scan(&db, &scan).unwrap();
        db::save_plan(&db, &plan).unwrap();
        Self {
            root,
            home,
            plan,
            db: Arc::new(Mutex::new(db)),
        }
    }
    fn report(&self) -> cleanup_preparation::Report {
        let now = db::now();
        // Explicit fixture-only eligibility to exercise confirmation/journal.
        // The production executor must independently recheck each actual rule.
        cleanup_preparation::Report {
            version: cleanup_preparation::VERSION,
            plan_id: self.plan.id.clone(),
            scope: if self.plan.items[0].candidate.is_directory {
                "userCachesAndLogs"
            } else {
                "userLogFiles"
            },
            method: "trash",
            read_only: true,
            checked_at: now,
            expires_at: now + 30_000,
            observed_count: self.plan.items.len(),
            authorization_count: 0,
            items: self
                .plan
                .items
                .iter()
                .map(|i| cleanup_preparation::Item {
                    id: i.candidate.id.clone(),
                    code: "observed",
                    reason: "passed-fixture, not real authorization".into(),
                })
                .collect(),
        }
    }
    fn authorize(&self) -> Authorized {
        let mut store = Confirmations::default();
        let offer = store
            .issue(&self.plan, &self.report(), db::now())
            .unwrap()
            .unwrap();
        store.take(&offer.id, db::now()).unwrap()
    }
    fn authorize_required(&self) -> Authorized {
        let mut report = self.report();
        report.authorization_count = report.observed_count;
        report.observed_count = 0;
        for item in &mut report.items {
            item.code = "authorizationRequired";
        }
        let mut store = Confirmations::default();
        let offer = store
            .issue(&self.plan, &report, db::now())
            .unwrap()
            .unwrap();
        assert!(offer.requires_authorization);
        store.take(&offer.id, db::now()).unwrap()
    }
    fn original_files(&self) -> usize {
        std::fs::read_dir(self.home.join("Library/Logs"))
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().file_type().unwrap().is_file())
            .count()
    }
}

#[tokio::test]
async fn conditional_authorization_failure_and_cancellation_have_durable_non_mutating_results() {
    for cancel in [false, true] {
        let f = Fixture::new();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed_calls = calls.clone();
        let result = run_authorized(
            f.db.clone(),
            f.home.clone(),
            f.authorize_required(),
            Arc::new(Job::new(TaskKind::LogTrash)),
            |_, _| async { panic!("failed authorization must not reach rules or Trash") },
            move |_, _, job| async move {
                observed_calls.fetch_add(1, Ordering::SeqCst);
                if cancel {
                    job.cancel();
                }
                Err("nonprivileged authorization failure fixture".into())
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(result.summary.moved_count, 0);
        assert!(result
            .items
            .iter()
            .all(|i| i.state == if cancel { "cancelled" } else { "skipped" }));
        assert!(result.items.iter().all(|i| i.receipt.is_none()));
        assert_eq!(f.original_files(), 3);
    }
}

#[tokio::test]
async fn authorized_observation_cannot_override_new_policy_busy_rules_or_changed_saved_plan() {
    for mode in [
        "busy",
        "checkFailed",
        "protected",
        "sourceChanged",
        "module",
        "cancel",
    ] {
        let f = Fixture::new();
        let db = f.db.clone();
        let saved = f.plan.clone();
        let home = f.home.clone();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed_calls = calls.clone();
        let result = run_authorized(
            f.db.clone(),
            f.home.clone(),
            f.authorize_required(),
            Arc::new(Job::new(TaskKind::LogTrash)),
            move |requested, job| {
                let db = db.clone();
                let home = home.clone();
                let mut saved = saved.clone();
                async move {
                    if mode == "cancel" {
                        job.cancel();
                    }
                    if mode == "protected" {
                        let db = db.lock().unwrap();
                        let mut settings = db::settings(&db).unwrap();
                        settings
                            .protected_paths
                            .push(home.join("Library/Logs").to_str().unwrap().into());
                        db::save_settings(&db, &settings).unwrap();
                    }
                    if mode == "sourceChanged" {
                        saved.items[0]
                            .candidate
                            .section
                            .push_str(" changed during authorization");
                        db.lock()
                            .unwrap()
                            .execute(
                                "UPDATE review_plans SET body=?1 WHERE id=?2",
                                rusqlite::params![serde_json::to_string(&saved).unwrap(), saved.id],
                            )
                            .unwrap();
                    }
                    let mut checks = clear(requested);
                    for rule in checks.values_mut() {
                        rule.activity = rules::ActivityCheck::from_code(
                            if matches!(mode, "busy" | "checkFailed") {
                                mode
                            } else {
                                "unknown"
                            },
                        )
                        .unwrap();
                        if mode == "module" {
                            rule.module.code = "unsupported".into();
                        }
                    }
                    Ok(checks)
                }
            },
            move |plan, home, job| async move {
                observed_calls.fetch_add(1, Ordering::SeqCst);
                selected_logs::fixture_evidence(&plan, &home, job)
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "one observation per batch: {mode}"
        );
        assert_eq!(result.summary.moved_count, 0, "{mode}");
        assert!(result.items.iter().all(|i| i.receipt.is_none()));
        assert_eq!(f.original_files(), 3, "{mode}");
    }
}
fn clear(requested: Vec<(String, String, String)>) -> HashMap<String, rules::RuleCheck> {
    requested
        .into_iter()
        .map(|(id, _, binding)| {
            (
                id,
                rules::RuleCheck {
                    code: "passed".into(),
                    source: "fixture".into(),
                    reason: "fixture only".into(),
                    module: rules::ModuleCheck::from_codes(
                        &binding,
                        if binding == "unsupported" {
                            "unsupported"
                        } else {
                            "checked"
                        },
                        &binding,
                    )
                    .unwrap(),
                    activity: rules::ActivityCheck::from_code("noOpenHandles").unwrap(),
                },
            )
        })
        .collect()
}

#[test]
fn confirmations_bind_subset_source_order_expiry_revocation_and_single_use() {
    let f = Fixture::new();
    let mut store = Confirmations::default();
    let mut report = f.report();
    report.items[1].code = "retained";
    report.observed_count = 1;
    let offer = store.issue(&f.plan, &report, db::now()).unwrap().unwrap();
    assert_eq!(offer.items.len(), 1);
    assert_eq!(offer.retained_count, 1);
    let authorized = store.take(&offer.id, db::now()).unwrap();
    authorized.validate_source(&f.plan, db::now()).unwrap();
    assert!(store.take(&offer.id, db::now()).is_err());
    let mut changed = f.plan.clone();
    changed.items.reverse();
    assert!(authorized.validate_source(&changed, db::now()).is_err());
    assert!(authorized
        .validate_source(&f.plan, offer.expires_at)
        .is_err());
    let old = store.issue(&f.plan, &report, db::now()).unwrap().unwrap();
    let fresh = store.issue(&f.plan, &report, db::now()).unwrap().unwrap();
    assert!(store.take(&old.id, db::now()).is_err());
    store.discard(&fresh.id);
    assert!(store.take(&fresh.id, db::now()).is_err());
    let fresh = store.issue(&f.plan, &report, db::now()).unwrap().unwrap();
    assert!(store.take(&fresh.id, fresh.created_at - 1).is_err());
    assert_eq!(f.original_files(), 3);
    assert!(execution::list(&*f.db.lock().unwrap()).unwrap().is_empty());
}

#[tokio::test]
async fn changed_policy_unknown_rules_and_cancellation_never_move_or_replay() {
    for mode in ["protected", "unknown", "cancel", "changedPlan"] {
        let f = Fixture::new();
        let authorized = f.authorize();
        let job = Arc::new(Job::new(TaskKind::LogTrash));
        if mode == "cancel" {
            job.cancel();
        }
        if mode == "protected" {
            let db = f.db.lock().unwrap();
            let mut settings = db::settings(&db).unwrap();
            settings
                .protected_paths
                .push(f.home.join("Library/Logs").to_str().unwrap().into());
            db::save_settings(&db, &settings).unwrap();
        }
        if mode == "changedPlan" {
            let mut changed = f.plan.clone();
            changed.items.reverse();
            f.db.lock()
                .unwrap()
                .execute(
                    "UPDATE review_plans SET body=?1 WHERE id=?2",
                    rusqlite::params![serde_json::to_string(&changed).unwrap(), changed.id],
                )
                .unwrap();
        }
        let result = run_with(
            f.db.clone(),
            f.home.clone(),
            authorized,
            job,
            move |request, _| async move {
                let mut checks = clear(request);
                if mode == "unknown" {
                    for rule in checks.values_mut() {
                        rule.activity = rules::ActivityCheck::from_code("unknown").unwrap();
                    }
                }
                Ok(checks)
            },
        )
        .await;
        if mode == "changedPlan" {
            assert!(result.is_err());
        } else {
            let result = result.unwrap();
            assert_eq!(result.summary.moved_count, 0);
            assert!(result.items.iter().all(|i| i.receipt.is_none()));
            assert!(run_with(
                f.db.clone(),
                f.home.clone(),
                f.authorize(),
                Arc::new(Job::new(TaskKind::LogTrash)),
                |request, _| async move { Ok(clear(request)) }
            )
            .await
            .is_err());
        }
        assert_eq!(f.original_files(), 3);
    }
}

#[tokio::test]
#[ignore = "Explicit production native executor on OWN isolated log fixtures with fixture rule runner; restores exact receipts"]
async fn selected_native_executor_records_results_and_stops_later_items_on_cancel() {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    for cancel_after_first in [false, true] {
        let mut f = Fixture::new();
        f.root.disable_cleanup(true);
        eprintln!(
            "Production executor fixture root (preserved on failure): {}",
            f.home.display()
        );
        let mut calls = 0;
        let live_db = f.db.clone();
        let result = run_with(
            f.db.clone(),
            f.home.clone(),
            f.authorize(),
            Arc::new(Job::new(TaskKind::LogTrash)),
            move |request, job| {
                calls += 1;
                let live_db = live_db.clone();
                async move {
                    if cancel_after_first && calls == 2 {
                        let runs = execution::list(&*live_db.lock().unwrap()).unwrap();
                        assert!(!job.cancel_execution("stale-batch"));
                        assert!(job.cancel_execution(&runs[0].id));
                        return Err("fixture cancellation".into());
                    }
                    Ok(clear(request))
                }
            },
        )
        .await;
        let records = execution::list(&*f.db.lock().unwrap()).unwrap();
        let details = records
            .iter()
            .map(|r| execution::detail(&*f.db.lock().unwrap(), &r.id).unwrap())
            .collect::<Vec<_>>();
        std::fs::write(
            f.home.join("executor-rescue.json"),
            serde_json::to_vec_pretty(&details).unwrap(),
        )
        .unwrap();
        // Recover by exact persisted receipts BEFORE outcome assertions.
        for detail in &details {
            for item in &detail.items {
                if let Some(receipt) = &item.receipt {
                    let m = std::fs::symlink_metadata(&receipt.destination).unwrap();
                    assert_eq!((m.dev(), m.ino()), (receipt.device, receipt.inode));
                    objc2::rc::autoreleasepool(|_| {
                        let from =
                            NSURL::fileURLWithPath(&NSString::from_str(&receipt.destination));
                        let to = NSURL::fileURLWithPath(&NSString::from_str(&item.path));
                        NSFileManager::defaultManager()
                            .moveItemAtURL_toURL_error(&from, &to)
                            .unwrap();
                    });
                }
            }
        }
        let detail = result.unwrap();
        assert_eq!(
            detail.summary.state,
            if cancel_after_first {
                "cancelled"
            } else {
                "completed"
            }
        );
        assert_eq!(
            detail.summary.moved_count,
            if cancel_after_first { 1 } else { 2 }
        );
        if cancel_after_first {
            assert_eq!(detail.items[1].state, "cancelled");
        }
        assert_eq!(f.original_files(), 3);
        for (item, expected) in f.plan.items.iter().zip(["first.log", "second.log"]) {
            assert_eq!(
                std::fs::read_to_string(&item.candidate.path).unwrap(),
                expected
            );
        }
        f.root.disable_cleanup(false);
    }
}

#[tokio::test]
#[ignore = "Pinned real Mole recheck must honor a NEW whitelist after confirmation; only isolated HOME log fixtures"]
async fn real_whitelist_after_confirmation_prevents_every_native_operation() {
    let mut f = Fixture::new();
    f.root.disable_cleanup(true);
    eprintln!(
        "Real whitelist fixture (preserved on failure): {}",
        f.home.display()
    );
    let authorized = f.authorize();
    std::fs::create_dir_all(f.home.join(".config/mole")).unwrap();
    std::fs::write(
        f.home.join(".config/mole/whitelist"),
        f.plan
            .items
            .iter()
            .map(|i| format!("{}\n", i.candidate.path))
            .collect::<String>(),
    )
    .unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
    let detail = run(
        f.db.clone(),
        root,
        f.home.clone(),
        authorized,
        Arc::new(Job::new(TaskKind::LogTrash)),
    )
    .await
    .unwrap();
    std::fs::write(
        f.home.join("whitelist-result.json"),
        serde_json::to_vec_pretty(&detail).unwrap(),
    )
    .unwrap();
    assert_eq!(detail.summary.state, "noChanges");
    assert!(detail
        .items
        .iter()
        .all(|i| i.state == "skipped" && i.receipt.is_none()));
    assert_eq!(f.original_files(), 3);
    f.root.disable_cleanup(false);
}

#[test]
fn execution_cancellation_binds_once_and_never_affects_a_later_or_unrelated_job() {
    let earlier = Job::new(TaskKind::LogTrash);
    let later = Job::new(TaskKind::LogTrash);
    let read = Job::new(TaskKind::Status);
    assert!(!earlier.cancel_execution("first"));
    earlier.bind_execution("first").unwrap();
    later.bind_execution("second").unwrap();
    assert!(read.bind_execution("first").is_err());
    assert!(earlier.bind_execution("second").is_err());
    assert!(!later.cancel_execution("first"));
    assert!(!read.cancel_execution("first"));
    assert!(!later.cancelled.load(Ordering::SeqCst));
    assert!(earlier.cancel_execution("first"));
    assert!(earlier.cancelled.load(Ordering::SeqCst));
    assert!(!later.cancelled.load(Ordering::SeqCst));
}

mod native_faults;

#[tokio::test]
#[ignore = "Interactive Finder Put Back on ONE fresh log; exact receipt fallback after 180s, never user files"]
async fn finder_put_back_uses_exact_native_receipt_without_rewriting_history() {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let mut f = Fixture::new();
    f.root.disable_cleanup(true);
    eprintln!("FINDER_FIXTURE_ROOT={}", f.home.display());
    let mut report = f.report();
    report.items[1].code = "retained";
    report.observed_count = 1;
    let mut offers = Confirmations::default();
    let offer = offers.issue(&f.plan, &report, db::now()).unwrap().unwrap();
    let authorized = offers.take(&offer.id, db::now()).unwrap();
    let result = run_with(
        f.db.clone(),
        f.home.clone(),
        authorized,
        Arc::new(Job::new(TaskKind::LogTrash)),
        |request, _| async move { Ok(clear(request)) },
    )
    .await
    .unwrap();
    let item = &result.items[0];
    let receipt = item
        .receipt
        .as_ref()
        .expect("fixture must have a real receipt");
    // Save exact rescue evidence before any UI work or assertions.
    std::fs::write(
        f.home.join("finder-receipt.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    let original = serde_json::to_value(&result).unwrap();
    let before = std::fs::symlink_metadata(&receipt.destination).unwrap();
    let location = crate::execution_location::Location::prepare(
        &execution::detail(&*f.db.lock().unwrap(), &result.summary.id).unwrap(),
        &item.id,
    );
    let opened = match location {
        Ok(location) => location.reveal().await,
        Err(error) => Err(error),
    };
    eprintln!(
        "FINDER_FIXTURE_FILE={}",
        Path::new(&receipt.destination)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
    );
    eprintln!("FINDER_FIXTURE_ORIGINAL={}", item.path);
    let deadline = Instant::now() + Duration::from_secs(180);
    while opened.is_ok() && !Path::new(&item.path).exists() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let put_back = Path::new(&item.path).exists() && !Path::new(&receipt.destination).exists();
    std::fs::write(
        f.home.join("finder-observation.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"putBackObserved":put_back,"openAccepted":opened.is_ok()}),
        )
        .unwrap(),
    )
    .unwrap();
    if !put_back {
        // Test-only timeout recovery by the exact API receipt, without overwrites.
        let current = std::fs::symlink_metadata(&receipt.destination).unwrap();
        assert_eq!(
            (current.dev(), current.ino()),
            (receipt.device, receipt.inode)
        );
        assert!(!Path::new(&item.path).exists());
        objc2::rc::autoreleasepool(|_| {
            NSFileManager::defaultManager()
                .moveItemAtURL_toURL_error(
                    &NSURL::fileURLWithPath(&NSString::from_str(&receipt.destination)),
                    &NSURL::fileURLWithPath(&NSString::from_str(&item.path)),
                )
                .unwrap();
        });
    }
    let restored = std::fs::symlink_metadata(&item.path).unwrap();
    assert_eq!(
        (restored.dev(), restored.ino()),
        (before.dev(), before.ino())
    );
    assert_eq!(std::fs::read_to_string(&item.path).unwrap(), "first.log");
    assert_eq!(f.original_files(), 3);
    assert_eq!(
        serde_json::to_value(
            execution::detail(&*f.db.lock().unwrap(), &result.summary.id).unwrap()
        )
        .unwrap(),
        original
    );
    assert!(crate::execution_location::Location::prepare(&result, &item.id).is_err());
    assert!(
        put_back,
        "Finder Put Back not observed; own file was rescued; evidence preserved in {}",
        f.home.display()
    );
    f.root.disable_cleanup(false);
}

mod shutdown;

#[tokio::test]
#[ignore = "Explicit full native cache execution and SQLite history on an owned fixture; restores exact receipt"]
async fn native_cache_executor_persists_results_and_rechecks_new_descendant_protection() {
    for protect_child in [true, false] {
        let mut temp = tempfile::tempdir().unwrap();
        temp.disable_cleanup(true);
        let home = temp.path().canonicalize().unwrap();
        eprintln!(
            "Owned cache journal fixture retained on failure: {}",
            home.display()
        );
        let plan = crate::cleanup_preparation::tests::cache_plan(&home);
        let view = crate::cleanup_preparation::tests::checked_cache(&plan, &home);
        let report = cleanup_preparation::observe(&plan, &view, &home, db::now(), || false);
        let mut confirmations = Confirmations::default();
        let offer = confirmations
            .issue(&plan, &report, db::now())
            .unwrap()
            .unwrap();
        let authorized = confirmations.take(&offer.id, db::now()).unwrap();
        let database = db::open_owned(&home.join("app.sqlite")).unwrap();
        db::save_scan(
            &database,
            &review::Scan {
                id: plan.scan_id.clone(),
                created_at: plan.created_at,
                expires_at: plan.expires_at,
                revision: plan.revision.clone(),
                protocol: plan.protocol,
                system_included: false,
                items: plan.items.clone(),
            },
        )
        .unwrap();
        db::save_plan(&database, &plan).unwrap();
        db::save_settings(
            &database,
            &db::Settings {
                acknowledged: true,
                protected_paths: if protect_child {
                    vec![home
                        .join("Library/Caches/com.apple.helpd/deep/fixture.bin")
                        .to_str()
                        .unwrap()
                        .into()]
                } else {
                    vec![]
                },
                ..Default::default()
            },
        )
        .unwrap();
        let database = Arc::new(Mutex::new(database));
        let result = run_with(
            database.clone(),
            home.clone(),
            authorized,
            Arc::new(Job::new(TaskKind::LogTrash)),
            |requested, _| async { Ok(clear(requested)) },
        )
        .await
        .unwrap();
        std::fs::write(
            home.join("receipt.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        for item in result.items.iter().filter(|item| item.receipt.is_some()) {
            crate::execution_location::Location::prepare(&result, &item.id)
                .unwrap()
                .validate()
                .unwrap();
        }
        for item in &result.items {
            if let Some(receipt) = &item.receipt {
                let m = std::fs::symlink_metadata(&receipt.destination).unwrap();
                assert_eq!((m.dev(), m.ino()), (receipt.device, receipt.inode));
                assert!(!Path::new(&item.path).exists());
                std::fs::rename(&receipt.destination, &item.path).unwrap();
            }
        }
        assert_eq!(result.summary.moved_count, usize::from(!protect_child));
        assert_eq!(
            result.items[0].state,
            if protect_child { "skipped" } else { "moved" }
        );
        let persisted = execution::detail(&*database.lock().unwrap(), &result.summary.id).unwrap();
        assert_eq!(persisted.items[0].state, result.items[0].state);
        assert_eq!(persisted.summary.moved_count, result.summary.moved_count);
        assert_eq!(
            std::fs::read_to_string(home.join("Library/Caches/com.apple.helpd/deep/fixture.bin"))
                .unwrap(),
            "owned rebuildable fixture"
        );
        temp.disable_cleanup(false);
    }
}
