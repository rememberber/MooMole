//! Recover a verified transfer copy to its recorded original path. The copy
//! and any original Trash item are retained; this never replays the move.
use super::*;
use sha2::{Digest, Sha256};
pub(crate) mod commands;
mod journal;
#[cfg(test)]
mod tests;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Draft {
    id: String,
    run_id: String,
    item_id: String,
    copy_path: String,
    original_path: String,
    bytes: u64,
    created_at: i64,
    expires_at: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Plan {
    draft: Draft,
    source_hash: String,
    item: Item,
    movement: movement::Movement,
    sha256: String,
    metadata_sha256: String,
    home: PathBuf,
    uid: u32,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Recovery {
    id: String,
    request_id: String,
    draft: Draft,
    created_at: i64,
    finished_at: Option<i64>,
    state: String,
    phase: String,
    temporary_path: String,
    receipt: Option<Receipt>,
    reason: String,
    reviewed_at: Option<i64>,
}
fn source(
    c: &rusqlite::Connection,
    run: &str,
    item: &str,
) -> Result<(Entry, Item, String), String> {
    let record = super::journal::list(c)?
        .into_iter()
        .find(|r| r.id == run)
        .ok_or("原跨磁盘记录不存在。")?;
    if record.state == "running"
        || record.finished_at.is_none()
        || record.state != "completed" && record.reviewed_at.is_none()
    {
        return Err("请先结束并核对原跨磁盘记录，再准备恢复。".into());
    }
    let entry = record
        .items
        .into_iter()
        .find(|i| i.id == item)
        .ok_or("原跨磁盘项目不存在。")?;
    if entry.state != "moved" || entry.source_trash_receipt.is_none() {
        return Err("本项没有确认完成的跨磁盘移动收据，请先在 Finder 核查。".into());
    }
    let plan = super::journal::snapshot(c, run)?;
    let original = plan
        .source
        .items
        .iter()
        .find(|i| i.id == item && i.path == entry.path)
        .cloned()
        .ok_or("原计划与项目位置不一致。")?;
    if plan.source.origin() != MODULE
        || !plan.source.allows(Path::new(&entry.path))
        || plan
            .movement
            .targets
            .iter()
            .find(|t| t.id == item)
            .and_then(|t| t.destination.as_ref())
            != entry.target.as_ref()
    {
        return Err("原计划的目标或范围不一致。".into());
    }
    let copy = native::recorded(&entry)?;
    if copy.witness.size != original.size
        || original
            .witness
            .nodes
            .last()
            .is_none_or(|n| n.dev == copy.receipt.device)
    {
        return Err("原记录的跨卷关系或大小无法核对。".into());
    }
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(&plan, &entry)).map_err(|e| e.to_string())?)
    );
    Ok((entry, original, fingerprint))
}
impl Plan {
    fn prepare(
        selected: (Entry, Item, String),
        run: &str,
        item: &str,
        home: &Path,
        settings: &db::Settings,
        job: &Job,
    ) -> Result<Self, String> {
        let (entry, original, source_hash) = selected;
        if entry.id != item {
            return Err("恢复项目与原记录不一致。".into());
        }
        let copy = native::recorded(&entry)?;
        native::verify_copy(&copy, job, Instant::now())?;
        let parent = Path::new(&entry.path)
            .parent()
            .ok_or("原位置缺少父目录。")?;
        let held = movement::directory(parent.to_str().ok_or("原位置无法编码。")?, settings)?;
        // The restore destination is the original recorded physical directory,
        // not a replacement at the same spelling after a mount/parent change.
        if original.witness.nodes[..original.witness.nodes.len().saturating_sub(1)]
            != held.witness.nodes
        {
            return Err("原位置目录或磁盘身份变化，请在 Finder 核对。".into());
        }
        let now = db::now();
        let id = uuid::Uuid::new_v4().to_string();
        let item = Item {
            id: entry.id.clone(),
            path: copy.receipt.destination.clone(),
            size: original.size,
            modified_at: original.modified_at,
            witness: copy.witness,
        };
        let plan = Self {
            draft: Draft {
                id,
                run_id: run.into(),
                item_id: item.id.clone(),
                copy_path: item.path.clone(),
                original_path: entry.path.clone(),
                bytes: item.size,
                created_at: now,
                expires_at: now + 120_000,
            },
            source_hash,
            item: item.clone(),
            movement: movement::Movement {
                directory: parent.to_string_lossy().into_owned(),
                witness: held.witness,
                conflict: movement::Conflict::Skip,
                targets: vec![movement::Target {
                    id: item.id,
                    destination: Some(entry.path),
                }],
            },
            sha256: copy.sha256,
            metadata_sha256: copy.metadata_sha256,
            home: home.into(),
            uid: unsafe { libc::geteuid() },
        };
        plan.gate(settings, home, job)?;
        Ok(plan)
    }
    fn binding(&self, c: &db::Database) -> Result<(), String> {
        crate::personal_files::automation::allow_restoration(c, &self.movement.witness.nodes)?;
        let (_, _, digest) = source(c, &self.draft.run_id, &self.draft.item_id)?;
        if digest != self.source_hash {
            return Err("原跨磁盘记录已变化，请重新准备恢复。".into());
        }
        Ok(())
    }
    fn gate(&self, settings: &db::Settings, home: &Path, job: &Job) -> Result<(), String> {
        stopped(job, Instant::now())?;
        if self.uid == 0
            || self.uid != unsafe { libc::geteuid() }
            || self.uid != unsafe { libc::getuid() }
            || self.home != home
            || !settings.acknowledged
        {
            return Err("恢复账户或首次使用状态变化。".into());
        }
        for path in [&self.draft.copy_path, &self.draft.original_path] {
            if !ordinary(Path::new(path)) || protected(Path::new(path), &settings.protected_paths)?
            {
                return Err("恢复副本或原位置已受保护。".into());
            }
        }
        self.movement
            .validate_record(std::slice::from_ref(&self.item))?;
        let directory = self.movement.validate_directory(settings)?;
        let current = AnchoredPath::open(Path::new(&self.item.path))?;
        let m = current.target().metadata().map_err(|e| e.to_string())?;
        if current.witness != self.item.witness
            || !current.local_materialized()
            || !m.is_file()
            || m.nlink() != 1
            || m.uid() != self.uid
        {
            return Err("恢复来源副本已变化，或不再是本账户的本地普通文件。".into());
        }
        match std::fs::symlink_metadata(&self.draft.original_path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => return Err("原位置已有同名项目，未覆盖或自动编号。".into()),
            Err(_) => return Err("原位置是否空闲无法核对。".into()),
        }
        use std::os::fd::AsRawFd;
        let d = directory.target();
        if crate::permissions::read_only(d).map_err(|_| "原位置写入状态无法核对。")?
            || crate::permissions::has_acl(d).map_err(|_| "原位置 ACL 无法核对。")?
            || unsafe {
                libc::faccessat(
                    d.as_raw_fd(),
                    c".".as_ptr(),
                    libc::W_OK | libc::X_OK,
                    libc::AT_EACCESS,
                )
            } != 0
        {
            return Err("原位置存在写入限制，未解除权限。".into());
        }
        Ok(())
    }
}
fn now_valid(plan: &Plan, issued: Instant) -> Result<(), String> {
    if db::now() < plan.draft.created_at
        || db::now() >= plan.draft.expires_at
        || issued.elapsed() >= Duration::from_secs(120)
    {
        return Err("跨磁盘恢复确认已过期，请重新检查。".into());
    }
    Ok(())
}
fn perform(
    plan: &Plan,
    job: &Job,
    settings: impl Fn() -> Result<db::Settings, String>,
    binding: impl Fn() -> Result<(), String>,
    r: &mut Recovery,
    save: impl Fn(&Recovery) -> Result<(), String>,
) -> Result<(), String> {
    let began = Instant::now();
    binding()?;
    plan.gate(&settings()?, &plan.home, job)?;
    let entry = Entry {
        id: plan.item.id.clone(),
        path: plan.draft.original_path.clone(),
        target: Some(plan.item.path.clone()),
        temporary_path: None,
        state: "moved".into(),
        reason: String::new(),
        copy_receipt: Some(Receipt {
            destination: plan.item.path.clone(),
            device: plan.item.witness.nodes.last().ok_or("缺少副本身份。")?.dev,
            inode: plan.item.witness.nodes.last().ok_or("缺少副本身份。")?.ino,
        }),
        source_trash_receipt: None,
        sha256: Some(plan.sha256.clone()),
        metadata_sha256: Some(plan.metadata_sha256.clone()),
        copy_witness: Some(plan.item.witness.clone()),
    };
    let existing = native::recorded(&entry)?;
    native::verify_copy(&existing, job, began)?;
    r.phase = "intent".into();
    r.reason = "正在恢复副本到原位置；原磁盘副本和原废纸篓项目均不处理。".into();
    save(r)?;
    let copied = native::copy(
        &plan.item,
        &plan.movement,
        &plan.draft.original_path,
        Path::new(&r.temporary_path),
        job,
        began,
        || {
            binding()?;
            plan.gate(&settings()?, &plan.home, job)
        },
    )?;
    if copied.sha256 != plan.sha256 || copied.metadata_sha256 != plan.metadata_sha256 {
        return Err("恢复后的内容或元数据未完整核对，请检查原位置。".into());
    }
    native::verify_copy(&copied, job, began)?;
    r.receipt = Some(copied.receipt);
    r.state = "restored".into();
    r.reason = "文件已核验并恢复到原位置。另一磁盘的副本保留，原废纸篓项目未改动。".into();
    Ok(())
}
