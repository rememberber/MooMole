//! Read-only preview protocol. Identities are upstream deduplication keys, not
//! filesystem identity evidence and must never be used as an execution plan.
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path},
};

pub const PROTOCOL: u64 = 2;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub upstream_identity: String,
    pub path: String,
    pub section: String,
    pub size: Option<u64>,
    pub item_count: u64,
    pub covered_by: Option<String>,
    pub is_directory: bool,
    pub resolved_path: Option<String>,
    #[serde(default)]
    pub impact: Option<crate::impact::Impact>,
    #[serde(default)]
    pub provenance: Option<crate::provenance::Provenance>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub output: String,
    pub dry_run: bool,
    pub protocol: u64,
    pub revision: String,
    pub system_included: bool,
    pub items: Vec<Candidate>,
}

fn absolute_path(value: &str) -> bool {
    Path::new(value).is_absolute()
        && !Path::new(value)
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
}

fn number(value: &str) -> Result<u64, String> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("清理协议包含无效数值。".into());
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|n| *n <= MAX_SAFE_INTEGER)
        .ok_or("清理协议数值超出范围。".into())
}

pub fn parse(bytes: &[u8]) -> Result<Preview, String> {
    let invalid = || "Mole 结构化预览不完整或与当前协议不匹配，请重新准备引擎。".to_string();
    if bytes.last() != Some(&0) {
        return Err(invalid());
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| "清理路径不是有效 UTF-8，已停止读取预览。")?;
    let mut fields: Vec<&str> = text.split_terminator('\0').collect();
    if fields.len() < 4
        || fields[0] != "MooMole.clean.v2"
        || fields[1] != crate::engine::REVISION
        || !["true", "false"].contains(&fields[2])
        || fields.pop() != Some("complete")
    {
        return Err(invalid());
    }
    let ledger_marker = (3..fields.len())
        .step_by(7)
        .find(|&i| fields[i] == "ledger")
        .ok_or_else(invalid)?;
    let marker = (ledger_marker + 1..fields.len())
        .step_by(6)
        .find(|&i| fields[i] == "origins")
        .ok_or_else(invalid)?;
    if (fields.len() - marker - 1) % 5 != 0
        || (fields.len() - marker - 1) / 5 > 20_000
        || (marker - ledger_marker - 1) / 6 > 20_000
    {
        return Err(invalid());
    }
    let system_included = fields[2] == "true";
    let mut identities = HashSet::new();
    let mut paths = HashSet::new();
    let mut items = Vec::new();
    for (index, row) in fields[3..ledger_marker].chunks_exact(7).enumerate() {
        let [identity, size_kb, count, known, section, path, covered_by] = row else {
            unreachable!()
        };
        if identity.is_empty()
            || section.is_empty()
            || !absolute_path(path)
            || !["true", "false"].contains(known)
            || !identities.insert(*identity)
            || !paths.insert(*path)
        {
            return Err(invalid());
        }
        let size = number(size_kb)?
            .checked_mul(1024)
            .filter(|n| *n <= MAX_SAFE_INTEGER)
            .ok_or_else(invalid)?;
        let count = number(count)?;
        if count == 0 {
            return Err(invalid());
        }
        let metadata = std::fs::symlink_metadata(path).ok();
        items.push(Candidate {
            id: format!("candidate-{index}"),
            upstream_identity: (*identity).into(),
            path: (*path).into(),
            section: (*section).into(),
            size: if *known == "true" { Some(size) } else { None },
            item_count: count,
            covered_by: if covered_by.is_empty() {
                None
            } else {
                Some((*covered_by).into())
            },
            is_directory: metadata.is_some_and(|m| m.is_dir()),
            resolved_path: Path::new(path)
                .canonicalize()
                .ok()
                .and_then(|p| p.to_str().map(String::from)),
            impact: None,
            provenance: Some(crate::provenance::Provenance {
                version: crate::provenance::VERSION,
                observations: vec![],
            }),
        });
    }
    let lookup: HashMap<String, usize> = items
        .iter()
        .enumerate()
        .map(|(i, item)| (item.upstream_identity.clone(), i))
        .collect();
    // A duplicate raw append whose origin was lost must not be hidden by an older observation.
    let specs: HashMap<&str, &[&str]> = fields[3..ledger_marker]
        .chunks_exact(7)
        .map(|row| (row[0], &row[..6]))
        .collect();
    let mut raw_seen = HashSet::new();
    let mut occurrences: HashMap<(&str, &str), usize> = HashMap::new();
    for row in fields[ledger_marker + 1..marker].chunks_exact(6) {
        let expected = specs.get(row[0]).ok_or_else(invalid)?;
        if row[4].is_empty()
            || !absolute_path(row[5])
            || !["true", "false"].contains(&row[3])
            || number(row[1]).is_err()
            || number(row[2])? == 0
        {
            return Err(invalid());
        }
        if raw_seen.insert(row[0]) && *expected != row {
            return Err(invalid());
        }
        *occurrences.entry((row[0], row[5])).or_default() += 1;
    }
    if raw_seen.len() != items.len() {
        return Err(invalid());
    }
    for row in fields[marker + 1..].chunks_exact(5) {
        let index = *lookup.get(row[0]).ok_or_else(invalid)?;
        let count = occurrences.get_mut(&(row[0], row[1])).ok_or_else(invalid)?;
        *count = count.checked_sub(1).ok_or_else(invalid)?;
        if !absolute_path(row[1]) {
            return Err(invalid());
        }
        let origin = crate::provenance::Origin::from_fields(row[1], row[2], row[3], row[4])?;
        let observations = &mut items[index].provenance.as_mut().unwrap().observations;
        if !observations.contains(&origin) {
            observations.push(origin);
        }
        if observations.len() > 16 {
            return Err("单项调用来源超过 16 种，请缩小扫描范围。".into());
        }
    }
    if occurrences.values().any(|&n| n != 0) {
        return Err("原始候选与调用来源数量不一致，请重新扫描。".into());
    }
    if items.iter().any(|item| {
        !item
            .provenance
            .as_ref()
            .unwrap()
            .observations
            .iter()
            .any(|o| o.path == item.path)
    }) {
        return Err("候选缺少对应路径的扫描来源，请重新扫描。".into());
    }
    let measured_paths: HashSet<&str> = items
        .iter()
        .filter(|item| item.size.is_some())
        .map(|item| item.path.as_str())
        .collect();
    for item in &items {
        if let Some(parent) = &item.covered_by {
            if parent == &item.path
                || !Path::new(&item.path).starts_with(parent)
                || !measured_paths.contains(parent.as_str())
            {
                return Err(invalid());
            }
        }
    }
    Ok(Preview {
        output: String::new(),
        dry_run: true,
        protocol: PROTOCOL,
        revision: crate::engine::REVISION.into(),
        system_included,
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report(rows: &[&[&str; 7]]) -> Vec<u8> {
        let mut fields = vec!["MooMole.clean.v2", crate::engine::REVISION, "false"];
        for row in rows {
            fields.extend_from_slice(*row);
        }
        fields.push("ledger");
        for row in rows {
            fields.extend_from_slice(&row[..6]);
        }
        fields.push("origins");
        for row in rows {
            fields.extend_from_slice(&[row[0], row[5], "fixture_module", "", ""]);
        }
        fields.push("complete");
        (fields.join("\0") + "\0").into_bytes()
    }
    #[test]
    fn preserves_unusual_paths_unknown_sizes_and_coverage() {
        let data = report(&[
            &["1", "8", "3", "true", "Apps", "/tmp/缓存\n\t[*]", ""],
            &[
                "2",
                "0",
                "1",
                "false",
                "Apps",
                "/tmp/缓存\n\t[*]/child",
                "/tmp/缓存\n\t[*]",
            ],
        ]);
        let result = parse(&data).unwrap();
        assert_eq!(result.items[0].size, Some(8192));
        assert_eq!(result.items[1].size, None);
        assert_eq!(
            result.items[1].covered_by.as_deref(),
            Some("/tmp/缓存\n\t[*]")
        );
        assert!(parse(&report(&[])).unwrap().items.is_empty());
    }
    #[test]
    fn rejects_truncation_invalid_encoding_versions_numbers_and_paths() {
        let valid = report(&[&["1", "0", "1", "true", "Apps", "/tmp/a", ""]]);
        for n in 0..valid.len() {
            assert!(parse(&valid[..n]).is_err());
        }
        let mut invalid = valid.clone();
        invalid[0] = 255;
        assert!(parse(&invalid).is_err());
        for row in [
            [
                "1",
                "18446744073709551615",
                "1",
                "true",
                "Apps",
                "/tmp/a",
                "",
            ],
            ["1", "1", "0", "true", "Apps", "/tmp/a", ""],
            ["1", "1", "1", "maybe", "Apps", "/tmp/a", ""],
            ["1", "1", "1", "true", "Apps", "/tmp/../a", ""],
            ["1", "1", "1", "true", "Apps", "relative", ""],
            ["1", "1", "1", "true", "Apps", "/tmp/a", "/tmp/absent"],
        ] {
            assert!(parse(&report(&[&row])).is_err());
        }
        assert!(parse(
            &String::from_utf8(valid)
                .unwrap()
                .replace("clean.v2", "clean.v1")
                .into_bytes()
        )
        .is_err());
    }
    #[test]
    fn rejects_duplicate_records_and_false_parent_coverage() {
        let parent = ["1", "1", "1", "true", "Apps", "/tmp/a", ""];
        assert!(parse(&report(&[&parent, &parent])).is_err());
        let sibling = ["2", "1", "1", "true", "Apps", "/tmp/ab", "/tmp/a"];
        assert!(parse(&report(&[&parent, &sibling])).is_err());
    }
    #[test]
    fn provenance_requires_exact_candidates_and_preserves_mixed_alias_observations() {
        let raw = report(&[&["1", "1", "1", "true", "Apps", "/fixture/cache", ""]]);
        let prefix = String::from_utf8(raw)
            .unwrap()
            .split("ledger\0")
            .next()
            .unwrap()
            .to_owned()
            + "ledger\0";
        let encoded = |rows: Vec<Vec<String>>| {
            let raw_rows = rows
                .iter()
                .flat_map(|row| {
                    [
                        row[0].clone(),
                        "1".into(),
                        "1".into(),
                        "true".into(),
                        "Apps".into(),
                        row[1].clone(),
                    ]
                })
                .collect::<Vec<_>>()
                .join("\0");
            (prefix.clone()
                + &raw_rows
                + "\0origins\0"
                + &rows.into_iter().flatten().collect::<Vec<_>>().join("\0")
                + "\0complete\0")
                .into_bytes()
        };
        let known=vec!["1","/fixture/cache","record_dry_run_cleanup_target,_safe_clean_impl,safe_clean_guarded,clean_user_essentials","_user_cache_deno_delete_guard","_user_cache_deno_delete_guard"].into_iter().map(String::from).collect::<Vec<_>>();
        let mut other = known.clone();
        other[2] = "fixture_other_module".into();
        other[3].clear();
        other[4].clear();
        let parsed = parse(&encoded(vec![known.clone(), known.clone(), other.clone()])).unwrap();
        let p = parsed.items[0].provenance.as_ref().unwrap();
        assert_eq!(p.observations.len(), 2);
        assert_eq!(p.binding("/fixture/cache"), "mixed");
        // A later duplicate raw append without its matching origin is incomplete.
        let lost = String::from_utf8(encoded(vec![known.clone()]))
            .unwrap()
            .replace(
                "origins\0",
                "1\x001\x001\0true\0Apps\0/fixture/cache\0origins\0",
            );
        assert!(parse(lost.as_bytes()).is_err());
        let restored: Preview =
            serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
        assert_eq!(
            restored.items[0]
                .provenance
                .as_ref()
                .unwrap()
                .binding("/fixture/cache"),
            "mixed"
        );
        let mut alias = known.clone();
        alias[1] = "/fixture/alias".into();
        assert_eq!(
            parse(&encoded(vec![known.clone(), alias.clone()]))
                .unwrap()
                .items[0]
                .provenance
                .as_ref()
                .unwrap()
                .binding("/fixture/cache"),
            "mixed"
        );
        assert!(parse(&encoded(vec![alias])).is_err());
        for (index, value) in [(0, "foreign"), (2, ""), (3, "bad;call")] {
            let mut bad = known.clone();
            bad[index] = value.into();
            assert!(parse(&encoded(vec![bad])).is_err());
        }
        let many = (0..17)
            .map(|i| {
                let mut row = other.clone();
                row[2] = format!("fixture_{i}");
                row
            })
            .collect();
        assert!(parse(&encoded(many)).is_err());
        assert!(parse(&(prefix + "complete\0").into_bytes()).is_err());
    }
}
