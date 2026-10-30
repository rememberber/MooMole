//! Independent backup tree domain, sized for device backups rather than caches.
use super::*;
use crate::directory_probe::{open_child, stat_at, Stream};
use sha2::{Digest, Sha256};
use std::{
    ffi::{CStr, CString},
    fs::File,
    os::fd::AsRawFd,
};
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Snapshot {
    pub digest: String,
    pub entries: usize,
    pub bytes: u64,
}
pub(super) struct Budget<'a> {
    started: Instant,
    remaining: usize,
    job: Option<&'a Job>,
}
impl<'a> Budget<'a> {
    pub fn new(job: &'a Job) -> Self {
        Self {
            started: Instant::now(),
            remaining: 500_000,
            job: Some(job),
        }
    }
    pub fn receipt() -> Self {
        Self {
            started: Instant::now(),
            remaining: 500_000,
            job: None,
        }
    }
    pub fn check(&self) -> Result<(), String> {
        if self.job.is_some_and(|j| j.cancelled.load(Ordering::SeqCst)) {
            return Err("备份检查已取消；已取得的列表不代表完整。".into());
        }
        if self.started.elapsed() > Duration::from_secs(60) {
            return Err("备份检查超过六十秒，本项不能处理；请在 Finder 核对。".into());
        }
        Ok(())
    }
    fn tick(&mut self) -> Result<(), String> {
        self.check()?;
        if self.remaining == 0 {
            return Err("备份目录超过本轮五十万项检查上限，本项保留。".into());
        }
        self.remaining -= 1;
        Ok(())
    }
}
pub(super) fn names(dir: &File, b: &mut Budget) -> Result<Vec<CString>, String> {
    b.check()?;
    // A fresh description avoids inheriting an old directory enumeration offset.
    let dot = CString::new(".").unwrap();
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            dot.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err("备份目录无法枚举。".into());
    }
    let ptr = unsafe { libc::fdopendir(fd) };
    if ptr.is_null() {
        unsafe {
            libc::close(fd);
        };
        return Err("备份目录无法枚举。".into());
    }
    let stream = Stream(ptr);
    let mut out = vec![];
    loop {
        unsafe {
            *libc::__error() = 0;
        }
        let next = unsafe { libc::readdir(stream.0) };
        if next.is_null() {
            if unsafe { *libc::__error() } != 0 {
                return Err("备份目录未完整读取。".into());
            }
            break;
        }
        let name = unsafe { CStr::from_ptr((*next).d_name.as_ptr()) };
        if [b".".as_slice(), b"..".as_slice()].contains(&name.to_bytes()) {
            continue;
        }
        b.tick()?;
        out.push(name.to_owned());
    }
    out.sort_by(|a, b| a.to_bytes().cmp(b.to_bytes()));
    Ok(out)
}
fn stat(file: &File) -> Result<libc::stat, String> {
    let mut s = std::mem::MaybeUninit::uninit();
    if unsafe { libc::fstat(file.as_raw_fd(), s.as_mut_ptr()) } != 0 {
        return Err("备份目录身份无法核对。".into());
    }
    Ok(unsafe { s.assume_init() })
}
fn stamp(s: &libc::stat) -> Vec<u8> {
    [
        s.st_dev as u64,
        s.st_ino,
        s.st_mode as u64,
        s.st_uid as u64,
        s.st_gid as u64,
        s.st_size as u64,
        s.st_mtime as u64,
        s.st_mtime_nsec as u64,
        s.st_ctime as u64,
        s.st_ctime_nsec as u64,
        s.st_nlink as u64,
        s.st_flags as u64,
    ]
    .into_iter()
    .flat_map(u64::to_le_bytes)
    .collect()
}
fn walk(
    dir: &File,
    path: &Path,
    device: u64,
    depth: usize,
    settings: &db::Settings,
    b: &mut Budget,
    h: &mut Sha256,
    count: &mut usize,
    bytes: &mut u64,
) -> Result<(), String> {
    if depth > 64 {
        return Err("备份目录超过六十四层，本项保留。".into());
    }
    b.check()?;
    let before = stat(dir)?;
    if before.st_dev as u64 != device
        || before.st_uid != unsafe { libc::geteuid() }
        || before.st_flags & 0x40000000 != 0
        || !crate::directory_probe::local(dir)
    {
        return Err("备份含跨卷、其他账户或云占位目录，保留。".into());
    }
    h.update(b"directory-start");
    for name in names(dir, b)? {
        b.check()?;
        let s = stat_at(dir, &name)?;
        let kind = s.st_mode & libc::S_IFMT;
        if s.st_dev as u64 != device
            || s.st_uid != unsafe { libc::geteuid() }
            || s.st_flags & 0x40000000 != 0
            || ![libc::S_IFDIR, libc::S_IFREG].contains(&kind)
            || kind == libc::S_IFREG && s.st_nlink != 1
        {
            return Err(
                "备份包含链接、共享文件、特殊对象、其他账户或未落地内容，本轮保留整份备份。".into(),
            );
        }
        let child = path.join(std::ffi::OsStr::from_bytes(name.to_bytes()));
        if protected(&child, settings)? {
            return Err("备份内含保护路径，整份保留。".into());
        }
        h.update((name.to_bytes().len() as u64).to_le_bytes());
        h.update(name.to_bytes());
        h.update(stamp(&s));
        *count += 1;
        if kind == libc::S_IFDIR {
            let held = open_child(dir, &name, &s)?;
            if stamp(&stat(&held)?) != stamp(&s) {
                return Err("备份子目录已变化。".into());
            }
            walk(
                &held,
                &child,
                device,
                depth + 1,
                settings,
                b,
                h,
                count,
                bytes,
            )?;
        } else {
            *bytes = bytes
                .checked_add(u64::try_from(s.st_size).map_err(|_| "备份文件大小无效。")?)
                .filter(|n| *n <= 9_007_199_254_740_991)
                .ok_or("备份大小超过显示范围。")?;
        }
        if stamp(&stat_at(dir, &name)?) != stamp(&s) {
            return Err("备份内容在检查时变化，保留。".into());
        }
    }
    if stamp(&stat(dir)?) != stamp(&before) {
        return Err("备份目录在检查时变化，保留。".into());
    }
    b.check()?;
    h.update(b"directory-end");
    Ok(())
}
pub(super) fn inspect(
    dir: &File,
    path: &Path,
    settings: &db::Settings,
    b: &mut Budget,
) -> Result<Snapshot, String> {
    let m = dir.metadata().map_err(|e| e.to_string())?;
    if !m.is_dir() {
        return Err("不是备份目录。".into());
    }
    let mut h = Sha256::new();
    h.update(b"MooMole.device-backup-tree.v1");
    let mut entries = 0;
    let mut bytes = 0;
    walk(
        dir,
        path,
        m.dev(),
        0,
        settings,
        b,
        &mut h,
        &mut entries,
        &mut bytes,
    )?;
    Ok(Snapshot {
        digest: format!("{:x}", h.finalize()),
        entries,
        bytes,
    })
}
