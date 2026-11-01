//! Explicit lifecycle of this app's own managed evidence, after verified export.
//! No Mole cleanup policy is granted for user files or other app-data artifacts.
use super::*;
use crate::{
    db,
    engine::Job,
    observation::{AnchoredPath, Witness},
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
pub(crate) mod commands;
mod journal;
#[cfg(test)]
mod tests;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Draft {
    id: String,
    header: Header,
    path: String,
    exported: String,
    bytes: u64,
    sha256: String,
    created_at: i64,
    expires_at: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Record {
    id: String,
    request_id: String,
    draft: Draft,
    state: String,
    phase: String,
    reason: String,
    created_at: i64,
    finished_at: Option<i64>,
    receipt: Option<crate::execution::Receipt>,
    reviewed_at: Option<i64>,
}
struct Plan {
    draft: Draft,
    source: Witness,
    copy: Witness,
    protected: Vec<String>,
    issued: Instant,
}
#[derive(Default)]
pub(crate) struct Store {
    plan: Option<Plan>,
}
fn check(job: &Job) -> Result<(), String> {
    if job.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
        Err("归档副本整理已停止，尚未处理的文件保留。".into())
    } else {
        Ok(())
    }
}
fn path_allowed(path: &Path, rules: &[String]) -> Result<(), String> {
    if !path.is_absolute()
        || path.components().any(|p| {
            matches!(
                p,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
        || path
            .to_str()
            .is_none_or(|s| s.len() > 4096 || s.chars().any(char::is_control))
        || rules
            .iter()
            .any(|p| path.starts_with(p) || Path::new(p).starts_with(path))
    {
        return Err("归档或导出位置已受保护或无法核对，原副本保留。".into());
    }
    Ok(())
}
fn observe(path: &Path, private: bool, job: &Job) -> Result<(Header, Witness, String), String> {
    check(job)?;
    let held = AnchoredPath::open(path)?;
    if !held.local_materialized() {
        return Err("归档必须完整保存在本机，未请求下载。".into());
    }
    let loaded = load(path, private)?;
    check(job)?;
    let mut file = open_file(path, private)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    let mut bytes = 0;
    loop {
        check(job)?;
        let n = file.read(&mut buffer).map_err(err)?;
        if n == 0 {
            break;
        }
        bytes += n;
        if bytes > MAX_BODY + MAX_HEADER {
            return Err("归档超过本轮读取范围。".into());
        }
        digest.update(&buffer[..n]);
    }
    held.validate()?;
    if AnchoredPath::open(path)?.witness != held.witness {
        return Err("归档读取期间文件已变化。".into());
    }
    Ok((
        loaded.header,
        held.witness.clone(),
        format!("{:x}", digest.finalize()),
    ))
}
impl Plan {
    fn prepare(
        root: &Path,
        id: &str,
        exported: &Path,
        rules: &[String],
        job: &Job,
    ) -> Result<Self, String> {
        check(job)?;
        directory(root)?;
        let source = filename(root, id)?;
        path_allowed(&source, rules)?;
        path_allowed(exported, rules)?;
        // A second managed/private app-data path is not an independent export.
        if exported.starts_with(root.parent().ok_or("归档父目录无效。")?) {
            return Err("请将完整副本保存到此应用数据目录以外的位置。".into());
        }
        let (header, before, digest) = observe(&source, true, job)?;
        if header.id != id {
            return Err("所选归档标识不一致。".into());
        }
        check(job)?;
        export(root, id, exported)?;
        check(job)?;
        let (h, copy, copy_digest) = observe(exported, false, job)?;
        if h != header
            || copy_digest != digest
            || AnchoredPath::open(&source)?.witness != before
            || before
                .nodes
                .last()
                .zip(copy.nodes.last())
                .is_none_or(|(a, b)| a.dev == b.dev && a.ino == b.ino)
        {
            return Err("导出副本与原归档不一致，原副本保留。".into());
        }
        let now = db::now();
        let draft = Draft {
            id: uuid::Uuid::new_v4().to_string(),
            header,
            path: source.to_str().ok_or("归档路径无法编码。")?.into(),
            exported: exported.to_str().ok_or("导出路径无法编码。")?.into(),
            bytes: before.size,
            sha256: digest,
            created_at: now,
            expires_at: now + 120000,
        };
        Ok(Self {
            draft,
            source: before,
            copy,
            protected: rules.to_vec(),
            issued: Instant::now(),
        })
    }
    fn validate(&self, root: &Path, rules: &[String], job: &Job) -> Result<(), String> {
        check(job)?;
        let now = db::now();
        if now < self.draft.created_at
            || now >= self.draft.expires_at
            || self.issued.elapsed() >= Duration::from_secs(120)
            || self.protected != rules
            || filename(root, &self.draft.header.id)? != Path::new(&self.draft.path)
        {
            return Err("归档副本确认已变化或过期，请重新核对。".into());
        }
        directory(root)?;
        for (path, private, witness) in [
            (&self.draft.path, true, &self.source),
            (&self.draft.exported, false, &self.copy),
        ] {
            path_allowed(Path::new(path), rules)?;
            let (header, actual, digest) = observe(Path::new(path), private, job)?;
            if actual != *witness || header != self.draft.header || digest != self.draft.sha256 {
                return Err("原归档或完整导出副本已变化，未处理本机副本。".into());
            }
        }
        Ok(())
    }
}
fn execute<D: std::ops::DerefMut<Target = Connection>>(
    plan: Plan,
    root: &Path,
    database: Arc<Mutex<D>>,
    job: Arc<Job>,
    request: String,
    rules: impl Fn() -> Result<Vec<String>, String>,
) -> Result<Record, String> {
    execute_with(plan, root, database, job, request, rules, |_| {})
}
fn execute_with<D: std::ops::DerefMut<Target = Connection>>(
    plan: Plan,
    root: &Path,
    database: Arc<Mutex<D>>,
    job: Arc<Job>,
    request: String,
    rules: impl Fn() -> Result<Vec<String>, String>,
    mut checkpoint: impl FnMut(&Record),
) -> Result<Record, String> {
    let now = db::now();
    let mut r = Record {
        id: plan.draft.id.clone(),
        request_id: request,
        draft: plan.draft.clone(),
        state: "running".into(),
        phase: "checking".into(),
        reason: "正在复查归档和完整导出副本。".into(),
        created_at: now,
        finished_at: None,
        receipt: None,
        reviewed_at: None,
    };
    {
        let mut db = database.lock().map_err(|e| e.to_string())?;
        journal::admission(&db)?;
        journal::begin(&mut db, &r)?;
    }
    let result = (|| {
        plan.validate(root, &rules()?, &job)?;
        let item = crate::review::ScanItem::personal_file(
            &plan.draft.id,
            &plan.draft.path,
            plan.draft.bytes,
            plan.source.clone(),
        );
        let prepared = crate::cleanup_target::PreparedFile::prepare(&item)?;
        plan.validate(root, &rules()?, &job)?;
        r.phase = "intent".into();
        r.reason = "已持久记录单份本机归档副本的处理意图。".into();
        journal::save(&*database.lock().map_err(|e| e.to_string())?, &r)?;
        prepared.move_to_trash_checked(|| plan.validate(root, &rules()?, &job))
    })();
    match result {
        Err(e) => {
            r.state = if job.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                "cancelled"
            } else {
                "retained"
            }
            .into();
            r.reason = e;
        }
        Ok(attempt) => {
            r.reason = attempt.reason;
            match attempt.outcome {
                crate::execution::Outcome::Moved(receipt) => {
                    r.state = "moved".into();
                    r.receipt = Some(receipt);
                }
                crate::execution::Outcome::Uncertain(receipt) => {
                    r.state = "unknown".into();
                    r.receipt = receipt;
                }
                crate::execution::Outcome::Failed => r.state = "retained".into(),
            }
            // Preserve the native result even when the final SQLite write fails.
            checkpoint(&r);
            let current = observe(
                Path::new(&plan.draft.exported),
                false,
                &Job::new(crate::engine::TaskKind::AppControl),
            );
            if current.map_or(true, |(h, w, d)| {
                h != plan.draft.header || w != plan.copy || d != plan.draft.sha256
            }) {
                r.state = "unknown".into();
                r.reason = "系统调用后导出副本无法再次核对，请保留两处证据并人工核查。".into();
            }
        }
    }
    r.phase = "finished".into();
    r.finished_at = Some(db::now().max(r.created_at));
    journal::save(&*database.lock().map_err(|e| e.to_string())?, &r)
        .map_err(|_| "归档副本结果未可靠落盘，请刷新记录并核查实际文件；不会自动重试。")?;
    Ok(r)
}

pub(crate) fn reconcile_startup(c: &Connection) -> Result<(), String> {
    journal::reconcile(c, &[]).map(|_| ())
}
