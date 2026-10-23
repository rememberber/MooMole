use super::*;
use crate::{
    clean_preview::{Candidate, Preview, PROTOCOL},
    engine::{Job, TaskKind},
    provenance::{Origin, Provenance},
};
use std::{os::unix::fs::MetadataExt, sync::Arc};

fn plan(home: &Path, count: usize) -> review::Plan {
    let root = home.join("Library/Logs");
    std::fs::create_dir_all(&root).unwrap();
    let items = (0..count).map(|index| {
        let path = root.join(format!("fixture-{index}.log"));
        std::fs::write(&path, "isolated log retained").unwrap();
        let m = path.metadata().unwrap();
        let path = path.to_str().unwrap().to_string();
        Candidate { id: String::new(), upstream_identity: format!("inode:{}:{}", m.dev(), m.ino()),
            path: path.clone(), section: "fixture logs".into(), size: Some(m.len()), item_count: 1,
            is_directory: false, resolved_path: None, covered_by: None, impact: None,
            provenance: Some(Provenance { version: 1, observations: vec![Origin::from_fields(&path,
                "record_dry_run_cleanup_target,_safe_clean_impl,safe_clean_guarded,_clean_user_log_directory,clean_user_essentials",
                "_user_log_delete_guard", "_user_log_delete_guard").unwrap()] }),
        }
    }).collect();
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
        crate::db::now(),
    )
    .unwrap();
    review::create(
        &scan,
        &scan
            .items
            .iter()
            .map(|i| i.candidate.id.clone())
            .collect::<Vec<_>>(),
        &[],
        crate::db::now(),
    )
    .unwrap()
}

fn checked(plan: &review::Plan, now: i64) -> review::PlanView {
    let mut view = review::check(plan, &[], now);
    view.rule_checked_at = Some(now);
    for item in &mut view.items {
        item.mole_rule = Some(rules::RuleCheck {
            code: "passed".into(),
            source: "fixture".into(),
            reason: "fixture".into(),
            module: rules::ModuleCheck::from_codes("userLogs", "checked", "userLogs").unwrap(),
            activity: rules::ActivityCheck::from_code("noOpenHandles").unwrap(),
        });
    }
    view
}

#[test]
fn only_current_unknown_occupancy_can_offer_conditional_authorization() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let plan = plan(&home, 1);
    let now = crate::db::now();
    for mode in [
        "unknown",
        "busy",
        "budgetExhausted",
        "checkFailed",
        "protected",
        "module",
        "cancel",
    ] {
        let mut view = checked(&plan, now);
        view.items[0].mole_rule.as_mut().unwrap().activity =
            rules::ActivityCheck::from_code(if matches!(mode, "protected" | "module" | "cancel") {
                "unknown"
            } else {
                mode
            })
            .unwrap();
        let checks = [(
            view.items[0].id.clone(),
            view.items[0].mole_rule.clone().unwrap(),
        )]
        .into_iter()
        .collect();
        view = review::apply_rule_checks(review::check(&plan, &[], now), &checks, now);
        if mode == "protected" {
            view.items[0].state = "protected".into();
        }
        if mode == "module" {
            view.items[0].mole_rule.as_mut().unwrap().module.code = "unsupported".into();
        }
        let mut report = observe(&plan, &view, &home, now, || mode == "cancel");
        assert_eq!(
            report.observed_count, 0,
            "unknown is never an observed pass"
        );
        assert_eq!(
            report.authorization_count,
            usize::from(mode == "unknown"),
            "{mode}"
        );
        if mode == "unknown" {
            assert_eq!(report.items[0].code, "authorizationRequired");
            let mut store = crate::log_cleanup::Confirmations::default();
            let offer = store.issue(&plan, &report, now).unwrap().unwrap();
            assert!(offer.requires_authorization);
            report.withdraw("fixture withdrawal");
            assert_eq!(report.authorization_count, 0);
            assert!(store.issue(&plan, &report, now).unwrap().is_none());
            assert!(store.take(&offer.id, now).is_err());
        }
    }
}

