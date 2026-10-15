use crate::engine::{self, Job};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::Path,
    sync::Arc,
    time::Duration,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleCheck {
    pub code: String,
    pub source: String,
    pub reason: String,
}

impl RuleCheck {
    fn from_code(code: &str) -> Result<Self, String> {
        let (source, reason) = match code {
            "passed" => (
                "Mole 路径级保护检查",
                "本次路径级检查未要求保留；不代表完整清理审计或执行授权。",
            ),
            "coreProtection" => ("Mole 核心保护", "路径受到 Mole 核心规则保护，应保留。"),
            "whitelist" => (
                "Mole 有效白名单",
                "路径与当前 Mole 用户或内置安全白名单匹配，应保留。",
            ),
            "compiledModel" => (
                "Mole 模型缓存保护",
                "路径为编译模型缓存或其直接父目录，应保留。",
            ),
            "activeOrUncertain" => (
                "Mole 活动缓存检查",
                "所属应用正在运行、路径被占用或无法确认空闲，应保留。",
            ),
            "sqliteInUse" => (
                "Mole SQLite 占用检查",
                "数据库或关联 WAL/SHM 文件正在使用，应保留。",
            ),
            "sqliteUnknown" => (
                "Mole SQLite 占用检查",
                "无法可靠确认数据库是否空闲，应保留。",
            ),
            "checkFailed" => ("Mole 保护检查", "路径级规则检查未可靠完成，不能视为通过。"),
            _ => return Err("Mole 规则返回了未知状态。".into()),
        };
        Ok(Self {
            code: code.into(),
            source: source.into(),
            reason: reason.into(),
        })
    }
}

pub fn parse(
    bytes: &[u8],
    requested: &[(String, String)],
) -> Result<HashMap<String, RuleCheck>, String> {
    let invalid = || "Mole 规则报告不完整或与请求不匹配。".to_string();
    if bytes.last() != Some(&0) {
        return Err(invalid());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut fields: Vec<_> = text.split_terminator('\0').collect();
    if fields.len() < 3
        || fields[0] != "MooMole.rules.v1"
        || fields[1] != engine::REVISION
        || fields.pop() != Some("complete")
        || (fields.len() - 2) % 3 != 0
    {
        return Err(invalid());
    }
    let expected: HashMap<_, _> = requested
        .iter()
        .map(|(id, path)| (id.as_str(), path.as_str()))
        .collect();
    if expected.len() != requested.len() {
        return Err(invalid());
    }
    let mut checks = HashMap::new();
    for row in fields[2..].chunks_exact(3) {
        if expected.get(row[0]) != Some(&row[1]) || checks.contains_key(row[0]) {
            return Err(invalid());
        }
        checks.insert(row[0].into(), RuleCheck::from_code(row[2])?);
    }
    if checks.len() != expected.len() {
        return Err(invalid());
    }
    Ok(checks)
}

pub async fn run(
    root: &Path,
    requested: &[(String, String)],
    job: Arc<Job>,
) -> Result<HashMap<String, RuleCheck>, String> {
    if requested.is_empty() || requested.len() > crate::review::MAX_SELECTION {
        return Err("规则复查范围无效。".into());
    }
    let mut seen = HashSet::new();
    for (id, path) in requested {
        if id.contains('\0')
            || path.contains('\0')
            || !Path::new(path).is_absolute()
            || !seen.insert(id)
        {
            return Err("规则复查项目无效。".into());
        }
    }
    let info = engine::info(root);
    if !info.available {
        return Err(info.reason.unwrap_or("引擎不可用。".into()));
    }
    let scratch = tempfile::Builder::new()
        .prefix("moomole-rules-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let request = scratch.path().join("request.nul");
    let report = scratch.path().join("report.nul");
    let mut fields = vec!["MooMole.rules.request.v1"];
    for (id, path) in requested {
        fields.extend([id.as_str(), path.as_str()]);
    }
    fields.push("complete");
    std::fs::write(&request, fields.join("\0") + "\0").map_err(|e| e.to_string())?;
    std::fs::File::create(&report).map_err(|e| e.to_string())?;
    let mut command = engine::command(Path::new("/bin/bash"));
    command
        .arg(root.join("moomole-review-rules.sh"))
        .arg(request)
        .arg(&report)
        .current_dir(root)
        .env("TMPDIR", scratch.path())
        .env_remove("MOLE_RESOLVED_TMPDIR")
        .env_remove("MOLE_TEMP_REGISTRY_FILE");
    engine::output(command, job, Duration::from_secs(45))
        .await
        .map_err(|e| format!("Mole 规则复查未完成：{e}"))?;
    let mut bytes = Vec::new();
    std::fs::File::open(report)
        .map_err(|e| e.to_string())?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("Mole 规则报告超过上限。".into());
    }
    parse(&bytes, requested)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rule_probe_cancellation_stops_its_process_group() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        std::fs::create_dir(root.join("bin")).unwrap();
        for name in ["analyze-go", "status-go"] {
            std::fs::write(root.join("bin").join(name), "fixture").unwrap();
        }
        std::fs::write(root.join("mole"), "VERSION=\"1.59.1\"\n").unwrap();
        std::fs::write(root.join("moomole-clean-preview.sh"), "fixture").unwrap();
        std::fs::write(root.join("moomole-review-rules.sh"), "sleep 30 & wait\n").unwrap();
        crate::engine_manifest::fixture(root);
        let job = Arc::new(Job::new(engine::TaskKind::Review));
        let cancel = job.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            cancel.cancel();
        });
        let error = tokio::time::timeout(
            Duration::from_secs(3),
            run(root, &[("id".into(), "/tmp/fixture".into())], job.clone()),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(error.contains("取消"));
        assert_eq!(job.pid.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
    fn report(fields: &[&str]) -> Vec<u8> {
        let mut all = vec!["MooMole.rules.v1", engine::REVISION];
        all.extend_from_slice(fields);
        all.push("complete");
        (all.join("\0") + "\0").into_bytes()
    }
    #[test]
    fn rules_are_bound_to_exact_ids_paths_and_complete_results() {
        let requested = vec![("id".into(), "/tmp/缓存\n[*]".into())];
        let valid = report(&["id", "/tmp/缓存\n[*]", "whitelist"]);
        assert_eq!(parse(&valid, &requested).unwrap()["id"].code, "whitelist");
        for n in 0..valid.len() {
            assert!(parse(&valid[..n], &requested).is_err());
        }
        for fields in [
            vec![],
            vec!["other", "/tmp/缓存\n[*]", "passed"],
            vec!["id", "/tmp/other", "passed"],
            vec!["id", "/tmp/缓存\n[*]", "unknown"],
            vec![
                "id",
                "/tmp/缓存\n[*]",
                "passed",
                "id",
                "/tmp/缓存\n[*]",
                "passed",
            ],
        ] {
            assert!(parse(&report(&fields), &requested).is_err());
        }
    }
}
