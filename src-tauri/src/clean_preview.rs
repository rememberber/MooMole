//! Read-only preview protocol. Identities are upstream deduplication keys, not
//! filesystem identity evidence and must never be used as an execution plan.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Component, Path},
};

pub const PROTOCOL: u64 = 1;
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
        || fields[0] != "MooMole.clean.v1"
        || fields[1] != crate::engine::REVISION
        || !["true", "false"].contains(&fields[2])
        || fields.pop() != Some("complete")
        || (fields.len() - 3) % 7 != 0
    {
        return Err(invalid());
    }
    let system_included = fields[2] == "true";
    let mut identities = HashSet::new();
    let mut paths = HashSet::new();
    let mut items = Vec::new();
    for (index, row) in fields[3..].chunks_exact(7).enumerate() {
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
        });
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
        let mut fields = vec!["MooMole.clean.v1", crate::engine::REVISION, "false"];
        for row in rows {
            fields.extend_from_slice(*row);
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
                .replace("clean.v1", "clean.v2")
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
}