#[test]
fn preparation_joins_rules_and_native_identity_without_modifying_logs_or_plan() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let plan = plan(&home, 2);
    let saved = serde_json::to_vec(&plan).unwrap();
    let now = crate::db::now();
    let report = observe(&plan, &checked(&plan, now), &home, now, || false);
    assert_eq!(report.observed_count, 2);
    assert!(report.read_only);
    assert_eq!(report.expires_at, now + LIFETIME_MS);
    assert_eq!(saved, serde_json::to_vec(&plan).unwrap());
    for item in &plan.items {
        assert_eq!(
            std::fs::read_to_string(&item.candidate.path).unwrap(),
            "isolated log retained"
        );
    }
}

#[test]
fn missing_busy_old_or_mismatched_evidence_never_reaches_native_preparation() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let original = plan(&home, 1);
    let now = crate::db::now();
    for change in 0..10 {
        let mut plan = original.clone();
        let mut view = checked(&plan, now);
        match change {
            0 => view.rule_checked_at = None,
            1 => view.items[0].mole_rule = None,
            2 => {
                view.items[0].mole_rule.as_mut().unwrap().activity =
                    rules::ActivityCheck::from_code("unknown").unwrap()
            }
            3 => view.items[0].mole_rule.as_mut().unwrap().module.code = "retainOrUnknown".into(),
            4 => view.items[0].state = "protected".into(),
            5 => view.items[0].permissions.code = permissions::Code::Unknown,
            6 => plan.items[0].candidate.provenance = None,
            7 => view.items[0].path.push_str(".other"),
            8 => view.items[0].mole_rule.as_mut().unwrap().module.version += 1,
            _ => view.summary.id = uuid::Uuid::new_v4().to_string(),
        }
        let report = observe_with(
            &plan,
            &view,
            &home,
            now,
            || false,
            |_| panic!("unverified item reached native preparation"),
        );
        assert_eq!(report.observed_count, 0, "case {change}");
    }
    let view = checked(&original, now);
    let report = observe_with(
        &original,
        &view,
        &home,
        now + LIFETIME_MS,
        || false,
        |_| panic!("stale rules reached native preparation"),
    );
    assert_eq!(report.observed_count, 0);
    let report = observe_with(
        &original,
        &view,
        &home,
        now + LIFETIME_MS - 1,
        || false,
        |_| Ok(()),
    );
    assert_eq!(
        report.expires_at,
        now + LIFETIME_MS,
        "native preparation must not extend old rule evidence"
    );
}

#[test]
fn native_budget_failure_cancel_and_withdrawal_never_publish_a_blanket_pass() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let plan = plan(&home, 22);
    let now = crate::db::now();
    let view = checked(&plan, now);
    let mut calls = 0;
    let mut report = observe_with(
        &plan,
        &view,
        &home,
        now,
        || false,
        |_| {
            calls += 1;
            Ok(())
        },
    );
    assert_eq!(calls, 20);
    assert_eq!(report.observed_count, 20);
    assert_eq!(report.items[20].code, "notChecked");
    report.withdraw("fixture protection changed");
    assert_eq!(report.observed_count, 0);
    assert!(report.items.iter().all(|i| i.code == "notChecked"));
    let report = observe_with(&plan, &view, &home, now, || true, |_| panic!("cancelled"));
    assert_eq!(report.observed_count, 0);
    let report = observe_with(
        &plan,
        &view,
        &home,
        now,
        || false,
        |_| Err("fixture identity changed".into()),
    );
    assert_eq!(report.observed_count, 0);
    assert_eq!(report.items[0].code, "retained");
}

#[tokio::test]
#[ignore = "Pinned Mole real log ledger + rules in a private HOME; read-only, no Trash or personal data"]
async fn pinned_log_ledger_rules_and_native_preparation_remain_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    std::fs::create_dir_all(home.join("Library/Logs")).unwrap();
    let log = home.join("Library/Logs/isolated-user.log");
    std::fs::write(&log, "real upstream functions, retained fixture").unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
    let report_path = home.join("preview.nul");
    std::fs::write(&report_path, "").unwrap();
    // Run ONLY the upstream user-essentials module under this fresh HOME. The
    // real append hook supplies call-chain provenance; no function is replaced.
    let script = r#"source "$1/bin/clean.sh"; source "$1/moomole-clean-preview.sh";
