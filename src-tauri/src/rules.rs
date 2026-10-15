use crate::engine::{self, Job};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::{Component, Path},
    sync::Arc,
    time::Duration,
};

pub const PROTOCOL: u64 = 2;
pub const MODULE_VERSION: u64 = 1;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleCheck {
    pub code: String,
    pub source: String,
    pub reason: String,
    pub module: ModuleCheck,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleCheck {
    pub version: u64,
    pub scope: String,
    pub code: String,
    pub source: String,
    pub reason: String,
}

impl ModuleCheck {
    pub fn from_codes(scope: &str, code: &str) -> Result<Self, String> {
        let source = match scope {
            "userCache" => "Mole clean_user_essentials / _user_cache_deno_delete_guard",
            "userLogs" => "Mole _clean_user_log_directory / _user_log_delete_guard",
            "unsupported" => "尚未适配的模块范围",
            _ => return Err("Mole 返回未知模块范围。".into()),
        };
        let reason = match (scope, code) {
            (_, "notChecked") => "路径级检查未满足条件，未继续模块检查。",
            ("unsupported", "unsupported") => "此路径的模块规则尚未适配，路径检查通过仍需保留审阅。",
            ("userCache", "checked") => "已复查当前 Deno 数据根目录的保留约束，未要求保留此项；其他模块条件仍需单独核对。",
            ("userLogs", "checked") => "已复查用户日志根目录及适用的诊断报告目录身份；不代表日志未被使用或其他模块条件已满足。",
            ("userLogs", "preserveRoot") => "Mole 保留 DiagnosticReports 目录本身，只检查其中的单个报告；请重新选择具体项目。",
            ("userCache"|"userLogs", "retainOrUnknown") => "模块要求保留或无法确认当前条件；不能继续处理此项。",
            ("userCache"|"userLogs", "checkFailed") => "模块检查未可靠完成；缺少函数、身份读取失败或异常返回均不能视为通过。",
            _ => return Err("Mole 返回未知或不匹配的模块状态。".into()),
        };
        Ok(Self {
            version: MODULE_VERSION,
            scope: scope.into(),
            code: code.into(),
            source: source.into(),
            reason: reason.into(),
        })
    }
}

// Deliberately mirrors only the exact non-hidden entries covered by the two
// upstream module globs. This is coverage selection, never cleanup eligibility.
fn module_scope(path: &Path, home: &Path) -> &'static str {
    if path
        .file_name()
        .and_then(|p| p.to_str())
        .is_none_or(|p| p.starts_with('.'))
    {
        return "unsupported";
    }
    let parent = path.parent();
    if parent == Some(home.join("Library/Caches").as_path()) {
        "userCache"
    } else if parent == Some(home.join("Library/Logs").as_path())
        || parent == Some(home.join("Library/Logs/DiagnosticReports").as_path())
    {
        "userLogs"
    } else {
        "unsupported"
    }
}

impl RuleCheck {
    fn from_code(code: &str, module: ModuleCheck) -> Result<Self, String> {
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
            module,
        })
    }
}

