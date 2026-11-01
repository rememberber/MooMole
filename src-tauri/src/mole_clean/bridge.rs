//! A bounded local protocol. Intent is committed synchronously before ACK.
use super::{journal, plan::Plan, Controller};
use crate::{db::Database, engine::Job};
use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
pub fn worker() -> bool {
    let args: Vec<_> = std::env::args_os().collect();
    if std::path::Path::new(&args[0])
        .file_name()
        .is_some_and(|n| n == "moomole-clean-askpass")
    {
        let result = (|| -> Result<(), String> {
            let socket = std::env::current_exe()
                .map_err(|e| e.to_string())?
                .parent()
                .ok_or("missing authorization session")?
                .join("events.sock");
            let mut stream = UnixStream::connect(socket).map_err(|e| e.to_string())?;
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .map_err(|e| e.to_string())?;
            stream
                .write_all(b"authorization\0\0required\0\0")
                .map_err(|e| e.to_string())?;
            stream
                .shutdown(std::net::Shutdown::Write)
                .map_err(|e| e.to_string())?;
            let mut ack = String::new();
            stream
                .take(128)
                .read_to_string(&mut ack)
                .map_err(|e| e.to_string())?;
            if ack != "ok" {
                return Err("authorization refused".into());
            }
            let mut tty = std::fs::File::open("/dev/tty").map_err(|e| e.to_string())?;
            let mut password = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                if tty.read(&mut byte).map_err(|e| e.to_string())? != 1 || password.len() > 1024 {
                    password.fill(0);
                    return Err("authorization input ended".into());
                }
                if byte[0] == b'\n' {
                    break;
                }
                password.push(byte[0]);
            }
            let output = std::io::stdout()
                .write_all(&password)
                .and_then(|_| std::io::stdout().write_all(b"\n"));
            password.fill(0);
            output.map_err(|e| e.to_string())
        })();
        if result.is_err() {
            std::process::exit(1);
        }
        return true;
    }
    if args.get(1).is_none_or(|a| a != "--moomole-clean-event") {
        return false;
    }
    let result = (|| -> Result<(), String> {
        if args.len() != 3 {
            return Err("event arguments".into());
        }
        let mut bytes = vec![];
        std::io::stdin()
            .take(16385)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        decode(&bytes)?;
        let mut stream = UnixStream::connect(&args[2]).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .map_err(|e| e.to_string())?;
        stream.write_all(&bytes).map_err(|e| e.to_string())?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(|e| e.to_string())?;
        let mut ack = String::new();
        stream
            .take(128)
            .read_to_string(&mut ack)
            .map_err(|e| e.to_string())?;
        if ack != "ok" && ack != "retained" && uuid::Uuid::parse_str(&ack).is_err() {
            return Err("event not acknowledged".into());
        }
        print!("{ack}");
        Ok(())
    })();
    if result.is_err() {
        eprintln!("Cleanup event was not acknowledged; stopping this operation.");
        std::process::exit(130);
    }
    true
}
fn decode(bytes: &[u8]) -> Result<[&str; 4], String> {
    if bytes.len() > 16384 || bytes.last() != Some(&0) {
        return Err("清理事件不完整。".into());
    }
    let values = bytes[..bytes.len() - 1]
        .split(|b| *b == 0)
        .map(std::str::from_utf8)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "清理事件无法编码。")?;
    let values: [&str; 4] = values.try_into().map_err(|_| "清理事件字段不匹配。")?;
    if values[1].len() > 4096
        || values[2].len() > 8192
        || values[3].len() > 64
        || (!values[1].is_empty() && !values[1].starts_with('/'))
        || ![
            "intent",
            "removed",
            "retained",
            "uncertain",
            "commandCompleted",
            "section",
            "upstream",
            "authorization",
            "complete",
        ]
        .contains(&values[0])
    {
        return Err("清理事件范围无法核对。".into());
    }
    Ok(values)
}
fn protected(path: &str, roots: &[String]) -> bool {
    if roots.is_empty() {
        return false;
    }
    if path.is_empty() {
        return true;
    }
    let overlaps = |p: &str| {
        roots.iter().any(|root| {
            root == "/"
                || p == root
                || p.strip_prefix(root)
                    .is_some_and(|suffix| suffix.starts_with('/'))
                || root
                    .strip_prefix(p)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        })
    };
    if overlaps(path) {
        return true;
    }
    match std::fs::canonicalize(path) {
        Ok(p) => p.to_str().is_none_or(overlaps),
        Err(_) => true,
    }
}
pub struct Server {
    done: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    pub incomplete: Arc<AtomicBool>,
    pub completion: Arc<AtomicBool>,
}
impl Server {
    pub fn start(
        listener: UnixListener,
        database: Arc<std::sync::Mutex<Database>>,
        id: String,
        plan: Plan,
        job: Arc<Job>,
        controller: Arc<Controller>,
        gate: impl Fn() -> Result<(), String> + Send + 'static,
    ) -> Result<Self, String> {
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let done = Arc::new(AtomicBool::new(false));
        let stopped = done.clone();
        let incomplete = Arc::new(AtomicBool::new(false));
        let pending = incomplete.clone();
        let completion = Arc::new(AtomicBool::new(false));
        let completed = completion.clone();
        let thread = std::thread::spawn(move || {
            let mut intents: HashMap<String, String> = HashMap::new();
            let mut auth_started = false;
            let mut auth_finished = !plan.system;
            while !stopped.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(_) => {
                        job.cancel();
                        break;
                    }
                };
                let result = (|| -> Result<String, String> {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .map_err(|e| e.to_string())?;
                    let mut bytes = vec![];
                    (&mut stream)
                        .take(16385)
                        .read_to_end(&mut bytes)
                        .map_err(|e| e.to_string())?;
                    let [kind, path, detail, token] = decode(&bytes)?;
                    if kind == "intent"
                        || kind == "section"
                        || (kind == "authorization" && detail != "finished")
                    {
                        if job.cancelled.load(Ordering::SeqCst) {
                            return Err("清理已停止。".into());
                        }
                        gate()?;
                    }
                    if kind == "intent" && protected(path, &plan.protected_paths) {
                        let db = database.lock().map_err(|_| "无法保存保护结果。")?;
                        journal::append(
                            &db,
                            &id,
                            "retained",
                            path,
                            "MooMole 保护列表覆盖此项，或所有者命令的范围无法核对。",
                            "",
                        )?;
                        return Ok("retained".into());
                    }
                    if kind == "authorization" {
                        if !plan.system || auth_finished {
                            return Err("此清理批次没有活动的管理员授权阶段。".into());
                        }
                        match detail {
                            "begin" if !auth_started => {
                                auth_started = true;
                            }
                            "required" if auth_started => (),
                            "finished" if auth_started => {
                                auth_finished = true;
                            }
                            _ => return Err("授权阶段无法核对。".into()),
                        }
                        controller
                            .authorization
                            .store(detail == "required", Ordering::SeqCst);
                    }
                    let next = if kind == "intent" {
                        uuid::Uuid::new_v4().to_string()
                    } else {
                        token.into()
                    };
                    if ["removed", "retained", "uncertain", "commandCompleted"].contains(&kind)
                        && intents.get(token).is_none_or(|p| p != path)
                    {
                        return Err("清理结果缺少对应意图。".into());
                    }
                    let db = database.lock().map_err(|_| "无法保存完整清理事件。")?;
                    journal::append(&db, &id, kind, path, detail, &next)?;
                    drop(db);
                    if kind == "complete" {
                        completed.store(true, Ordering::SeqCst);
                    }
                    if kind == "intent" {
                        intents.insert(next.clone(), path.into());
                    } else if ["removed", "retained", "uncertain", "commandCompleted"]
                        .contains(&kind)
                    {
                        intents.remove(token);
                        if kind == "uncertain" {
                            pending.store(true, Ordering::SeqCst);
                            job.cancel();
                        }
                    }
                    Ok(if kind == "intent" { next } else { "ok".into() })
                })();
                match result {
                    Ok(ack) => {
                        if stream.write_all(ack.as_bytes()).is_err() {
                            job.cancel();
                        }
                    }
                    Err(_) => {
                        job.cancel();
                        let _ = stream.write_all(b"refused");
                    }
                }
            }
            if !intents.is_empty() {
                pending.store(true, Ordering::SeqCst);
            }
        });
        Ok(Self {
            done,
            thread: Some(thread),
            incomplete,
            completion,
        })
    }
    pub fn close(&mut self) {
        self.done.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.close();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_and_terminal_log_text_never_grant_authority() {
        assert!(decode(b"intent\0/tmp/owned\0safe_remove\0\0").is_ok());
        assert!(decode(b"removed /Users/data\n").is_err());
        assert!(decode(b"intent\0relative\0safe_remove\0\0").is_err());
        assert!(decode(b"unknown\0\0\0\0").is_err());
    }
}
