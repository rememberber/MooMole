//! Bounded, read-only descendant path guards. These observations are NOT
//! recursive module coverage, a consistent snapshot, or execution authority.
use crate::{provenance, review, rules, tree};
use serde::Serialize;
use std::{collections::HashMap, path::Path};

pub const VERSION: u64 = 1;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub version: u64,
    pub code: String,
    pub checked_entries: usize,
    pub path: Option<String>,
    pub source: Option<String>,
    pub reason: String,
}

impl Check {
    fn new(code: &str, reason: impl Into<String>) -> Self {
        Self {
            version: VERSION,
            code: code.into(),
            checked_entries: 0,
            path: None,
            source: None,
            reason: reason.into(),
        }
    }
}

struct Group {
    observation: Check,
    entries: Vec<String>,
}

pub struct Batch {
    pub requested: Vec<(String, String, String)>,
    groups: HashMap<String, Group>,
}

pub fn prepare(plan: &review::Plan, local: &review::PlanView, home: &Path) -> Batch {
    let mut batch = Batch {
        requested: Vec::new(),
        groups: HashMap::new(),
    };
    for current in local.items.iter().filter(|i| i.state == "unchanged") {
        let Some(item) = plan.items.iter().find(|i| i.candidate.id == current.id) else {
            continue;
        };
        let binding = provenance::binding(item.candidate.provenance.as_ref(), &item.candidate.path);
        batch
            .requested
            .push((current.id.clone(), current.path.clone(), binding.into()));
    }
    // Root selection keeps priority; descendant inputs share the SAME 500-row
    // adapter limit and the same 45-second process, never a second full pass.
    let mut budget = tree::Budget::new();
    for current in local.items.iter().filter(|i| i.state == "unchanged") {
        let Some(item) = plan.items.iter().find(|i| i.candidate.id == current.id) else {
            continue;
        };
        let binding = provenance::binding(item.candidate.provenance.as_ref(), &item.candidate.path);
        let mut group = Group {
            observation: Check::new(
                "notChecked",
                "扫描来源或模块范围尚未适配，未检查后代路径保护。",
            ),
            entries: Vec::new(),
        };
        if ["userCache", "userLogs"].contains(&binding)
            && rules::module_scope(Path::new(&current.path), home) == binding
        {
            if let Some(target) = item.witness.as_ref().and_then(|w| w.nodes.last()) {
                if target.mode & libc::S_IFMT as u32 != libc::S_IFDIR as u32 {
                    group.observation =
                        Check::new("notApplicable", "所选项目是普通文件，没有目录后代可检查。");
                } else {
                    let limit = rules::MAX_RECORDS.saturating_sub(batch.requested.len());
                    match tree::inspect_paths(
                        Path::new(&current.path),
                        target.dev,
                        target.ino,
                        &mut budget,
                        limit,
                    ) {
                        Ok((observed, paths)) if item.tree.as_ref() == Some(&observed) => {
                            group.observation = Check::new("pending", "后代路径保护结果尚未确认。");
                            for (index, path) in paths.into_iter().enumerate() {
                                let id = format!("descendant:{}:{index}", current.id);
                                group.entries.push(id.clone());
                                // Descendants are NOT separate scan candidates or module
                                // bindings. Only their path guard result will be used.
                                batch.requested.push((id, path, "unsupported".into()));
                            }
                        }
                        Ok(_) => {
                            group.observation = Check::new(
                                "unavailable",
                                "目录内容与保存计划不一致，未发布后代路径保护结果，请重新扫描。",
                            )
                        }
                        Err(reason) => group.observation = Check::new("unavailable", reason),
                    }
                }
            }
        }
        batch.groups.insert(current.id.clone(), group);
    }
    batch
}

