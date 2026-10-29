use crate::db::TaskRecord;
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

const SCHEMA: &str = "moomole-client-task-summary-v1";
const NOTE: &str = "客户端任务元数据摘要；不含路径、任务 ID、精确时间和原始诊断，不是扫描结果或清理/恢复凭证。引擎信息为生成报告时的当前版本，不代表历史任务使用的版本。";
const KINDS: &[&str] = &[
    "analyze",
    "status",
    "statuswatch",
    "apps",
    "cleanup",
    "clean",
    "history",
    "unknown",
];
const STATES: &[&str] = &[
    "queued",
    "running",
    "succeeded",
    "failed",
    "cancelled",
    "interrupted",
    "unknown",
];

// No free-form task text crosses the export boundary. Unknown values become a
// fixed token, rather than relying on heuristics to redact paths or secrets.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Report {
    schema: String,
    app_version: String,
    platform: String,
    architecture: String,
    current_engine_version: Option<String>,
    task_kind: String,
    task_state: String,
    directory_recorded: bool,
    diagnostic_recorded: bool,
    record_elapsed_ms: Option<u64>,
    note: String,
}

fn version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || matches!(c, b'v' | b'.' | b'-'))
}

pub fn prepare(task: &TaskRecord, engine_version: Option<String>) -> Result<String, String> {
    let report = Report {
        schema: SCHEMA.into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        platform: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        current_engine_version: engine_version.filter(|v| version(v)),
        task_kind: if KINDS.contains(&task.kind.as_str()) {
            task.kind.as_str()
        } else {
            "unknown"
        }
        .into(),
        task_state: if STATES.contains(&task.state.as_str()) {
            task.state.as_str()
        } else {
            "unknown"
        }
        .into(),
        directory_recorded: task.path.is_some(),
        diagnostic_recorded: task.message.is_some(),
        record_elapsed_ms: task.finished_at.and_then(|end| {
            if !(0..=8_640_000_000_000_000).contains(&task.created_at)
                || !(0..=8_640_000_000_000_000).contains(&end)
            {
                return None;
            }
            end.checked_sub(task.created_at)
                .and_then(|ms| u64::try_from(ms).ok())
        }),
        note: NOTE.into(),
    };
    encode(&report)
}

fn encode(report: &Report) -> Result<String, String> {
    serde_json::to_string_pretty(report)
        .map(|json| json + "\n")
        .map_err(|_| "无法生成诊断摘要。".into())
}

pub fn validate(json: &str) -> Result<(), String> {
    let invalid = || "报告格式无效，请重新生成预览。".to_string();
    if json.len() > 8192 {
        return Err(invalid());
    }
    let report: Report = serde_json::from_str(json).map_err(|_| invalid())?;
    if report.schema != SCHEMA
        || report.app_version != env!("CARGO_PKG_VERSION")
        || report.platform != std::env::consts::OS
        || report.architecture != std::env::consts::ARCH
        || report.note != NOTE
        || !KINDS.contains(&report.task_kind.as_str())
        || !STATES.contains(&report.task_state.as_str())
        || report
            .current_engine_version
            .as_deref()
            .is_some_and(|v| !version(v))
        || encode(&report)? != json
    {
        return Err(invalid());
    }
    Ok(())
}

pub fn save(path: &Path, json: &str) -> Result<(), String> {
    validate(json)?;
    if !path.is_absolute()
        || !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        return Err("请选择绝对路径下的 .json 文件。".into());
    }
    let parent = path.parent().ok_or("保存目录无效。")?;
    let mut file =
        tempfile::NamedTempFile::new_in(parent).map_err(|e| format!("无法创建报告：{e}"))?;
    file.write_all(json.as_bytes())
        .and_then(|_| file.as_file().sync_all())
        .map_err(|e| format!("报告写入失败：{e}"))?;
    file.persist_noclobber(path).map_err(|e| {
        if e.error.kind() == std::io::ErrorKind::AlreadyExists {
            "该位置已有文件，未覆盖。请使用新的文件名。".into()
        } else {
            format!("报告保存失败：{}", e.error)
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> TaskRecord {
        TaskRecord {
            id: "private-id".into(),
            kind: "analyze".into(),
            path: Some("/Users/private/secret\n[*]".into()),
            state: "failed".into(),
            created_at: 1000,
            finished_at: Some(2250),
            message: Some(
                "token=password@example.com https://private.invalid/?key=secret /Volumes/private"
                    .into(),
            ),
        }
    }
    #[test]
    fn report_omits_all_free_text_and_keeps_unknown_and_time_boundaries() {
        let mut task = record();
        let json = prepare(&task, Some("1.65.0".into())).unwrap();
        validate(&json).unwrap();
        for secret in [
            "private",
            "secret",
            "password",
            "example.com",
            "1000",
            "2250",
        ] {
            assert!(!json.contains(secret));
        }
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["recordElapsedMs"], 1250);
        task.kind = "private-kind".into();
        task.state = "private-state".into();
        task.finished_at = Some(999);
        let value: serde_json::Value =
            serde_json::from_str(&prepare(&task, Some("private-version".into())).unwrap()).unwrap();
        assert_eq!(value["taskKind"], "unknown");
        assert_eq!(value["taskState"], "unknown");
        assert!(value["recordElapsedMs"].is_null());
        assert!(value["currentEngineVersion"].is_null());
        task.finished_at = None;
        assert!(prepare(&task, None)
            .unwrap()
            .contains("\"recordElapsedMs\": null"));
        task.finished_at = Some(1000);
        assert!(prepare(&task, None)
            .unwrap()
            .contains("\"recordElapsedMs\": 0"));
        task.created_at = -1;
        assert!(prepare(&task, None)
            .unwrap()
            .contains("\"recordElapsedMs\": null"));
        task.created_at = i64::MAX - 1;
        task.finished_at = Some(i64::MAX);
        assert!(prepare(&task, None)
            .unwrap()
            .contains("\"recordElapsedMs\": null"));
    }
    #[test]
    fn export_rejects_extra_fields_and_modified_or_noncanonical_reports() {
        let json = prepare(&record(), None).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["path"] = "/Users/private".into();
        assert!(validate(&serde_json::to_string_pretty(&value).unwrap()).is_err());
        assert!(validate(&json.replace("failed", "private-state")).is_err());
        assert!(validate(&json.replace(NOTE, "private note")).is_err());
        assert!(validate(&json.repeat(20)).is_err());
        assert!(validate(json.trim_end()).is_err());
    }
    #[test]
    fn save_matches_preview_and_preserves_existing_files_links_and_private_permissions() {
        use std::os::unix::{fs::symlink, fs::PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("中文 空格 [*].json");
        let json = prepare(&record(), None).unwrap();
        save(&path, &json).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), json);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(save(&path, &json).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), json);
        let link = dir.path().join("link.json");
        symlink(&path, &link).unwrap();
        assert!(save(&link, &json).is_err());
        assert!(save(&dir.path().join("report.txt"), &json).is_err());
        assert!(save(&dir.path().join("missing/report.json"), &json).is_err());
        assert!(save(&dir.path().join("invalid.json"), "private").is_err());
        assert!(!dir.path().join("invalid.json").exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}
