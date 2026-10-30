//! Fresh, one-use restoration from a verified local execution receipt.
//! Archived records and uncertain outcomes never grant mutation authority.
use crate::{
    db,
    engine::Job,
    execution::{self, Receipt},
    observation::AnchoredPath,
    personal_files::{self, movement},
};
use serde::{Deserialize, Serialize};
use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

pub(crate) const MODULE: &str = "fileRestoration";
pub(crate) mod batch;
pub(crate) const REVISION: &str = "MooMole.file-restoration.v1";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Source {
    pub run_id: String,
    pub item_id: String,
    pub snapshot_sha256: String,
    pub original_path: String,
    pub receipt: Receipt,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Plan {
    pub module: String,
    pub protocol: u64,
    pub revision: String,
    pub id: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub source: Source,
    pub item: personal_files::Item,
    pub movement: movement::Movement,
    #[serde(skip)]
    issued: Option<Instant>,
}
impl Plan {
    pub(crate) fn validate_record(&self) -> Result<(), String> {
        if self.module != MODULE
            || self.protocol != 1
            || self.revision != REVISION
            || uuid::Uuid::parse_str(&self.id).is_err()
            || uuid::Uuid::parse_str(&self.source.run_id).is_err()
            || uuid::Uuid::parse_str(&self.item.id).is_err()
            || self.source.item_id.is_empty()
            || self.source.item_id.len() > 256
            || self.source.snapshot_sha256.len() != 64
            || !self
                .source
                .snapshot_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
            || self.created_at < 0
            || self.expires_at != self.created_at + 30_000
            || self.item.path != self.source.receipt.destination
            || !Path::new(&self.item.path).is_absolute()
            || self.item.path.len() > 4096
            || self.item.path.chars().any(char::is_control)
            || self.item.witness.version != crate::observation::VERSION
            || self.item.size != self.item.witness.size
            || self.item.witness.nodes.last().is_none_or(|n| {
                n.path != Path::new(&self.item.path)
                    || n.dev != self.source.receipt.device
                    || n.ino != self.source.receipt.inode
            })
            || self.movement.targets.len() != 1
            || self.movement.targets[0].destination.as_deref() != Some(&self.source.original_path)
            || self.item.size > 9_007_199_254_740_991
        {
            return Err("恢复计划的原记录、文件身份或目标无法核对。".into());
        }
        self.movement
            .validate_record(std::slice::from_ref(&self.item))
    }
    fn validate_source(&self, database: &db::Database) -> Result<(), String> {
        crate::personal_files::automation::allow_restoration(
            database,
            &self.movement.witness.nodes,
        )?;
        let current = execution::detail(database, &self.source.run_id)?;
        let source = source(&current, &self.source.item_id)?;
        if source.snapshot_sha256 != self.source.snapshot_sha256
            || source.original_path != self.source.original_path
            || source.receipt != self.source.receipt
        {
            return Err("原处理记录已变化，请重新检查恢复范围。".into());
        }
        Ok(())
    }
    fn validate(&self, home: &Path, settings: &db::Settings) -> Result<(), String> {
        self.validate_record()?;
        if !settings.acknowledged
            || unsafe { libc::geteuid() } == 0
            || unsafe { libc::geteuid() } != unsafe { libc::getuid() }
            || db::now() < self.created_at
            || db::now() >= self.expires_at
            || self
                .issued
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(30))
            || !Path::new(&self.item.path).starts_with(home)
            || !Path::new(&self.source.original_path).starts_with(home)
        {
            return Err("恢复确认已过期或不是当前账户的个人文件。".into());
        }
        for path in [&self.item.path, &self.source.original_path] {
            if !crate::app_protection::check(path, None, None, "", &settings.protected_paths)?
                .reasons
                .is_empty()
            {
                return Err("当前文件或原位置与保护范围重叠，本项保留。".into());
            }
        }
        let held = AnchoredPath::open(Path::new(&self.item.path))?;
        let meta = held.target().metadata().map_err(|e| e.to_string())?;
        if held.witness != self.item.witness
            || !held.local_materialized()
            || !meta.is_file()
            || meta.nlink() != 1
            || meta.uid() != unsafe { libc::geteuid() }
        {
            return Err("收据位置的文件已变化，或不是本账户的本地普通文件。".into());
        }
        let parent = self.movement.validate_directory(settings)?;
        if parent.target().metadata().map_err(|e| e.to_string())?.dev() != meta.dev() {
            return Err("首批恢复仅支持同卷，跨卷需要后续内容复制与核验。".into());
        }
        match std::fs::symlink_metadata(&self.source.original_path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Ok(_) => Err("原位置已有同名项目，保留当前文件；不会覆盖或自动编号。".into()),
            Err(_) => Err("无法确认原位置是否空闲，请先检查目录权限。".into()),
        }
    }
}
fn source(detail: &execution::Detail, item_id: &str) -> Result<Source, String> {
    if detail.summary.state == "running"
        || !matches!(
            detail.origin,
            "personalFiles" | "duplicateFiles" | "similarPhotos" | "scheduledFiles"
        )
        || !matches!(detail.summary.method.as_str(), "trash" | "move")
    {
        return Err(
            "首批恢复支持已结束的个人文件或重复文件处理，其他记录请先在 Finder 核查。".into(),
        );
    }
    let item = detail
        .items
        .iter()
        .find(|i| i.id == item_id)
        .ok_or("原批次没有此项目。")?;
    if item.state != "moved" {
        return Err("此项没有可靠的完成记录，不能自动恢复。".into());
    }
    Ok(Source {
        run_id: detail.summary.id.clone(),
        item_id: item_id.into(),
        snapshot_sha256: detail.snapshot_sha256.clone(),
        original_path: item.path.clone(),
        receipt: item.receipt.clone().ok_or("此项没有实际位置收据。")?,
    })
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Draft {
    pub id: String,
    pub source_run_id: String,
    pub source_item_id: String,
    pub current_path: String,
    pub original_path: String,
    pub expires_at: i64,
}
struct Offered {
    plan: Plan,
    issued: Instant,
}
#[derive(Default)]
pub(crate) struct Store {
    offer: Option<Offered>,
    generation: u64,
    batch: Option<batch::Plan>,
}
impl Store {
    fn begin(&mut self) -> Result<u64, String> {
        self.generation = self.generation.checked_add(1).ok_or("请重新打开应用。")?;
        self.offer = None;
        self.batch = None;
        Ok(self.generation)
    }
    fn publish(&mut self, plan: Plan) -> Draft {
        let d = Draft {
            id: plan.id.clone(),
            source_run_id: plan.source.run_id.clone(),
            source_item_id: plan.source.item_id.clone(),
            current_path: plan.item.path.clone(),
            original_path: plan.source.original_path.clone(),
            expires_at: plan.expires_at,
        };
        self.offer = Some(Offered {
            plan,
            issued: Instant::now(),
        });
        d
    }
    fn take(&mut self, id: &str) -> Result<Plan, String> {
        if self.offer.as_ref().is_none_or(|o| o.plan.id != id) {
            return Err("恢复确认已撤下、取代或已使用。".into());
        }
        let o = self.offer.take().ok_or("恢复确认不存在。")?;
        if o.issued.elapsed() >= Duration::from_secs(30) {
            return Err("恢复确认已过期。".into());
        }
        Ok(o.plan)
    }
    fn discard(&mut self, id: &str) {
        if self.batch.as_ref().is_some_and(|p| p.id == id) {
            self.batch = None;
        }
        if self.offer.as_ref().is_some_and(|o| o.plan.id == id) {
            self.offer.take();
        }
    }
}
fn prepare(
    detail: &execution::Detail,
    item_id: &str,
    home: &Path,
    settings: &db::Settings,
) -> Result<Plan, String> {
    let source = source(detail, item_id)?;
    let held = AnchoredPath::open(Path::new(&source.receipt.destination)).map_err(|e| format!("无法核对收据位置，可能已被移走或权限受限。请在 Finder 核对当前文件，不会猜测或重新执行原操作。{e}"))?;
    let meta = held.target().metadata().map_err(|e| e.to_string())?;
    let directory = Path::new(&source.original_path)
        .parent()
        .and_then(Path::to_str)
        .ok_or("原目录路径无效。")?;
    let parent = movement::directory(directory, settings)?;
    let item = personal_files::Item {
        id: uuid::Uuid::new_v4().to_string(),
        path: source.receipt.destination.clone(),
        size: meta.size(),
        modified_at: meta.mtime().saturating_mul(1000),
        witness: held.witness,
    };
    let movement = movement::Movement {
        directory: directory.into(),
        witness: parent.witness,
        conflict: movement::Conflict::Skip,
        targets: vec![movement::Target {
            id: item.id.clone(),
            destination: Some(source.original_path.clone()),
        }],
    };
    let now = db::now();
    let plan = Plan {
        module: MODULE.into(),
        protocol: 1,
        revision: REVISION.into(),
        id: uuid::Uuid::new_v4().to_string(),
        created_at: now,
        expires_at: now + 30_000,
        source,
        item,
        movement,
        issued: Some(Instant::now()),
    };
    plan.validate(home, settings)?;
    Ok(plan)
}
fn bounded(s: &str) -> String {
    let mut n = s.len().min(1000);
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    s[..n].replace('\0', " ")
}
fn run(
    database: Arc<std::sync::Mutex<db::Database>>,
    home: PathBuf,
    plan: Plan,
    job: Arc<Job>,
) -> Result<execution::Detail, String> {
    let run = {
        let mut db = database.lock().map_err(|e| e.to_string())?;
        plan.validate_source(&db)?;
        plan.validate(&home, &db::settings(&db)?)?;
        execution::begin_restoration(&mut db, &plan, db::now())?
    };
    let result = (|| -> Result<(), String> {
        job.bind_execution(&run)?;
        if !job.cancelled.load(Ordering::SeqCst) {
            let settings = db::settings(&*database.lock().map_err(|e| e.to_string())?)?;
            let target = movement::Prepared::new(
                &plan.item,
                &plan.movement,
                &plan.source.original_path,
                &settings,
            );
            match target {
                Err(e) => execution::skip(
                    &mut *database.lock().map_err(|e| e.to_string())?,
                    &run,
                    &plan.item.id,
                    &bounded(&e),
                    db::now(),
                )?,
                Ok(target) => {
                    execution::intent(
                        &mut *database.lock().map_err(|e| e.to_string())?,
                        &run,
                        &plan.item.id,
                        db::now(),
                    )?;
                    let outcome = target.move_checked(|| {
                        if job.cancelled.load(Ordering::SeqCst) {
                            return Err("系统调用前已取消恢复。".into());
                        }
                        let db = database.lock().map_err(|e| e.to_string())?;
                        plan.validate_source(&db)?;
                        plan.validate(&home, &db::settings(&db)?)
                    });
                    let (outcome, reason) =
                        outcome.unwrap_or_else(|e| (execution::Outcome::Failed, e));
                    execution::complete_item(
                        &mut *database.lock().map_err(|e| e.to_string())?,
                        &run,
                        &plan.item.id,
                        outcome,
                        &bounded(&reason),
                        db::now(),
                    )?;
                }
            }
        }
        execution::finish(
            &mut *database.lock().map_err(|e| e.to_string())?,
            &run,
            job.cancelled.load(Ordering::SeqCst),
            db::now(),
        )
    })();
    if let Err(e) = result {
        if let Ok(mut db) = database.lock() {
            let _ = execution::stop(&mut db, &run, db::now());
        }
        return Err(format!("{e} 请在历史核对恢复批次 {run}，不要重复操作。"));
    }
    execution::detail(&*database.lock().map_err(|e| e.to_string())?, &run)
}

pub(crate) mod commands;
#[cfg(test)]
mod tests;
