use super::{bridge, journal, plan::Authorized, terminal};
use crate::{engine::Job, AppState};
use std::{
    fs,
    io::Write,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{MetadataExt, PermissionsExt},
            net::UnixListener,
        },
    },
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
fn copy(source: &Path, destination: &Path) -> Result<(), String> {
    let m = fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    if m.is_dir() {
        fs::create_dir(destination).map_err(|e| e.to_string())?;
        fs::set_permissions(destination, fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if m.is_file() && m.nlink() == 1 {
        fs::copy(source, destination).map_err(|e| e.to_string())?;
    } else {
        return Err("完整清理引擎包含链接或非普通文件，未执行。".into());
    }
    Ok(())
}
fn available(home: &Path) -> Option<u64> {
    let file = fs::File::open(home).ok()?;
    let mut v: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatfs(file.as_raw_fd(), &mut v) } != 0 {
        return None;
    }
    v.f_bavail
        .checked_mul(v.f_bsize as u64)
        .filter(|b| *b <= 9_007_199_254_740_991)
}
fn command(home: &Path) -> tokio::process::Command {
    let mut command = crate::engine::command(Path::new("/bin/bash"));
    command.env("HOME", home).current_dir(home);
    // Keep tool configuration, but never import shell functions/startup code or
    // an inherited sudo identity into a sealed desktop operation.
    for (key, value) in std::env::vars_os() {
        let key_text = key.to_string_lossy();
        if key_text.starts_with("BASH_FUNC_") || value.to_string_lossy().starts_with("() {") {
            command.env_remove(key);
        }
    }
    for key in [
        "BASH_ENV",
        "ENV",
        "SHELLOPTS",
        "BASHOPTS",
        "CDPATH",
        "SUDO_USER",
        "SUDO_UID",
        "SUDO_GID",
        "SUDO_ASKPASS",
        "SSH_ASKPASS",
    ] {
        command.env_remove(key);
    }
    let mut paths = vec![
        "/usr/bin".into(),
        "/bin".into(),
        "/usr/sbin".into(),
        "/sbin".into(),
        "/opt/homebrew/bin".into(),
        "/usr/local/bin".into(),
    ];
    for relative in [
        ".local/bin",
        ".cargo/bin",
        ".bun/bin",
        ".volta/bin",
        ".npm-global/bin",
        "go/bin",
    ] {
        let path = home.join(relative);
        if path.is_dir() {
            paths.push(path);
        }
    }
    if let Some(parent) = std::env::var_os("PATH") {
        for path in std::env::split_paths(&parent) {
            if path.is_absolute() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    if let Ok(path) = std::env::join_paths(paths) {
        command.env("PATH", path);
    }
    command
}
pub async fn run(state: AppState, id: String, a: Authorized, job: Arc<Job>) -> Result<(), String> {
    a.validate(&state)?;
    let scratch = tempfile::Builder::new()
        .prefix("mc-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let root = scratch.path().canonicalize().map_err(|e| e.to_string())?;
    let engine = root.join("engine");
    copy(&a.engine, &engine)?;
    crate::engine_manifest::verify(&engine)?;
    let worker = root.join("event-worker");
    fs::copy(std::env::current_exe().map_err(|e| e.to_string())?, &worker)
        .map_err(|e| e.to_string())?;
    fs::copy(&worker, root.join("moomole-clean-askpass")).map_err(|e| e.to_string())?;
    let protections = root.join("protected");
    let mut p = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&protections)
        .map_err(|e| e.to_string())?;
    for path in &a.view.protected_paths {
        p.write_all(path.as_bytes())
            .and_then(|_| p.write_all(&[0]))
            .map_err(|e| e.to_string())?;
    }
    p.sync_all().map_err(|e| e.to_string())?;
    drop(p);
    let socket = root.join("events.sock");
    let listener = UnixListener::bind(&socket).map_err(|e| e.to_string())?;
    let (master, slave) = terminal::open()?;
    a.validate(&state)?;
    {
        let db = state.db.lock().map_err(|_| "无法保存空间观察。")?;
        journal::before(&db, &id, available(&state.home))?;
    }
    *state
        .mole_clean
        .input
        .lock()
        .map_err(|_| "无法建立授权输入。")? = Some((id.clone(), master));
    let a = Arc::new(a);
    let gate_state = state.clone();
    let gate_plan = a.clone();
    let mut server = bridge::Server::start(
        listener,
        state.db.clone(),
        id.clone(),
        a.view.clone(),
        job.clone(),
        state.mole_clean.clone(),
        move || gate_plan.validate_running(&gate_state),
    )?;
    let mut command = command(&state.home);
    command
        .arg(engine.join("moomole-clean-run.sh"))
        .arg(&worker)
        .arg(&socket)
        .arg(&protections)
        .arg(if a.view.system { "system" } else { "user" })
        .env("TMPDIR", &root)
        .env_remove("MOLE_RESOLVED_TMPDIR")
        .env_remove("MOLE_TEMP_REGISTRY_FILE");
    let mut result = crate::process::output_with_terminal(
        command,
        job.clone(),
        Duration::from_secs(3600),
        slave,
    )
    .await;
    server.close();
    if result.is_ok() && !server.completion.load(Ordering::SeqCst) {
        result = Err("Mole 未返回完整结束标记，请核查已保存的逐项结果；不会自动重试。".into());
    }
    state
        .mole_clean
        .authorization
        .store(false, Ordering::SeqCst);
    *state
        .mole_clean
        .input
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    let uncertain = server.incomplete.load(Ordering::SeqCst);
    let cancelled = job.cancelled.load(Ordering::SeqCst);
    let final_state = if uncertain {
        "uncertain"
    } else if cancelled {
        "cancelled"
    } else if result.is_ok() {
        "completed"
    } else {
        "failed"
    };
    let diagnostic = match &result {
        Ok(output) => String::from_utf8_lossy(output).into_owned(),
        Err(e) => e.clone(),
    };
    let error = result.as_ref().err().map(String::as_str);
    let after = if a.validate_running(&state).is_ok() {
        available(&state.home)
    } else {
        None
    };
    let mut db = state.db.lock().map_err(|_| "完整清理结果仍待核查。")?;
    journal::finish_with_diagnostics(&mut db, &id, final_state, after, error, &diagnostic)?;
    Ok(())
}
