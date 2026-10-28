use super::{parse_report, Evidence, Report, SCHEMA, SKILL};
use crate::{engine::Job, process};
use serde::Serialize;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::process::Command;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Agent {
    pub id: String,
    name: String,
    path: Option<String>,
    supported: bool,
    note: String,
}
fn executable(path: &Path) -> bool {
    path.is_absolute()
        && path
            .metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
fn locate(id: &str, home: &Path) -> Option<PathBuf> {
    // GUI apps have a different PATH from Terminal. Never run a login shell or
    // search the working directory; include fixed user-managed CLI locations.
    let mut dirs = std::env::var_os("PATH")
        .map(|s| {
            std::env::split_paths(&s)
                .filter(|p| p.is_absolute())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    dirs.extend([
        home.join(".local/bin"),
        home.join(".cargo/bin"),
        home.join(".bun/bin"),
        home.join(".npm-global/bin"),
        home.join(".volta/bin"),
        home.join(".asdf/shims"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    if let Ok(entries) = std::fs::read_dir(home.join(".nvm/versions/node")) {
        let mut versions = entries
            .take(32)
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect::<Vec<_>>();
        versions.sort();
        dirs.extend(versions.into_iter().rev().take(8).map(|p| p.join("bin")));
    }
    let mut paths = dirs.into_iter().map(|p| p.join(id)).collect::<Vec<_>>();
    if id == "codex" {
        paths.extend([
            PathBuf::from("/Applications/Codex.app/Contents/Resources/codex"),
            PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex"),
        ]);
    }
    paths
        .into_iter()
        .find(|p| executable(p))
        .and_then(|p| p.canonicalize().ok())
}
pub(crate) fn detect(home: &Path) -> Vec<Agent> {
    [
        ("codex", "Codex CLI", true),
        ("claude", "Claude Code", true),
        ("gemini", "Gemini CLI", false),
        ("opencode", "OpenCode", false),
    ]
    .into_iter()
    .map(|(id, name, supported)| {
        let path = locate(id, home).map(|p| p.to_string_lossy().into_owned());
        let note = if path.is_none() {
            "未在常用位置发现 CLI；桌面应用不等于已安装命令行 Agent。"
        } else if !supported {
            "已发现；尚未适配受限调用，当前不能用于扫描。"
        } else if id == "claude" {
            "已发现；隔离 API 模式需要已配置的 API 环境凭据，不复用 Claude 订阅登录。开始时验证参数与调用。"
        } else {
            "已发现；开始时验证受限调用参数。登录、模型和额度需由实际调用确认。"
        };
        Agent {
            id: id.into(),
            name: name.into(),
            path,
            supported,
            note: note.into(),
        }
    })
    .collect()
}
fn required_flags(id: &str) -> Result<&'static [&'static str], String> {
    match id {
        "codex" => Ok(&[
            "--ignore-user-config",
            "--ephemeral",
            "--output-schema",
            "--sandbox",
        ]),
        "claude" => Ok(&[
            "--bare",
            "--tools",
            "--strict-mcp-config",
            "--json-schema",
            "--no-session-persistence",
            "--setting-sources",
        ]),
        _ => Err("该 Agent 尚未适配受限扫描。".into()),
    }
}
pub(crate) async fn prepare(
    id: &str,
    home: &Path,
    cwd: &Path,
    job: Arc<Job>,
) -> Result<PathBuf, String> {
    let required = required_flags(id)?;
    let path = locate(id, home)
        .ok_or("未找到该 Agent 的可执行 CLI。请先安装并在终端登录，再重新检测。")?;
    let mut help = Command::new(&path);
    if id == "codex" {
        help.arg("exec");
    }
    help.arg("--help").current_dir(cwd);
    let bytes = process::output_bounded(help, job, Duration::from_secs(10), 128 * 1024).await?;
    let text = String::from_utf8_lossy(&bytes);
    if required.iter().any(|flag| !text.contains(flag)) {
        return Err("Agent 版本缺少受限扫描参数，请升级 CLI 后重试；未调用模型。".into());
    }
    Ok(path)
}
fn command(id: &str, path: &Path, cwd: &Path) -> Result<Command, String> {
    let mut cmd = Command::new(path);
    cmd.current_dir(cwd);
    match id {
        "codex" => {
            cmd.args([
                "exec",
                "--ignore-user-config",
                "--ignore-rules",
                "--ephemeral",
                "--skip-git-repo-check",
                "--sandbox",
                "read-only",
                "--color",
                "never",
                "--output-schema",
            ])
            .arg(cwd.join("output-schema.json"))
            .args(["--output-last-message"])
            .arg(cwd.join("report.json"));
            // Disable local customizations/tools, including shell, hooks, MCP/apps
            // and plugins. The read-only sandbox remains a second write barrier.
            for feature in [
                "shell_tool",
                "unified_exec",
                "hooks",
                "plugins",
                "apps",
                "multi_agent",
                "browser_use",
                "computer_use",
                "image_generation",
                "in_app_browser",
                "view_image",
                "code_mode",
                "code_mode_host",
                "artifact",
                "memories",
                "skill_search",
                "skill_mcp_dependency_install",
            ] {
                cmd.arg("--disable").arg(feature);
            }
            cmd.args([
                "-c",
                "web_search=\"disabled\"",
                "-c",
                "mcp_servers={}",
                "-c",
                "project_doc_max_bytes=0",
                "-",
            ]);
        }
        "claude" => {
            cmd.args([
                "--bare",
                "--print",
                "--tools",
                "",
                "--disallowedTools",
                "mcp__*",
                "--strict-mcp-config",
                "--mcp-config",
                "{\"mcpServers\":{}}",
                "--setting-sources",
                "",
                "--no-session-persistence",
                "--output-format",
                "json",
                "--json-schema",
                SCHEMA,
            ]);
        }
        _ => return Err("该 Agent 尚未适配受限扫描。".into()),
    }
    Ok(cmd)
}
pub(crate) async fn analyze(
    id: &str,
    path: &Path,
    cwd: &Path,
    evidence: &[Evidence],
    limitations: &[String],
    language: &str,
    job: Arc<Job>,
) -> Result<Report, String> {
    std::fs::write(cwd.join("output-schema.json"), SCHEMA).map_err(|_| "无法准备 AI 输出协议。")?;
    std::fs::write(cwd.join("SKILL.md"), SKILL).map_err(|_| "无法准备内置 Skill。")?;
    let response_language =
        match crate::language::resolve(language, &crate::language::system_language()) {
            "zh" => "Simplified Chinese",
            "ja" => "Japanese",
            _ => "English",
        };
    let prompt = format!(
        "{SKILL}\n\nApp-requested response language: {response_language}\n\nOutput JSON schema:\n{SCHEMA}\n\nUntrusted inventory JSON:\n{}",
        serde_json::json!({"evidence":evidence,"limitations":limitations})
    );
    if prompt.len() > 256 * 1024 {
        return Err("本次元数据超过发送上限，请缩小范围。".into());
    }
    let bytes = process::output_with_input(
        command(id, path, cwd)?,
        job,
        prompt.into_bytes(),
        Duration::from_secs(180),
        1024 * 1024,
    )
    .await
    .map_err(|error| format!("Agent 调用未完成。请检查 CLI 登录、网络、模型与额度。{error}"))?;
    let raw = if id == "codex" {
        use std::io::Read;
        let mut raw = vec![];
        std::fs::File::open(cwd.join("report.json"))
            .map_err(|_| "Agent 没有生成报告。")?
            .take(256 * 1024 + 1)
            .read_to_end(&mut raw)
            .map_err(|_| "无法读取 AI 报告。")?;
        raw
    } else {
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "Agent 返回格式无法识别。")?;
        if value["is_error"].as_bool() == Some(true) {
            return Err("Agent 返回错误，请检查终端登录、模型与额度。".into());
        }
        serde_json::to_vec(
            value
                .get("structured_output")
                .ok_or("Agent 未返回结构化建议。")?,
        )
        .map_err(|e| e.to_string())?
    };
    if raw.len() > 256 * 1024 {
        return Err("AI 报告超过上限。".into());
    }
    parse_report(&raw, evidence)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "uses an installed authenticated Codex CLI and model quota; sends synthetic metadata only"]
    async fn live_codex_metadata_roundtrip() {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
        let scratch = tempfile::Builder::new()
            .prefix("moomole-ai-live-")
            .tempdir()
            .unwrap();
        let job = Arc::new(Job::new(crate::engine::TaskKind::AiScan));
        let executable = prepare("codex", &home, scratch.path(), job.clone())
            .await
            .unwrap();
        let evidence = vec![Evidence {
            id: "e0".into(),
            source: "metadata".into(),
            path: "/Users/synthetic/Downloads/archive.zip".into(),
            size: Some(1024),
            modified_at: None,
            is_directory: false,
            category: "file".into(),
            candidate_id: None,
        }];
        let report = analyze(
            "codex",
            &executable,
            scratch.path(),
            &evidence,
            &["Synthetic acceptance sample. File contents and backup status are unknown.".into()],
            "en",
            job,
        )
        .await
        .unwrap();
        assert!(!report.summary.is_empty());
        assert!(report
            .findings
            .iter()
            .all(|f| f.recommendation != super::super::Recommendation::Review));
        println!(
            "Live Codex synthetic roundtrip: {} evidence item, {} valid findings",
            evidence.len(),
            report.findings.len()
        );
    }
    #[test]
    fn adapters_do_not_accept_shell_or_mutation_flags() {
        let cmd = command(
            "codex",
            Path::new("/test/codex"),
            Path::new("/test/private"),
        )
        .unwrap();
        let args = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.windows(2).any(|a| a == ["--sandbox", "read-only"]));
        assert!(args.iter().any(|a| a == "--ignore-user-config"));
        assert!(!args
            .iter()
            .any(|a| a.contains("bypass") || a == "--approve-for-me"));
        let cmd = command(
            "claude",
            Path::new("/test/claude"),
            Path::new("/test/private"),
        )
        .unwrap();
        let args = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.windows(2).any(|a| a == ["--tools", ""]));
        assert!(args.iter().any(|a| a == "--bare"));
        assert!(command("unknown", Path::new("/test/x"), Path::new("/test")).is_err());
    }
}
