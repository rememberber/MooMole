use super::Evidence;
use crate::{
    directory_probe::{self, Stream},
    engine::Job,
    observation::AnchoredPath,
    review::Scan,
};
use std::{
    collections::HashSet,
    ffi::{CStr, CString},
    fs::File,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
const MAX_EVIDENCE: usize = 400;
const MAX_VISITS: usize = 2000;
fn overlap(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}
fn protected(path: &Path, rules: &[String]) -> bool {
    rules.iter().any(|r| overlap(path, Path::new(r)))
}
fn sensitive(name: &str) -> bool {
    name.starts_with('.')
        || ["Library", "Applications", "System", "Keychains"]
            .iter()
            .any(|n| name.eq_ignore_ascii_case(n))
        || [".pem", ".key", ".p12", ".pfx", ".keychain-db"]
            .iter()
            .any(|suffix| name.to_ascii_lowercase().ends_with(suffix))
}
pub(super) fn validate_root(root: &str, rules: &[String]) -> Result<(), String> {
    if root.len() > 4096
        || !root.starts_with('/')
        || root.chars().any(char::is_control)
        || root[1..]
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || sensitive(p))
        || rules.iter().any(|r| Path::new(root).starts_with(r))
    {
        return Err(
            "请选择未受保护的普通目录；系统目录、隐藏目录、符号链接和云占位目录不在本次范围。"
                .into(),
        );
    }
    let anchored = AnchoredPath::open(Path::new(root))?;
    if !anchored.target().metadata().is_ok_and(|m| m.is_dir()) || !anchored.local_materialized() {
        return Err("请选择已下载到本机的物理目录。".into());
    }
    Ok(())
}
pub(super) fn collect(
    scan: Option<&Scan>,
    root: &Path,
    rules: &[String],
    job: Arc<Job>,
) -> Result<(Vec<Evidence>, Vec<String>), String> {
    validate_root(root.to_str().ok_or("目录名称无法编码。")?, rules)?;
    let mut inventory = Inventory {
        evidence: vec![],
        limitations: vec![],
        paths: HashSet::new(),
        visits: 0,
        skipped: 0,
        started: Instant::now(),
        rules,
        job,
    };
    if let Some(scan) = scan {
        let mut rows = scan.items.iter().collect::<Vec<_>>();
        rows.sort_by_key(|row| std::cmp::Reverse(row.candidate.size.unwrap_or(0)));
        let mut count = 0;
        for row in rows {
            let c = &row.candidate;
            if protected(Path::new(&c.path), rules)
                || c.resolved_path
                    .as_ref()
                    .is_some_and(|p| protected(Path::new(p), rules))
            {
                inventory.skipped += 1;
                continue;
            }
            if count >= 200 {
                inventory.note("Mole 候选最多发送按大小排序的前 200 项，其余未交给 AI 分析。");
                break;
            }
            // Explicitly omit terminal output, content and internal raw provenance.
            inventory.push(Evidence {
                id: String::new(),
                source: "mole".into(),
                path: c.path.clone(),
                size: c.size,
                modified_at: None,
                is_directory: c.is_directory,
                category: c
                    .impact
                    .as_ref()
                    .map(|i| i.kind.clone())
                    .unwrap_or("unknown".into()),
                candidate_id: Some(c.id.clone()),
            });
            count += 1;
        }
    }
    let anchored = AnchoredPath::open(root)?;
    inventory.walk(
        anchored.target(),
        root,
        0,
        anchored
            .target()
            .metadata()
            .map_err(|_| "无法读取目录。")?
            .dev(),
    )?;
    anchored.validate()?;
    if inventory.skipped > 0 {
        inventory.note(&format!(
            "已跳过 {} 项：保护、隐藏/敏感位置、符号链接、其他卷、云占位或不可读项目。",
            inventory.skipped
        ));
    }
    inventory.note("扩展扫描只读取名称、类型、文件逻辑大小和修改时间；目录大小未知。最多 3 层、2000 个目录条目、400 条总线索、5 秒预算，不是全盘完整扫描。");
    Ok((inventory.evidence, inventory.limitations))
}
struct Inventory<'a> {
    evidence: Vec<Evidence>,
    limitations: Vec<String>,
    paths: HashSet<String>,
    visits: usize,
    skipped: usize,
    started: Instant,
    rules: &'a [String],
    job: Arc<Job>,
}
impl Inventory<'_> {
    fn note(&mut self, note: &str) {
        if !self.limitations.iter().any(|s| s == note) {
            self.limitations.push(note.into());
        }
    }
    fn push(&mut self, mut e: Evidence) {
        if self.paths.insert(e.path.clone()) {
            e.id = format!("e{}", self.evidence.len());
            self.evidence.push(e);
        }
    }
    fn bounded(&mut self) -> bool {
        if self.visits >= MAX_VISITS
            || self.evidence.len() >= MAX_EVIDENCE
            || self.started.elapsed() >= Duration::from_secs(5)
        {
            self.note("扩展扫描达到数量或时间上限，仅保留部分结果；可缩小目录重新扫描。");
            true
        } else {
            false
        }
    }
    fn walk(&mut self, dir: &File, path: &Path, depth: usize, device: u64) -> Result<(), String> {
        if self.job.cancelled.load(Ordering::SeqCst) {
            return Err("扫描已取消。".into());
        }
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
            self.skipped += 1;
            return Ok(());
        }
        let ptr = unsafe { libc::fdopendir(fd) };
        if ptr.is_null() {
            unsafe {
                libc::close(fd);
            }
            self.skipped += 1;
            return Ok(());
        }
        let stream = Stream(ptr);
        loop {
            if self.job.cancelled.load(Ordering::SeqCst) {
                return Err("扫描已取消。".into());
            }
            if self.bounded() {
                break;
            }
            unsafe {
                *libc::__error() = 0;
            }
            let entry = unsafe { libc::readdir(stream.0) };
            if entry.is_null() {
                if unsafe { *libc::__error() } != 0 {
                    self.note("部分目录读取失败，结果不完整。");
                }
                break;
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if [b".".as_slice(), b"..".as_slice()].contains(&name.to_bytes()) {
                continue;
            }
            self.visits += 1;
            let Ok(text) = name.to_str() else {
                self.skipped += 1;
                continue;
            };
            let child_path = path.join(text);
            if sensitive(text)
                || text.chars().any(char::is_control)
                || child_path.as_os_str().len() > 4096
                || protected(&child_path, self.rules)
            {
                self.skipped += 1;
                continue;
            }
            let Ok(stat) = directory_probe::stat_at(dir, name) else {
                self.skipped += 1;
                continue;
            };
            let kind = stat.st_mode & libc::S_IFMT;
            if ![libc::S_IFREG, libc::S_IFDIR].contains(&kind)
                || stat.st_dev as u64 != device
                || stat.st_flags & 0x40000000 != 0
            {
                self.skipped += 1;
                continue;
            }
            let is_dir = kind == libc::S_IFDIR;
            self.push(Evidence {
                id: String::new(),
                source: "metadata".into(),
                path: child_path.to_string_lossy().into_owned(),
                size: (!is_dir).then_some(stat.st_size.max(0) as u64),
                modified_at: stat.st_mtime.checked_mul(1000),
                is_directory: is_dir,
                category: if is_dir { "directory" } else { "file" }.into(),
                candidate_id: None,
            });
            if is_dir {
                if depth >= 2 {
                    self.note("部分子目录达到 3 层深度上限，未继续展开。");
                    continue;
                }
                let name = CString::new(name.to_bytes()).map_err(|_| "目录名无效。")?;
                match directory_probe::open_child(dir, &name, &stat) {
                    Ok(file) if directory_probe::local(&file) => {
                        self.walk(&file, &child_path, depth + 1, device)?
                    }
                    _ => self.skipped += 1,
                }
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::TaskKind;
    #[test]
    fn bounded_metadata_excludes_secrets_protection_and_links() {
        let dir = tempfile::Builder::new()
            .prefix("moomole-ai-test-")
            .tempdir()
            .unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("movie.mp4"), b"synthetic").unwrap();
        std::fs::write(root.join(".env"), b"secret").unwrap();
        std::fs::write(root.join("private.key"), b"secret").unwrap();
        std::fs::create_dir(root.join("retained")).unwrap();
        std::fs::write(root.join("retained/file"), b"keep").unwrap();
        std::os::unix::fs::symlink("/", root.join("link")).unwrap();
        let rules = vec![root.join("retained").to_string_lossy().into_owned()];
        let (items, limits) =
            collect(None, &root, &rules, Arc::new(Job::new(TaskKind::AiScan))).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].path.ends_with("movie.mp4"));
        assert_eq!(items[0].size, Some(9));
        assert!(items[0].candidate_id.is_none());
        assert!(!limits.is_empty());
        assert!(validate_root("/", &[]).is_err());
        let job = Arc::new(Job::new(TaskKind::AiScan));
        job.cancel();
        assert!(collect(None, &root, &rules, job).is_err());
    }
}
