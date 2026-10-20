use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tokio::process::Command;

pub const REVISION: &str = "ba3a0f57f78e4c22818fda701219cc3dd4d6953f";
pub(crate) const OUTPUT_LIMIT: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskKind {
    Analyze,
    Status,
    Apps,
    Cleanup,
    Clean,
    Review,
    History,
    LogTrash,
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
            Self::History => "history",
            Self::LogTrash => "logtrash",
        }
    }
}

pub struct Job {
    pub kind: TaskKind,
    pub cancelled: AtomicBool,
    process: Mutex<Option<OwnedProcess>>,
    execution_run: OnceLock<String>,
}
struct OwnedProcess {
    pid: u32,
    lease: Arc<()>,
}

impl Job {
    pub fn new(kind: TaskKind) -> Self {
        Self {
            kind,
            cancelled: AtomicBool::new(false),
            process: Mutex::new(None),
            execution_run: OnceLock::new(),
        }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
    pub(crate) fn bind_execution(&self, run: &str) -> Result<(), String> {
        if self.kind != TaskKind::LogTrash {
            return Err("此任务不能绑定日志执行批次。".into());
        }
        self.execution_run
            .set(run.to_string())
            .map_err(|_| "任务已绑定执行批次。".into())
    }
    pub(crate) fn cancel_execution(&self, run: &str) -> bool {
        if self.execution_run.get().is_some_and(|id| id == run) {
            self.cancel();
            true
        } else {
            false
        }
    }
    pub fn kill_group(&self) {
        let mut current = self.process.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(process) = current.take() {
            // The runner cannot reap until it has acquired this same lock and
            // released its registration. Signal while holding that ownership.
            unsafe {
                libc::kill(-(process.pid as i32), libc::SIGKILL);
            }
        }
    }
    pub(crate) fn register(&self, pid: u32) -> Result<Arc<()>, String> {
        let mut current = self.process.lock().unwrap_or_else(|e| e.into_inner());
        if current.is_some() {
            return Err("同一任务已有运行中的进程。".into());
        }
        let lease = Arc::new(());
        *current = Some(OwnedProcess {
            pid,
            lease: lease.clone(),
        });
        Ok(lease)
    }
    pub(crate) fn signal(&self, pid: u32, lease: &Arc<()>, signal: i32) {
        let current = self.process.lock().unwrap_or_else(|e| e.into_inner());
        if current
            .as_ref()
            .is_some_and(|p| p.pid == pid && Arc::ptr_eq(&p.lease, lease))
        {
            unsafe {
                libc::kill(-(pid as i32), signal);
            }
        }
    }
    pub(crate) fn release(&self, pid: u32, lease: &Arc<()>, signal: Option<i32>) {
        let mut current = self.process.lock().unwrap_or_else(|e| e.into_inner());
        if current
            .as_ref()
            .is_some_and(|p| p.pid == pid && Arc::ptr_eq(&p.lease, lease))
        {
            if let Some(signal) = signal {
                unsafe {
                    libc::kill(-(pid as i32), signal);
                }
            }
            *current = None;
        }
    }
    #[cfg(test)]
    pub(crate) fn process_id(&self) -> Option<u32> {
        self.process
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|p| p.pid)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub available: bool,
    pub version: Option<String>,
    pub revision: String,
    pub reason: Option<String>,
}

pub fn info(root: &Path) -> EngineInfo {
    let checked = (|| -> Result<String, String> {
        if !root.join("UPSTREAM.json").is_file() {
            return Err("尚未准备 Mole 引擎，请在项目中运行 npm run engine:prepare。".into());
        }
        let manifest = crate::engine_manifest::verify(root).map_err(|error| {
            format!("Mole 引擎完整性检查失败：{error} 请重新运行 npm run engine:prepare。")
        })?;
        let source = std::fs::read_to_string(root.join("mole")).map_err(|e| e.to_string())?;
        let version = source_version(&source)?;
        if manifest.version != version {
            return Err("Mole CLI 版本清单与源码不一致，请重新准备引擎。".into());
        }
        Ok(version.to_owned())
    })();
    EngineInfo {
        available: checked.is_ok(),
        version: checked.as_ref().ok().cloned(),
        revision: REVISION.into(),
        reason: checked.err(),
    }
}

fn source_version(source: &str) -> Result<&str, String> {
    let versions: Vec<_> = source
        .lines()
        .filter_map(|line| {
            line.strip_prefix("VERSION=\"")
                .and_then(|v| v.strip_suffix('"'))
        })
        .collect();
    if versions.len() == 1 {
        let parts: Vec<_> = versions[0].split('.').collect();
        if parts.len() == 3
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        {
            return Ok(versions[0]);
        }
    }
    Err("无法确认 Mole CLI 版本，请重新准备引擎。".into())
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

pub async fn output(command: Command, job: Arc<Job>, timeout: Duration) -> Result<Vec<u8>, String> {
    crate::process::output(command, job, timeout).await
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
    let prepared = info(root);
    if !prepared.available {
        return Err(prepared.reason.unwrap_or("引擎不可用。".into()));
    }
    if job.kind == TaskKind::Cleanup {
        return inspect_cleanup(root, home, job, protected).await;
    }
    if job.kind == TaskKind::History {
        return crate::mole_history::run(root, home, job).await;
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
        TaskKind::Cleanup | TaskKind::Review | TaskKind::History | TaskKind::LogTrash => {
            return Err("请使用对应的专用任务入口。".into())
        }
    };
    command.current_dir(root);
    let started_at = crate::db::now();
    let started = Instant::now();
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
    let mut result: Value =
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
        TaskKind::History => false,
        TaskKind::LogTrash => false,
    };
    if !valid {
        return Err("Mole 数据与当前协议不匹配，请核对引擎版本。".into());
    }
    if job.kind == TaskKind::Analyze {
        record_analysis_read(
            &mut result,
            started_at,
            crate::db::now(),
            started.elapsed(),
            prepared.version.as_deref(),
        );
    }
    Ok(result)
}

fn record_analysis_read(
    result: &mut Value,
    started_at: i64,
    finished_at: i64,
    elapsed: Duration,
    version: Option<&str>,
) {
    // These describe this client invocation, not the age of measurements
    // inside Mole's caches. The pinned JSON protocol reports no cache hits.
    result["mooObservation"] = json!({
        "version": 1,
        "source": "mole-directory-json",
        "startedAt": started_at,
        "finishedAt": finished_at,
        "elapsedMs": elapsed.as_millis() as u64,
        "engineVersion": version,
        "revision": REVISION,
        "cacheUse": "unreported",
    });
}

pub(crate) fn plain_terminal_text(text: &str) -> String {
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
    #[test]
    fn version_is_read_from_source_and_must_match_prepared_manifest() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        std::fs::create_dir(root.join("bin")).unwrap();
        for name in [
            "bin/analyze-go",
            "bin/status-go",
            "moomole-clean-preview.sh",
            "moomole-review-rules.sh",
        ] {
            std::fs::write(root.join(name), "fixture").unwrap();
        }
        std::fs::write(
            root.join("mole"),
            "#!/bin/bash\nVERSION=\"1.59.1\"\nexit 99\n",
        )
        .unwrap();
        crate::engine_manifest::fixture(root);
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("UPSTREAM.json")).unwrap()).unwrap();
        let info = super::info(root);
        assert!(info.available);
        assert_eq!(info.version.as_deref(), Some("1.59.1"));
        for source in [
            "",
            "VERSION=\"1.59.2\"\n",
            "VERSION=\"$(echo 1.59.1)\"\n",
            "VERSION=\"1.59.1\"\nVERSION=\"1.59.1\"\n",
        ] {
            std::fs::write(root.join("mole"), source).unwrap();
            crate::engine_manifest::fixture(root);
            let info = super::info(root);
            assert!(!info.available);
            assert_eq!(info.version, None);
            assert!(info.reason.is_some());
        }
        std::fs::write(root.join("mole"), "VERSION=\"1.59.1\"\n").unwrap();
        let mut legacy = manifest.clone();
        legacy.as_object_mut().unwrap().remove("version");
        std::fs::write(root.join("UPSTREAM.json"), legacy.to_string()).unwrap();
        assert!(!super::info(root).available);
    }

    use super::*;
    #[test]
    fn analysis_read_provenance_preserves_measurements_and_survives_snapshot_retrieval() {
        let mut result = json!({
            "path": "/fixture", "scan_status": "partial", "total_size": 17,
            "entries": [{"name": "unknown", "scan_status": "unavailable", "size": 0}],
            "mooObservation": {"cacheUse": "fresh", "finishedAt": 999}
        });
        let original = result.clone();
        // Wall-clock rollback must not turn a monotonic duration negative.
        record_analysis_read(
            &mut result,
            2000,
            1000,
            Duration::from_millis(250),
            Some("1.59.1"),
        );
        for key in ["path", "scan_status", "total_size", "entries"] {
            assert_eq!(result[key], original[key]);
        }
        assert_eq!(
            result["mooObservation"],
            json!({
                "version": 1, "source": "mole-directory-json", "startedAt": 2000,
                "finishedAt": 1000, "elapsedMs": 250, "engineVersion": "1.59.1",
                "revision": REVISION, "cacheUse": "unreported"
            })
        );
        let mut cache = crate::tasks::Results::default();
        cache.remember(&crate::tasks::Snapshot {
            id: "analysis-fixture".into(),
            kind: "analyze".into(),
            state: "succeeded".into(),
            result: Some(result.clone()),
            error: None,
            result_unavailable: None,
        });
        assert_eq!(cache.get("analysis-fixture").unwrap().result, Some(result));
    }
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
            Duration::from_secs(6),
            output(command, job.clone(), Duration::from_secs(60)),
        )
        .await
        .unwrap();
        assert!(result.unwrap_err().contains("取消"));
        assert_eq!(job.process_id(), None);
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
        assert_eq!(job.process_id(), None);
    }
}
