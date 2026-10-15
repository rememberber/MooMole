//! Bounded metadata inventory. Directory descriptors and no-follow traversal
//! keep this read-only observation inside the selected physical directory.
//! A matching digest is not an atomic snapshot or execution authorization.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::{CStr, CString},
    fs::File,
    mem::MaybeUninit,
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd},
        unix::ffi::OsStrExt,
    },
    path::{Component, Path},
    time::{Duration, Instant},
};

pub const MAX_ENTRIES: usize = 10_000;
const MAX_DEPTH: usize = 64;

pub struct Budget {
    remaining: usize,
    started: Instant,
}
impl Budget {
    pub fn new() -> Self {
        Self {
            remaining: MAX_ENTRIES,
            started: Instant::now(),
        }
    }
    fn tick(&mut self) -> Result<(), String> {
        if self.remaining == 0 {
            return Err("目录子项超过单次 10000 项限制，请缩小选择。".into());
        }
        self.check_time()?;
        self.remaining -= 1;
        Ok(())
    }
    fn check_time(&self) -> Result<(), String> {
        if self.started.elapsed() > Duration::from_secs(5) {
            return Err("目录内容检查超过 5 秒预算，请缩小选择。".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeWitness {
    pub version: u32,
    pub digest: String,
    pub entries: usize,
}

fn open_directory(parent: &File, name: &CStr) -> Result<File, String> {
    // SAFETY: parent owns the directory fd; name is NUL terminated. The returned
    // descriptor has exactly one File owner and cannot follow a symlink.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err("目录无法打开或已替换为符号链接，需要重新扫描。".into());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn stat_entry(parent: &File, name: &CStr) -> Result<libc::stat, String> {
    let mut stat = MaybeUninit::uninit();
    // SAFETY: the fd and C string are alive; successful fstatat initializes stat.
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err("目录子项已消失或无法读取，需要重新扫描。".into());
    }
    Ok(unsafe { stat.assume_init() })
}

fn stat_fd(directory: &File) -> Result<libc::stat, String> {
    let mut stat = MaybeUninit::uninit();
    if unsafe { libc::fstat(directory.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err("目录身份无法读取。".into());
    }
    Ok(unsafe { stat.assume_init() })
}

fn stamp(stat: &libc::stat) -> Vec<u8> {
    let mut bytes = Vec::new();
    for n in [
        stat.st_dev as u64,
        stat.st_ino,
        stat.st_mode as u64,
        stat.st_uid as u64,
        stat.st_gid as u64,
        stat.st_size as u64,
        stat.st_mtime as u64,
        stat.st_mtime_nsec as u64,
        stat.st_ctime as u64,
        stat.st_ctime_nsec as u64,
        stat.st_nlink as u64,
    ] {
        bytes.extend(n.to_le_bytes());
    }
    bytes
}

struct DirectoryStream(*mut libc::DIR);
impl Drop for DirectoryStream {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}

fn names(directory: &File, budget: &mut Budget) -> Result<Vec<CString>, String> {
    let fd = directory
        .try_clone()
        .map_err(|e| e.to_string())?
        .into_raw_fd();
    // fdopendir consumes this duplicate only on success.
    let pointer = unsafe { libc::fdopendir(fd) };
    if pointer.is_null() {
        unsafe {
            libc::close(fd);
        }
        return Err("无法列出目录内容。".into());
    }
    let stream = DirectoryStream(pointer);
    let mut result = Vec::new();
    loop {
        // This project targets macOS. errno is thread-local; no await occurs here.
        unsafe {
            *libc::__error() = 0;
        }
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            if unsafe { *libc::__error() } != 0 {
                return Err("目录枚举未完成，需要重新扫描。".into());
            }
            break;
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if [b".".as_slice(), b"..".as_slice()].contains(&name.to_bytes()) {
            continue;
        }
        budget.tick()?;
        result.push(name.to_owned());
    }
    result.sort_by(|a, b| a.to_bytes().cmp(b.to_bytes()));
    Ok(result)
}

fn walk(
    directory: &File,
    device: u64,
    depth: usize,
    digest: &mut Sha256,
    budget: &mut Budget,
    count: &mut usize,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err("目录层级超过 64 层限制，请缩小选择。".into());
    }
    let before = stat_fd(directory)?;
    digest.update(b"directory-start");
    digest.update(stamp(&before));
    for name in names(directory, budget)? {
        budget.check_time()?;
        let stat = stat_entry(directory, &name)?;
        if stat.st_dev as u64 != device {
            return Err("目录内存在其他挂载卷，需要单独审阅。".into());
        }
        let kind = stat.st_mode & libc::S_IFMT;
        if kind != libc::S_IFDIR && kind != libc::S_IFREG {
            return Err("目录包含符号链接或特殊文件，需要单独审阅。".into());
        }
        if kind == libc::S_IFREG && stat.st_nlink > 1 {
            return Err("目录包含共享硬链接文件，需要单独审阅。".into());
        }
        digest.update((name.to_bytes().len() as u64).to_le_bytes());
        digest.update(name.to_bytes());
        digest.update(stamp(&stat));
        *count += 1;
        if kind == libc::S_IFDIR {
            let child = open_directory(directory, &name)?;
            if stamp(&stat_fd(&child)?) != stamp(&stat) {
                return Err("子目录身份已变化，需要重新扫描。".into());
            }
            walk(&child, device, depth + 1, digest, budget, count)?;
        }
    }
    budget.check_time()?;
    if stamp(&stat_fd(directory)?) != stamp(&before) {
        return Err("检查期间目录发生变化，需要重新扫描。".into());
    }
    digest.update(b"directory-end");
    Ok(())
}

pub fn inspect(
    path: &Path,
    dev: u64,
    ino: u64,
    budget: &mut Budget,
) -> Result<TreeWitness, String> {
    if !path.is_absolute() || path == Path::new("/") {
        return Err("目录范围无效。".into());
    }
    let mut directory = File::open("/").map_err(|e| e.to_string())?;
    for part in path.components() {
        budget.check_time()?;
        match part {
            Component::RootDir => (),
            Component::Normal(name) => {
                directory = open_directory(
                    &directory,
                    &CString::new(name.as_bytes()).map_err(|_| "目录名称无法编码。")?,
                )?
            }
            _ => return Err("目录范围无效。".into()),
        }
    }
    let root = stat_fd(&directory)?;
    if root.st_dev as u64 != dev || root.st_ino != ino {
        return Err("目录身份与计划不一致，需要重新扫描。".into());
    }
    budget.tick()?;
    let mut digest = Sha256::new();
    let mut count = 0;
    walk(&directory, dev, 0, &mut digest, budget, &mut count)?;
    Ok(TreeWitness {
        version: 1,
        digest: format!("{:x}", digest.finalize()),
        entries: count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, MetadataExt};
    fn inspect_fixture(path: &Path) -> Result<TreeWitness, String> {
        let m = std::fs::metadata(path).unwrap();
        inspect(path, m.dev(), m.ino(), &mut Budget::new())
    }
    #[test]
    fn deep_changes_are_detected_even_when_root_metadata_does_not_change() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().canonicalize().unwrap();
        std::fs::create_dir(path.join("child")).unwrap();
        let file = path.join("child/缓存\n[*]");
        std::fs::write(&file, "a").unwrap();
        let root = std::fs::metadata(&path).unwrap();
        let original = inspect_fixture(&path).unwrap();
        assert_eq!(original, inspect_fixture(&path).unwrap());
        std::fs::write(&file, "changed").unwrap();
        assert_eq!(
            root.mtime_nsec(),
            std::fs::metadata(&path).unwrap().mtime_nsec()
        );
        assert_ne!(original.digest, inspect_fixture(&path).unwrap().digest);
    }
    #[test]
    fn links_special_files_and_budget_overflow_are_rejected() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().canonicalize().unwrap();
        symlink("/", path.join("link")).unwrap();
        assert!(inspect_fixture(&path).unwrap_err().contains("符号链接"));
        std::fs::remove_file(path.join("link")).unwrap();
        std::fs::write(path.join("a"), "x").unwrap();
        std::fs::hard_link(path.join("a"), path.join("b")).unwrap();
        assert!(inspect_fixture(&path).unwrap_err().contains("硬链接"));
        let meta = std::fs::metadata(&path).unwrap();
        let mut small = Budget {
            remaining: 1,
            started: Instant::now(),
        };
        assert!(inspect(&path, meta.dev(), meta.ino(), &mut small)
            .unwrap_err()
            .contains("10000"));
        std::fs::remove_file(path.join("a")).unwrap();
        std::fs::remove_file(path.join("b")).unwrap();
        let fifo = CString::new(path.join("pipe").as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(inspect_fixture(&path).unwrap_err().contains("特殊文件"));
        let mut budget = Budget {
            remaining: 0,
            started: Instant::now(),
        };
        assert!(budget.tick().is_err());
        let mut budget = Budget {
            remaining: 1,
            started: Instant::now() - Duration::from_secs(6),
        };
        assert!(budget.tick().is_err());
    }
    #[test]
    fn ancestor_links_and_excessive_nesting_are_not_traversed() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("real")).unwrap();
        std::fs::create_dir(root.join("real/child")).unwrap();
        symlink(root.join("real"), root.join("alias")).unwrap();
        let meta = std::fs::metadata(root.join("real/child")).unwrap();
        assert!(inspect(
            &root.join("alias/child"),
            meta.dev(),
            meta.ino(),
            &mut Budget::new()
        )
        .is_err());
        let mut nested = root.join("real");
        for _ in 0..66 {
            nested = nested.join("d");
            std::fs::create_dir(&nested).unwrap();
        }
        assert!(inspect_fixture(&root.join("real"))
            .unwrap_err()
            .contains("64"));
    }
}
