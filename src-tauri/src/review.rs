//! Persisted read-only review plans. Metadata observations are deliberately not
//! an authorization to delete; execution still needs Mole rules and secure I/O.
use crate::{
    clean_preview::{Candidate, Preview, PROTOCOL},
    observation::{witness, Witness},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

pub const LIFETIME_MS: i64 = 15 * 60 * 1000;
pub const MAX_SELECTION: usize = 500;

#[derive(Clone, Deserialize, Serialize)]
pub struct ScanItem {
    pub candidate: Candidate,
    pub(crate) witness: Option<Witness>,
    unavailable: Option<String>,
    #[serde(default)]
    pub(crate) tree: Option<crate::tree::TreeWitness>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Scan {
    pub id: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub revision: String,
    pub protocol: u64,
    pub system_included: bool,
    pub items: Vec<ScanItem>,
}

pub fn capture(id: String, preview: Preview, now: i64) -> Result<Scan, String> {
    if preview.revision != crate::engine::REVISION
        || preview.protocol != PROTOCOL
        || !preview.dry_run
    {
        return Err("扫描版本与当前应用不匹配。".into());
    }
    let items = preview
        .items
        .into_iter()
        .enumerate()
        .map(|(index, mut candidate)| {
            candidate.id = format!("{id}:{index}");
            let (witness, unavailable) = match witness(Path::new(&candidate.path)) {
                Ok(value) => {
                    let target = value
                        .nodes
                        .last()
                        .expect("a non-root absolute path has a target");
                    if candidate.upstream_identity == format!("inode:{}:{}", target.dev, target.ino)
                    {
                        (Some(value), None)
                    } else {
                        (
                            None,
                            Some("Mole 记录的身份无法与当前目标核对，需要重新扫描。".into()),
                        )
                    }
                }
                Err(reason) => (None, Some(reason)),
            };
            ScanItem {
                candidate,
                witness,
                unavailable,
                tree: None,
            }
        })
        .collect();
    Ok(Scan {
        id,
        created_at: now,
        expires_at: now + LIFETIME_MS,
        revision: preview.revision,
        protocol: preview.protocol,
        system_included: preview.system_included,
        items,
    })
}

impl Scan {
    pub fn preview(&self) -> serde_json::Value {
        serde_json::json!({"scanId": self.id, "scannedAt": self.created_at, "expiresAt": self.expires_at,
            "protocol": self.protocol, "revision": self.revision, "dryRun": true,
            "systemIncluded": self.system_included,
            "output": "此结果来自本地保存的扫描，原始日志未保存。请留意扫描时间，操作前需重新复核。",
            "items": self.items.iter().map(|item| &item.candidate).collect::<Vec<_>>()})
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Plan {
    pub id: String,
    pub scan_id: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub revision: String,
    pub protocol: u64,
    pub items: Vec<ScanItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub id: String,
    pub scan_id: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub item_count: usize,
}

impl Plan {
    pub fn summary(&self) -> PlanSummary {
        PlanSummary {
            id: self.id.clone(),
            scan_id: self.scan_id.clone(),
            created_at: self.created_at,
            expires_at: self.expires_at,
            item_count: self.items.len(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemCheck {
    pub id: String,
    pub path: String,
    pub size: Option<u64>,
    pub section: String,
    pub state: String,
    pub reason: String,
    pub mole_rule: Option<crate::rules::RuleCheck>,
    pub descendant_rules: Option<crate::descendant_rules::Check>,
    pub provenance: Option<crate::provenance::Provenance>,
    pub inventory_entries: Option<usize>,
    pub impact: Option<crate::impact::Impact>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    #[serde(flatten)]
    pub summary: PlanSummary,
    pub checked_at: i64,
    pub items: Vec<ItemCheck>,
    pub read_only: bool,
    pub requires_mole_recheck: bool,
    pub rule_checked_at: Option<i64>,
}

fn protected(item: &Candidate, rules: &[String]) -> bool {
    rules.iter().any(|rule| {
        [
            &item.path,
            item.resolved_path.as_ref().unwrap_or(&item.path),
        ]
        .iter()
        .any(|path| Path::new(path).starts_with(rule) || Path::new(rule).starts_with(path))
    })
}

pub fn check(plan: &Plan, rules: &[String], now: i64) -> PlanView {
    check_internal(plan, rules, now, true)
}

fn check_internal(plan: &Plan, rules: &[String], now: i64, verify_tree: bool) -> PlanView {
    let mut budget = crate::tree::Budget::new();
    let items = plan
        .items
        .iter()
        .map(|item| {
            let (mut state, mut reason) =
                if plan.revision != crate::engine::REVISION || plan.protocol != PROTOCOL {
                    (
                        "versionMismatch",
                        "引擎或协议版本已变化，需要重新扫描。".into(),
                    )
                } else if now >= plan.expires_at || now < plan.created_at {
                    ("expired", "计划已过期或系统时间回退，需要重新扫描。".into())
                } else if protected(&item.candidate, rules) {
                    ("protected", "与当前客户端保护目录重叠，已排除。".into())
                } else if item
                    .witness
                    .as_ref()
                    .is_some_and(|w| w.version != crate::observation::VERSION)
                {
                    (
                        "versionMismatch",
                        "这份扫描使用旧版路径观察，请重新扫描。".into(),
                    )
                } else if let Some(original) = &item.witness {
                    match witness(Path::new(&item.candidate.path)) {
                        Ok(current) if &current == original => (
                            "unchanged",
                            "本次本地元数据复核未发现变化；仍需复查 Mole 规则和操作影响。".into(),
                        ),
                        Ok(_) => (
                            "changed",
                            "文件或祖先目录的身份、属性已变化，需要重新扫描。".into(),
                        ),
                        Err(reason) => ("unavailable", reason),
                    }
                } else {
                    (
                        "unavailable",
                        item.unavailable
                            .clone()
                            .unwrap_or("无法确认扫描时的文件身份。".into()),
                    )
                };
            if state == "unchanged"
                && verify_tree
                && item.witness.as_ref().is_some_and(|w| {
                    w.nodes
                        .last()
                        .is_some_and(|n| n.mode & libc::S_IFMT as u32 == libc::S_IFDIR as u32)
                })
            {
                if let Some(original) = item
                    .tree
                    .as_ref()
                    .filter(|t| t.version == crate::tree::VERSION)
                {
                    let target = item.witness.as_ref().unwrap().nodes.last().unwrap();
                    match crate::tree::inspect(
                        Path::new(&item.candidate.path),
                        target.dev,
                        target.ino,
                        &mut budget,
                    ) {
                        Ok(current) if &current == original => (),
                        Ok(_) => {
                            state = "contentsChanged";
                            reason = "目录深层子项已变化，请重新生成审阅计划。".into();
                        }
                        Err(error) => {
                            state = "inventoryUnavailable";
                            reason = error;
                        }
                    }
                } else {
                    state = "inventoryMissing";
                    reason = "缺少当前版本的目录内容记录，请重新生成审阅计划。".into();
                }
            }
            ItemCheck {
                id: item.candidate.id.clone(),
                path: item.candidate.path.clone(),
                size: item.candidate.size,
                section: item.candidate.section.clone(),
                state: state.into(),
                reason,
                mole_rule: None,
                descendant_rules: None,
                provenance: item.candidate.provenance.clone(),
                inventory_entries: item.tree.as_ref().map(|t| t.entries),
                impact: item.candidate.impact.clone(),
            }
        })
        .collect();
    PlanView {
        summary: plan.summary(),
        checked_at: now,
        items,
        read_only: true,
        requires_mole_recheck: true,
        rule_checked_at: None,
    }
}

pub fn create(scan: &Scan, ids: &[String], rules: &[String], now: i64) -> Result<Plan, String> {
    if now < scan.created_at || now >= scan.expires_at {
        return Err("扫描已过期或系统时间回退，请重新扫描。".into());
    }
    if ids.is_empty() || ids.len() > MAX_SELECTION {
        return Err("请选择 1 至 500 个项目。".into());
    }
    let unique: HashSet<&String> = ids.iter().collect();
    if unique.len() != ids.len() {
        return Err("选中项目重复，请重新审阅。".into());
    }
    let by_id: HashMap<&String, &ScanItem> = scan
        .items
        .iter()
        .map(|item| (&item.candidate.id, item))
        .collect();
    let items = ids
        .iter()
        .map(|id| {
            by_id
                .get(id)
                .map(|item| (*item).clone())
                .ok_or("项目不属于这份扫描，请重新扫描。".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut plan = Plan {
        id: uuid::Uuid::new_v4().to_string(),
        scan_id: scan.id.clone(),
        created_at: now,
        expires_at: scan.expires_at,
        revision: scan.revision.clone(),
        protocol: scan.protocol,
        items,
    };
    let view = check_internal(&plan, rules, now, false);
    let invalid: Vec<_> = view
        .items
        .iter()
        .filter(|item| item.state != "unchanged")
        .collect();
    if let Some(first) = invalid.first() {
        return Err(format!(
            "有 {} 项无法保存为计划：{} {}",
            invalid.len(),
            first.path,
            first.reason
        ));
    }
    let mut budget = crate::tree::Budget::new();
    for item in &mut plan.items {
        let target = item
            .witness
            .as_ref()
            .and_then(|w| w.nodes.last())
            .ok_or("无法确认目标身份。")?;
        if target.mode & libc::S_IFMT as u32 == libc::S_IFDIR as u32 {
            item.tree = Some(
                crate::tree::inspect(
                    Path::new(&item.candidate.path),
                    target.dev,
                    target.ino,
                    &mut budget,
                )
                .map_err(|e| format!("{}：{e}", item.candidate.path))?,
            );
        }
    }
    if let Some(item) = check(&plan, rules, now)
        .items
        .iter()
        .find(|item| item.state != "unchanged")
    {
        return Err(format!(
            "保存期间目标发生变化：{} {}",
            item.path, item.reason
        ));
    }
    Ok(plan)
}

pub fn apply_rule_checks(
    mut view: PlanView,
    checks: &HashMap<String, crate::rules::RuleCheck>,
    now: i64,
) -> PlanView {
    view.rule_checked_at = Some(now);
    for item in &mut view.items {
        if let Some(check) = checks.get(&item.id) {
            // A passing probe never overrides a changed/expired/protected local target.
            if item.state == "unchanged" && check.code != "passed" {
                item.state = if ["coreProtection", "whitelist", "compiledModel"]
                    .contains(&check.code.as_str())
                {
                    "moleProtected".into()
                } else if ["activeOrUncertain", "sqliteInUse"].contains(&check.code.as_str()) {
                    "active".into()
                } else {
                    "ruleUnavailable".into()
                };
                item.reason = check.reason.clone();
            }
            if item.state == "unchanged" && check.code == "passed" {
                match check.module.code.as_str() {
                    "checked" => {}
                    "retainOrUnknown" | "preserveRoot" => {
                        item.state = "moduleRetained".into();
                        item.reason = check.module.reason.clone();
                    }
                    "unsupported" => {
                        item.state = "moduleUnsupported".into();
                        item.reason = check.module.reason.clone();
                    }
                    _ => {
                        item.state = "ruleUnavailable".into();
                        item.reason = check.module.reason.clone();
                    }
                }
            }
            if item.state == "unchanged" {
                match check.activity.code.as_str() {
                    "noOpenHandles" => (),
                    "busy" => {
                        item.state = "activityBusy".into();
                        item.reason = check.activity.reason.clone();
                    }
                    "unknown" | "budgetExhausted" => {
                        item.state = "activityUnknown".into();
                        item.reason = check.activity.reason.clone();
                    }
                    _ => {
                        item.state = "activityUnavailable".into();
                        item.reason = check.activity.reason.clone();
                    }
                }
            }
            item.mole_rule = Some(check.clone());
        }
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    #[test]
    fn legacy_path_observations_stay_readable_but_require_rescanning() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().canonicalize().unwrap().join("file");
        std::fs::write(&path, "fixture").unwrap();
        let scan = scan(&path);
        let plan = create(&scan, &["scan-1:0".into()], &[], 1001).unwrap();
        let mut json = serde_json::to_value(&plan).unwrap();
        json["items"][0]["witness"]
            .as_object_mut()
            .unwrap()
            .remove("version");
        json["items"][0]["witness"]
            .as_object_mut()
            .unwrap()
            .remove("links");
        let old: Plan = serde_json::from_value(json).unwrap();
        assert_eq!(check(&old, &[], 1002).items[0].state, "versionMismatch");
        let mut old_scan = scan.clone();
        old_scan.items = old.items;
        assert!(create(&old_scan, &["scan-1:0".into()], &[], 1002)
            .err()
            .unwrap()
            .contains("旧版路径观察"));
    }
    #[test]
    fn directory_plans_freeze_deep_contents_and_reject_legacy_unfrozen_plans() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().canonicalize().unwrap();
        std::fs::create_dir(path.join("nested")).unwrap();
        std::fs::write(path.join("nested/file"), "old").unwrap();
        let mut plan = create(&scan(&path), &["scan-1:0".into()], &[], 1001).unwrap();
        assert_eq!(check(&plan, &[], 1002).items[0].inventory_entries, Some(2));
        assert_eq!(check(&plan, &[], 1002).items[0].state, "unchanged");
        let encoded = serde_json::to_string(&plan).unwrap();
        let restored: Plan = serde_json::from_str(&encoded).unwrap();
        assert_eq!(check(&restored, &[], 1002).items[0].state, "unchanged");
        let mut legacy = restored.clone();
        legacy.items[0].tree.as_mut().unwrap().version = 1;
        assert_eq!(check(&legacy, &[], 1002).items[0].state, "inventoryMissing");
        std::fs::write(path.join("nested/file"), "new content").unwrap();
        assert_eq!(
            check(&restored, &[], 1002).items[0].state,
            "contentsChanged"
        );
        plan.items[0].tree = None;
        assert_eq!(check(&plan, &[], 1002).items[0].state, "inventoryMissing");
    }
    #[test]
    fn passing_mole_rules_do_not_override_new_client_protection_or_expiry() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().canonicalize().unwrap().join("file");
        std::fs::write(&path, "fixture").unwrap();
        let plan = create(&scan(&path), &["scan-1:0".into()], &[], 1001).unwrap();
        let mut legacy = plan.clone();
        legacy.protocol = 1;
        assert_eq!(check(&legacy, &[], 1002).items[0].state, "versionMismatch");
        let checks = HashMap::from([(
            "scan-1:0".into(),
            crate::rules::RuleCheck {
                code: "passed".into(),
                source: "fixture".into(),
                reason: "fixture".into(),
                activity: crate::rules::ActivityCheck::from_code("noOpenHandles").unwrap(),
                module: crate::rules::ModuleCheck::from_codes("userCache", "checked", "userCache")
                    .unwrap(),
            },
        )]);
        assert_eq!(
            apply_rule_checks(
                check(&plan, &[path.to_str().unwrap().into()], 1002),
                &checks,
                1002
            )
            .items[0]
                .state,
            "protected"
        );
        assert_eq!(
            apply_rule_checks(check(&plan, &[], plan.expires_at), &checks, plan.expires_at).items
                [0]
            .state,
            "expired"
        );
    }
    #[test]
    fn module_observations_do_not_turn_partial_coverage_into_execution_eligibility() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().canonicalize().unwrap().join("file");
        std::fs::write(&path, "fixture").unwrap();
        let plan = create(&scan(&path), &["scan-1:0".into()], &[], 1001).unwrap();
        for (scope, code, activity, state) in [
            ("userCache", "checked", "noOpenHandles", "unchanged"),
            ("userLogs", "checked", "busy", "activityBusy"),
            ("userLogs", "checked", "unknown", "activityUnknown"),
            ("userCache", "checked", "budgetExhausted", "activityUnknown"),
            ("userCache", "checked", "checkFailed", "activityUnavailable"),
            (
                "userCache",
                "retainOrUnknown",
                "notChecked",
                "moduleRetained",
            ),
            ("userLogs", "preserveRoot", "notChecked", "moduleRetained"),
            ("userLogs", "checkFailed", "notChecked", "ruleUnavailable"),
            (
                "unsupported",
                "unsupported",
                "notChecked",
                "moduleUnsupported",
            ),
        ] {
            let checks = HashMap::from([(
                "scan-1:0".into(),
                crate::rules::RuleCheck {
                    code: "passed".into(),
                    source: "fixture".into(),
                    reason: "fixture".into(),
                    activity: crate::rules::ActivityCheck::from_code(activity).unwrap(),
                    module: crate::rules::ModuleCheck::from_codes(scope, code, scope).unwrap(),
                },
            )]);
            let view = apply_rule_checks(check(&plan, &[], 1002), &checks, 1002);
            assert_eq!(view.items[0].state, state);
            assert!(view.read_only && view.requires_mole_recheck);
            assert_eq!(
                apply_rule_checks(check(&plan, &[], plan.expires_at), &checks, plan.expires_at)
                    .items[0]
                    .state,
                "expired"
            );
            let mut changed = check(&plan, &[], 1002);
            changed.items[0].state = "changed".into();
            assert_eq!(
                apply_rule_checks(changed, &checks, 1002).items[0].state,
                "changed"
            );
            assert_eq!(
                apply_rule_checks(
                    check(&plan, &[path.to_str().unwrap().into()], 1002),
                    &checks,
                    1002
                )
                .items[0]
                    .state,
                "protected"
            );
        }
    }
    fn scan(path: &Path) -> Scan {
        let metadata = std::fs::metadata(path).unwrap();
        let candidate = Candidate {
            id: "a".into(),
            upstream_identity: format!("inode:{}:{}", metadata.dev(), metadata.ino()),
            path: path.to_str().unwrap().into(),
            section: "Fixture".into(),
            size: Some(1),
            item_count: 1,
            covered_by: None,
            is_directory: path.is_dir(),
            resolved_path: Some(path.to_str().unwrap().into()),
            impact: None,
            provenance: None,
        };
        capture(
            "scan-1".into(),
            Preview {
                output: String::new(),
                dry_run: true,
                protocol: PROTOCOL,
                revision: crate::engine::REVISION.into(),
                system_included: false,
                items: vec![candidate],
            },
            1000,
        )
        .unwrap()
    }
    #[test]
    fn rejects_unknown_duplicate_expired_and_protected_selections() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().canonicalize().unwrap().join("fixture");
        std::fs::write(&path, "a").unwrap();
        let scan = scan(&path);
        let ids = vec!["scan-1:0".into()];
        assert!(create(&scan, &ids, &[], 1001).is_ok());
        assert!(create(&scan, &["foreign".into()], &[], 1001).is_err());
        assert!(create(&scan, &["scan-1:0".into(), "scan-1:0".into()], &[], 1001).is_err());
        assert!(create(&scan, &["other-scan:0".into()], &[], 1001).is_err());
        assert!(create(&scan, &ids, &[], 999).is_err());
        assert!(create(&scan, &ids, &[], scan.expires_at).is_err());
        assert!(create(&scan, &ids, &[path.to_str().unwrap().into()], 1001).is_err());
    }
    #[test]
    fn recheck_detects_replacement_disappearance_and_later_protection() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().canonicalize().unwrap().join("fixture");
        std::fs::write(&path, "old").unwrap();
        let plan = create(&scan(&path), &["scan-1:0".into()], &[], 1001).unwrap();
        assert_eq!(check(&plan, &[], 1002).items[0].state, "unchanged");
        assert_eq!(
            check(&plan, &[path.to_str().unwrap().into()], 1002).items[0].state,
            "protected"
        );
        std::fs::rename(&path, path.with_extension("old")).unwrap();
        std::fs::write(&path, "new").unwrap();
        assert_eq!(check(&plan, &[], 1002).items[0].state, "changed");
        std::fs::remove_file(&path).unwrap();
        assert_eq!(check(&plan, &[], 1002).items[0].state, "unavailable");
    }
    #[test]
    fn ancestor_replacement_and_symlinks_fail_review() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().canonicalize().unwrap();
        let parent = root.join("parent");
        std::fs::create_dir(&parent).unwrap();
        let path = parent.join("fixture");
        std::fs::write(&path, "same").unwrap();
        let plan = create(&scan(&path), &["scan-1:0".into()], &[], 1001).unwrap();
        std::fs::rename(&parent, root.join("old")).unwrap();
        std::fs::create_dir(&parent).unwrap();
        // Preserve the same single-link file at its old absolute path while
        // replacing its ancestor.
        std::fs::rename(root.join("old/fixture"), &path).unwrap();
        assert_eq!(check(&plan, &[], 1002).items[0].state, "changed");
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(root.join("old/fixture"), &path).unwrap();
        assert_eq!(check(&plan, &[], 1002).items[0].state, "unavailable");
    }
}
