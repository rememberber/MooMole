use super::*;
use crate::{
    directory_probe::{self, Stream},
    engine::Job,
    observation::AnchoredPath,
};
use std::{
    ffi::CStr,
    fs::File,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    sync::{atomic::Ordering, Arc},
};

pub(super) fn collect(root: &Path, rules: &[String], job: Arc<Job>) -> Result<Scan, String> {
    if !ordinary(root) || rules.iter().any(|rule| root.starts_with(rule)) {
        return Err(
            "请选择未受保护的普通个人目录；系统、应用、隐藏和资料库目录不在本次范围。".into(),
        );
    }
    let anchor = AnchoredPath::open(root)?;
    let meta = anchor.target().metadata().map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || !anchor.local_materialized() {
        return Err("请选择本账户拥有、已下载到本机的物理目录。".into());
    }
    let now = crate::db::now();
    let mut walker = Walker {
        items: vec![],
        bytes: 0,
        visited: 0,
        skipped: 0,
        complete: true,
        began: Instant::now(),
        job,
        rules,
        device: meta.dev(),
    };
    walker.walk(anchor.target(), root, 0)?;
    if AnchoredPath::open(root)?.witness.nodes != anchor.witness.nodes {
        return Err("扫描目录的身份已变化，请重新选择。".into());
    }
    // The root may legitimately change while a bounded scan runs; do not claim
    // an atomic directory snapshot. Every selected file is revalidated later.
    let report = Report {
        id: uuid::Uuid::new_v4().to_string(),
        root: root.to_str().unwrap().into(),
        created_at: now,
        expires_at: now + TTL,
        complete: walker.complete,
        cancelled: walker.job.cancelled.load(Ordering::SeqCst),
        skipped: walker.skipped,
        visited: walker.visited,
        items: walker.items.iter().map(row).collect(),
    };
    Ok(Scan {
        id: report.id.clone(),
        root: report.root.clone(),
        created_at: now,
        expires_at: now + TTL,
        items: walker.items,
        report,
    })
}
struct Walker<'a> {
    items: Vec<Item>,
    bytes: usize,
    visited: usize,
    skipped: usize,
    complete: bool,
    began: Instant,
    job: Arc<Job>,
    rules: &'a [String],
    device: u64,
}
impl Walker<'_> {
    fn bounded(&mut self) -> bool {
        let stop = self.job.cancelled.load(Ordering::SeqCst)
            || self.visited >= 10000
            || self.items.len() >= 2000
            || self.bytes >= 7 * 1024 * 1024
            || self.began.elapsed() >= Duration::from_secs(10);
        if stop {
            self.complete = false;
        }
        stop
    }
    fn walk(&mut self, dir: &File, path: &Path, depth: usize) -> Result<(), String> {
        if self.bounded() {
            return Ok(());
        }
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                c".".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            self.complete = false;
            self.skipped += 1;
            return Ok(());
        }
        let ptr = unsafe { libc::fdopendir(fd) };
        if ptr.is_null() {
            unsafe {
                libc::close(fd);
            }
            self.complete = false;
            return Ok(());
        }
        let stream = Stream(ptr);
        while !self.bounded() {
            unsafe {
                *libc::__error() = 0;
            }
            let entry = unsafe { libc::readdir(stream.0) };
            if entry.is_null() {
                if unsafe { *libc::__error() } != 0 {
                    self.complete = false;
                }
                break;
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if [b".".as_slice(), b"..".as_slice()].contains(&name.to_bytes()) {
                continue;
            }
            self.visited += 1;
            let Ok(text) = name.to_str() else {
                self.skipped += 1;
                continue;
            };
            let child = path.join(text);
            if !ordinary(&child) || protected(&child, self.rules)? {
                self.skipped += 1;
                continue;
            }
            let Ok(stat) = directory_probe::stat_at(dir, name) else {
                self.skipped += 1;
                self.complete = false;
                continue;
            };
            if stat.st_dev as u64 != self.device
                || stat.st_flags & 0x40000000 != 0
                || stat.st_uid != unsafe { libc::geteuid() }
            {
                self.skipped += 1;
                continue;
            }
            match stat.st_mode & libc::S_IFMT {
                libc::S_IFDIR => {
                    if depth >= 12 {
                        self.skipped += 1;
                        self.complete = false;
                        continue;
                    }
                    match directory_probe::open_child(dir, name, &stat) {
                        Ok(file) if directory_probe::local(&file) => {
                            self.walk(&file, &child, depth + 1)?
                        }
                        _ => {
                            self.skipped += 1;
                            self.complete = false;
                        }
                    }
                }
                libc::S_IFREG if stat.st_nlink == 1 => {
                    let observed = AnchoredPath::open(&child).and_then(|held| {
                        let node = held.witness.nodes.last().unwrap();
                        if node.dev != self.device
                            || node.ino != stat.st_ino
                            || !held.local_materialized()
                            || held.witness.size > 9_007_199_254_740_991
                        {
                            return Err("扫描期间文件变化。".into());
                        }
                        Ok(held.witness)
                    });
                    match observed {
                        Ok(witness) => {
                            let item = Item {
                                id: uuid::Uuid::new_v4().to_string(),
                                path: child.to_str().unwrap().into(),
                                size: witness.size,
                                modified_at: witness.mtime.saturating_mul(1000),
                                witness,
                            };
                            let size = serde_json::to_vec(&item).map_err(|e| e.to_string())?.len();
                            if self.bytes + size > 7 * 1024 * 1024 {
                                self.bytes = 7 * 1024 * 1024;
                                self.complete = false;
                                break;
                            }
                            self.bytes += size;
                            self.items.push(item);
                        }
                        Err(_) => {
                            self.skipped += 1;
                            self.complete = false;
                        }
                    }
                }
                _ => self.skipped += 1,
            }
        }
        Ok(())
    }
}
