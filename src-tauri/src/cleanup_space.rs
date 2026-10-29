//! Supplementary disk observations, never an execution/space-reclamation proof.
//! Absence or failure must not hide durable item outcomes or authorize a retry.
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub(crate) const TABLE: &str = "cleanup_space_observations";
pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS cleanup_space_observations (
 run_id TEXT PRIMARY KEY REFERENCES execution_runs(id) ON DELETE CASCADE,
 before_sample TEXT CHECK(before_sample IS NULL OR length(before_sample)<=2048),
 after_sample TEXT CHECK(after_sample IS NULL OR length(after_sample)<=2048)
);";
const MAX_EXACT: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Sample {
    pub measured_at: i64,
    pub volume_id: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}
impl Sample {
    fn valid(&self) -> bool {
        self.measured_at >= 0
            && self.measured_at <= MAX_EXACT as i64
            && !self.volume_id.is_empty()
            && self.volume_id.len() <= 128
            && self
                .volume_id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || b == b':')
            && self.total_bytes > 0
            && self.total_bytes <= MAX_EXACT
            && self.available_bytes <= self.total_bytes
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    pub version: u32,
    pub before: Option<Sample>,
    pub after: Option<Sample>,
}

pub(crate) fn has_rows(db: &Connection) -> Result<bool, String> {
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [TABLE],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Ok(false);
    }
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM cleanup_space_observations)",
        [],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}
fn encode(sample: Option<&Sample>) -> Result<Option<String>, String> {
    sample
        .map(|s| {
            if !s.valid() {
                return Err("磁盘空间观察无效。".into());
            }
            serde_json::to_string(s).map_err(|e| e.to_string())
        })
        .transpose()
}
pub(crate) fn begin(db: &Connection, run: &str, sample: Option<&Sample>) -> Result<(), String> {
    let sample = encode(sample)?;
    db.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    let count = db.execute("INSERT INTO cleanup_space_observations(run_id,before_sample) SELECT id,?2 FROM execution_runs WHERE id=?1 AND state='running'", params![run, sample]).map_err(|e| e.to_string())?;
    if count != 1 {
        return Err("空间观察未绑定活动清理批次。".into());
    }
    Ok(())
}
pub(crate) fn finish(db: &Connection, run: &str, sample: Option<&Sample>) -> Result<(), String> {
    let Some(sample) = encode(sample)? else {
        return Ok(());
    };
    let count = db.execute("UPDATE cleanup_space_observations SET after_sample=?2 WHERE run_id=?1 AND after_sample IS NULL AND EXISTS(SELECT 1 FROM execution_runs WHERE id=?1 AND state<>'running')", params![run, sample]).map_err(|e| e.to_string())?;
    if count != 1 {
        return Err("空间观察缺少批次绑定或已记录。".into());
    }
    Ok(())
}
fn decode(raw: Option<String>, earliest: i64) -> Option<Sample> {
    let raw = raw?;
    if raw.len() > 2048 {
        return None;
    }
    let sample: Sample = serde_json::from_str(&raw).ok()?;
    (sample.valid() && sample.measured_at >= earliest).then_some(sample)
}
// Supplemental corruption degrades the metric to unknown; the original raw
// row remains available to the portable archive. Never hide recovery receipts.
pub(crate) fn read(
    db: &Connection,
    run: &str,
    created: i64,
    finished: Option<i64>,
) -> Option<Report> {
    let row: Option<(Option<String>, Option<String>)> = db
        .query_row(
            "SELECT before_sample,after_sample FROM cleanup_space_observations WHERE run_id=?1",
            [run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .ok()?;
    let (before, after) = row?;
    Some(Report {
        version: 1,
        before: decode(before, created),
        after: finished.and_then(|at| decode(after, at)),
    })
}

#[cfg(target_os = "macos")]
fn observe(path: &Path) -> Option<Sample> {
    use std::{
        mem::MaybeUninit,
        os::{fd::AsRawFd, unix::fs::MetadataExt},
    };
    let anchor = crate::observation::AnchoredPath::open(path).ok()?;
    if !anchor.local_materialized() {
        return None;
    }
    let before = anchor.target().metadata().ok()?;
    if !before.is_dir() {
        return None;
    }
    let mut raw = MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: valid held descriptor and writable statfs buffer; success checked.
    if unsafe { libc::fstatfs(anchor.target().as_raw_fd(), raw.as_mut_ptr()) } != 0 {
        return None;
    }
    let raw = unsafe { raw.assume_init() };
    if raw.f_flags & libc::MNT_LOCAL as u32 == 0 || raw.f_bsize == 0 {
        return None;
    }
    use sha2::{Digest, Sha256};
    let mount_identity: Vec<u8> = raw
        .f_mntfromname
        .iter()
        .chain(raw.f_mntonname.iter())
        .map(|b| *b as u8)
        .collect();
    let sample = Sample {
        measured_at: crate::db::now(),
        volume_id: format!("{:x}:{:x}", before.dev(), Sha256::digest(mount_identity)),
        total_bytes: raw.f_blocks.checked_mul(raw.f_bsize as u64)?,
        available_bytes: raw.f_bavail.checked_mul(raw.f_bsize as u64)?,
    };
    anchor.validate().ok()?;
    sample.valid().then_some(sample)
}
#[cfg(not(target_os = "macos"))]
fn observe(_: &Path) -> Option<Sample> {
    None
}

pub(crate) async fn measure(home: PathBuf) -> Option<Sample> {
    // A timed-out OS read keeps its permit until the actual worker ends. No
    // accumulating detached reads after repeated failures or cancellations.
    static SLOTS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
        std::sync::OnceLock::new();
    let slot = SLOTS
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(1)))
        .clone()
        .try_acquire_owned()
        .ok()?;
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tauri::async_runtime::spawn_blocking(move || {
            let _slot = slot;
            observe(&home)
        }),
    )
    .await
    .ok()?
    .ok()?
}

#[cfg(test)]
mod tests;
