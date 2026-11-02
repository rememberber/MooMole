//! Exact, no-overwrite same-volume moves. No copy/delete fallback or retries.
use super::*;
use crate::{
    db,
    execution::{Outcome, Receipt},
    observation::AnchoredPath,
    permissions,
};
use std::{
    ffi::CString,
    os::{
        fd::AsRawFd,
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::PathBuf,
};

pub(super) const REVISION: &str = "MooMole.personal-files.move.v1";
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Conflict {
    Skip,
    KeepBoth,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Target {
    pub id: String,
    pub destination: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Movement {
    pub directory: String,
    pub witness: Witness,
    pub conflict: Conflict,
    pub targets: Vec<Target>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Confirmation {
    directory: String,
    conflict: Conflict,
    targets: Vec<Target>,
}
impl Movement {
    pub(super) fn confirmation(&self) -> Confirmation {
        Confirmation {
            directory: self.directory.clone(),
            conflict: self.conflict,
            targets: self.targets.clone(),
        }
    }
    pub(crate) fn validate_record(&self, items: &[Item]) -> Result<(), String> {
        let dir = Path::new(&self.directory);
        let mut paths = HashSet::new();
        if !ordinary(dir)
            || self.targets.len() != items.len()
            || self.witness.nodes.last().is_none_or(|n| n.path != dir)
            || self.targets.iter().zip(items).any(|(t, i)| {
                t.id != i.id
                    || t.destination.as_ref().is_some_and(|s| {
                        let p = Path::new(s);
                        !ordinary(p)
                            || p.parent() != Some(dir)
                            || s == &i.path
                            || !paths.insert(s.to_lowercase())
                    })
            })
        {
            return Err("文件移动计划无法核对，请重新选择。".into());
        }
        Ok(())
    }
    pub(crate) fn validate_directory(
        &self,
        settings: &db::Settings,
    ) -> Result<AnchoredPath, String> {
        let held = directory(&self.directory, settings)?;
        // Directory timestamps change as this batch adds entries. Bind identity,
        // owner and mode of every ancestor, rather than treating our own writes
        // as permission to accept a replacement directory.
        if held.witness.nodes != self.witness.nodes {
            return Err("移动目标目录或祖先已变化，本项保留。".into());
        }
        Ok(held)
    }
}
pub(crate) fn directory(path: &str, settings: &db::Settings) -> Result<AnchoredPath, String> {
    if !settings.acknowledged
        || !ordinary(Path::new(path))
        || protected(Path::new(path), &settings.protected_paths)?
    {
        return Err("请选择未受保护的普通目标目录。".into());
    }
    let held = AnchoredPath::open(Path::new(path))?;
    let m = held.target().metadata().map_err(|e| e.to_string())?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || !held.local_materialized() {
        return Err("移动目标必须是本账户拥有、已经落地的本地物理目录。".into());
    }
    Ok(held)
}
pub(super) fn prepare(
    items: &[Item],
    path: &str,
    conflict: Conflict,
    settings: &db::Settings,
) -> Result<Movement, String> {
    prepare_locations(items, path, conflict, settings, true)
}
pub(super) fn prepare_cross_volume(
    items: &[Item],
    path: &str,
    conflict: Conflict,
    settings: &db::Settings,
) -> Result<Movement, String> {
    prepare_locations(items, path, conflict, settings, false)
}
fn prepare_locations(
    items: &[Item],
    path: &str,
    conflict: Conflict,
    settings: &db::Settings,
    same_volume: bool,
) -> Result<Movement, String> {
    let held = directory(path, settings)?;
    let device = held.target().metadata().map_err(|e| e.to_string())?.dev();
    let mut reserved = HashSet::new();
    let mut targets = Vec::new();
    for item in items {
        if item
            .witness
            .nodes
            .last()
            .is_none_or(|n| (n.dev == device) != same_volume)
        {
            return Err("源与目标磁盘关系已变化，请重新选择移动方式。".into());
        }
        let source = Path::new(&item.path);
        let name = source
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("文件名无法编码。")?;
        let mut chosen = None;
        if source.parent() != Some(Path::new(path)) {
            for number in 0..=1000 {
                let candidate = if number == 0 {
                    name.into()
                } else {
                    let stem = source
                        .file_stem()
                        .and_then(|n| n.to_str())
                        .ok_or("文件名无法编码。")?;
                    match source.extension().and_then(|n| n.to_str()) {
                        Some(ext) => format!("{stem} ({number}).{ext}"),
                        None => format!("{stem} ({number})"),
                    }
                };
                if candidate.len() > 255 {
                    return Err("编号后文件名超过目标目录限制，请缩短名称。".into());
                }
                let target = Path::new(path).join(candidate);
                let value = target.to_str().ok_or("目标路径无法编码。")?.to_string();
                if !ordinary(&target) || protected(&target, &settings.protected_paths)? {
                    return Err("移动目标文件受保护，请另选目录。".into());
                }
                let exists = match std::fs::symlink_metadata(&target) {
                    Ok(_) => true,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
                    Err(_) => return Err("无法确认目标名称是否可用，请检查目录权限。".into()),
                };
                if !exists && reserved.insert(value.to_lowercase()) {
                    chosen = Some(value);
                    break;
                }
                if conflict == Conflict::Skip {
                    break;
                }
            }
            if chosen.is_none() && conflict == Conflict::KeepBoth {
                return Err("未找到可用的编号名称，请整理目标目录后重试。".into());
            }
        }
        targets.push(Target {
            id: item.id.clone(),
            destination: chosen,
        });
    }
    held.validate()?;
    let result = Movement {
        directory: path.into(),
        witness: held.witness,
        conflict,
        targets,
    };
    result.validate_record(items)?;
    Ok(result)
}

pub(crate) struct Prepared {
    source: AnchoredPath,
    directory: AnchoredPath,
    destination: PathBuf,
    old_name: CString,
    new_name: CString,
    search_source: bool,
}
impl Prepared {
    pub(crate) fn new(
        item: &Item,
        movement: &Movement,
        target: &str,
        settings: &db::Settings,
    ) -> Result<Self, String> {
        Self::with_source_access(item, movement, target, settings, false)
    }
    /// Restoration already has a verified receipt for this exact source name.
    /// Search-only ancestors avoid requesting directory contents; all native
    /// identity, ACL, flag, volume and write/search checks remain mandatory.
    pub(crate) fn from_receipt(
        item: &Item,
        movement: &Movement,
        target: &str,
        settings: &db::Settings,
    ) -> Result<Self, String> {
        Self::with_source_access(item, movement, target, settings, true)
    }
    fn with_source_access(
        item: &Item,
        movement: &Movement,
        target: &str,
        settings: &db::Settings,
        search_source: bool,
    ) -> Result<Self, String> {
        let source = if search_source {
            AnchoredPath::open_search(Path::new(&item.path))?
        } else {
            AnchoredPath::open(Path::new(&item.path))?
        };
        if source.witness != item.witness || !source.local_materialized() {
            return Err("移动源文件已变化，请重新扫描。".into());
        }
        let directory = movement.validate_directory(settings)?;
        let destination = PathBuf::from(target);
        if movement
            .targets
            .iter()
            .find(|t| t.id == item.id)
            .and_then(|t| t.destination.as_deref())
            != Some(target)
            || !ordinary(&destination)
            || destination.parent() != Some(Path::new(&movement.directory))
            || protected(&destination, &settings.protected_paths)?
            || source.target().metadata().map_err(|e| e.to_string())?.dev()
                != directory
                    .target()
                    .metadata()
                    .map_err(|e| e.to_string())?
                    .dev()
        {
            return Err("移动目标范围或卷已变化，本项保留。".into());
        }
        let old_name = CString::new(
            Path::new(&item.path)
                .file_name()
                .ok_or("源文件名无效。")?
                .as_bytes(),
        )
        .map_err(|e| e.to_string())?;
        let new_name = CString::new(
            destination
                .file_name()
                .ok_or("目标文件名无效。")?
                .as_bytes(),
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            source,
            directory,
            destination,
            old_name,
            new_name,
            search_source,
        })
    }
    pub(crate) fn move_checked(
        self,
        gate: impl FnOnce() -> Result<(), String>,
    ) -> Result<(Outcome, String), String> {
        for held in [&self.source, &self.directory] {
            held.validate()?;
            let access = permissions::observe(held, &permissions::Budget::new())?;
            if access.code != permissions::Code::NoRestrictionObserved {
                return Err(access.refusal_reason());
            }
        }
        if unsafe {
            libc::faccessat(
                self.directory.target().as_raw_fd(),
                c".".as_ptr(),
                libc::W_OK | libc::X_OK,
                libc::AT_EACCESS,
            )
        } != 0
        {
            return Err("目标目录当前不可写入或搜索，本项保留。".into());
        }
        gate()?;
        self.source.validate()?;
        self.directory.validate()?;
        let before = self.source.target().metadata().map_err(|e| e.to_string())?;
        // Darwin RENAME_EXCL refuses an existing destination atomically. Source
        // names can still race external writers; post-validation never hides it.
        let status = unsafe {
            libc::renameatx_np(
                self.source.parent().as_raw_fd(),
                self.old_name.as_ptr(),
                self.directory.target().as_raw_fd(),
                self.new_name.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        if status != 0 {
            let error = std::io::Error::last_os_error();
            return Ok(
                if matches!(
                    error.raw_os_error(),
                    Some(
                        libc::EEXIST
                            | libc::EXDEV
                            | libc::ENOENT
                            | libc::EACCES
                            | libc::EPERM
                            | libc::EINVAL
                            | libc::ENOTSUP
                    )
                ) {
                    (
                        Outcome::Failed,
                        format!("系统拒绝移动，未覆盖目标：{error}"),
                    )
                } else {
                    (
                        Outcome::Uncertain(None),
                        format!("移动系统调用未取得可靠结果，请核对源与目标：{error}"),
                    )
                },
            );
        }
        let receipt = Receipt {
            destination: self
                .destination
                .to_str()
                .ok_or("目标路径无法编码。")?
                .into(),
            device: before.dev(),
            inode: before.ino(),
        };
        let verified = || -> Result<(), String> {
            let at = AnchoredPath::open(&self.destination)?;
            let after = at.target().metadata().map_err(|e| e.to_string())?;
            if !at.local_materialized()
                || !after.is_file()
                || after.nlink() != 1
                || (after.uid(), after.gid(), after.mode())
                    != (before.uid(), before.gid(), before.mode())
                || (
                    after.dev(),
                    after.ino(),
                    after.size(),
                    after.mtime(),
                    after.mtime_nsec(),
                ) != (
                    before.dev(),
                    before.ino(),
                    before.size(),
                    before.mtime(),
                    before.mtime_nsec(),
                )
                || at.witness.nodes[..at.witness.nodes.len() - 1] != self.directory.witness.nodes
            {
                return Err("移动后目标身份或内容属性无法核对。".into());
            }
            // Re-open parent names; held descriptors alone would hide rename.
            let source_parent = self.source.witness.nodes[self.source.witness.nodes.len() - 2]
                .path
                .as_path();
            let current_parent = if self.search_source {
                AnchoredPath::open_search(source_parent)?
            } else {
                AnchoredPath::open(source_parent)?
            };
            if current_parent.witness.nodes
                != self.source.witness.nodes[..self.source.witness.nodes.len() - 1]
            {
                return Err("移动后源目录绑定已变化。".into());
            }
            let original = self.source.witness.nodes.last().unwrap().path.as_path();
            match std::fs::symlink_metadata(original) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err("移动后原路径状态无法核对。".into()),
            }
            self.source
                .parent()
                .sync_all()
                .and_then(|_| self.directory.target().sync_all())
                .map_err(|e| e.to_string())?;
            Ok(())
        };
        Ok(match verified() {
            Ok(()) => (
                Outcome::Moved(receipt),
                "已移至确认的目标位置；原目录与目标身份已核对，未覆盖文件。".into(),
            ),
            Err(error) => (
                Outcome::Uncertain(
                    std::fs::symlink_metadata(&self.destination)
                        .ok()
                        .filter(|m| m.dev() == receipt.device && m.ino() == receipt.inode)
                        .map(|_| receipt),
                ),
                format!("系统已接受移动，但完成证据不足：{error}"),
            ),
        })
    }
}

#[cfg(test)]
mod tests;
