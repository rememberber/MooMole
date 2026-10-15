use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};

pub const REVISION: &str = "ba3a0f57f78e4c22818fda701219cc3dd4d6953f";
const OUTPUT_LIMIT: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskKind {
    Analyze,
    Status,
    Apps,
    Cleanup,
    Clean,
    Review,
}

impl TaskKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Analyze => "analyze",
            Self::Status => "status",
            Self::Apps => "apps",
            Self::Cleanup => "cleanup",
            Self::Clean => "clean",
            Self::Review => "review",
        }
    }
}

pub struct Job {
    pub kind: TaskKind,
    pub cancelled: AtomicBool,
    pub pid: AtomicU32,
}

impl Job {
    pub fn new(kind: TaskKind) -> Self {
        Self {
            kind,
            cancelled: AtomicBool::new(false),
            pid: AtomicU32::new(0),
        }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
    pub fn kill_group(&self) {
        let pid = self.pid.swap(0, Ordering::SeqCst);
        if pid > 0 {
            // Each child is started in its own process group. Never signal the app's group.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}

struct ProcessGuard(Arc<Job>);
impl Drop for ProcessGuard {
    fn drop(&mut self) {
        self.0.kill_group();
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub available: bool,
    pub revision: String,
    pub reason: Option<String>,
}

pub fn info(root: &Path) -> EngineInfo {
    let checked = (|| -> Result<(), String> {
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(root.join("UPSTREAM.json"))
                .map_err(|_| "尚未准备 Mole 引擎，请在项目中运行 npm run engine:prepare。")?,
        )
        .map_err(|e| e.to_string())?;
        if manifest["revision"] != REVISION {
            return Err("引擎版本与应用不匹配，请重新准备引擎。".into());
        }
        if manifest["cleanProtocol"] != crate::clean_preview::PROTOCOL
            || !root.join("moomole-clean-preview.sh").is_file()
        {
            return Err("清理适配协议未准备，请重新运行 npm run engine:prepare。".into());
        }
        if manifest["rulesProtocol"] != 1 || !root.join("moomole-review-rules.sh").is_file() {
            return Err("Mole 规则适配未准备，请重新运行 npm run engine:prepare。".into());
        }
        if manifest["arch"]
            != std::env::consts::ARCH
                .replace("aarch64", "arm64")
                .replace("x86_64", "x64")
        {
            return Err("引擎架构与当前应用不匹配。".into());
        }
        for binary in ["analyze-go", "status-go"] {
            let metadata = std::fs::metadata(root.join("bin").join(binary))
                .map_err(|_| format!("缺少 {binary}，请重新准备引擎。"))?;
            if !metadata.is_file() {
                return Err("引擎文件无效。".into());
            }
        }
        Ok(())
    })();
    EngineInfo {
        available: checked.is_ok(),
        revision: REVISION.into(),
        reason: checked.err(),
    }
}

pub fn directory(raw: &str, home: &Path) -> Result<PathBuf, String> {
    let expanded = if raw == "~" {
        home.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(raw)
    };
    if !expanded.is_absolute() {
        return Err("请选择绝对目录路径。".into());
    }
    let canonical = expanded
        .canonicalize()
        .map_err(|e| format!("无法访问该目录：{e}"))?;
    if !canonical.is_dir() {
        return Err("请选择文件夹。".into());
    }
    Ok(canonical)
}

pub fn protection_paths(
    requested: &[String],
    previous: &[String],
    home: &Path,
) -> Result<Vec<String>, String> {
    let mut paths = requested
        .iter()
        .map(|path| {
            // Already saved canonical rules retain their intent when a disk is
            // disconnected or a directory disappears. New rules must be accessible.
            if previous.contains(path) && Path::new(path).is_absolute() {
                Ok(path.clone())
            } else {
                directory(path, home).map(|p| p.to_string_lossy().into_owned())
            }
        })
        .collect::<Result<Vec<_>, String>>()?;
    paths.sort();
    paths.dedup();
    Ok(paths)
}

pub async fn output(
    mut command: Command,
    job: Arc<Job>,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    if job.cancelled.load(Ordering::SeqCst) {
        return Err("任务已取消。".into());
    }
    use std::os::unix::process::CommandExt;
    command.as_std_mut().process_group(0);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|e| format!("无法启动 Mole：{e}"))?;
    job.pid
        .store(child.id().ok_or("无法取得任务进程。")?, Ordering::SeqCst);
    let guard = ProcessGuard(job.clone());
    let stdout = child.stdout.take().ok_or("无法读取任务输出。")?;
    let stderr = child.stderr.take().ok_or("无法读取任务诊断。")?;
    let out = tokio::spawn(async move {
        let mut data = vec![];
        stdout
            .take(OUTPUT_LIMIT + 1)
            .read_to_end(&mut data)
            .await
            .map(|_| data)
    });
    let err = tokio::spawn(async move {
        let mut data = vec![];
        stderr
            .take(OUTPUT_LIMIT + 1)
            .read_to_end(&mut data)
            .await
            .map(|_| data)
    });
    let started = tokio::time::Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut interrupted = None;
    let status = loop {
        tokio::select! {
            status = child.wait() => break status,
            _ = tick.tick() => {
                if job.cancelled.load(Ordering::SeqCst) { interrupted = Some("任务已取消。"); }
                else if started.elapsed() >= timeout { interrupted = Some("任务超时，已停止进程；可缩小扫描范围后重试。"); }
                if interrupted.is_some() { job.kill_group(); break child.wait().await; }
            }
        }
    };
    drop(guard);
    let stdout = out
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let stderr = err
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    if let Some(message) = interrupted {
        return Err(message.into());
    }
    if stdout.len() as u64 > OUTPUT_LIMIT || stderr.len() as u64 > OUTPUT_LIMIT {
        return Err("任务输出超过上限，请缩小扫描目录。".into());
    }
    if !status.map_err(|e| e.to_string())?.success() {
        let source = if stderr.is_empty() { &stdout } else { &stderr };
        let diagnostic: String = plain_terminal_text(&String::from_utf8_lossy(source))
            .chars()
            .rev()
            .take(1600)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        return Err(if diagnostic.trim().is_empty() {
            "Mole 未能完成任务。".into()
        } else {
            diagnostic
        });
    }
    Ok(stdout)
}

pub(crate) fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .env(
            "PATH",
            "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin",
        )
        .env("MOLE_TEST_NO_AUTH", "1")
        .env("MO_NO_OPLOG", "1");
    command
}

pub async fn run(
    root: &Path,
    home: &Path,
    job: Arc<Job>,
    path: Option<PathBuf>,
    protected: &[String],
) -> Result<Value, String> {
    if !info(root).available {
        return Err(info(root).reason.unwrap_or("引擎不可用。".into()));
    }
    if job.kind == TaskKind::Cleanup {
        return inspect_cleanup(root, home, job, protected).await;
    }
    // A new private directory for each run prevents stale or shared preview files.
    let scratch = if job.kind == TaskKind::Clean {
        Some(
            tempfile::Builder::new()
                .prefix("moomole-preview-")
                .tempdir()
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    let report = scratch.as_ref().map(|d| d.path().join("report.nul"));
    if let Some(report) = &report {
        std::fs::File::create(report).map_err(|e| e.to_string())?;
    }
    let mut command = match job.kind {
        TaskKind::Analyze => {
            let mut command = command(&root.join("bin/analyze-go"));
            command.arg("--json").arg(path.ok_or("缺少扫描目录。")?);
            command
        }
        TaskKind::Status => {
            let mut command = command(&root.join("bin/status-go"));
            command.arg("--json");
            command
        }
        TaskKind::Apps => {
            let mut command = command(Path::new("/bin/bash"));
            command.arg(root.join("bin/uninstall.sh")).arg("--list");
            command
        }
        TaskKind::Clean => {
            let mut command = command(Path::new("/bin/bash"));
            command
                .arg(root.join("moomole-clean-preview.sh"))
                .arg(report.as_ref().ok_or("缺少清理预览工作目录。")?)
                .env(
                    "TMPDIR",
                    scratch.as_ref().ok_or("缺少清理预览工作目录。")?.path(),
                )
                .env_remove("MOLE_RESOLVED_TMPDIR")
                .env_remove("MOLE_TEMP_REGISTRY_FILE")
                .env("MOLE_DRY_RUN", "1")
                .env("NO_COLOR", "1")
                .env("TERM", "dumb");
            command
        }
        TaskKind::Cleanup | TaskKind::Review => return Err("请使用对应的专用任务入口。".into()),
    };
    command.current_dir(root);
    let bytes = output(
        command,
        job.clone(),
        Duration::from_secs(if job.kind == TaskKind::Status {
            45
        } else if job.kind == TaskKind::Clean {
            300
        } else {
            180
        }),
    )
    .await?;
    if job.kind == TaskKind::Clean {
        let text = String::from_utf8(bytes).map_err(|_| "Mole 清理预览返回了无法解码的文本。")?;
        let report = report.ok_or("缺少清理预览记录。")?;
        let mut data = Vec::new();
        use std::io::Read;
        std::fs::File::open(report)
            .map_err(|e| e.to_string())?
            .take(OUTPUT_LIMIT + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        if data.len() as u64 > OUTPUT_LIMIT {
            return Err("清理预览超过大小上限。".into());
        }
        let mut preview = crate::clean_preview::parse(&data)?;
        for item in &mut preview.items {
            item.impact = Some(crate::impact::describe(Path::new(&item.path), home));
        }
        preview.output = plain_terminal_text(&text);
        return serde_json::to_value(preview).map_err(|e| e.to_string());
    }
    let result: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("Mole 输出格式无法识别：{e}"))?;
    let valid = match job.kind {
        TaskKind::Analyze => {
            result["entries"].is_array()
                && ["complete", "partial", "unavailable"]
                    .contains(&result["scan_status"].as_str().unwrap_or(""))
        }
        TaskKind::Apps => result.is_array(),
        TaskKind::Status => result["collected_at"].is_string(),
        TaskKind::Cleanup => false,
        TaskKind::Clean => false,
        TaskKind::Review => false,
    };
    if !valid {
        return Err("Mole 数据与当前协议不匹配，请核对引擎版本。".into());
    }
    Ok(result)
}

fn plain_terminal_text(text: &str) -> String {
    let mut output = String::new();
    let mut state = 0;
    for c in text.chars() {
        match state {
            0 if c == '\u{1b}' => state = 1,
            0 if !c.is_control() || c == '\n' || c == '\t' => output.push(c),
            1 => {
                state = if c == '[' {
                    2
                } else if c == ']' {
                    3
                } else {
                    0
                }
            }
            2 if ('@'..='~').contains(&c) => state = 0,
            3 if c == '\u{7}' => state = 0,
            3 if c == '\u{1b}' => state = 4,
            4 => state = if c == '\\' { 0 } else { 3 },
            _ => (),
        }
    }
    output
}

async fn inspect_cleanup(
    root: &Path,
    home: &Path,
    job: Arc<Job>,
    protected: &[String],
) -> Result<Value, String> {
    let rules = [
        (
            "xcode",
            "Xcode 构建缓存",
            "Library/Developer/Xcode/DerivedData",
            "由 Xcode 重新生成，后续构建可能变慢。",
        ),
        (
            "npm",
            "npm 下载缓存",
            ".npm/_cacache",
            "包下载缓存，后续安装可能需要重新联网下载。",
        ),
        (
            "homebrew",
            "Homebrew 下载缓存",
            "Library/Caches/Homebrew",
            "安装包缓存，后续安装可能需要重新下载。",
        ),
    ];
    let mut items = vec![];
    for (id, name, relative, impact) in rules {
        if job.cancelled.load(Ordering::SeqCst) {
            return Err("任务已取消。".into());
        }
        let path = home.join(relative);
        let blocked = protected
            .iter()
            .any(|rule| path.starts_with(rule) || Path::new(rule).starts_with(&path));
        let mut item = json!({"id": id, "name": name, "path": path, "impact": impact, "size": null, "scanStatus": "unavailable", "protected": blocked, "reason": null});
        if blocked {
            item["reason"] = json!("此目录或其内容位于保护列表，本次跳过检查。");
        } else if !path.exists() {
            item["reason"] = json!("未发现此目录。");
        } else if std::fs::symlink_metadata(&path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(true)
        {
            item["reason"] = json!("目录为符号链接或无法确认身份，已跳过。");
        } else {
            let mut command = command(&root.join("bin/analyze-go"));
            command.arg("--json").arg(&path);
            match output(command, job.clone(), Duration::from_secs(90)).await {
                Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                    Ok(result)
                        if result["total_size"].is_u64() && result["scan_status"].is_string() =>
                    {
                        item["size"] = result["total_size"].clone();
                        item["scanStatus"] = result["scan_status"].clone();
                    }
                    _ => item["reason"] = json!("引擎返回了无法识别的扫描结果。"),
                },
                Err(error) => {
                    if job.cancelled.load(Ordering::SeqCst) {
                        return Err(error);
                    }
                    item["reason"] = json!(error);
                }
            }
        }
        items.push(item);
    }
    Ok(json!({ "items": items, "readOnly": true }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_saved_protection_survives_unrelated_settings_changes() {
        let scratch = tempfile::tempdir().unwrap();
        let missing = scratch
            .path()
            .join("disconnected")
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(
            protection_paths(&[missing.clone()], &[missing.clone()], scratch.path()).unwrap(),
            vec![missing.clone()]
        );
        assert!(protection_paths(&[missing], &[], scratch.path()).is_err());
    }
    #[tokio::test]
    #[ignore = "Explicit local read-only smoke check; requires prepared Mole engine"]
    async fn real_clean_preview_smoke() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
        let result = run(&root, &home, Arc::new(Job::new(TaskKind::Clean)), None, &[])
            .await
            .unwrap();
        assert_eq!(result["dryRun"], true);
        assert_eq!(result["protocol"], crate::clean_preview::PROTOCOL);
        assert!(result["items"].is_array());
        println!(
            "Real Mole preview: {} candidate paths, structured protocol accepted.",
            result["items"].as_array().unwrap().len()
        );
    }
    #[test]
    fn terminal_preview_keeps_unicode_and_strips_control_sequences() {
        assert_eq!(
            plain_terminal_text("\u{1b}[32m缓存\u{1b}[0m\n\u{1b}]0;title\u{7}预览\r"),
            "缓存\n预览"
        );
    }
    #[test]
    fn path_validation_rejects_relative_paths_and_files() {
        let directory = tempfile::tempdir().unwrap();
        assert!(super::directory("relative", directory.path()).is_err());
        let file = directory.path().join("file");
        std::fs::write(&file, "fixture").unwrap();
        assert!(super::directory(file.to_str().unwrap(), directory.path()).is_err());
        assert_eq!(
            super::directory("~", directory.path()).unwrap(),
            directory.path().canonicalize().unwrap()
        );
    }

    #[tokio::test]
    async fn failed_shell_commands_preserve_errors_written_to_stdout() {
        let job = Arc::new(Job::new(TaskKind::Clean));
        let mut command = Command::new("/bin/sh");
        command.args(["-c", r"printf '\033[31mMissing dependency\033[0m'; exit 1"]);
        let error = output(command, job, Duration::from_secs(3))
            .await
            .unwrap_err();
        assert_eq!(error, "Missing dependency");
    }

    #[tokio::test]
    async fn cancellation_stops_the_process_group() {
        let job = Arc::new(Job::new(TaskKind::Analyze));
        let cancel = job.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            cancel.cancel();
        });
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 30 & wait"]);
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            output(command, job.clone(), Duration::from_secs(60)),
        )
        .await
        .unwrap();
        assert!(result.unwrap_err().contains("取消"));
        assert_eq!(job.pid.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn deadline_stops_a_nonresponsive_process() {
        let job = Arc::new(Job::new(TaskKind::Status));
        let mut command = Command::new("/bin/sleep");
        command.arg("30");
        let error = output(command, job.clone(), Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(error.contains("超时"));
        assert_eq!(job.pid.load(Ordering::SeqCst), 0);
    }
}
