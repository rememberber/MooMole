//! Persisted read-only review plans. Metadata observations are deliberately not
//! an authorization to delete; execution still needs Mole rules and secure I/O.
use crate::clean_preview::{Candidate, Preview, PROTOCOL};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};

pub const LIFETIME_MS: i64 = 15 * 60 * 1000;
pub const MAX_SELECTION: usize = 500;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
struct Node {
    path: PathBuf,
    dev: u64,
    ino: u64,
    mode: u32,
    uid: u32,
    gid: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
struct Witness {
    nodes: Vec<Node>,
    size: u64,
    mtime: i64,
    mtime_ns: i64,
    ctime: i64,
    ctime_ns: i64,
}

fn witness(path: &Path) -> Result<Witness, String> {
    if !path.is_absolute()
        || path == Path::new("/")
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err("路径范围无效，需要重新扫描。".into());
    }
    let mut nodes = Vec::new();
    let mut current = PathBuf::new();
    let mut target = None;
    for component in path.components() {
        current.push(component);
        let metadata =
            std::fs::symlink_metadata(&current).map_err(|_| "路径已不存在或无法读取。")?;
        if metadata.file_type().is_symlink() {
            return Err("路径或祖先目录包含符号链接，需要重新确认。".into());
        }
        if !metadata.is_file() && !metadata.is_dir() {
            return Err("不是普通文件或目录，无法纳入审阅计划。".into());
        }
        nodes.push(Node {
            path: current.clone(),
            dev: metadata.dev(),
            ino: metadata.ino(),
            mode: metadata.mode(),
            uid: metadata.uid(),
            gid: metadata.gid(),
        });
        target = Some(metadata);
    }
    let target = target.ok_or("没有可复核的目标。")?;
    Ok(Witness {
        nodes,
        size: target.size(),
        mtime: target.mtime(),
        mtime_ns: target.mtime_nsec(),
        ctime: target.ctime(),
        ctime_ns: target.ctime_nsec(),
    })
}

#[derive(Clone, Deserialize, Serialize)]
pub struct ScanItem {
    pub candidate: Candidate,
    witness: Option<Witness>,
    unavailable: Option<String>,
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
    let items = plan
        .items
        .iter()
        .map(|item| {
            let (state, reason) =
                if plan.revision != crate::engine::REVISION || plan.protocol != PROTOCOL {
                    (
                        "versionMismatch",
                        "引擎或协议版本已变化，需要重新扫描。".into(),
                    )
                } else if now >= plan.expires_at || now < plan.created_at {
                    ("expired", "计划已过期或系统时间回退，需要重新扫描。".into())
                } else if protected(&item.candidate, rules) {
                    ("protected", "与当前客户端保护目录重叠，已排除。".into())
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
            ItemCheck {
                id: item.candidate.id.clone(),
                path: item.candidate.path.clone(),
                size: item.candidate.size,
                section: item.candidate.section.clone(),
                state: state.into(),
                reason,
                mole_rule: None,
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
    let plan = Plan {
        id: uuid::Uuid::new_v4().to_string(),
        scan_id: scan.id.clone(),
        created_at: now,
        expires_at: scan.expires_at,
        revision: scan.revision.clone(),
        protocol: scan.protocol,
        items,
    };
    let view = check(&plan, rules, now);
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
            item.mole_rule = Some(check.clone());
        }
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn passing_mole_rules_do_not_override_new_client_protection_or_expiry() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().canonicalize().unwrap().join("file");
        std::fs::write(&path, "fixture").unwrap();
        let plan = create(&scan(&path), &["scan-1:0".into()], &[], 1001).unwrap();
        let checks = HashMap::from([(
            "scan-1:0".into(),
            crate::rules::RuleCheck {
                code: "passed".into(),
                source: "fixture".into(),
                reason: "fixture".into(),
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
        std::fs::hard_link(root.join("old/fixture"), &path).unwrap();
        assert_eq!(check(&plan, &[], 1002).items[0].state, "changed");
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(root.join("old/fixture"), &path).unwrap();
        assert_eq!(check(&plan, &[], 1002).items[0].state, "unavailable");
    }
}
