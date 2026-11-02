//! Descriptor-relative, bounded copying. Never removes a source, backup,
//! quarantine attribute, or an unexpected staging object.
use crate::{engine::Job, observation::AnchoredPath};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::{CStr, CString},
    fs::{File, Metadata},
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        macos::fs::MetadataExt as _,
        unix::fs::MetadataExt,
    },
    path::Path,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

extern "C" {
    fn fcopyfile(from: i32, to: i32, state: *mut libc::c_void, flags: u32) -> i32;
}
type Result<T> = std::result::Result<T, String>;
fn io<T>(value: std::io::Result<T>) -> Result<T> {
    value.map_err(|e| e.to_string())
}
pub(super) fn syscall(value: i32) -> Result<()> {
    if value == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}
pub(super) fn name(path: &Path) -> Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(path.file_name().ok_or("缺少应用名称。")?.as_bytes())
        .map_err(|_| "应用名称无效。".into())
}
pub(super) fn open(parent: &File, name: &CStr, directory: bool, create: bool) -> Result<File> {
    let flags = libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | if directory {
            libc::O_DIRECTORY | libc::O_RDONLY
        } else if create {
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL
        } else {
            libc::O_RDONLY
        };
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags, 0o600) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn stamp(m: &Metadata) -> Vec<u64> {
    vec![
        m.dev(),
        m.ino(),
        m.mode() as u64,
        m.uid() as u64,
        m.gid() as u64,
        m.size(),
        m.mtime() as u64,
        m.mtime_nsec() as u64,
        m.ctime() as u64,
        m.ctime_nsec() as u64,
        m.nlink(),
        m.st_flags() as u64,
    ]
}
fn at(parent: &File, name: &CStr) -> Result<libc::stat> {
    let mut s = std::mem::MaybeUninit::uninit();
    syscall(unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            s.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    })?;
    Ok(unsafe { s.assume_init() })
}
fn same_at(s: &libc::stat, m: &Metadata) -> bool {
    s.st_dev as u64 == m.dev()
        && s.st_ino == m.ino()
        && s.st_mode as u32 == m.mode()
        && s.st_size as u64 == m.size()
        && s.st_mtime == m.mtime()
        && s.st_mtime_nsec == m.mtime_nsec()
        && s.st_ctime == m.ctime()
        && s.st_ctime_nsec == m.ctime_nsec()
        && s.st_nlink as u64 == m.nlink()
}
struct Dir(*mut libc::DIR);
impl Drop for Dir {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}
pub(super) fn names(file: &File) -> Result<Vec<CString>> {
    // A new open description avoids sharing a directory offset with the held FD.
    let fresh = open(file, c".", true, false)?;
    use std::os::fd::IntoRawFd;
    let fd = fresh.into_raw_fd();
    let raw = unsafe { libc::fdopendir(fd) };
    if raw.is_null() {
        unsafe {
            libc::close(fd);
        };
        return Err("应用目录无法列出。".into());
    }
    let stream = Dir(raw);
    let mut out = vec![];
    loop {
        unsafe {
            *libc::__error() = 0;
        }
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            if unsafe { *libc::__error() } != 0 {
                return Err("应用目录未完整读取。".into());
            }
            break;
        }
        let n = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if n == c"." || n == c".." {
            continue;
        }
        if out.len() >= 10_000 {
            return Err("应用包超过本轮 10000 项限制。".into());
        }
        out.push(n.to_owned());
    }
    out.sort_by(|a, b| a.to_bytes().cmp(b.to_bytes()));
    if out.windows(2).any(|w| w[0] == w[1]) {
        return Err("应用目录在检查期间变化。".into());
    }
    Ok(out)
}
fn attributes(file: &File) -> Result<Vec<u8>> {
    let fd = file.as_raw_fd();
    let len = unsafe { libc::flistxattr(fd, std::ptr::null_mut(), 0, 0) };
    if !(0..=65536).contains(&len) {
        return Err("应用扩展属性无法完整读取。".into());
    }
    let mut bytes = vec![0u8; len as usize];
    if unsafe { libc::flistxattr(fd, bytes.as_mut_ptr().cast(), bytes.len(), 0) } != len {
        return Err("应用扩展属性已变化。".into());
    }
    let mut keys = bytes
        .split(|b| *b == 0)
        .filter(|k| !k.is_empty())
        .collect::<Vec<_>>();
    keys.sort();
    let mut out = vec![];
    for key in keys {
        let n = CString::new(key).map_err(|_| "扩展属性名称无效。")?;
        let size = unsafe { libc::fgetxattr(fd, n.as_ptr(), std::ptr::null_mut(), 0, 0, 0) };
        if !(0..=8_388_608).contains(&size) || out.len() + size as usize > 8_388_608 {
            return Err("应用扩展属性超过本轮容量。".into());
        }
        let mut value = vec![0u8; size as usize];
        if unsafe { libc::fgetxattr(fd, n.as_ptr(), value.as_mut_ptr().cast(), value.len(), 0, 0) }
            != size
        {
            return Err("应用扩展属性已变化。".into());
        }
        out.extend((key.len() as u64).to_le_bytes());
        out.extend(key);
        out.extend((value.len() as u64).to_le_bytes());
        out.extend(value);
    }
    Ok(out)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Tree {
    pub content: String,
    pub bound: String,
    pub entries: usize,
    pub bytes: u64,
}
struct Walk<'a> {
    job: &'a Job,
    start: Instant,
    content: Sha256,
    bound: Sha256,
    entries: usize,
    bytes: u64,
}
impl Walk<'_> {
    fn tick(&self) -> Result<()> {
        if self.job.cancelled.load(Ordering::SeqCst) {
            return Err("已取消；已创建的暂存包保留供核对。".into());
        }
        if self.start.elapsed() > Duration::from_secs(120) {
            return Err("应用包检查超过本轮时间预算。".into());
        }
        Ok(())
    }
    fn field(&mut self, bytes: &[u8]) {
        self.content.update((bytes.len() as u64).to_le_bytes());
        self.content.update(bytes);
    }
    fn visit(&mut self, file: &File, dest: Option<&File>, depth: usize, owner: bool) -> Result<()> {
        self.tick()?;
        self.entries += 1;
        if depth > 64 || self.entries > 10_000 {
            return Err("应用包超过本轮目录范围。".into());
        }
        let before = io(file.metadata())?;
        if (!before.is_dir() && !before.is_file())
            || (before.is_file() && before.nlink() != 1)
            || before.mode() & 0o7000 != 0
            || before.st_flags()
                & (0x40000000
                    | libc::UF_IMMUTABLE
                    | libc::UF_APPEND
                    | libc::SF_IMMUTABLE
                    | libc::SF_APPEND)
                != 0
            || owner && before.uid() != unsafe { libc::geteuid() }
            || crate::permissions::has_acl(file).map_err(|_| "应用 ACL 无法核对。")?
        {
            return Err("应用包包含不支持的所有权、链接、权限、云占位或文件属性。".into());
        }
        let mut volume = std::mem::MaybeUninit::<libc::statfs>::zeroed();
        syscall(unsafe { libc::fstatfs(file.as_raw_fd(), volume.as_mut_ptr()) })?;
        if unsafe { volume.assume_init() }.f_flags & libc::MNT_LOCAL as u32 == 0 {
            return Err("应用包必须位于本地卷。".into());
        }
        for n in stamp(&before) {
            self.bound.update(n.to_le_bytes());
        }
        self.field(&before.mode().to_le_bytes());
        self.field(&before.st_flags().to_le_bytes());
        self.field(&attributes(file)?);
        if before.is_dir() {
            let children = names(file)?;
            self.field(&(children.len() as u64).to_le_bytes());
            for n in &children {
                self.tick()?;
                self.field(n.to_bytes());
                let s = at(file, n)?;
                match s.st_mode & libc::S_IFMT {
                    libc::S_IFDIR | libc::S_IFREG => {
                        let is_dir = s.st_mode & libc::S_IFMT == libc::S_IFDIR;
                        let child = open(file, n, is_dir, false)?;
                        if !same_at(&s, &io(child.metadata())?) {
                            return Err("应用子项身份已变化。".into());
                        }
                        let copied = if let Some(d) = dest {
                            if is_dir {
                                syscall(unsafe {
                                    libc::mkdirat(d.as_raw_fd(), n.as_ptr(), 0o700)
                                })?;
                            }
                            Some(open(d, n, is_dir, !is_dir)?)
                        } else {
                            None
                        };
                        self.visit(&child, copied.as_ref(), depth + 1, owner)?;
                        if !same_at(&at(file, n)?, &io(child.metadata())?) {
                            return Err("应用子项名称已变化。".into());
                        }
                    }
                    libc::S_IFLNK => {
                        self.entries += 1;
                        if self.entries > 10_000 {
                            return Err("应用包超过本轮目录范围。".into());
                        }
                        let mut raw = vec![0u8; 4096];
                        let count = unsafe {
                            libc::readlinkat(
                                file.as_raw_fd(),
                                n.as_ptr(),
                                raw.as_mut_ptr().cast(),
                                raw.len(),
                            )
                        };
                        if count <= 0 || count as usize >= raw.len() {
                            return Err("应用内部链接无法核对。".into());
                        }
                        raw.truncate(count as usize);
                        // Relative framework links may ascend, but never leave the App.
                        let mut level = depth as i32;
                        if raw.starts_with(b"/") {
                            return Err("应用包含包外链接。".into());
                        }
                        for part in raw.split(|b| *b == b'/') {
                            match part {
                                b".." => level -= 1,
                                b"." | b"" => {}
                                _ => level += 1,
                            }
                            if level < 0 {
                                return Err("应用包含包外链接。".into());
                            }
                        }
                        if s.st_flags != 0 || owner && s.st_uid != unsafe { libc::geteuid() } {
                            return Err("应用链接属性不支持。".into());
                        }
                        // Symlink metadata cannot be read with fgetxattr. Reject
                        // attributed links rather than silently losing security data.
                        let attr = unsafe {
                            libc::openat(
                                file.as_raw_fd(),
                                n.as_ptr(),
                                libc::O_SYMLINK | libc::O_CLOEXEC,
                            )
                        };
                        if attr < 0 {
                            return Err("应用链接属性无法读取。".into());
                        }
                        let link = unsafe { File::from_raw_fd(attr) };
                        if !attributes(&link)?.is_empty() || !same_at(&s, &io(link.metadata())?) {
                            return Err("应用链接属性不支持或已变化。".into());
                        }
                        self.field(&s.st_mode.to_le_bytes());
                        self.field(&raw);
                        for x in [
                            s.st_dev as u64,
                            s.st_ino,
                            s.st_ctime as u64,
                            s.st_ctime_nsec as u64,
                        ] {
                            self.bound.update(x.to_le_bytes());
                        }
                        if let Some(d) = dest {
                            let to = CString::new(raw).map_err(|_| "应用链接无效。")?;
                            syscall(unsafe {
                                libc::symlinkat(to.as_ptr(), d.as_raw_fd(), n.as_ptr())
                            })?;
                        }
                        if !same_at(&at(file, n)?, &io(link.metadata())?) {
                            return Err("应用链接已变化。".into());
                        }
                    }
                    _ => return Err("应用包包含特殊文件。".into()),
                }
            }
            if names(file)? != children {
                return Err("应用目录内容已变化。".into());
            }
        } else {
            self.bytes = self
                .bytes
                .checked_add(before.size())
                .ok_or("应用包大小超限。")?;
            if self.bytes > 8 * 1024 * 1024 * 1024 {
                return Err("应用包超过本轮 8 GiB 限制。".into());
            }
            self.field(&before.size().to_le_bytes());
            let mut input = io(file.try_clone())?;
            let mut content = Sha256::new();
            let mut count = 0u64;
            let mut buf = [0u8; 65536];
            loop {
                self.tick()?;
                let n = io(input.read(&mut buf))?;
                if n == 0 {
                    break;
                }
                count += n as u64;
                if count > before.size() {
                    return Err("应用文件已增长。".into());
                }
                content.update(&buf[..n]);
            }
            if count != before.size() {
                return Err("应用文件已变化。".into());
            }
            self.field(&content.finalize());
        }
        if let Some(d) = dest {
            // COPYFILE_ALL=15, METADATA=7. fcopyfile may update source offset;
            // reset it before copying bytes inspected above.
            if before.is_file() {
                syscall(
                    if unsafe { libc::lseek(file.as_raw_fd(), 0, libc::SEEK_SET) } == 0 {
                        0
                    } else {
                        -1
                    },
                )?;
            }
            syscall(unsafe {
                fcopyfile(
                    file.as_raw_fd(),
                    d.as_raw_fd(),
                    std::ptr::null_mut(),
                    if before.is_dir() { 7 } else { 15 },
                )
            })?;
            io(d.sync_all())?;
        }
        if stamp(&io(file.metadata())?) != stamp(&before) {
            return Err("应用包在读取或复制期间变化。".into());
        }
        self.tick()
    }
}
pub(super) fn tree(file: &File, dest: Option<&File>, job: &Job, owner: bool) -> Result<Tree> {
    let mut w = Walk {
        job,
        start: Instant::now(),
        content: Sha256::new(),
        bound: Sha256::new(),
        entries: 0,
        bytes: 0,
    };
    w.visit(file, dest, 0, owner)?;
    Ok(Tree {
        content: format!("{:x}", w.content.finalize()),
        bound: format!("{:x}", w.bound.finalize()),
        entries: w.entries,
        bytes: w.bytes,
    })
}
pub(super) fn inspect(path: &Path, job: &Job, owner: bool) -> Result<Tree> {
    let held = AnchoredPath::open(path)?;
    if !held.local_materialized() || !io(held.target().metadata())?.is_dir() {
        return Err("请选择本地完整应用包。".into());
    }
    let result = tree(held.target(), None, job, owner)?;
    held.validate()?;
    Ok(result)
}
pub(super) fn exchange(stage: &File, staged: &CStr, parent: &File, target: &CStr) -> Result<()> {
    syscall(unsafe {
        libc::renameatx_np(
            stage.as_raw_fd(),
            staged.as_ptr(),
            parent.as_raw_fd(),
            target.as_ptr(),
            libc::RENAME_SWAP,
        )
    })?;
    io(stage.sync_all())?;
    io(parent.sync_all())
}
