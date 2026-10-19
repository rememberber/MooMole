//! Read-only, descriptor-relative path observation. A held descriptor does not
//! lock its name or contents; revalidation detects changes, not an atomic snapshot.
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CStr, CString},
    fs::{File, Metadata},
    mem::MaybeUninit,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path, PathBuf},
};

pub const VERSION: u32 = 2;
const MAX_COMPONENTS: usize = 128;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Node {
    pub path: PathBuf,
    pub dev: u64,
    pub ino: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Witness {
    // Old JSON remains readable, but cannot be used as current evidence.
    #[serde(default)]
    pub version: u32,
    pub nodes: Vec<Node>,
    pub size: u64,
    pub mtime: i64,
    pub mtime_ns: i64,
    pub ctime: i64,
    pub ctime_ns: i64,
    #[serde(default)]
    pub links: u64,
}

fn node(path: PathBuf, m: &Metadata) -> Node {
    Node {
        path,
        dev: m.dev(),
        ino: m.ino(),
        mode: m.mode(),
        uid: m.uid(),
        gid: m.gid(),
    }
}

fn matches(stat: &libc::stat, n: &Node) -> bool {
    stat.st_dev as u64 == n.dev
        && stat.st_ino == n.ino
        && stat.st_mode as u32 == n.mode
        && stat.st_uid == n.uid
        && stat.st_gid == n.gid
}

fn stat_at(parent: &File, name: &CStr) -> Result<libc::stat, String> {
    let mut stat = MaybeUninit::uninit();
    // SAFETY: parent/name outlive the call; only success initializes stat.
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err("路径已不存在或无法读取，请重新扫描。".into());
    }
    Ok(unsafe { stat.assume_init() })
}

struct Handle {
    file: File,
    node: Node,
    name: Option<CString>,
}

pub struct AnchoredPath {
    handles: Vec<Handle>,
    pub witness: Witness,
}

impl AnchoredPath {
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::open_with(path, |_, _| {})
    }

    fn open_with(path: &Path, mut hook: impl FnMut(&Path, bool)) -> Result<Self, String> {
        if !path.is_absolute()
            || path == Path::new("/")
            || path
                .as_os_str()
                .as_bytes()
                .split(|b| *b == b'/')
                .any(|p| p == b"." || p == b"..")
        {
            return Err("路径范围无效，需要重新扫描。".into());
        }
        let parts = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(n) => Some(n),
                _ => None,
            })
            .collect::<Vec<_>>();
        if parts.is_empty() || parts.len() > MAX_COMPONENTS {
            return Err("路径层级超过 128 层限制，请缩小范围。".into());
        }
        let root = File::open("/").map_err(|e| e.to_string())?;
        let root_node = node(
            PathBuf::from("/"),
            &root.metadata().map_err(|e| e.to_string())?,
        );
        let mut handles = vec![Handle {
            file: root,
            node: root_node,
            name: None,
        }];
        let mut current = PathBuf::from("/");
        for (i, part) in parts.iter().enumerate() {
            current.push(part);
            let name = CString::new(part.as_bytes()).map_err(|_| "路径名称无法编码。")?;
            let parent = &handles.last().unwrap().file;
            let before = stat_at(parent, &name)?;
            let kind = before.st_mode & libc::S_IFMT;
            if kind != libc::S_IFDIR && kind != libc::S_IFREG {
                return Err("路径或祖先包含符号链接或特殊文件，请重新扫描。".into());
            }
            if i + 1 < parts.len() && kind != libc::S_IFDIR {
                return Err("祖先路径不是目录。".into());
            }
            if i + 1 == parts.len()
                && path.as_os_str().as_bytes().ends_with(b"/")
                && kind != libc::S_IFDIR
            {
                return Err("以分隔符结尾的路径不是目录。".into());
            }
            hook(&current, false);
            // O_EVTONLY observes regular-file metadata without opening its contents
            // for reading. Every ancestor is opened relative to its held parent.
            let access = if kind == libc::S_IFDIR {
                libc::O_RDONLY | libc::O_DIRECTORY
            } else {
                libc::O_EVTONLY
            };
            let fd = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    access | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err("路径无法打开或检查期间已变化，请重新扫描。".into());
            }
            // SAFETY: successful openat transfers one descriptor to File.
            let file = unsafe { File::from_raw_fd(fd) };
            let metadata = file.metadata().map_err(|e| e.to_string())?;
            let observed = node(current.clone(), &metadata);
            if !matches(&before, &observed) {
                return Err("打开期间文件或祖先身份已变化，请重新扫描。".into());
            }
            if i + 1 == parts.len()
                && (before.st_size as u64 != metadata.size()
                    || before.st_mtime != metadata.mtime()
                    || before.st_mtime_nsec != metadata.mtime_nsec()
                    || before.st_ctime != metadata.ctime()
                    || before.st_ctime_nsec != metadata.ctime_nsec()
                    || before.st_nlink as u64 != metadata.nlink())
            {
                return Err("打开期间目标属性已变化，请重新扫描。".into());
            }
            handles.push(Handle {
                file,
                node: observed,
                name: Some(name),
            });
        }
        let target = handles
            .last()
            .unwrap()
            .file
            .metadata()
            .map_err(|e| e.to_string())?;
        if target.is_file() && target.nlink() != 1 {
            return Err("目标是共享硬链接文件，需要单独审阅。".into());
        }
        let witness = Witness {
            version: VERSION,
            nodes: handles.iter().map(|h| h.node.clone()).collect(),
            size: target.size(),
            mtime: target.mtime(),
            mtime_ns: target.mtime_nsec(),
            ctime: target.ctime(),
            ctime_ns: target.ctime_nsec(),
            links: target.nlink(),
        };
        let result = Self { handles, witness };
        hook(path, true);
        result.validate()?;
        Ok(result)
    }

    /// Metadata-only callers may restrict observations to local, materialized paths.
    #[cfg(target_os = "macos")]
    pub fn local_materialized(&self) -> bool {
        use std::os::macos::fs::MetadataExt as _;
        self.handles.iter().all(|handle| {
            let mut out = MaybeUninit::<libc::statfs>::zeroed();
            let local = unsafe {
                libc::fstatfs(handle.file.as_raw_fd(), out.as_mut_ptr()) == 0
                    && out.assume_init().f_flags & libc::MNT_LOCAL as u32 != 0
            };
            local
                && handle
                    .file
                    .metadata()
                    .is_ok_and(|m| m.st_flags() & 0x40000000 == 0)
        })
    }

    pub fn target(&self) -> &File {
        &self.handles.last().unwrap().file
    }

    pub fn parent(&self) -> &File {
        // open() requires a non-root path, so a parent handle always exists.
        &self.handles[self.handles.len() - 2].file
    }

    pub fn validate(&self) -> Result<(), String> {
        for (index, handle) in self.handles.iter().enumerate() {
            let current = handle.file.metadata().map_err(|e| e.to_string())?;
            if node(handle.node.path.clone(), &current) != handle.node {
                return Err("检查期间文件或祖先属性已变化，请重新扫描。".into());
            }
            if index > 0
                && !matches(
                    &stat_at(&self.handles[index - 1].file, handle.name.as_ref().unwrap())?,
                    &handle.node,
                )
            {
                return Err("检查期间路径绑定已变化，请重新扫描。".into());
            }
        }
        let m = self.target().metadata().map_err(|e| e.to_string())?;
        let w = &self.witness;
        if (
            m.size(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
            m.nlink(),
        ) != (w.size, w.mtime, w.mtime_ns, w.ctime, w.ctime_ns, w.links)
        {
            return Err("检查期间目标内容或属性已变化，请重新扫描。".into());
        }
        Ok(())
    }
}

