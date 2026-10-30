//! Bounded, read-only mounted-volume selection. A selection is a short-lived
//! locator for analysis, never file mutation authority or a portable identity.
use crate::{
    observation::{AnchoredPath, Witness},
    AppState,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    mem::MaybeUninit,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
const MAX: usize = 32;
const TTL: i64 = 120_000;
#[derive(Clone, Copy, Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
enum Kind {
    Account,
    Startup,
    Mounted,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Volume {
    id: String,
    name: String,
    path: String,
    kind: Kind,
    file_system: Option<String>,
    read_only: Option<bool>,
    total_bytes: Option<u64>,
    available_bytes: Option<u64>,
    can_analyze: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    version: u32,
    created_at: i64,
    expires_at: i64,
    complete: bool,
    volumes: Vec<Volume>,
}
struct Offered {
    path: PathBuf,
    kind: Kind,
    witness: Option<Witness>,
    binding: String,
    created_at: i64,
    issued: Instant,
}
#[derive(Default)]
pub(crate) struct Store {
    generation: u64,
    entries: HashMap<String, Offered>,
}
impl Store {
    fn begin(&mut self) -> Result<u64, String> {
        self.generation = self.generation.checked_add(1).ok_or("请重新打开应用。")?;
        self.entries.clear();
        Ok(self.generation)
    }
    fn publish(
        &mut self,
        ticket: u64,
        report: &Report,
        entries: HashMap<String, Offered>,
    ) -> Result<(), String> {
        if ticket != self.generation || entries.len() > MAX || report.volumes.len() > MAX {
            return Err("磁盘列表已被刷新，请重新选择。".into());
        }
        self.entries = entries;
        Ok(())
    }
    fn take(&mut self, id: &str) -> Result<Offered, String> {
        let offered = self
            .entries
            .remove(id)
            .ok_or("磁盘选择已过期、使用或被刷新，请重新读取。")?;
        offered.fresh()?;
        Ok(offered)
    }
}
impl Offered {
    fn fresh(&self) -> Result<(), String> {
        let now = crate::db::now();
        if now < self.created_at
            || now >= self.created_at + TTL
            || self.issued.elapsed() >= Duration::from_millis(TTL as u64)
        {
            return Err("磁盘选择已过期，请刷新列表。".into());
        }
        Ok(())
    }
    fn resolve(&self) -> Result<String, String> {
        self.fresh()?;
        let (_, current) = observe(&self.path, self.kind, self.created_at)?;
        let current = current.ok_or("磁盘已离线或位置无法核对，请刷新列表。")?;
        if current.binding != self.binding
            || current.witness.as_ref().map(|w| &w.nodes) != self.witness.as_ref().map(|w| &w.nodes)
        {
            return Err("磁盘或挂载位置已变化，请刷新后重新选择。".into());
        }
        self.fresh()?;
        Ok(self.path.to_str().ok_or("磁盘路径无法编码。")?.into())
    }
}
fn string(bytes: &[i8]) -> Result<String, String> {
    let end = bytes
        .iter()
        .position(|b| *b == 0)
        .ok_or("挂载信息不完整。")?;
    String::from_utf8(bytes[..end].iter().map(|b| *b as u8).collect())
        .map_err(|_| "挂载信息无法编码。".into())
}
fn external_root(path: &Path) -> bool {
    path.parent() == Some(Path::new("/Volumes"))
        && path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| !s.starts_with('.') && !s.chars().any(char::is_control))
}
fn roots() -> Result<(Vec<PathBuf>, bool), String> {
    // getmntinfo owns a shared mutable buffer; getfsstat fills our own memory.
    let mut buffer: Vec<libc::statfs> = (0..256).map(|_| unsafe { std::mem::zeroed() }).collect();
    let count = unsafe {
        libc::getfsstat(
            buffer.as_mut_ptr(),
            std::mem::size_of_val(buffer.as_slice()) as i32,
            libc::MNT_NOWAIT,
        )
    };
    if count < 0 || count as usize >= buffer.len() {
        return Err("挂载列表无法完整读取，请稍后刷新。".into());
    }
    let mut paths = Vec::new();
    let mut complete = true;
    for raw in buffer.into_iter().take(count as usize) {
        let path = match string(&raw.f_mntonname) {
            Ok(p) => PathBuf::from(p),
            Err(_) => {
                complete = false;
                continue;
            }
        };
        if raw.f_flags & libc::MNT_LOCAL as u32 != 0 && external_root(&path) {
            paths.push(path);
        }
    }
    paths.sort();
    paths.dedup();
    if paths.len() > MAX - 2 {
        complete = false;
        paths.truncate(MAX - 2);
    }
    Ok((paths, complete))
}
fn capacity(raw: &libc::statfs) -> (Option<u64>, Option<u64>) {
    let total = raw.f_blocks.checked_mul(raw.f_bsize as u64);
    let available = raw.f_bavail.checked_mul(raw.f_bsize as u64);
    match (total, available) {
        (Some(t), Some(a)) if t > 0 && t <= 9_007_199_254_740_991 && a <= t => (Some(t), Some(a)),
        _ => (None, None),
    }
}
fn observe(path: &Path, kind: Kind, created_at: i64) -> Result<(Volume, Option<Offered>), String> {
    let text = path
        .to_str()
        .filter(|s| s.len() <= 4096 && !s.chars().any(char::is_control))
        .ok_or("磁盘路径无法编码。")?;
    let mut row = Volume {
        id: uuid::Uuid::new_v4().to_string(),
        name: if kind != Kind::Mounted {
            "".into()
        } else {
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .into()
        },
        path: text.into(),
        kind,
        file_system: None,
        read_only: None,
        total_bytes: None,
        available_bytes: None,
        can_analyze: false,
    };
    let result = (|| -> Result<Offered, String> {
        // The shared mutation-oriented witness deliberately rejects `/`.
        // Read only this fixed root descriptor here; never extend that witness
        // API or grant root/child mutation authority through a volume locator.
        let held = if kind == Kind::Startup {
            if path != Path::new("/") {
                return Err("磁盘挂载位置已变化。".into());
            }
            None
        } else {
            Some(AnchoredPath::open(path)?)
        };
        let root_file = if held.is_none() {
            Some(std::fs::File::open("/").map_err(|e| e.to_string())?)
        } else {
            None
        };
        let file = held
            .as_ref()
            .map(|a| a.target())
            .or(root_file.as_ref())
            .ok_or("磁盘位置无法核对。")?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_dir() || held.as_ref().is_some_and(|a| !a.local_materialized()) {
            return Err("磁盘位置不是可读取的本地物理目录。".into());
        }
        let mut raw = MaybeUninit::<libc::statfs>::zeroed();
        if unsafe { libc::fstatfs(file.as_raw_fd(), raw.as_mut_ptr()) } != 0 {
            return Err("磁盘容量无法读取。".into());
        }
        let raw = unsafe { raw.assume_init() };
        if raw.f_flags & libc::MNT_LOCAL as u32 == 0
            || (kind == Kind::Startup
                && (path != Path::new("/") || string(&raw.f_mntonname)? != "/"))
            || (kind == Kind::Mounted
                && (Path::new(&string(&raw.f_mntonname)?) != path || !external_root(path)))
        {
            return Err("磁盘挂载位置已变化。".into());
        }
        let kind_name = string(&raw.f_fstypename)?;
        let fsid = unsafe {
            std::slice::from_raw_parts(
                (&raw.f_fsid as *const libc::fsid_t).cast::<u8>(),
                std::mem::size_of::<libc::fsid_t>(),
            )
        };
        let binding = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    path,
                    kind,
                    &kind_name,
                    raw.f_flags & (libc::MNT_LOCAL | libc::MNT_RDONLY) as u32,
                    raw.f_mntfromname.to_vec(),
                    raw.f_mntonname.to_vec(),
                    fsid,
                    metadata.dev(),
                    metadata.ino()
                ))
                .map_err(|e| e.to_string())?
            )
        );
        if let Some(held) = &held {
            held.validate()?;
        } else {
            let after = std::fs::metadata("/").map_err(|e| e.to_string())?;
            if (metadata.dev(), metadata.ino(), metadata.mode())
                != (after.dev(), after.ino(), after.mode())
            {
                return Err("磁盘挂载位置已变化。".into());
            }
        }
        let (total, available) = capacity(&raw);
        row.file_system = Some(kind_name);
        row.read_only = Some(raw.f_flags & libc::MNT_RDONLY as u32 != 0);
        row.total_bytes = total;
        row.available_bytes = available;
        row.can_analyze = true;
        Ok(Offered {
            path: path.into(),
            kind,
            witness: held.map(|a| a.witness),
            binding,
            created_at,
            issued: Instant::now(),
        })
    })();
    Ok((row, result.ok()))
}
fn collect(home: &Path) -> Result<(Report, HashMap<String, Offered>), String> {
    let created_at = crate::db::now();
    let began = Instant::now();
    let (mounted, mut complete) = roots()?;
    let mut seen = HashSet::new();
    let mut volumes = Vec::new();
    let mut entries = HashMap::new();
    for (path, kind) in [
        (PathBuf::from("/"), Kind::Startup),
        (home.to_owned(), Kind::Account),
    ]
    .into_iter()
    .chain(mounted.into_iter().map(|p| (p, Kind::Mounted)))
    {
        if !seen.insert(path.clone()) {
            continue;
        }
        if began.elapsed() >= Duration::from_secs(2) {
            complete = false;
            break;
        }
        match observe(&path, kind, created_at) {
            Ok((row, entry)) => {
                if let Some(entry) = entry {
                    entries.insert(row.id.clone(), entry);
                } else {
                    complete = false;
                }
                volumes.push(row);
            }
            Err(_) => complete = false,
        }
    }
    Ok((
        Report {
            version: 1,
            created_at,
            expires_at: created_at + TTL,
            complete,
            volumes,
        },
        entries,
    ))
}
#[tauri::command]
pub(crate) async fn get_mounted_volumes(
    state: tauri::State<'_, AppState>,
) -> Result<Report, String> {
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "文件读取仍在进行，请稍后刷新磁盘。")?;
    let operation = state.updates.installation.enter()?;
    let ticket = state
        .mounted_volumes
        .lock()
        .map_err(|e| e.to_string())?
        .begin()?;
    let home = state.home.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let (_slot, _operation) = (slot, operation);
        collect(&home)
    });
    let (report, entries) = tokio::time::timeout(Duration::from_secs(3), worker)
        .await
        .map_err(|_| "磁盘读取尚未返回，原读取仍可能在结束；不会自动重试。")?
        .map_err(|e| e.to_string())??;
    state
        .mounted_volumes
        .lock()
        .map_err(|e| e.to_string())?
        .publish(ticket, &report, entries)?;
    Ok(report)
}
#[tauri::command]
pub(crate) async fn resolve_mounted_volume(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "文件读取仍在进行，请稍后选择磁盘。")?;
    let operation = state.updates.installation.enter()?;
    let offered = state
        .mounted_volumes
        .lock()
        .map_err(|e| e.to_string())?
        .take(&id)?;
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let (_slot, _operation) = (slot, operation);
        offered.resolve()
    });
    tokio::time::timeout(Duration::from_secs(3), worker)
        .await
        .map_err(|_| "磁盘复查尚未返回，请稍后刷新列表；不会自动开始分析。")?
        .map_err(|e| e.to_string())?
}
#[cfg(test)]
pub(crate) fn owned_selected_mount(path: &Path) -> Result<String, String> {
    let home = std::env::var_os("HOME").ok_or("缺少账户目录。")?;
    let (report, mut entries) = collect(Path::new(&home))?;
    let row = report
        .volumes
        .iter()
        .find(|v| v.kind == Kind::Mounted && Path::new(&v.path) == path)
        .ok_or("未观察到指定的已挂载本地卷。")?;
    entries
        .remove(&row.id)
        .ok_or("样例卷当前无法核对。")?
        .resolve()
}
#[cfg(test)]
mod tests;
