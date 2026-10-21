use super::*;
use std::{fs, os::unix::fs::symlink};

#[test]
#[ignore = "Explicit read-only observation of the current user's six preferences and optimize rules"]
fn live_read_only_preferences() {
    let home = std::env::var_os("HOME").unwrap();
    let report = inspect(Path::new(&home)).unwrap();
    assert_eq!(report.items.len(), 2);
    assert_eq!(
        report
            .items
            .iter()
            .map(|item| item.findings.len())
            .sum::<usize>(),
        6
    );
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
#[test]
fn rules_prefer_current_even_empty_and_never_migrate_legacy() {
    let home = tempfile::tempdir_in("/private/tmp").unwrap();
    let directory = home.path().join(".config/mole");
    fs::create_dir_all(&directory).unwrap();
    assert_eq!(rules(home.path()), ("defaults", Some(vec![])));
    fs::write(
        directory.join("whitelist_checks"),
        " # note\n legacy_overrides_audit \n",
    )
    .unwrap();
    assert_eq!(
        rules(home.path()),
        ("legacy", Some(vec!["legacy_overrides_audit".into()]))
    );
    assert!(!directory.join("whitelist_optimize").exists());
    fs::write(directory.join("whitelist_optimize"), "").unwrap();
    assert_eq!(rules(home.path()), ("current", Some(vec![])));
    fs::write(
        directory.join("whitelist_optimize"),
        "*\nprevent_network_dsstore\n",
    )
    .unwrap();
    let (_, patterns) = rules(home.path());
    assert_eq!(
        item(IDS[0], patterns.as_deref(), vec![]).task_protection,
        "matched"
    );
    assert_eq!(
        item(IDS[1], patterns.as_deref(), vec![]).task_protection,
        "notMatched"
    );
}
#[test]
fn uncertain_rule_files_do_not_fall_back_or_infer_empty() {
    let home = tempfile::tempdir_in("/private/tmp").unwrap();
    let directory = home.path().join(".config/mole");
    fs::create_dir_all(&directory).unwrap();
    let primary = directory.join("whitelist_optimize");
    for bytes in [
        vec![255],
        vec![0],
        b"legacy_overrides_audit".to_vec(),
        vec![b'x'; MAX_RULES as usize + 1],
        "x\n".repeat(257).into_bytes(),
    ] {
        fs::write(&primary, bytes).unwrap();
        assert_eq!(rules(home.path()), ("unknown", None));
    }
    fs::remove_file(&primary).unwrap();
    symlink(directory.join("missing"), &primary).unwrap();
    assert_eq!(rules(home.path()), ("unknown", None));
    fs::remove_file(&primary).unwrap();
    fs::create_dir(&primary).unwrap();
    assert_eq!(rules(home.path()), ("unknown", None));
}
#[test]
fn ancestor_links_and_path_patterns_cannot_clear_task_or_path_protection() {
    let home = tempfile::tempdir_in("/private/tmp").unwrap();
    let other = tempfile::tempdir_in("/private/tmp").unwrap();
    symlink(other.path(), home.path().join(".config")).unwrap();
    assert_eq!(rules(home.path()), ("unknown", None));
    let patterns = vec!["legacy_*".into(), "~/Library/Preferences/*".into()];
    assert_eq!(
        item(IDS[1], Some(&patterns), vec![]).task_protection,
        "notMatched"
    );
}
#[test]
fn managed_or_opted_out_items_are_protected_and_missing_is_unknown() {
    let finding = |value, managed| Finding {
        domain: "-g",
        key: "NSAppSleepDisabled",
        label: "App Nap",
        value,
        managed,
    };
    assert_eq!(
        item(IDS[1], Some(&[]), vec![finding(Value::Unknown, false)]).code,
        "unknown"
    );
    assert_eq!(
        item(IDS[1], None, vec![finding(Value::Disabled, false)]).code,
        "unknown"
    );
    assert_eq!(
        item(IDS[1], Some(&[]), vec![finding(Value::Disabled, true)]).code,
        "protected"
    );
    assert_eq!(
        item(
            IDS[1],
            Some(&[IDS[1].into()]),
            vec![finding(Value::Unknown, false)]
        )
        .code,
        "protected"
    );
    assert_eq!(
        item(IDS[1], Some(&[]), vec![finding(Value::Enabled, false)]).code,
        "observed"
    );
}
