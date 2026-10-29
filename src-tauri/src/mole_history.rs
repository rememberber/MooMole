//! Read-only visualization of the pinned Mole history JSON. Originals are never
//! passed to a writer; bounded private copies are parsed by the upstream reader.
use crate::engine::{self, Job};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

const LIMIT: usize = 200;
const MAX_LOG: u64 = 1024 * 1024;
const MAX_LINES: usize = 10_000;
const SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Source {
    path: PathBuf,
    state: &'static str,
    reason: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Actions {
    removed: u64,
    trashed: u64,
    skipped: u64,
    failed: u64,
    rebuilt: u64,
    other: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Session {
    command: String,
    run_id: String,
    attribution: String,
    started_at: String,
    ended_at: String,
    items: u64,
    size: String,
    operation_count: u64,
    failed_tasks: u64,
    actions: Actions,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Deletion {
    timestamp: String,
    mode: String,
    status: String,
    size_kb: Option<u64>,
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Logs {
    operations: PathBuf,
    deletions: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    logs: Logs,
    limit: usize,
    sessions: Vec<Session>,
    deletions: Vec<Deletion>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    version: u32,
    revision: &'static str,
    collected_at: i64,
    limit: usize,
    operations_source: Source,
    deletions_source: Source,
    sessions: Vec<Session>,
    deletions: Vec<Deletion>,
}

struct Inputs {
    scratch: tempfile::TempDir,
    operations: Source,
    deletions: Source,
}

fn read_log(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("无法读取日志（不跟随末级链接）：{e}")),
    };
    read_open_log(file, || {})
}

fn read_open_log(mut file: File, after_read: impl FnOnce()) -> Result<Option<Vec<u8>>, String> {
    let before = file.metadata().map_err(|e| e.to_string())?;
    if !before.is_file() {
        return Err("日志不是普通文件，未读取。".into());
    }
    if before.len() > MAX_LOG {
        return Err("日志超过 1 MiB 读取上限；本次未读取，也未截断或修改原文件。".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_LOG + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    after_read();
    let after = file.metadata().map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_LOG
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err("日志在读取期间发生变化，请重试读取。".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "日志不是有效 UTF-8，未发布结果。")?;
    if text.contains('\0')
        || text.lines().count() > MAX_LINES
        || text.lines().any(|line| line.len() > 16 * 1024)
    {
        return Err("日志包含无效字符或超过行数/单行上限，未发布结果。".into());
    }
    Ok(Some(bytes))
}

fn snapshot(path: PathBuf, destination: &Path) -> Result<Source, String> {
    let (state, reason, bytes) = match read_log(&path) {
        Ok(Some(bytes)) => ("ready", None, bytes),
        Ok(None) => ("missing", None, Vec::new()),
        Err(reason) => ("unavailable", Some(reason), Vec::new()),
    };
    // Empty private inputs let the independent, readable log remain available.
    std::fs::write(destination, bytes).map_err(|e| format!("无法准备历史读取目录：{e}"))?;
    Ok(Source {
        path,
        state,
        reason,
    })
}

fn inputs(home: &Path) -> Result<Inputs, String> {
    let scratch = tempfile::Builder::new()
        .prefix("moomole-history-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let logs = home.join("Library/Logs/mole");
    let operations = snapshot(
        logs.join("operations.log"),
        &scratch.path().join("operations"),
    )?;
    let deletions = snapshot(
        logs.join("deletions.log"),
        &scratch.path().join("deletions"),
    )?;
    Ok(Inputs {
        scratch,
        operations,
        deletions,
    })
}

fn parse(bytes: &[u8], inputs: &Inputs) -> Result<Report, String> {
    let raw: Raw = serde_json::from_slice(bytes)
        .map_err(|e| format!("Mole 历史 JSON 与固定版本不匹配：{e}"))?;
    let text = |s: &str, max: usize| s.len() <= max && !s.contains('\0');
    let valid = raw.limit == LIMIT
        && raw.logs.operations == inputs.scratch.path().join("operations")
        && raw.logs.deletions == inputs.scratch.path().join("deletions")
        && raw.sessions.len() <= LIMIT
        && raw.deletions.len() <= LIMIT
        && (inputs.operations.state == "ready" || raw.sessions.is_empty())
        && (inputs.deletions.state == "ready" || raw.deletions.is_empty())
        && raw.sessions.iter().all(|s| {
            let a = &s.actions;
            !s.command.is_empty()
                && text(&s.command, 128)
                && text(&s.run_id, 256)
                && s.run_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                && match s.attribution.as_str() {
                    "run" => !s.run_id.is_empty(),
                    "command" => s.run_id.is_empty(),
                    "ambiguous" => s.run_id.is_empty(),
                    _ => false,
                }
                && text(&s.started_at, 128)
                && text(&s.ended_at, 128)
                && text(&s.size, 128)
                && [
                    s.items,
                    s.operation_count,
                    s.failed_tasks,
                    a.removed,
                    a.trashed,
                    a.skipped,
                    a.failed,
                    a.rebuilt,
                    a.other,
                ]
                .iter()
                .all(|n| *n <= SAFE_INTEGER)
                && [
                    a.removed,
                    a.trashed,
                    a.skipped,
                    a.failed,
                    a.rebuilt,
                    a.other,
                    s.failed_tasks,
                ]
                .iter()
                .try_fold(0u64, |total, n| total.checked_add(*n))
                    == Some(s.operation_count)
        })
        && raw.deletions.iter().all(|d| {
            text(&d.timestamp, 128)
                && !d.mode.is_empty()
                && text(&d.mode, 128)
                && !d.status.is_empty()
                && text(&d.status, 128)
                && text(&d.path, 4096)
                && d.size_kb.is_none_or(|n| n <= SAFE_INTEGER / 1024)
        });
    if !valid {
        return Err("Mole 历史字段、数量、归属或日志来源不匹配，未发布结果。".into());
    }
    Ok(Report {
        version: 1,
        revision: engine::REVISION,
        collected_at: crate::db::now(),
        limit: LIMIT,
        operations_source: inputs.operations.clone(),
        deletions_source: inputs.deletions.clone(),
        sessions: raw.sessions,
        deletions: raw.deletions,
    })
}

pub async fn run(root: &Path, home: &Path, job: Arc<Job>) -> Result<serde_json::Value, String> {
    if job.cancelled.load(Ordering::SeqCst) {
        return Err("任务已取消。".into());
    }
    let owned_home = home.to_path_buf();
    let inputs = tauri::async_runtime::spawn_blocking(move || inputs(&owned_home))
        .await
        .map_err(|e| e.to_string())??;
    if job.cancelled.load(Ordering::SeqCst) {
        return Err("任务已取消。".into());
    }
    let mut command = engine::command(Path::new("/bin/bash"));
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("HOME", home)
        .env("TMPDIR", inputs.scratch.path())
        .env("NO_COLOR", "1")
        .env(
            // V1.65.0 reads OPERATIONS_LOG_FILE directly. Keep the upstream
            // parser bound to our private snapshot instead of the live log.
            "OPERATIONS_LOG_FILE",
            inputs.scratch.path().join("operations"),
        )
        .env("MOLE_DELETE_LOG", inputs.scratch.path().join("deletions"))
        .arg(root.join("bin/history.sh"))
        .args(["--json", "--limit", "200"])
        .current_dir(root);
    let bytes = engine::output(command, job, Duration::from_secs(30)).await?;
    serde_json::to_value(parse(&bytes, &inputs)?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::TaskKind;
    use serde_json::{json, Value};

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let scratch = tempfile::tempdir().unwrap();
        let home = scratch.path().canonicalize().unwrap();
        std::fs::create_dir_all(home.join("Library/Logs/mole")).unwrap();
        (scratch, home)
    }

    fn raw(inputs: &Inputs) -> Value {
        json!({"logs": {"operations": inputs.scratch.path().join("operations"),
            "deletions": inputs.scratch.path().join("deletions")}, "limit": LIMIT,
            "sessions": [], "deletions": []})
    }

    fn session() -> Value {
        json!({"command": "clean", "run_id": "run-a", "attribution": "run",
            "started_at": "2026-10-16 10:00:00", "ended_at": "", "items": 1,
            "size": "4KB", "operation_count": 1, "failed_tasks": 0,
            "actions": {"removed": 1, "trashed": 0, "skipped": 0, "failed": 0, "rebuilt": 0, "other": 0}})
    }

    #[test]
    fn bounded_log_reader_refuses_special_files_invalid_text_and_excessive_input() {
        let (_scratch, home) = fixture();
        let path = home.join("log");
        assert!(read_log(&path).unwrap().is_none());
        std::fs::write(&path, "original 中文\\n[*]\n").unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(read_log(&path).unwrap().unwrap(), bytes);
        let link = home.join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(read_log(&link).is_err());
        assert!(read_log(&home).is_err());
        let fifo = std::ffi::CString::new(home.join("fifo").to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(read_log(&home.join("fifo")).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        for invalid in [
            vec![0xff],
            vec![0],
            vec![b'a'; 16 * 1024 + 1],
            b"a\n".repeat(MAX_LINES + 1),
        ] {
            std::fs::write(&path, &invalid).unwrap();
            assert!(read_log(&path).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), invalid);
        }
        File::create(&path).unwrap().set_len(MAX_LOG + 1).unwrap();
        assert!(read_log(&path).is_err());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), MAX_LOG + 1);
    }

    #[test]
    fn changes_during_a_read_are_not_published_as_a_complete_snapshot() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("log");
        std::fs::write(&path, "before").unwrap();
        assert!(read_open_log(File::open(&path).unwrap(), || {
            std::fs::write(&path, "after and longer").unwrap();
        })
        .unwrap_err()
        .contains("变化"));
    }

    #[test]
    fn unavailable_and_missing_sources_are_distinct_and_do_not_hide_the_other_log() {
        let (_scratch, home) = fixture();
        let logs = home.join("Library/Logs/mole");
        std::os::unix::fs::symlink(home.join("absent"), logs.join("operations.log")).unwrap();
        // A dangling symlink is ELOOP with O_NOFOLLOW, not an invented empty log.
        let prepared = inputs(&home).unwrap();
        assert_eq!(prepared.operations.state, "unavailable");
        assert_eq!(prepared.deletions.state, "missing");
        assert!(parse(&serde_json::to_vec(&raw(&prepared)).unwrap(), &prepared).is_ok());
        std::fs::remove_file(logs.join("operations.log")).unwrap();
        std::fs::write(logs.join("operations.log"), "fixture\n").unwrap();
        let prepared = inputs(&home).unwrap();
        assert_eq!(prepared.operations.state, "ready");
        assert_eq!(
            std::fs::read(prepared.scratch.path().join("operations")).unwrap(),
            b"fixture\n"
        );
    }

    #[test]
    fn json_contract_rejects_wrong_sources_attribution_truncation_and_unsafe_numbers() {
        let (_scratch, home) = fixture();
        std::fs::write(home.join("Library/Logs/mole/operations.log"), "").unwrap();
        std::fs::write(home.join("Library/Logs/mole/deletions.log"), "").unwrap();
        let prepared = inputs(&home).unwrap();
        let mut value = raw(&prepared);
        value["sessions"] = json!([session()]);
        value["deletions"] = json!([{"timestamp": "now", "mode": "future-mode", "status": "future-state", "size_kb": null, "path": "/fixture/中文\\n[*]"}]);
        assert!(parse(&serde_json::to_vec(&value).unwrap(), &prepared).is_ok());
        for (pointer, bad) in [
            ("/logs/operations", json!("/other/log")),
            ("/limit", json!(201)),
            ("/sessions/0/attribution", json!("new")),
            ("/sessions/0/run_id", json!("")),
            ("/sessions/0/run_id", json!("not allowed")),
            ("/sessions/0/items", json!(-1)),
            ("/sessions/0/operation_count", json!(2)),
            ("/sessions/0/actions/removed", json!(SAFE_INTEGER + 1)),
            ("/deletions/0/size_kb", json!(SAFE_INTEGER)),
            ("/deletions/0/path", json!("x".repeat(4097))),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).unwrap() = bad;
            assert!(
                parse(&serde_json::to_vec(&changed).unwrap(), &prepared).is_err(),
                "{pointer}"
            );
        }
        let mut legacy = value.clone();
        legacy["sessions"][0]["attribution"] = json!("command");
        legacy["sessions"][0]["run_id"] = json!("");
        assert!(parse(&serde_json::to_vec(&legacy).unwrap(), &prepared).is_ok());
        legacy["sessions"][0]["attribution"] = json!("ambiguous");
        assert!(parse(&serde_json::to_vec(&legacy).unwrap(), &prepared).is_ok());
        value["sessions"][0]
            .as_object_mut()
            .unwrap()
            .remove("failed_tasks");
        assert!(parse(&serde_json::to_vec(&value).unwrap(), &prepared).is_err());
        assert!(parse(b"{\"sessions\":[", &prepared).is_err());
    }

    #[tokio::test]
    #[ignore = "Requires prepared pinned Mole resources; reads only isolated fixture logs"]
    async fn upstream_json_preserves_interleaved_run_identity_without_linking_deletions() {
        let (_scratch, home) = fixture();
        let logs = home.join("Library/Logs/mole");
        let operations = concat!(
            "# ========== clean run=run-a session started at 2026-10-16 10:00:00 ==========\n",
            "# ========== clean run=run-b session started at 2026-10-16 10:00:01 ==========\n",
            "[2026-10-16 10:00:02] [clean run=run-a] REMOVED /fixture/one (4KB)\n",
            "[2026-10-16 10:00:03] [clean run=run-b] SKIPPED /fixture/keep (whitelist)\n",
            "# ========== clean run=run-a session ended at 2026-10-16 10:00:04, 1 items, 4KB ==========\n",
            "[2026-10-16 10:00:05] [optimize] REBUILT /fixture/rebuild\n",
        );
        let deletions = "2026-10-16T10:00:02+0800\ttrash\tunknown\tdry-run\t/fixture/缓存 空格\\n[*] $(literal)\n";
        std::fs::write(logs.join("operations.log"), operations).unwrap();
        std::fs::write(logs.join("deletions.log"), deletions).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        let result = engine::run(
            &root,
            &home,
            Arc::new(Job::new(TaskKind::History)),
            None,
            &[],
        )
        .await
        .unwrap();
        let sessions = result["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 3);
        let a = sessions.iter().find(|s| s["run_id"] == "run-a").unwrap();
        let b = sessions.iter().find(|s| s["run_id"] == "run-b").unwrap();
        assert_eq!(a["actions"]["removed"], 1);
        assert_eq!(a["actions"]["skipped"], 0);
        assert_eq!(b["actions"]["skipped"], 1);
        assert_eq!(b["ended_at"], "");
        assert!(sessions
            .iter()
            .any(|s| s["attribution"] == "command" && s["command"] == "optimize"));
        assert_eq!(result["deletions"][0]["size_kb"], Value::Null);
        assert_eq!(result["deletions"][0]["status"], "dry-run");
        assert_eq!(
            result["deletions"][0]["path"],
            "/fixture/缓存 空格\\n[*] $(literal)"
        );
        assert!(result["deletions"][0].get("run_id").is_none());
        assert_eq!(
            std::fs::read_to_string(logs.join("operations.log")).unwrap(),
            operations
        );
        assert_eq!(
            std::fs::read_to_string(logs.join("deletions.log")).unwrap(),
            deletions
        );
    }

    #[tokio::test]
    #[ignore = "Requires prepared pinned Mole resources; reads only isolated fixture logs"]
    async fn missing_logs_and_cancellation_do_not_create_original_log_files() {
        let (_scratch, home) = fixture();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        let result = engine::run(
            &root,
            &home,
            Arc::new(Job::new(TaskKind::History)),
            None,
            &[],
        )
        .await
        .unwrap();
        assert_eq!(result["operationsSource"]["state"], "missing");
        assert_eq!(result["deletionsSource"]["state"], "missing");
        assert!(result["sessions"].as_array().unwrap().is_empty());
        let job = Arc::new(Job::new(TaskKind::History));
        job.cancel();
        assert!(run(&root, &home, job).await.is_err());
        assert_eq!(
            std::fs::read_dir(home.join("Library/Logs/mole"))
                .unwrap()
                .count(),
            0
        );
    }
}