pub fn parse(
    bytes: &[u8],
    requested: &[(String, String)],
    home: &Path,
) -> Result<HashMap<String, RuleCheck>, String> {
    let invalid = || "Mole 规则报告不完整或与请求不匹配。".to_string();
    if bytes.last() != Some(&0) {
        return Err(invalid());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut fields: Vec<_> = text.split_terminator('\0').collect();
    if fields.len() < 5
        || fields[0] != "MooMole.rules.v2"
        || fields[1] != engine::REVISION
        || fields[2] != MODULE_VERSION.to_string()
        || Some(fields[3]) != home.to_str()
        || fields.pop() != Some("complete")
        || (fields.len() - 4) % 5 != 0
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
    for row in fields[4..].chunks_exact(5) {
        if expected.get(row[0]) != Some(&row[1]) || checks.contains_key(row[0]) {
            return Err(invalid());
        }
        let scope = module_scope(Path::new(row[1]), home);
        if row[3] != scope
            || (row[2] != "passed") != (row[4] == "notChecked")
            || (row[4] == "preserveRoot"
                && Path::new(row[1]) != home.join("Library/Logs/DiagnosticReports"))
        {
            return Err(invalid());
        }
        checks.insert(
            row[0].into(),
            RuleCheck::from_code(row[2], ModuleCheck::from_codes(row[3], row[4])?)?,
        );
    }
    if checks.len() != expected.len() {
        return Err(invalid());
    }
    Ok(checks)
}

pub async fn run(
    root: &Path,
    home: &Path,
    requested: &[(String, String)],
    job: Arc<Job>,
) -> Result<HashMap<String, RuleCheck>, String> {
    if !home.is_absolute()
        || home.to_str().is_none()
        || requested.is_empty()
        || requested.len() > crate::review::MAX_SELECTION
    {
        return Err("规则复查范围无效。".into());
    }
    let mut seen = HashSet::new();
    for (id, path) in requested {
        if id.is_empty()
            || id == "complete"
            || id.contains('\0')
            || path.contains('\0')
            || !Path::new(path).is_absolute()
            || Path::new(path)
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
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
    let mut fields = vec!["MooMole.rules.request.v2"];
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
        .env("HOME", home)
        .env_remove("SUDO_USER")
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
    parse(&bytes, requested, home)
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
            run(
                root,
                root,
                &[("id".into(), "/tmp/fixture".into())],
                job.clone(),
            ),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(error.contains("取消"));
        assert_eq!(job.pid.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
    fn report(fields: &[&str]) -> Vec<u8> {
        let mut all = vec!["MooMole.rules.v2", engine::REVISION, "1", "/Users/fixture"];
        all.extend_from_slice(fields);
        all.push("complete");
        (all.join("\0") + "\0").into_bytes()
    }
    #[test]
    fn rules_bind_paths_home_module_scope_version_and_complete_results() {
        let home = Path::new("/Users/fixture");
        let requested = vec![("id".into(), "/tmp/缓存\n[*]".into())];
        let valid = report(&[
            "id",
            "/tmp/缓存\n[*]",
            "whitelist",
            "unsupported",
            "notChecked",
        ]);
        assert_eq!(
            parse(&valid, &requested, home).unwrap()["id"].code,
            "whitelist"
        );
        for n in 0..valid.len() {
            assert!(parse(&valid[..n], &requested, home).is_err());
        }
        for fields in [
            vec![],
            vec![
                "other",
                "/tmp/缓存\n[*]",
                "passed",
                "unsupported",
                "unsupported",
            ],
            vec!["id", "/tmp/other", "passed", "unsupported", "unsupported"],
            vec![
                "id",
                "/tmp/缓存\n[*]",
                "unknown",
                "unsupported",
                "notChecked",
            ],
            vec!["id", "/tmp/缓存\n[*]", "passed", "userCache", "checked"],
            vec!["id", "/tmp/缓存\n[*]", "passed", "unsupported", "checked"],
            vec![
                "id",
                "/tmp/缓存\n[*]",
                "whitelist",
                "unsupported",
                "unsupported",
            ],
            vec![
                "id",
                "/tmp/缓存\n[*]",
                "passed",
                "unsupported",
                "unsupported",
                "id",
                "/tmp/缓存\n[*]",
                "passed",
                "unsupported",
                "unsupported",
            ],
        ] {
            assert!(parse(&report(&fields), &requested, home).is_err());
        }
        assert!(parse(&valid, &requested, Path::new("/Users/other")).is_err());
        for old in ["MooMole.rules.v1", "MooMole.rules.v3"] {
            assert!(parse(
                &String::from_utf8(valid.clone())
                    .unwrap()
                    .replace("MooMole.rules.v2", old)
                    .into_bytes(),
                &requested,
                home
            )
            .is_err());
        }
        assert!(parse(
            &String::from_utf8(valid)
                .unwrap()
                .replace("\x001\0", "\x009\0")
                .into_bytes(),
            &requested,
            home
        )
        .is_err());
    }
    #[test]
    fn module_coverage_is_exact_and_unknown_or_inconsistent_states_are_refused() {
        let home = Path::new("/Users/fixture");
        for (path, scope, code) in [
            ("Library/Caches/app", "userCache", "checked"),
            ("Library/Caches/deno", "userCache", "retainOrUnknown"),
            ("Library/Logs/app.log", "userLogs", "checked"),
            ("Library/Logs/DiagnosticReports", "userLogs", "preserveRoot"),
            (
                "Library/Logs/DiagnosticReports/report.ips",
                "userLogs",
                "checked",
            ),
            ("Library/Logs/deep/other", "unsupported", "unsupported"),
            ("Library/Caches/.hidden", "unsupported", "unsupported"),
            ("Library/CachesOther/app", "unsupported", "unsupported"),
            ("Library/Caches/app/child", "unsupported", "unsupported"),
            (
                "Library/Logs/DiagnosticReports/.hidden",
                "unsupported",
                "unsupported",
            ),
        ] {
            let path = home.join(path).to_str().unwrap().to_owned();
            let request = vec![("id".into(), path.clone())];
            let parsed = parse(
                &report(&["id", &path, "passed", scope, code]),
                &request,
                home,
            )
            .unwrap();
            assert_eq!(parsed["id"].module.code, code);
            assert_eq!(parsed["id"].module.version, MODULE_VERSION);
        }
        let path = "/Users/fixture/Library/Logs/app.log";
        for (scope, code) in [
            ("userLogs", "preserveRoot"),
            ("userLogs", "unsupported"),
            ("userLogs", "unknown"),
            ("unknown", "checked"),
        ] {
            assert!(parse(
                &report(&["id", path, "passed", scope, code]),
                &[("id".into(), path.into())],
                home
            )
            .is_err());
        }
    }
}
