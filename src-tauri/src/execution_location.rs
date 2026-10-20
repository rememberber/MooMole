//! Read-only Finder handoff for an exact persisted receipt. Never restores,
//! changes journal state, follows observed links or accepts a destination in IPC.
//! Trash may allow an exact file open while denying a directory open/enumeration.
//! This locator uses metadata-only ancestor checks; cleanup keeps AnchoredPath.
use crate::{execution, observation::Node};
use std::{
    fs::{File, Metadata, OpenOptions},
    os::{
        fd::AsRawFd,
        macos::fs::MetadataExt as _,
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, Path, PathBuf},
    time::Duration,
};

const MISSING: &str = "记录位置已不存在或无法访问，可能已被移走、放回或权限受限；原记录保持不变。";
fn chain(path: &Path) -> Result<Vec<Node>, String> {
    if !path.is_absolute()
        || path.parent().is_none()
        || path
            .as_os_str()
            .as_bytes()
            .split(|c| *c == b'/')
            .any(|part| part == b"." || part == b"..")
    {
        return Err("收据位置无效，请先核对记录。".into());
    }
    let parts = path
        .components()
        .filter_map(|part| {
            if let Component::Normal(name) = part {
                Some(name)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 128 {
        return Err("收据位置层级无效。".into());
    }
    let mut current = PathBuf::from("/");
    let mut result = Vec::new();
    for index in 0..=parts.len() {
        if index > 0 {
            current.push(parts[index - 1]);
        }
        let meta = std::fs::symlink_metadata(&current).map_err(|_| MISSING)?;
        if (index < parts.len() && !meta.is_dir()) || (index == parts.len() && !meta.is_file()) {
            return Err("记录位置或祖先是链接或特殊对象，无法核对原文件。".into());
        }
        if index == parts.len()
            && (meta.nlink() != 1
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.st_flags() & 0x40000000 != 0)
        {
            return Err("记录位置不是本账户已落地的单链接文件，无法自动定位。".into());
        }
        result.push(Node {
            path: current.clone(),
            dev: meta.dev(),
            ino: meta.ino(),
            mode: meta.mode(),
            uid: meta.uid(),
            gid: meta.gid(),
        });
    }
    Ok(result)
}
fn observation(meta: &Metadata) -> (u64, i64, i64, i64, i64, u32) {
    (
        meta.size(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
        meta.st_flags(),
    )
}
pub(crate) struct Location {
    held: File,
    path: PathBuf,
    nodes: Vec<Node>,
    observed: (u64, i64, i64, i64, i64, u32),
    device: u64,
    inode: u64,
}
impl Location {
    pub fn prepare(detail: &execution::Detail, item_id: &str) -> Result<Self, String> {
        if detail.summary.method != "trash" {
            return Err("此记录不是废纸篓处理，无法定位。".into());
        }
        let item = detail
            .items
            .iter()
            .find(|item| item.id == item_id)
            .ok_or("本批次没有该项目，请刷新记录。")?;
        if !matches!(item.state.as_str(), "moved" | "uncertain") {
            return Err("此项没有可核对的移入结果，不会猜测废纸篓位置。".into());
        }
        let receipt = item
            .receipt
            .as_ref()
            .ok_or("此项没有实际位置收据，无法自动定位；请先核对实际结果。")?;
        let path = PathBuf::from(&receipt.destination);
        let nodes = chain(&path)?;
        let held = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(&path)
            .map_err(|_| MISSING)?;
        let meta = held.metadata().map_err(|_| MISSING)?;
        let result = Self {
            held,
            path,
            nodes,
            observed: observation(&meta),
            device: receipt.device,
            inode: receipt.inode,
        };
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self) -> Result<(), String> {
        let meta = self.held.metadata().map_err(|_| MISSING)?;
        let nodes = chain(&self.path)?;
        let named = nodes.last().unwrap();
        let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
        let local = unsafe { libc::fstatfs(self.held.as_raw_fd(), stat.as_mut_ptr()) } == 0
            && unsafe { stat.assume_init() }.f_flags & libc::MNT_LOCAL as u32 != 0;
        if nodes != self.nodes
            || !meta.is_file()
            || meta.nlink() != 1
            || meta.dev() != self.device
            || meta.ino() != self.inode
            || named.dev != self.device
            || named.ino != self.inode
            || observation(&meta) != self.observed
            || !local
        {
            return Err(
                "记录位置的文件身份或本地状态已变化，无法确认是原对象；不会定位替代文件。".into(),
            );
        }
        Ok(())
    }
    pub async fn reveal(self) -> Result<(), String> {
        // Finder selection is not atomic with external changes; this does not
        // authorize Put Back, overwrite or any other filesystem mutation.
        self.validate()?;
        let mut command = tokio::process::Command::new("/usr/bin/open");
        command
            .args(["-R", "--"])
            .arg(&self.path)
            .kill_on_drop(true);
        let status = tokio::time::timeout(Duration::from_secs(5), command.status())
            .await
            .map_err(|_| "Finder 定位超时，请重试；处理记录保持不变。")?
            .map_err(|_| "无法请求 Finder 定位，请手动核对记录位置。")?;
        if !status.success() {
            return Err("Finder 未接受定位请求，请手动核对记录位置。".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