pub fn apply(
    mut view: review::PlanView,
    batch: &Batch,
    results: &HashMap<String, rules::RuleCheck>,
) -> review::PlanView {
    for item in &mut view.items {
        let Some(group) = batch.groups.get(&item.id) else {
            continue;
        };
        // A local final recheck has priority. Evidence for a stale tree must not
        // remain as a successful observation even when its root path is stable.
        if ![
            "unchanged",
            "activityBusy",
            "activityUnknown",
            "activityUnavailable",
            "moleProtected",
            "active",
            "ruleUnavailable",
            "moduleRetained",
            "moduleUnsupported",
        ]
        .contains(&item.state.as_str())
        {
            item.descendant_rules = Some(Check::new(
                "unavailable",
                "当前身份、目录内容、保护或有效期已变化，之前的后代观察不再适用。",
            ));
            continue;
        }
        let mut observed = group.observation.clone();
        if observed.code == "pending" {
            if !results
                .get(&item.id)
                .is_some_and(|r| r.code == "passed" && r.module.code == "checked")
            {
                observed = Check::new(
                    "notChecked",
                    "根项目的路径或模块条件未满足，不发布后代检查完成结论。",
                );
            } else {
                observed = Check::new("checked", "本次已逐项复查目录后代的 Mole 路径保护，未发现要求保留的项目；不包含全部后代模块规则，也不是执行许可。");
                for id in &group.entries {
                    let Some(result) = results.get(id) else {
                        observed =
                            Check::new("unavailable", "后代路径保护报告缺项，本次检查未完成。");
                        break;
                    };
                    observed.checked_entries += 1;
                    if result.code != "passed" {
                        observed.code = if ["coreProtection", "whitelist", "compiledModel"]
                            .contains(&result.code.as_str())
                        {
                            "protected"
                        } else {
                            "unavailable"
                        }
                        .into();
                        observed.path = batch
                            .requested
                            .iter()
                            .find(|r| &r.0 == id)
                            .map(|r| r.1.clone());
                        observed.source = Some(result.source.clone());
                        observed.reason = format!("目录后代要求保留或未能确认：{}", result.reason);
                        break;
                    }
                }
            }
        }
        if item.state == "unchanged"
            && !["checked", "notApplicable"].contains(&observed.code.as_str())
        {
            item.state = if observed.code == "protected" {
                "descendantProtected"
            } else {
                "descendantUnavailable"
            }
            .into();
            item.reason = observed.reason.clone();
        }
        item.descendant_rules = Some(observed);
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        clean_preview::{Candidate, Preview, PROTOCOL},
        provenance::{Origin, Provenance},
    };
    use std::os::unix::fs::{symlink, MetadataExt};

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, review::Plan) {
        let scratch = tempfile::tempdir().unwrap();
        let home = scratch.path().canonicalize().unwrap();
        let path = home.join("Library/Caches/app");
        std::fs::create_dir_all(path.join("deep")).unwrap();
        std::fs::write(path.join("deep/保留\n[*].log"), "fixture").unwrap();
        let plan = plan_for(&[path]);
        (scratch, home, plan)
    }
    fn plan_for(paths: &[std::path::PathBuf]) -> review::Plan {
        let candidates = paths.iter().map(|path| {
            let m = std::fs::metadata(path).unwrap();
            let path = path.to_str().unwrap();
            Candidate {
                id: String::new(), upstream_identity: format!("inode:{}:{}", m.dev(), m.ino()),
                path: path.into(), section: "fixture".into(), size: Some(m.size()), item_count: 1, covered_by: None,
                is_directory: m.is_dir(), resolved_path: Some(path.into()), impact: None,
                provenance: Some(Provenance { version: 1, observations: vec![Origin::from_fields(path, "record_dry_run_cleanup_target,_safe_clean_impl,safe_clean_guarded,clean_user_essentials", "_user_cache_deno_delete_guard", "_user_cache_deno_delete_guard").unwrap()] }),
            }
        }).collect();
        let scan = review::capture(
            "fixture".into(),
            Preview {
                output: String::new(),
                dry_run: true,
                protocol: PROTOCOL,
                revision: crate::engine::REVISION.into(),
                system_included: false,
                items: candidates,
            },
            1000,
        )
        .unwrap();
        let ids = scan
            .items
            .iter()
            .map(|i| i.candidate.id.clone())
            .collect::<Vec<_>>();
        review::create(&scan, &ids, &[], 1001).unwrap()
    }
    // Exercise the real v4 parser, including descendants' unsupported module
    // binding. Fixture path codes are never used by production authorization.
    fn results(batch: &Batch, special: Option<(&str, &str)>) -> HashMap<String, rules::RuleCheck> {
        let mut fields = vec![
            "MooMole.rules.v4".to_string(),
            crate::engine::REVISION.into(),
            "2".into(),
            "1".into(),
            "/fixture-home".into(),
        ];
        for (id, path, binding) in &batch.requested {
            let code = special
                .filter(|(p, _)| *p == path)
                .map_or("passed", |(_, code)| code);
            // These fixtures live in a temporary HOME, used below for scope recomputation.
            let home = Path::new(path)
                .ancestors()
                .find(|p| p.join("Library/Caches").is_dir())
                .unwrap();
            fields[4] = home.to_str().unwrap().into();
            let scope = if binding == "userCache" {
                "userCache"
            } else {
                "unsupported"
            };
            let module = if code != "passed" {
                "notChecked"
            } else if scope == "userCache" {
                "checked"
            } else {
                "unsupported"
            };
            let activity = if module == "checked" {
                "noOpenHandles"
            } else {
                "notChecked"
            };
            fields.extend([
                id.clone(),
                path.clone(),
                binding.clone(),
                code.into(),
                scope.into(),
                module.into(),
                activity.into(),
            ]);
        }
        fields.push("complete".into());
        rules::parse(
            (fields.join("\0") + "\0").as_bytes(),
            &batch.requested,
            Path::new(&fields[4]),
        )
        .unwrap()
    }
    fn apply_fixture(
        plan: &review::Plan,
        batch: &Batch,
        results: &HashMap<String, rules::RuleCheck>,
    ) -> review::PlanView {
        apply(
            review::apply_rule_checks(review::check(plan, &[], 1002), results, 1002),
            batch,
            results,
        )
    }
    #[test]
    fn exact_walk_paths_bind_to_plan_and_child_guards_are_separate_from_modules() {
        let (_scratch, home, plan) = fixture();
        let batch = prepare(&plan, &review::check(&plan, &[], 1002), &home);
        assert_eq!(batch.requested.len(), 3);
        assert!(batch.requested[1..].iter().all(|r| r.2 == "unsupported"));
        assert!(batch.requested[2].1.ends_with("deep/保留\n[*].log"));
        let view = apply_fixture(&plan, &batch, &results(&batch, None));
        assert_eq!(
            view.items[0].descendant_rules.as_ref().unwrap().code,
            "checked"
        );
        assert_eq!(
            view.items[0]
                .descendant_rules
                .as_ref()
                .unwrap()
                .checked_entries,
            2
        );
        assert_eq!(view.items[0].state, "unchanged");
        assert!(view.read_only && view.requires_mole_recheck);
        // Missing or mixed scan source never gets inferred from a cache path.
        for mode in ["missing", "mixed"] {
            let mut changed = plan.clone();
            if mode == "missing" {
                changed.items[0].candidate.provenance = None;
            } else {
                changed.items[0]
                    .candidate
                    .provenance
                    .as_mut()
                    .unwrap()
                    .observations[0]
                    .path = "/alias".into();
            }
            let batch = prepare(&changed, &review::check(&changed, &[], 1002), &home);
            assert_eq!(batch.requested.len(), 1);
            assert_eq!(
                batch.groups.values().next().unwrap().observation.code,
                "notChecked"
            );
        }
    }
    #[test]
    fn protected_unknown_and_missing_children_never_become_complete() {
        let (_scratch, home, plan) = fixture();
        let batch = prepare(&plan, &review::check(&plan, &[], 1002), &home);
        let child = &batch.requested[2].1;
        for (code, state, evidence) in [
            ("whitelist", "descendantProtected", "protected"),
            ("compiledModel", "descendantProtected", "protected"),
            ("sqliteUnknown", "descendantUnavailable", "unavailable"),
            ("checkFailed", "descendantUnavailable", "unavailable"),
        ] {
            let view = apply_fixture(&plan, &batch, &results(&batch, Some((child, code))));
            assert_eq!(view.items[0].state, state);
            let observed = view.items[0].descendant_rules.as_ref().unwrap();
            assert_eq!(observed.code, evidence);
            assert_eq!(observed.path.as_ref(), Some(child));
            assert!(observed.source.is_some());
        }
        let mut missing = results(&batch, None);
        missing.remove(&batch.requested[2].0);
        assert_eq!(
            apply_fixture(&plan, &batch, &missing).items[0].state,
            "descendantUnavailable"
        );
        let root_blocked = results(&batch, Some((&batch.requested[0].1, "whitelist")));
        let view = apply_fixture(&plan, &batch, &root_blocked);
        assert_eq!(view.items[0].state, "moleProtected");
        assert_eq!(
            view.items[0].descendant_rules.as_ref().unwrap().code,
            "notChecked"
        );
    }
    #[test]
    fn changed_tree_and_final_local_failures_discard_descendant_success() {
        let (_scratch, home, plan) = fixture();
        let batch = prepare(&plan, &review::check(&plan, &[], 1002), &home);
        let results = results(&batch, None);
        for now in [999, plan.expires_at] {
            let view = apply(review::check(&plan, &[], now), &batch, &results);
            assert_eq!(view.items[0].state, "expired");
            assert_eq!(
                view.items[0].descendant_rules.as_ref().unwrap().code,
                "unavailable"
            );
        }
        let protected = vec![home
            .join("Library/Caches/app/deep")
            .to_str()
            .unwrap()
            .into()];
        let view = apply(review::check(&plan, &protected, 1002), &batch, &results);
        assert_eq!(view.items[0].state, "protected");
        std::fs::write(&batch.requested[2].1, "changed content").unwrap();
        let view = apply(review::check(&plan, &[], 1002), &batch, &results);
        assert_eq!(view.items[0].state, "contentsChanged");
        assert_eq!(
            view.items[0].descendant_rules.as_ref().unwrap().code,
            "unavailable"
        );
        // Even with a stale caller view, listing digest must still match plan.
        let mut stale = review::check(&plan, &[], 1002);
        stale.items[0].state = "unchanged".into();
        let refused = prepare(&plan, &stale, &home);
        assert_eq!(refused.requested.len(), 1);
        assert_eq!(
            refused.groups.values().next().unwrap().observation.code,
            "unavailable"
        );
    }
    #[test]
    fn shared_record_budget_never_publishes_a_partial_tree_as_complete() {
        let scratch = tempfile::tempdir().unwrap();
        let home = scratch.path().canonicalize().unwrap();
        let directory = home.join("Library/Caches/large");
        let empty = home.join("Library/Caches/empty");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::create_dir(&empty).unwrap();
        for i in 0..rules::MAX_RECORDS {
            std::fs::write(directory.join(i.to_string()), "fixture").unwrap();
        }
        let file = home.join("Library/Caches/plain");
        std::fs::write(&file, "file").unwrap();
        let plan = plan_for(&[directory, empty, file]);
        let batch = prepare(&plan, &review::check(&plan, &[], 1002), &home);
        assert_eq!(
            batch.requested.len(),
            3,
            "over-budget paths must not leak into request"
        );
        let view = apply_fixture(&plan, &batch, &results(&batch, None));
        assert_eq!(
            view.items[0].descendant_rules.as_ref().unwrap().code,
            "unavailable"
        );
        assert_eq!(
            view.items[1].descendant_rules.as_ref().unwrap().code,
            "checked"
        );
        assert_eq!(
            view.items[2].descendant_rules.as_ref().unwrap().code,
            "notApplicable"
        );
    }
    #[test]
    fn multiple_selected_roots_share_one_record_limit_without_partial_inputs() {
        let scratch = tempfile::tempdir().unwrap();
        let home = scratch.path().canonicalize().unwrap();
        let paths = [
            home.join("Library/Caches/first"),
            home.join("Library/Caches/second"),
        ];
        for path in &paths {
            std::fs::create_dir_all(path).unwrap();
            for n in 0..260 {
                std::fs::write(path.join(n.to_string()), "fixture").unwrap();
            }
        }
        let plan = plan_for(&paths);
        let batch = prepare(&plan, &review::check(&plan, &[], 1002), &home);
        assert_eq!(batch.requested.len(), 262);
        let view = apply_fixture(&plan, &batch, &results(&batch, None));
        assert_eq!(
            view.items[0]
                .descendant_rules
                .as_ref()
                .unwrap()
                .checked_entries,
            260
        );
        assert_eq!(
            view.items[1].descendant_rules.as_ref().unwrap().code,
            "unavailable"
        );
        assert_eq!(
            view.items[1]
                .descendant_rules
                .as_ref()
                .unwrap()
                .checked_entries,
            0
        );
    }
    #[test]
    fn listing_refuses_new_links_even_if_given_a_stale_local_view() {
        let (_scratch, home, plan) = fixture();
        let local = review::check(&plan, &[], 1002);
        symlink("/", home.join("Library/Caches/app/link")).unwrap();
        let batch = prepare(&plan, &local, &home);
        assert_eq!(batch.requested.len(), 1);
        assert_eq!(
            batch.groups.values().next().unwrap().observation.code,
            "unavailable"
        );
    }
}