CLEAN_PREVIEW_FINAL_FILE="$2.txt"; prepare_clean_preview_file || exit 1
export MOOMOLE_ORIGINS="$2.origins" MOOMOLE_ORIGIN_FAILURE="$2.failed"
: > "$MOOMOLE_ORIGINS"; moomole_install_origin_hook || exit 1
clean_user_essentials > /dev/null || exit 1
SYSTEM_CLEAN=false; moomole_write_clean_report "$2"
"#;
    let mut command = crate::engine::command(Path::new("/bin/bash"));
    command
        .args(["-c", script, "moomole-log-fixture"])
        .arg(&root)
        .arg(&report_path)
        .env("HOME", &home)
        .env_remove("SUDO_USER")
        .env("DENO_DIR", "")
        .env("MOLE_DRY_RUN", "1")
        .env("MOLE_TEST_NO_AUTH", "1")
        .env("MO_NO_OPLOG", "1")
        .env("MOLE_SKIP_TRASH_CLEANUP", "1")
        .env("TMPDIR", &home);
    crate::engine::output(
        command,
        Arc::new(Job::new(TaskKind::Review)),
        Duration::from_secs(30),
    )
    .await
    .unwrap();
    let preview = crate::clean_preview::parse(&std::fs::read(&report_path).unwrap()).unwrap();
    let scan =
        review::capture(uuid::Uuid::new_v4().to_string(), preview, crate::db::now()).unwrap();
    let item = scan
        .items
        .iter()
        .find(|item| Path::new(&item.candidate.path) == log)
        .expect("real log ledger candidate");
    assert_eq!(
        provenance::binding(item.candidate.provenance.as_ref(), &item.candidate.path),
        "userLogs"
    );
    let plan = review::create(&scan, &[item.candidate.id.clone()], &[], crate::db::now()).unwrap();
    let requested = vec![(
        item.candidate.id.clone(),
        item.candidate.path.clone(),
        "userLogs".into(),
    )];
    let checks = rules::run(
        &root,
        &home,
        &requested,
        Arc::new(Job::new(TaskKind::Review)),
    )
    .await
    .unwrap();
    let now = crate::db::now();
    let view = review::apply_rule_checks(review::check(&plan, &[], now), &checks, now);
    let report = observe(&plan, &view, &home, now, || false);
    let rule = &checks[&item.candidate.id];
    eprintln!(
        "Real log review: path={}, module={}, activity={}, preparation={}, observed={}, authorizationRequired={}",
        rule.code, rule.module.code, rule.activity.code, report.items[0].code, report.observed_count, report.authorization_count
    );
    assert_eq!(rule.module.binding, "userLogs");
    assert_eq!(
        report.observed_count == 1,
        view.items[0].state == "unchanged"
            && view.items[0].permissions.code == permissions::Code::NoRestrictionObserved
    );
    assert_eq!(
        report.authorization_count == 1,
        view.items[0].state == "activityUnknown"
            && rule.activity.code == "unknown"
            && view.items[0].permissions.code == permissions::Code::NoRestrictionObserved
    );
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "real upstream functions, retained fixture"
    );
    // Real persisted policy must also reach the same preparation gate.
    std::fs::create_dir_all(home.join(".config/mole")).unwrap();
    std::fs::write(
        home.join(".config/mole/whitelist"),
        format!("{}\n", log.display()),
    )
    .unwrap();
    let checks = rules::run(
        &root,
        &home,
        &requested,
        Arc::new(Job::new(TaskKind::Review)),
    )
    .await
    .unwrap();
    assert_eq!(checks[&item.candidate.id].code, "whitelist");
    let now = crate::db::now();
    let view = review::apply_rule_checks(review::check(&plan, &[], now), &checks, now);
    let report = observe(&plan, &view, &home, now, || false);
    assert_eq!(report.observed_count + report.authorization_count, 0);
    assert!(log.is_file());
}
