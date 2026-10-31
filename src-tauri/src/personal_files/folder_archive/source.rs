//! Whole-scope, no-follow inventory. Unsupported members refuse the whole ZIP.
use super::*;
use crate::directory_probe;
use std::{ffi::CStr, os::macos::fs::MetadataExt as _};
extern "C" {
    fn fcopyfile(from: i32, to: i32, state: *mut libc::c_void, flags: u32) -> i32;
}
const META_LIMIT: u64 = 32 * 1024 * 1024;
#[derive(Clone, Debug, Serialize)]
pub(super) struct Node {
    pub name: String,
    pub directory: bool,
    pub size: u64,
    pub stamp: Vec<u64>,
    pub hash: String,
    pub metadata_hash: String,
    pub metadata_bytes: u64,
    #[serde(skip_serializing)]
    pub accessed_at: u32,
}
impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        // Reading may update atime. Preserve the first observed value in the
        // ZIP without treating our own reads as a source content change.
        self.name == other.name
            && self.directory == other.directory
            && self.size == other.size
            && self.stamp == other.stamp
            && self.hash == other.hash
            && self.metadata_hash == other.metadata_hash
            && self.metadata_bytes == other.metadata_bytes
    }
}
pub(super) fn stamp(m: &std::fs::Metadata) -> Vec<u64> {
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
fn component(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && s != "."
        && s != ".."
        && !s.starts_with("._")
        && s != "__MACOSX"
        && !s.chars().any(char::is_control)
        && !s.contains(['/', '\\', ':'])
        && ![
            ".app",
            ".bundle",
            ".framework",
            ".photoslibrary",
            ".musiclibrary",
            ".sparsebundle",
        ]
        .iter()
        .any(|suffix| s.to_ascii_lowercase().ends_with(suffix))
}
pub(super) fn open(parent: &File, name: &CStr, directory: bool) -> Result<File, String> {
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY
                | libc::O_NOFOLLOW
                | libc::O_NONBLOCK
                | libc::O_CLOEXEC
                | if directory { libc::O_DIRECTORY } else { 0 },
        )
    };
    if fd < 0 {
        return Err("文件夹归档子项不可读，整份归档未发布。".into());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn names(dir: &File) -> Result<Vec<CString>, String> {
    use std::os::fd::IntoRawFd;
    let fd = open(dir, c".", true)?.into_raw_fd();
    let ptr = unsafe { libc::fdopendir(fd) };
    if ptr.is_null() {
        unsafe { libc::close(fd) };
        return Err("归档目录无法完整列出。".into());
    }
    let stream = directory_probe::Stream(ptr);
    let mut out = vec![];
    loop {
        unsafe { *libc::__error() = 0 };
        let row = unsafe { libc::readdir(stream.0) };
        if row.is_null() {
            if unsafe { *libc::__error() } != 0 {
                return Err("归档目录未完整读取。".into());
            }
            break;
        }
        let n = unsafe { CStr::from_ptr((*row).d_name.as_ptr()) };
        if n == c"." || n == c".." {
            continue;
        }
        if out.len() >= 10_000 {
            return Err("整份文件夹归档最多核对 10000 项。".into());
        }
        out.push(n.to_owned());
    }
    out.sort_by(|a, b| a.to_bytes().cmp(b.to_bytes()));
    if out.windows(2).any(|w| w[0] == w[1]) {
        return Err("归档目录在读取时变化。".into());
    }
    Ok(out)
}
pub(super) fn attributes(file: &File) -> Result<(String, u64), String> {
    let fd = file.as_raw_fd();
    let len = unsafe { libc::flistxattr(fd, std::ptr::null_mut(), 0, 0) };
    if !(0..=65536).contains(&len) {
        return Err("文件夹归档扩展属性无法完整读取。".into());
    }
    let mut buf = vec![0u8; len as usize];
    if unsafe { libc::flistxattr(fd, buf.as_mut_ptr().cast(), buf.len(), 0) } != len {
        return Err("归档扩展属性在读取时变化。".into());
    }
    let mut keys = buf
        .split(|b| *b == 0)
        .filter(|k| !k.is_empty())
        .collect::<Vec<_>>();
    keys.sort();
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    for key in keys {
        let name = CString::new(key).map_err(|_| "归档属性名称无效。")?;
        let size = unsafe { libc::fgetxattr(fd, name.as_ptr(), std::ptr::null_mut(), 0, 0, 0) };
        if size < 0
            || bytes
                .saturating_add(size as u64)
                .saturating_add(key.len() as u64 + 16)
                > META_LIMIT
        {
            return Err("单项归档元数据超过 32 MiB，整份保留。".into());
        }
        let mut value = vec![0u8; size as usize];
        if unsafe {
            libc::fgetxattr(
                fd,
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
                0,
                0,
            )
        } != size
        {
            return Err("归档属性内容在读取时变化。".into());
        }
        hash.update((key.len() as u64).to_le_bytes());
        hash.update(key);
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value);
        bytes += size as u64 + key.len() as u64 + 16;
    }
    Ok((format!("{:x}", hash.finalize()), bytes))
}
pub(super) fn packed(file: &File) -> Result<File, String> {
    let out = tempfile::tempfile().map_err(|e| e.to_string())?;
    if unsafe {
        fcopyfile(
            file.as_raw_fd(),
            out.as_raw_fd(),
            std::ptr::null_mut(),
            (1 << 22) | 7,
        )
    } != 0
    {
        return Err("系统未完成 AppleDouble 元数据归档，整份未发布。".into());
    }
    if out.metadata().map_err(|e| e.to_string())?.len() > META_LIMIT + 65536 {
        return Err("归档元数据封装超过本轮上限。".into());
    }
    Ok(out)
}
struct Walk<'a> {
    root: &'a Path,
    rules: &'a [String],
    device: u64,
    nodes: Vec<Node>,
    bytes: u64,
    metadata: u64,
    path_bytes: usize,
    job: &'a Job,
    started: Instant,
}
impl Walk<'_> {
    fn visit(&mut self, file: &File, name: String, depth: usize) -> Result<(), String> {
        archive::check(self.job, self.started)?;
        if depth > 64
            || self.nodes.len() >= 10_000
            || self.path_bytes.saturating_add(name.len()) > 4 * 1024 * 1024
        {
            return Err("文件夹层级、项目数或名称总量超过本轮范围，整份保留。".into());
        }
        let before = file.metadata().map_err(|e| e.to_string())?;
        let path = if depth == 0 {
            self.root.to_path_buf()
        } else {
            self.root
                .parent()
                .ok_or("归档来源缺少父目录。")?
                .join(&name)
        };
        if protected(&path, self.rules)?
            || !directory_probe::local(file)
            || before.dev() != self.device
            || before.uid() != unsafe { libc::geteuid() }
            || (!before.is_dir() && !before.is_file())
            || (before.is_file() && before.nlink() != 1)
            || before.mode() & 0o7000 != 0
            || before.st_flags() != 0
            || crate::permissions::has_acl(file).map_err(|_| "归档 ACL 无法核对。")?
        {
            return Err(
                "文件夹包含受保护、其他卷、未落地、共享、链接或不支持权限的内容，整份不归档。"
                    .into(),
            );
        }
        if u32::try_from(before.mtime()).is_err() || u32::try_from(before.atime()).is_err() {
            return Err("修改时间超出本轮 ZIP 时间范围，整份保留。".into());
        }
        let (metadata_hash, metadata) = attributes(file)?;
        self.metadata = self.metadata.saturating_add(metadata);
        if self.metadata > 256 * 1024 * 1024 {
            return Err("整份文件夹元数据超过 256 MiB，未发布。".into());
        }
        let size = if before.is_file() { before.len() } else { 0 };
        self.bytes = self.bytes.saturating_add(size);
        if self.bytes.saturating_add(self.metadata) > 8 * 1024 * 1024 * 1024 {
            return Err("整份文件夹归档最多读取 8 GiB 内容与元数据。".into());
        }
        self.path_bytes += name.len();
        let children = if before.is_dir() {
            Some(names(file)?)
        } else {
            None
        };
        let hash = if before.is_file() {
            let mut input = file.try_clone().map_err(|e| e.to_string())?;
            input.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            archive::digest(&mut input, self.job, self.started, size)?
        } else {
            String::new()
        };
        let index = self.nodes.len();
        self.nodes.push(Node {
            name: name.clone(),
            directory: before.is_dir(),
            size,
            stamp: stamp(&before),
            hash,
            metadata_hash,
            metadata_bytes: metadata,
            accessed_at: before.atime() as u32,
        });
        if let Some(children) = children {
            for n in &children {
                let s = n.to_str().map_err(|_| "归档内部名称无法编码。")?;
                if !component(s) {
                    return Err(
                        "文件夹包含应用资料库、AppleDouble 保留名称或不支持的路径，整份保留。"
                            .into(),
                    );
                }
                if protected(
                    &self.root.parent().unwrap().join(format!("{name}/{s}")),
                    self.rules,
                )? {
                    return Err("文件夹中存在受保护项目，整份不归档。".into());
                }
                let before = directory_probe::stat_at(file, n)?;
                let kind = before.st_mode & libc::S_IFMT;
                if ![libc::S_IFDIR, libc::S_IFREG].contains(&kind)
                    || before.st_flags & 0x40000000 != 0
                {
                    return Err("文件夹含链接、特殊文件或云占位，整份不归档。".into());
                }
                let child = open(file, n, kind == libc::S_IFDIR)?;
                let m = child.metadata().map_err(|e| e.to_string())?;
                if (
                    m.dev(),
                    m.ino(),
                    m.mode(),
                    m.size(),
                    m.mtime(),
                    m.mtime_nsec(),
                    m.ctime(),
                    m.ctime_nsec(),
                ) != (
                    before.st_dev as u64,
                    before.st_ino,
                    before.st_mode as u32,
                    before.st_size as u64,
                    before.st_mtime,
                    before.st_mtime_nsec,
                    before.st_ctime,
                    before.st_ctime_nsec,
                ) {
                    return Err("归档子项身份变化，整份保留。".into());
                }
                self.visit(&child, format!("{name}/{s}"), depth + 1)?;
                let after = directory_probe::stat_at(file, n)?;
                let m = child.metadata().map_err(|e| e.to_string())?;
                if (after.st_dev as u64, after.st_ino) != (m.dev(), m.ino()) {
                    return Err("归档子项名称在读取时变化。".into());
                }
            }
            if names(file)? != children {
                return Err("归档目录内容在读取时变化，整份保留。".into());
            }
        }
        if stamp(&file.metadata().map_err(|e| e.to_string())?) != stamp(&before)
            || attributes(file)?.0 != self.nodes[index].metadata_hash
        {
            return Err("归档内容或元数据在读取时变化，整份保留。".into());
        }
        Ok(())
    }
}
pub(super) fn inventory(
    root: &Path,
    rules: &[String],
    job: &Job,
    started: Instant,
) -> Result<(Witness, Vec<Node>), String> {
    if !ordinary(root) || protected(root, rules)? {
        return Err("请选择未受保护的普通文件夹，不归档系统或应用资料库。".into());
    }
    let anchor = AnchoredPath::open(root)?;
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| component(s))
        .ok_or("归档文件夹名称不受支持。")?;
    let dir = open(
        anchor.parent(),
        &CString::new(name).map_err(|_| "归档目录名称无效。")?,
        true,
    )?;
    let mut walk = Walk {
        root,
        rules,
        device: dir.metadata().map_err(|e| e.to_string())?.dev(),
        nodes: vec![],
        bytes: 0,
        metadata: 0,
        path_bytes: 0,
        job,
        started,
    };
    walk.visit(&dir, name.into(), 0)?;
    anchor.validate()?;
    if AnchoredPath::open(root)?.witness != anchor.witness {
        return Err("归档根目录身份在读取时变化。".into());
    }
    Ok((anchor.witness, walk.nodes))
}
