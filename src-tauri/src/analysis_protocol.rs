//! Required explicit-directory JSON fields, before publishing a task result.
use serde_json::Value;
use std::{collections::HashSet, path::Path};

const MAX_ENTRIES: usize = 20_000;
const MAX_LARGE_FILES: usize = 1_000;
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

fn text(value: &Value, max: usize) -> Option<&str> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && !s.contains('\0') && s.len() <= max)
}
fn path(value: &Value) -> Option<&str> {
    text(value, 4096).filter(|p| {
        p.starts_with('/')
            && (*p == "/"
                || p[1..]
                    .split('/')
                    .all(|part| !["", ".", ".."].contains(&part)))
    })
}
fn count(value: &Value, min: i64) -> bool {
    value
        .as_i64()
        .is_some_and(|n| (min..=MAX_SAFE_INTEGER).contains(&n))
}
fn state(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|s| ["complete", "partial", "unavailable"].contains(&s))
}
fn invalid(field: &str) -> String {
    format!("空间分析结果格式异常，未更新。请重新分析；若反复出现，请核对内置引擎。诊断：{field}")
}

pub fn validate(value: &Value, expected: &Path) -> Result<(), String> {
    let root = path(&value["path"]).ok_or_else(|| invalid("summary"))?;
    if expected.to_str() != Some(root)
        || !state(&value["scan_status"])
        || !count(&value["total_size"], -1)
    {
        return Err(invalid("summary"));
    }
    if value.get("overview").is_some_and(|v| v != false) {
        return Err(invalid("overview"));
    }
    if value.get("total_files").is_some_and(|v| !count(v, 0)) {
        return Err(invalid("total_files"));
    }
    let entries = value["entries"]
        .as_array()
        .ok_or_else(|| invalid("entries"))?;
    if entries.len() > MAX_ENTRIES {
        return Err(
            "空间分析返回的直接子项超过 20000 项，未更新。请选择更小的目录后重新分析。".into(),
        );
    }
    let mut seen = HashSet::new();
    for entry in entries {
        let name = text(&entry["name"], 1024).ok_or_else(|| invalid("entry"))?;
        let item_path = path(&entry["path"]).ok_or_else(|| invalid("entry"))?;
        let (parent, basename) = item_path.rsplit_once('/').ok_or_else(|| invalid("entry"))?;
        let parent = if parent.is_empty() { "/" } else { parent };
        if [".", ".."].contains(&name)
            || name.contains('/')
            || parent != root
            || basename.is_empty()
            || (name != basename && name != format!("{basename} →"))
            || !seen.insert(item_path)
            || !entry["is_dir"].is_boolean()
            || !state(&entry["scan_status"])
            || !count(&entry["size"], -1)
        {
            return Err(invalid("entry"));
        }
    }
    if value
        .get("large_files")
        .filter(|v| !v.is_null())
        .is_some_and(|v| {
            v.as_array()
                .is_none_or(|items| items.len() > MAX_LARGE_FILES)
        })
    {
        return Err(invalid("large_files"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn corpus() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/analysis-protocol.json")).unwrap()
    }
    #[test]
    fn shared_field_corpus_and_required_fields() {
        let c = corpus();
        for kind in ["accept", "reject"] {
            for sample in c[kind].as_array().unwrap() {
                let mut value = c["base"].clone();
                *value
                    .pointer_mut(sample["pointer"].as_str().unwrap())
                    .unwrap() = sample["value"].clone();
                assert_eq!(
                    validate(&value, Path::new("/fixture")).is_ok(),
                    kind == "accept",
                    "{kind} {sample}"
                );
            }
        }
        for key in ["path", "scan_status", "total_size", "entries"] {
            let mut value = c["base"].clone();
            value.as_object_mut().unwrap().remove(key);
            assert!(validate(&value, Path::new("/fixture")).is_err());
        }
        for key in ["name", "path", "scan_status", "size", "is_dir"] {
            let mut value = c["base"].clone();
            value["entries"][0].as_object_mut().unwrap().remove(key);
            assert!(validate(&value, Path::new("/fixture")).is_err());
        }
    }
    #[test]
    fn exact_root_binding_duplicates_and_utf8_names() {
        let mut v = corpus()["base"].clone();
        assert!(validate(&v, Path::new("/another-root")).is_err());
        let item = v["entries"][0].clone();
        v["entries"].as_array_mut().unwrap().push(item);
        assert!(validate(&v, Path::new("/fixture")).is_err());
        for (name, valid) in [
            ("中文\n[*] $(literal)".into(), true),
            ("x".repeat(1024), true),
            ("x".repeat(1025), false),
            ("中".repeat(342), false),
        ] {
            let mut v = corpus()["base"].clone();
            v["entries"][0]["name"] = json!(name);
            v["entries"][0]["path"] = json!(format!("/fixture/{name}"));
            assert_eq!(validate(&v, Path::new("/fixture")).is_ok(), valid);
        }
        v = corpus()["base"].clone();
        v["path"] = json!("/");
        v["entries"][0]["path"] = json!("/child");
        assert!(validate(&v, Path::new("/")).is_ok());
    }
    #[test]
    fn array_and_path_budgets_reject_whole_result() {
        let mut v = corpus()["base"].clone();
        v["entries"] = json!((0..MAX_ENTRIES)
            .map(|i| json!({
                "name": format!("item{i}"), "path": format!("/fixture/item{i}"),
                "size": 0, "scan_status": "complete", "is_dir": false
            }))
            .collect::<Vec<_>>());
        assert!(validate(&v, Path::new("/fixture")).is_ok());
        v["entries"].as_array_mut().unwrap().push(Value::Null);
        assert!(validate(&v, Path::new("/fixture"))
            .unwrap_err()
            .contains("更小的目录"));
        v["entries"] = json!([]);
        v["large_files"] = json!(vec![Value::Null; MAX_LARGE_FILES]);
        assert!(validate(&v, Path::new("/fixture")).is_ok());
        v["large_files"].as_array_mut().unwrap().push(Value::Null);
        assert!(validate(&v, Path::new("/fixture")).is_err());
        v["large_files"] = Value::Null;
        let root = format!("/{}", "x".repeat(4095));
        v["path"] = json!(root);
        assert!(validate(&v, Path::new(&root)).is_ok());
        v["path"] = json!(format!("{root}x"));
        assert!(validate(&v, Path::new(&format!("{root}x"))).is_err());
    }
}
