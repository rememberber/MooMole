//! Shared read-only held file preparation for installer data probes.
use crate::{directory_probe, observation::AnchoredPath};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::Path,
};
pub(super) struct FileProbe {
    pub(super) scope: AnchoredPath,
    pub(super) held: AnchoredPath,
    pub(super) file: File,
    pub(super) stamp: String,
    pub(super) root: String,
    pub(super) path: String,
}
impl FileProbe {
    pub(super) fn open(root: &Path, path: &Path, extension: &str) -> Result<Self, String> {
        let valid = |p: &Path| {
            p.to_str().is_some_and(|s| {
                s.len() <= 4096
                    && s.starts_with('/')
                    && !s.ends_with('/')
                    && !s.chars().any(char::is_control)
                    && !s[1..].split('/').any(|v| ["", ".", ".."].contains(&v))
            })
        };
        if !valid(root) || !valid(path) {
            return Err("请选择有效的扫描范围与文件。".into());
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "文件不在所选范围内。")?;
        if relative.components().count() == 0
            || relative.components().count() > 2
            || !path
                .extension()
                .is_some_and(|v| v.eq_ignore_ascii_case(extension))
        {
            return Err("只能检查所选范围两层内的指定类型文件。".into());
        }
        let scope = AnchoredPath::open(root)?;
        let held = AnchoredPath::open(path)?;
        let before = held
            .target()
            .metadata()
            .map_err(|_| "文件元数据无法读取。")?;
        if !scope
            .target()
            .metadata()
            .is_ok_and(|m| m.is_dir() && m.dev() == before.dev())
            || !before.is_file()
            || !scope.local_materialized()
            || !held.local_materialized()
        {
            return Err("只能读取已下载到本机的普通文件。".into());
        }
        let name = CString::new(path.file_name().unwrap().as_encoded_bytes())
            .map_err(|_| "文件名称无效。")?;
        let fd = unsafe {
            libc::openat(
                held.parent().as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err("文件无法只读打开，请检查访问权限。".into());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let after = file.metadata().map_err(|_| "文件元数据无法读取。")?;
        if !directory_probe::local(&file)
            || !after.is_file()
            || before.dev() != after.dev()
            || before.ino() != after.ino()
            || before.size() != after.size()
            || before.mtime() != after.mtime()
            || before.mtime_nsec() != after.mtime_nsec()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
            || before.nlink() != after.nlink()
        {
            return Err("打开期间文件已变化，请重新扫描。".into());
        }
        held.validate()?;
        let stamp = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&held.witness).map_err(|_| "文件观察无法编码。")?)
        );
        Ok(Self {
            scope,
            held,
            file,
            stamp,
            root: root.to_str().unwrap().into(),
            path: path.to_str().unwrap().into(),
        })
    }
}
