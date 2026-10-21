//! Bounded metadata-only file allocation observations, never reclaim estimates.
use super::{catalog, local, open_child, stat_at, Skipped, Stream, MAX_DEPTH};
use crate::{engine, observation::AnchoredPath};
use serde::Serialize;
use std::{
    ffi::{CStr, CString},
    fs::File,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::Path,
    time::{Duration, Instant},
};
const ENTRY_LIMIT: usize = 20_000;
const DEPTH_LIMIT: usize = 32;
const BYTE_LIMIT: u64 = 9_007_199_254_740_991;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Measurement {
    protocol: u32,
    revision: &'static str,
    root: String,
    path: String,
    checked_at: i64,
    completion: &'static str,
    stop_reason: Option<&'static str>,
    allocated_bytes: u64,
    files: usize,
    visited: usize,
    skipped: Skipped,
    hard_links: usize,
    special: usize,
    can_execute: bool,
}
struct Walker {
    result: Measurement,
    began: Instant,
    budget: Duration,
    limit: usize,
    device: u64,
}
fn same(a: &libc::stat, b: &libc::stat) -> bool {
    a.st_dev == b.st_dev
        && a.st_ino == b.st_ino
        && a.st_mode == b.st_mode
        && a.st_size == b.st_size
        && a.st_blocks == b.st_blocks
        && a.st_nlink == b.st_nlink
        && a.st_mtime == b.st_mtime
        && a.st_mtime_nsec == b.st_mtime_nsec
        && a.st_ctime == b.st_ctime
        && a.st_ctime_nsec == b.st_ctime_nsec
        && a.st_flags == b.st_flags
}
impl Walker {
    fn ready(&mut self) -> bool {
        if self.result.stop_reason.is_some() {
            return false;
        }
        self.result.stop_reason = if self.began.elapsed() >= self.budget {
            Some("timeLimit")
        } else if self.result.visited >= self.limit {
            Some("entryLimit")
        } else {
            None
        };
        self.result.stop_reason.is_none()
    }
    fn walk(&mut self, directory: &File, depth: usize) {
        if !self.ready() {
            return;
        }
        let Ok(before) = directory.metadata() else {
            self.result.skipped.unreadable += 1;
            return;
        };
        let dot = CString::new(".").unwrap();
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                dot.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            self.result.skipped.unreadable += 1;
            return;
        }
        let pointer = unsafe { libc::fdopendir(fd) };
        if pointer.is_null() {
            unsafe {
                libc::close(fd);
            }
            self.result.skipped.unreadable += 1;
            return;
        }
        let stream = Stream(pointer);
        loop {
            if !self.ready() {
                break;
            }
            unsafe {
                *libc::__error() = 0;
            }
            let raw = unsafe { libc::readdir(stream.0) };
            if raw.is_null() {
                if unsafe { *libc::__error() } != 0 {
                    self.result.skipped.unreadable += 1;
                }
                break;
            }
            let name = unsafe { CStr::from_ptr((*raw).d_name.as_ptr()) }.to_owned();
            if [b".".as_slice(), b"..".as_slice()].contains(&name.to_bytes()) {
                continue;
            }
            self.result.visited += 1;
            let Ok(stat) = stat_at(directory, &name) else {
                self.result.skipped.unreadable += 1;
                continue;
            };
            let kind = stat.st_mode & libc::S_IFMT;
            if kind == libc::S_IFLNK {
                self.result.skipped.links += 1;
                continue;
            }
            if stat.st_dev as u64 != self.device || stat.st_flags & 0x40000000 != 0 {
                self.result.skipped.non_local += 1;
                continue;
            }
            if kind == libc::S_IFDIR {
                if depth >= DEPTH_LIMIT {
                    self.result.skipped.depth += 1;
                    continue;
                }
                let Ok(child) = open_child(directory, &name, &stat) else {
                    self.result.skipped.unreadable += 1;
                    continue;
                };
                if !local(&child) {
                    self.result.skipped.non_local += 1;
                    continue;
                }
                self.walk(&child, depth + 1);
                if !stat_at(directory, &name).is_ok_and(|now| same(&stat, &now)) {
                    self.result.skipped.changed += 1;
                }
            } else if kind == libc::S_IFREG {
                // Shared links are omitted: neither ownership nor reclaimability is inferred.
                if stat.st_nlink != 1 {
                    self.result.hard_links += 1;
                    continue;
                }
                if stat.st_blocks < 0
                    || !stat_at(directory, &name).is_ok_and(|now| same(&stat, &now))
                {
                    self.result.skipped.changed += 1;
                    continue;
                }
                let Some(total) = (stat.st_blocks as u64)
                    .checked_mul(512)
                    .and_then(|n| self.result.allocated_bytes.checked_add(n))
                    .filter(|n| *n <= BYTE_LIMIT)
                else {
                    self.result.stop_reason = Some("sizeLimit");
                    break;
                };
                self.result.allocated_bytes = total;
                self.result.files += 1;
            } else {
                self.result.special += 1;
            }
        }
        if !directory.metadata().is_ok_and(|after| {
            before.dev() == after.dev()
                && before.ino() == after.ino()
                && before.mtime() == after.mtime()
                && before.mtime_nsec() == after.mtime_nsec()
                && before.ctime() == after.ctime()
                && before.ctime_nsec() == after.ctime_nsec()
        }) {
            self.result.skipped.changed += 1;
        }
    }
}
pub fn measure(root: &Path, path: &Path) -> Result<Measurement, String> {
    measure_with(root, path, ENTRY_LIMIT, Duration::from_secs(3))
}
fn measure_with(
    root: &Path,
    path: &Path,
    limit: usize,
    budget: Duration,
) -> Result<Measurement, String> {
    let names = catalog()?.targets;
    let valid = |p: &Path| {
        p.to_str().is_some_and(|s| {
            s.len() <= 4096
                && s.starts_with('/')
                && !s.ends_with('/')
                && !s.chars().any(char::is_control)
                && !s[1..].split('/').any(|v| ["", ".", ".."].contains(&v))
        })
    };
    if !valid(root)
        || !valid(path)
        || root
            .file_name()
            .and_then(|v| v.to_str())
            .is_some_and(|n| names.iter().any(|v| v == n))
    {
        return Err("请选择有效的项目范围并重新扫描。".into());
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "测量目录不在所选项目范围内。")?;
    let parts = relative
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect::<Vec<_>>();
    if parts.is_empty()
        || parts.len() > MAX_DEPTH
        || !names.iter().any(|v| v == parts[parts.len() - 1])
        || parts[..parts.len() - 1].iter().any(|n| {
            n.starts_with('.')
                || ["Library", "Applications"].contains(n)
                || names.iter().any(|v| v == n)
        })
    {
        return Err("测量目录不属于本次名称发现范围。".into());
    }
    let scope = AnchoredPath::open(root)?;
    let target = AnchoredPath::open(path)?;
    if !scope.target().metadata().is_ok_and(|m| m.is_dir())
        || !target.target().metadata().is_ok_and(|m| m.is_dir())
        || !scope.local_materialized()
        || !target.local_materialized()
    {
        return Err("只能测量已下载到本机的物理目录。".into());
    }
    let mut walker = Walker {
        result: Measurement {
            protocol: 1,
            revision: engine::REVISION,
            root: root.to_str().unwrap().into(),
            path: path.to_str().unwrap().into(),
            checked_at: 0,
            completion: "complete",
            stop_reason: None,
            allocated_bytes: 0,
            files: 0,
            visited: 0,
            skipped: Skipped::default(),
            hard_links: 0,
            special: 0,
            can_execute: false,
        },
        began: Instant::now(),
        budget,
        limit,
        device: target
            .target()
            .metadata()
            .map_err(|_| "目录无法读取。")?
            .dev(),
    };
    walker.walk(target.target(), 0);
    scope
        .validate()
        .and_then(|_| target.validate())
        .map_err(|_| "测量期间目录或路径已变化，请重新测量。")?;
    walker.result.checked_at = crate::db::now();
    if walker.result.skipped.any()
        || walker.result.hard_links > 0
        || walker.result.special > 0
        || walker.result.stop_reason.is_some()
    {
        walker.result.completion = "partial";
    }
    Ok(walker.result)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{symlink, MetadataExt},
    };
    #[test]
    fn allocation_includes_hidden_files_without_reading_contents_or_directory_inode_sizes() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let target = root.path().join("dist");
        fs::create_dir_all(target.join(".hidden")).unwrap();
        fs::write(target.join(".hidden/file"), vec![42; 8192]).unwrap();
        let sparse = File::create(target.join("sparse")).unwrap();
        sparse.set_len(16 * 1024 * 1024).unwrap();
        let expected = fs::metadata(target.join(".hidden/file")).unwrap().blocks() * 512
            + sparse.metadata().unwrap().blocks() * 512;
        let r = measure(root.path(), &target).unwrap();
        assert_eq!(r.allocated_bytes, expected);
        assert_eq!(r.files, 2);
        assert_eq!(r.completion, "complete");
        assert!(!r.can_execute);
        let wire = serde_json::to_value(r).unwrap();
        assert_eq!(wire["allocatedBytes"], expected);
        assert_eq!(wire["canExecute"], false);
        assert!(wire.get("reclaimableBytes").is_none());
    }
    #[test]
    fn shared_files_links_and_special_files_are_not_counted_as_complete() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(root.path().join("shared"), "outside").unwrap();
        fs::hard_link(root.path().join("shared"), target.join("hard")).unwrap();
        symlink(root.path(), target.join("link")).unwrap();
        let fifo = CString::new(target.join("pipe").to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let r = measure(root.path(), &target).unwrap();
        assert_eq!(r.allocated_bytes, 0);
        assert_eq!(r.hard_links, 1);
        assert_eq!(r.special, 1);
        assert_eq!(r.skipped.links, 1);
        assert_eq!(r.completion, "partial");
    }
    #[test]
    fn limits_are_partial_and_scope_aliases_or_replaced_targets_are_rejected() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let target = root.path().join("dist");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("file"), "x").unwrap();
        assert_eq!(
            measure_with(root.path(), &target, 1, Duration::from_secs(3))
                .unwrap()
                .stop_reason,
            Some("entryLimit")
        );
        assert_eq!(
            measure_with(root.path(), &target, ENTRY_LIMIT, Duration::ZERO)
                .unwrap()
                .stop_reason,
            Some("timeLimit")
        );
        for invalid in [
            root.path().join("source"),
            root.path().join("dist/target"),
            root.path().join(".git/dist"),
            root.path().join("../dist"),
        ] {
            assert!(measure(root.path(), &invalid).is_err());
        }
        assert!(measure(Path::new("/"), &target).is_err());
        assert!(measure(Path::new("/private/tmp/other"), &target).is_err());
        fs::rename(&target, root.path().join("old")).unwrap();
        symlink(root.path().join("old"), &target).unwrap();
        assert!(measure(root.path(), &target).is_err());
    }
    #[test]
    fn depth_bound_never_reports_a_truncated_tree_as_complete() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let target = root.path().join("dist");
        let mut nested = target.clone();
        for _ in 0..34 {
            nested.push("d");
        }
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("deep"), "x").unwrap();
        let r = measure(root.path(), &target).unwrap();
        assert_eq!(r.completion, "partial");
        assert_eq!(r.skipped.depth, 1);
        assert_eq!(r.files, 0);
    }
}
