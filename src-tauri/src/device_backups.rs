//! Whole, explicitly selected local device backups. Never edits their contents.
use crate::{
    db,
    engine::{Job, TaskKind},
    execution::Receipt,
    observation::{AnchoredPath, Witness},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
pub(crate) mod commands;
mod journal;
mod metadata;
mod snapshot;
#[cfg(test)]
mod tests;
const REVISION: &str = "MooMole.device-backups.v1";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Row {
    id: String,
    path: String,
    name: String,
    device_name: Option<String>,
    product: Option<String>,
    encrypted: Option<bool>,
    marked_finished: bool,
    modified_at: i64,
    size: Option<u64>,
    entries: Option<usize>,
    eligible: bool,
    reason: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Candidate {
    row: Row,
    witness: Witness,
    metadata: metadata::Metadata,
    snapshot: snapshot::Snapshot,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    id: String,
    root: String,
    created_at: i64,
    expires_at: i64,
    complete: bool,
    cancelled: bool,
    reason: String,
    items: Vec<Row>,
}
struct Scan {
    report: Report,
    root: Witness,
    candidates: Vec<Candidate>,
    issued: Instant,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Plan {
    revision: String,
    id: String,
    scan_id: String,
    root: String,
    root_witness: Witness,
    item: Candidate,
    created_at: i64,
    expires_at: i64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Draft {
    id: String,
    expires_at: i64,
    item: Row,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Record {
    revision: String,
    id: String,
    request_id: String,
    created_at: i64,
    finished_at: Option<i64>,
    state: String,
    phase: String,
    plan: Plan,
    reason: String,
    receipt: Option<Receipt>,
    reviewed_at: Option<i64>,
}
#[derive(Default)]
pub(crate) struct Store {
    phase: &'static str,
    request: Option<String>,
    scan: Option<Scan>,
    offers: HashMap<String, (Plan, Instant)>,
    error: Option<String>,
}
#[derive(Clone)]
struct Context {
    home: PathBuf,
    settings: db::Settings,
}
fn id(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok()
}
fn protected(path: &Path, settings: &db::Settings) -> Result<bool, String> {
    let raw = path.to_str().ok_or("备份路径无法编码。")?;
    let folded = PathBuf::from(raw.to_lowercase());
    Ok(!crate::app_protection::check(
        raw,
        None,
        None,
        "io.github.rememberber.moomole",
        &settings.protected_paths,
    )?
    .reasons
    .is_empty()
        || settings.protected_paths.iter().any(|r| {
            let r = PathBuf::from(r.to_lowercase());
            folded.starts_with(&r) || r.starts_with(&folded)
        }))
}
fn root(ctx: &Context, path: &Path) -> Result<AnchoredPath, String> {
    let fixed = ctx
        .home
        .join("Library/Application Support/MobileSync/Backup");
    let folded = path.to_string_lossy().to_lowercase();
    // Reading a parent is allowed when one of its children is protected. That
    // child (and a backup containing it) remains ineligible for whole moves.
    let mut root_settings = ctx.settings.clone();
    root_settings.protected_paths.retain(|rule| {
        let rule = rule.to_lowercase();
        !Path::new(&rule).starts_with(Path::new(&folded)) || rule == folded
    });
    if path == ctx.home
        || protected(path, &root_settings)?
        || !path.is_absolute()
        || path.components().any(|c| {
            !matches!(
                c,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
        || (folded != fixed.to_string_lossy().to_lowercase()
            && path.components().any(|c| {
                let n = c.as_os_str().to_string_lossy().to_lowercase();
                n.starts_with('.')
                    || ["library", "system", "applications", "keychains"].contains(&n.as_str())
                    || [".app", ".photoslibrary", ".photolibrary"]
                        .iter()
                        .any(|e| n.ends_with(e))
            }))
    {
        return Err("请选择普通本地备份副本目录，或当前账户的 MobileSync/Backup；保护、应用与资料库位置保留。".into());
    }
    let held = AnchoredPath::open(path)?;
    let m = held.target().metadata().map_err(|e| e.to_string())?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || !held.local_materialized() {
        return Err("备份目录不是本账户拥有的已落地本地目录。".into());
    }
    let home = AnchoredPath::open(&ctx.home)?;
    if held.witness.nodes.last().map(|n| (n.dev, n.ino))
        == home.witness.nodes.last().map(|n| (n.dev, n.ino))
    {
        return Err("请勿将整个账户目录作为备份范围。".into());
    }
    Ok(held)
}
use std::os::unix::fs::MetadataExt;
fn collect(ctx: &Context, path: &Path, job: &Job) -> Result<Scan, String> {
    let held = root(ctx, path)?;
    let mut budget = snapshot::Budget::new(job);
    let names = snapshot::names(held.target(), &mut budget)?;
    let mut complete = true;
    let mut reason = String::new();
    let mut rows = vec![];
    let mut candidates = vec![];
    for name in names.into_iter().take(129) {
        if rows.len() == 128 {
            complete = false;
            reason = "备份目录超过本轮 128 项，请选择更具体的副本目录。".into();
            break;
        }
        if let Err(e) = budget.check() {
            complete = false;
            reason = e;
            break;
        }
        let child = path.join(std::ffi::OsStr::from_bytes(name.to_bytes()));
        if child
            .to_str()
            .is_none_or(|s| s.len() > 4096 || s.chars().any(char::is_control))
        {
            complete = false;
            reason = "部分项目名称无法完整显示，本轮保留。".into();
            continue;
        }
        let mut row = Row {
            id: uuid::Uuid::new_v4().to_string(),
            path: child.to_string_lossy().into_owned(),
            name: name.to_string_lossy().into_owned(),
            device_name: None,
            product: None,
            encrypted: None,
            marked_finished: false,
            modified_at: 0,
            size: None,
            entries: None,
            eligible: false,
            reason: String::new(),
        };
        let observed = (|| -> Result<Candidate, String> {
            let folded = row.name.to_lowercase();
            if row.name.starts_with('.')
                || [
                    ".app",
                    ".bundle",
                    ".framework",
                    ".photoslibrary",
                    ".photolibrary",
                ]
                .iter()
                .any(|suffix| folded.ends_with(suffix))
                || protected(&child, &ctx.settings)?
            {
                return Err("隐藏、应用资料库或受保护项目保留。".into());
            }
            let target = AnchoredPath::open(&child)?;
            let m = target.target().metadata().map_err(|e| e.to_string())?;
            if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || !target.local_materialized()
            {
                return Err("不是可核对的本账户本地备份目录。".into());
            }
            row.modified_at = m.mtime().saturating_mul(1000).max(0);
            let metadata = metadata::read(&target)?;
            row.device_name = Some(metadata.device.clone());
            row.product = metadata.product.clone();
            row.encrypted = metadata.encrypted;
            row.marked_finished = metadata.finished;
            let snap = snapshot::inspect(target.target(), &child, &ctx.settings, &mut budget)?;
            target.validate()?;
            row.size = Some(snap.bytes);
            row.entries = Some(snap.entries);
            let access = crate::permissions::observe(&target, &crate::permissions::Budget::new())?;
            if access.code != crate::permissions::Code::NoRestrictionObserved {
                return Err(access.refusal_reason());
            }
            if !metadata.finished {
                return Err(
                    "状态文件未标记完成，本轮保留；请在 Finder 检查备份或同步状态。".into(),
                );
            }
            row.eligible = true;
            row.reason = "状态文件标记完成；不证明备份可恢复、存在其他副本或当前未被使用。".into();
            Ok(Candidate {
                row: row.clone(),
                witness: target.witness,
                metadata,
                snapshot: snap,
            })
        })();
        match observed {
            Ok(c) => candidates.push(c),
            Err(e) => {
                row.reason = e;
                complete = false;
            }
        }
        rows.push(row);
    }
    held.validate()?;
    let now = db::now();
    Ok(Scan {
        report: Report {
            id: uuid::Uuid::new_v4().to_string(),
            root: path.to_string_lossy().into_owned(),
            created_at: now,
            expires_at: now + 900_000,
            complete,
            cancelled: job.cancelled.load(Ordering::SeqCst),
            reason,
            items: rows,
        },
        root: held.witness,
        candidates,
        issued: Instant::now(),
    })
}
use std::os::unix::ffi::OsStrExt;
impl Store {
    fn prepare(&mut self, scan_id: &str, item_id: &str, ctx: &Context) -> Result<Draft, String> {
        self.offers.clear();
        let scan = self
            .scan
            .as_ref()
            .filter(|s| s.report.id == scan_id)
            .ok_or("备份扫描已变化，请重新扫描。")?;
        let now = db::now();
        if now < scan.report.created_at
            || now >= scan.report.expires_at
            || scan.issued.elapsed() >= Duration::from_secs(900)
        {
            return Err("备份扫描已过期。".into());
        }
        let c = scan
            .candidates
            .iter()
            .find(|c| c.row.id == item_id && c.row.eligible)
            .ok_or("本项尚未满足处理条件，保留。")?
            .clone();
        let current = root(ctx, Path::new(&scan.report.root))?;
        if current.witness != scan.root
            || AnchoredPath::open(Path::new(&c.row.path))?.witness != c.witness
        {
            return Err("备份目录已变化，请重新扫描。".into());
        }
        if protected(Path::new(&c.row.path), &ctx.settings)? {
            return Err("备份已受保护。".into());
        }
        let plan = Plan {
            revision: REVISION.into(),
            id: uuid::Uuid::new_v4().to_string(),
            scan_id: scan_id.into(),
            root: scan.report.root.clone(),
            root_witness: scan.root.clone(),
            item: c,
            created_at: now,
            expires_at: now + 30_000,
        };
        let d = Draft {
            id: plan.id.clone(),
            expires_at: plan.expires_at,
            item: plan.item.row.clone(),
        };
        self.offers.insert(
            plan.id.clone(),
            (plan, Instant::now() + Duration::from_secs(30)),
        );
        Ok(d)
    }
    fn take(&mut self, id: &str) -> Result<Plan, String> {
        let (p, deadline) = self.offers.remove(id).ok_or("备份确认已使用或撤下。")?;
        let now = db::now();
        if now < p.created_at || now >= p.expires_at || Instant::now() >= deadline {
            return Err("备份确认已过期，请重新检查。".into());
        }
        Ok(p)
    }
}
fn validate(p: &Plan, ctx: &Context, job: &Job) -> Result<AnchoredPath, String> {
    if !ctx.settings.acknowledged
        || p.revision != REVISION
        || !p.item.row.eligible
        || !p.item.metadata.finished
        || protected(Path::new(&p.item.row.path), &ctx.settings)?
    {
        return Err("备份范围、确认或保护状态无法核对。".into());
    }
    let directory = root(ctx, Path::new(&p.root))?;
    if directory.witness.nodes != p.root_witness.nodes {
        return Err("备份来源目录身份变化。".into());
    }
    let path = Path::new(&p.item.row.path);
    if path.parent() != Some(Path::new(&p.root)) {
        return Err("备份不属于确认的直属目录。".into());
    }
    let held = AnchoredPath::open(path)?;
    if held.witness != p.item.witness || metadata::read(&held)? != p.item.metadata {
        return Err("备份身份、状态或清单已变化，保留。".into());
    }
    let actual = snapshot::inspect(
        held.target(),
        path,
        &ctx.settings,
        &mut snapshot::Budget::new(job),
    )?;
    if actual != p.item.snapshot {
        return Err("备份目录内容已变化，保留。".into());
    }
    directory.validate()?;
    held.validate()?;
    Ok(held)
}
fn native(
    p: &Plan,
    ctx: &Context,
    job: &Job,
    gate: impl FnOnce() -> Result<(), String>,
) -> Result<(String, Option<Receipt>, String), String> {
    use objc2::rc::autoreleasepool;
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    autoreleasepool(|_| {
        let held = validate(p, ctx, job)?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(&p.item.row.path));
        let reference = url
            .fileReferenceURL()
            .ok_or("系统无法为备份建立对象引用。")?;
        if !reference.isFileReferenceURL()
            || reference
                .filePathURL()
                .and_then(|u| u.path())
                .is_none_or(|s| s.to_string() != p.item.row.path)
        {
            return Err("备份对象引用与确认位置不一致。".into());
        }
        held.validate()?;
        let access = crate::permissions::observe(&held, &crate::permissions::Budget::new())?;
        if access.code != crate::permissions::Code::NoRestrictionObserved {
            return Err(access.refusal_reason());
        }
        if job.cancelled.load(Ordering::SeqCst) {
            return Err("已在系统请求前停止，备份保留。".into());
        }
        gate()?;
        held.validate()?;
        let mut result = None;
        let response = NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&reference, Some(&mut result));
        let receipt = result.and_then(|u| u.path()).and_then(|s| {
            let dest = s.to_string();
            std::fs::symlink_metadata(&dest).ok().map(|m| Receipt {
                destination: dest,
                device: m.dev(),
                inode: m.ino(),
            })
        });
        let verified = response.is_ok()
            && receipt.as_ref().is_some_and(|r| {
                let node = p.item.witness.nodes.last().unwrap();
                r.device == node.dev
                    && r.inode == node.ino
                    && std::fs::symlink_metadata(&r.destination).is_ok_and(|m| {
                        m.is_dir()
                            && m.uid() == node.uid
                            && m.mode() == node.mode
                            && m.gid() == node.gid
                    })
                    && std::fs::symlink_metadata(&p.item.row.path)
                        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                    && snapshot::inspect(
                        held.target(),
                        Path::new(&r.destination),
                        &ctx.settings,
                        &mut snapshot::Budget::receipt(),
                    )
                    .is_ok_and(|s| s == p.item.snapshot)
                    && AnchoredPath::open(Path::new(&p.root))
                        .is_ok_and(|a| a.witness.nodes == p.root_witness.nodes)
            });
        // After the system request no error may claim nothing happened.
        Ok(if verified {
            (
                "moved".into(),
                receipt,
                "整份备份已移入废纸篓，返回位置、目录身份与内容快照已核对；尚未释放其占用。".into(),
            )
        } else {
            ("unknown".into(),receipt,"系统请求已发出，但结果或目录内容无法完整核对。请核查原位置与实际收据，不会自动重试。".into())
        })
    })
}