pub fn witness(path: &Path) -> Result<Witness, String> {
    Ok(AnchoredPath::open(path)?.witness)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn swapped_target_and_ancestor_are_refused_during_open() {
        for mode in ["file", "link", "parent"] {
            let t = tempfile::tempdir().unwrap();
            let root = t.path().canonicalize().unwrap();
            let parent = root.join("parent");
            std::fs::create_dir(&parent).unwrap();
            let path = parent.join("缓存\n[*]");
            std::fs::write(&path, "original").unwrap();
            let mut changed = false;
            let result = AnchoredPath::open_with(&path, |at, finished| {
                if at == path && !finished && !changed {
                    changed = true;
                    if mode == "parent" {
                        std::fs::rename(&parent, root.join("detached")).unwrap();
                        std::fs::create_dir(&parent).unwrap();
                        std::fs::write(&path, "replacement").unwrap();
                    } else {
                        std::fs::rename(&path, parent.join("original")).unwrap();
                        if mode == "link" {
                            symlink(parent.join("original"), &path).unwrap();
                        } else {
                            std::fs::write(&path, "replacement").unwrap();
                        }
                    }
                }
            });
            assert!(changed);
            assert!(result.is_err(), "{mode}");
        }
    }

    #[test]
    fn held_descriptors_do_not_hide_late_name_parent_or_content_changes() {
        for mode in ["name", "parent", "content", "hardlink"] {
            let t = tempfile::tempdir().unwrap();
            let root = t.path().canonicalize().unwrap();
            let parent = root.join("parent");
            std::fs::create_dir(&parent).unwrap();
            let path = parent.join("file");
            std::fs::write(&path, "original").unwrap();
            let anchor = AnchoredPath::open(&path).unwrap();
            anchor.validate().unwrap();
            match mode {
                "name" => {
                    std::fs::rename(&path, parent.join("old")).unwrap();
                    std::fs::write(&path, "replacement").unwrap();
                }
                "parent" => {
                    std::fs::rename(&parent, root.join("old")).unwrap();
                    std::fs::create_dir(&parent).unwrap();
                }
                "content" => std::fs::write(&path, "changed").unwrap(),
                "hardlink" => std::fs::hard_link(&path, parent.join("shared")).unwrap(),
                _ => unreachable!(),
            }
            assert!(anchor.validate().is_err(), "{mode}");
        }
    }

    #[test]
    fn stable_observations_round_trip_and_links_special_files_and_ranges_fail() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().canonicalize().unwrap();
        let path = root.join("file");
        std::fs::write(&path, "original").unwrap();
        let w = witness(&path).unwrap();
        assert_eq!(w, witness(&path).unwrap());
        let copy: Witness = serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();
        assert_eq!(w, copy);
        assert!(witness(&PathBuf::from(format!("{}/", path.display()))).is_err());
        assert!(witness(&PathBuf::from(format!("{}/", root.display()))).is_ok());
        let too_deep = PathBuf::from(format!("/{}", vec!["a"; 129].join("/")));
        assert!(witness(&too_deep).unwrap_err().contains("128"));
        std::fs::hard_link(&path, root.join("shared")).unwrap();
        assert!(witness(&path).is_err());
        symlink(&root, root.join("alias")).unwrap();
        assert!(witness(&root.join("alias/file")).is_err());
        let fifo = CString::new(root.join("pipe").as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(witness(&root.join("pipe")).is_err());
        for path in [
            Path::new("/"),
            Path::new("relative"),
            Path::new("/a/../b"),
            Path::new("/a/./b"),
        ] {
            assert!(witness(path).is_err());
        }
    }
}
