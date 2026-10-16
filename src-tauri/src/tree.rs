//! Bounded metadata inventory. Directory descriptors and no-follow traversal
//! keep this read-only observation inside the selected physical directory.
//! A matching digest is not an atomic snapshot or execution authorization.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::{CStr, CString},
    fs::File,
    mem::MaybeUninit,
    os::fd::{AsRawFd, FromRawFd, IntoRawFd},
    os::unix::ffi::OsStrExt,
    path::Path,
    time::{Duration, Instant},
};

pub const MAX_ENTRIES: usize = 10_000;
pub const VERSION: u32 = 2;
const MAX_DEPTH: usize = 64;

#[derive(Clone, Copy)]
enum Checkpoint<'a> {
    Opened,
    Entry(#[allow(dead_code)] &'a CStr),
    Finished,
}

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
    path: &Path,
    device: u64,
    depth: usize,
    digest: &mut Sha256,
    budget: &mut Budget,
    count: &mut usize,
    hook: &mut impl FnMut(Checkpoint<'_>),
    visit: &mut impl FnMut(&Path) -> Result<(), String>,
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
        let child_path = path.join(std::ffi::OsStr::from_bytes(name.to_bytes()));
        visit(&child_path)?;
        if kind == libc::S_IFDIR {
            let child = open_directory(directory, &name)?;
            if stamp(&stat_fd(&child)?) != stamp(&stat) {
                return Err("子目录身份已变化，需要重新扫描。".into());
            }
            hook(Checkpoint::Entry(&name));
            walk(
                &child,
                &child_path,
                device,
                depth + 1,
                digest,
                budget,
                count,
                hook,
                visit,
            )?;
        } else {
            hook(Checkpoint::Entry(&name));
        }
        // A held child directory can be detached or replaced while walking it.
        // Recheck the parent's name, as well as the held directory's stamp.
        if stamp(&stat_entry(directory, &name)?) != stamp(&stat) {
            return Err("检查期间目录子项发生变化，需要重新扫描。".into());
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
    inspect_with(path, dev, ino, budget, |_| {})
}

fn inspect_with(
    path: &Path,
    dev: u64,
    ino: u64,
    budget: &mut Budget,
    hook: impl FnMut(Checkpoint<'_>),
) -> Result<TreeWitness, String> {
    inspect_observed(path, dev, ino, budget, hook, |_| Ok(()))
}

// Paths come from the same no-follow walk as the digest, never a second glob.
// They are temporary probe inputs, not added to the persisted witness.
fn probe_path(entry: &Path) -> Result<String, String> {
    entry
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "后代路径含无法传递给 Mole 的字符，本次未完成检查。".into())
}

pub fn inspect_paths(
    path: &Path,
    dev: u64,
    ino: u64,
    budget: &mut Budget,
    limit: usize,
) -> Result<(TreeWitness, Vec<String>), String> {
    let mut paths = Vec::new();
    let tree = inspect_observed(
        path,
        dev,
        ino,
        budget,
        |_| {},
        |entry| {
            if paths.len() >= limit {
                return Err("后代路径超过本批规则检查的剩余额度；本次未完成后代保护检查。".into());
            }
            paths.push(probe_path(entry)?);
            Ok(())
        },
    )?;
    Ok((tree, paths))
}

fn inspect_observed(
    path: &Path,
    dev: u64,
    ino: u64,
    budget: &mut Budget,
    mut hook: impl FnMut(Checkpoint<'_>),
    mut visit: impl FnMut(&Path) -> Result<(), String>,
) -> Result<TreeWitness, String> {
    budget.check_time()?;
    let anchor = crate::observation::AnchoredPath::open(path)?;
    let directory = anchor.target();
    let root = stat_fd(directory)?;
    if root.st_mode & libc::S_IFMT != libc::S_IFDIR {
        return Err("目标不是目录，请重新扫描。".into());
    }
    if root.st_dev as u64 != dev || root.st_ino != ino {
        return Err("目录身份与计划不一致，需要重新扫描。".into());
    }
    budget.tick()?;
    hook(Checkpoint::Opened);
    let mut digest = Sha256::new();
    let mut count = 0;
    walk(
        directory,
        path,
        dev,
        0,
        &mut digest,
        budget,
        &mut count,
        &mut hook,
        &mut visit,
    )?;
    hook(Checkpoint::Finished);
    anchor.validate()?;
    budget.check_time()?;
    Ok(TreeWitness {
        version: VERSION,
        digest: format!("{:x}", digest.finalize()),
        entries: count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::{
        ffi::OsStrExt,
        fs::{symlink, MetadataExt},
    };
    fn inspect_fixture(path: &Path) -> Result<TreeWitness, String> {
        let m = std::fs::metadata(path).unwrap();
        inspect(path, m.dev(), m.ino(), &mut Budget::new())
    }
    #[test]
    fn probe_paths_share_the_digest_and_reject_truncation_and_unrepresentable_names() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().canonicalize().unwrap();
        std::fs::create_dir(path.join("nested")).unwrap();
        std::fs::write(path.join("nested/中文\n[*]"), "fixture").unwrap();
        let m = std::fs::metadata(&path).unwrap();
        let (listed, paths) =
            inspect_paths(&path, m.dev(), m.ino(), &mut Budget::new(), 2).unwrap();
        assert_eq!(listed, inspect_fixture(&path).unwrap());
        assert_eq!(
            paths,
            vec![
                path.join("nested").to_str().unwrap(),
                path.join("nested/中文\n[*]").to_str().unwrap()
            ]
        );
        assert!(
            inspect_paths(&path, m.dev(), m.ino(), &mut Budget::new(), 1)
                .unwrap_err()
                .contains("额度")
        );
        let invalid = std::ffi::OsStr::from_bytes(b"invalid-\xff");
        // APFS may reject creating this name with EILSEQ. Validate the exact
        // conversion boundary without depending on a volume accepting it.
        assert!(probe_path(&path.join(invalid))
            .unwrap_err()
            .contains("字符"));
    }
    #[test]
    fn detached_roots_and_ancestors_cannot_publish_a_matching_inventory() {
        for mode in ["root", "ancestor", "late-root", "late-content"] {
            let scratch = tempfile::tempdir().unwrap();
            let base = scratch.path().canonicalize().unwrap();
            let parent = base.join("parent");
            let path = parent.join("selected");
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("file"), "original").unwrap();
            let m = std::fs::metadata(&path).unwrap();
            let mut changed = false;
            let result = inspect_with(&path, m.dev(), m.ino(), &mut Budget::new(), |at| {
                let should_change = match at {
                    Checkpoint::Opened => mode == "root" || mode == "ancestor",
                    Checkpoint::Finished => mode.starts_with("late-"),
                    _ => false,
                };
                if !should_change || changed {
                    return;
                }
                changed = true;
                match mode {
                    "root" | "late-root" => {
                        std::fs::rename(&path, parent.join("detached")).unwrap();
                        std::fs::create_dir(&path).unwrap();
                        std::fs::write(path.join("file"), "replacement").unwrap();
                    }
                    "ancestor" => {
                        std::fs::rename(&parent, base.join("detached")).unwrap();
                        std::fs::create_dir_all(&path).unwrap();
                    }
                    "late-content" => std::fs::write(path.join("new-file"), "late").unwrap(),
                    _ => unreachable!(),
                }
            });
            assert!(changed);
            assert!(result.is_err(), "{mode}");
        }
    }

    #[test]
    fn child_replacement_and_leaf_mutation_during_walk_are_refused() {
        for mode in ["directory", "file", "hardlink"] {
            let scratch = tempfile::tempdir().unwrap();
            let path = scratch.path().canonicalize().unwrap();
            std::fs::create_dir(path.join("nested")).unwrap();
            std::fs::write(path.join("nested/file"), "original").unwrap();
            let m = std::fs::metadata(&path).unwrap();
            let mut changed = false;
            let result = inspect_with(&path, m.dev(), m.ino(), &mut Budget::new(), |at| {
                let Checkpoint::Entry(name) = at else {
                    return;
                };
                if changed {
                    return;
                }
                if mode == "directory" && name.to_bytes() == b"nested" {
                    changed = true;
                    std::fs::rename(path.join("nested"), path.join("detached")).unwrap();
                    symlink("/", path.join("nested")).unwrap();
                } else if mode != "directory" && name.to_bytes() == b"file" {
                    changed = true;
                    if mode == "file" {
                        std::fs::write(path.join("nested/file"), "changed").unwrap();
                    } else {
                        std::fs::hard_link(path.join("nested/file"), path.join("shared")).unwrap();
                    }
                }
            });
            assert!(changed);
            assert!(result.is_err(), "{mode}");
        }
    }
    #[test]
    fn late_deep_mutation_demonstrates_that_inventory_is_not_an_atomic_snapshot() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().canonicalize().unwrap();
        std::fs::create_dir(path.join("nested")).unwrap();
        let file = path.join("nested/file");
        std::fs::write(&file, "original").unwrap();
        let before = inspect_fixture(&path).unwrap();
        let m = std::fs::metadata(&path).unwrap();
        let observed = inspect_with(&path, m.dev(), m.ino(), &mut Budget::new(), |at| {
            if matches!(at, Checkpoint::Finished) {
                // This does not change either parent directory's metadata.
                std::fs::write(&file, "modified after walking this descendant").unwrap();
            }
        })
        .unwrap();
        assert_eq!(before, observed);
        assert_ne!(observed, inspect_fixture(&path).unwrap());
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
