//! Exact returned-object read-only evidence. Does not open/enumerate a Trash
//! parent; checks ancestor lstat identity before/after and rejects links. Kernel
//! O_NOFOLLOW_ANY also forbids a symlink anywhere during the exact object open.
//! Unlike source AnchoredPath, ancestor descriptors are NOT retained: this must
//! never become source/mutation authority or a general permission fallback.
use crate::observation::{Node, Witness, VERSION};
use std::{
    fs::{File, Metadata, OpenOptions},
    os::{
        fd::AsRawFd,
        macos::fs::MetadataExt as _,
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, Path, PathBuf},
};

fn node(path: PathBuf, meta: &Metadata) -> Node {
    Node {
        path,
        dev: meta.dev(),
        ino: meta.ino(),
        mode: meta.mode(),
        uid: meta.uid(),
        gid: meta.gid(),
    }
}
fn witness(meta: &Metadata, nodes: Vec<Node>) -> Witness {
    Witness {
        version: VERSION,
        nodes,
        size: meta.size(),
        mtime: meta.mtime(),
        mtime_ns: meta.mtime_nsec(),
        ctime: meta.ctime(),
        ctime_ns: meta.ctime_nsec(),
        links: meta.nlink(),
    }
}
fn chain(path: &Path) -> Result<Witness, String> {
    if !path.is_absolute()
        || path == Path::new("/")
        || path
            .as_os_str()
            .as_bytes()
            .split(|c| *c == b'/')
            .any(|p| p == b"." || p == b"..")
    {
        return Err("系统返回位置无效。".into());
    }
    let parts: Vec<_> = path
        .components()
        .filter_map(|c| {
            if let Component::Normal(p) = c {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    if parts.is_empty() || parts.len() > 128 {
        return Err("系统返回位置层级不支持。".into());
    }
    let mut current = PathBuf::from("/");
    let mut nodes = Vec::new();
    for index in 0..=parts.len() {
        if index > 0 {
            current.push(parts[index - 1]);
        }
        let meta =
            std::fs::symlink_metadata(&current).map_err(|_| "系统返回位置或祖先无法核对。")?;
        if (index < parts.len() && !meta.is_dir())
            || (index == parts.len() && !meta.is_dir() && !meta.is_file())
            || meta.st_flags() & 0x40000000 != 0
        {
            return Err("系统返回位置包含链接、特殊对象或未落地状态。".into());
        }
        if meta.is_file() && meta.nlink() != 1 {
            return Err("系统返回对象是共享硬链接，无法核对。".into());
        }
        nodes.push(node(current.clone(), &meta));
        if index == parts.len() {
            return Ok(witness(&meta, nodes));
        }
    }
    Err("系统返回位置缺少对象。".into())
}

pub(super) struct ReturnedPath {
    path: PathBuf,
    held: File,
    pub(super) witness: Witness,
}
impl ReturnedPath {
    pub(super) fn open(path: &Path) -> Result<Self, String> {
        Self::open_with(path, || {})
    }
    fn open_with(path: &Path, between: impl FnOnce()) -> Result<Self, String> {
        let expected = chain(path)?;
        between();
        let directory = expected
            .nodes
            .last()
            .is_some_and(|n| n.mode & libc::S_IFMT as u32 == libc::S_IFDIR as u32);
        let held = OpenOptions::new()
            .read(true)
            .custom_flags(
                libc::O_CLOEXEC
                    | libc::O_NONBLOCK
                    | libc::O_NOFOLLOW_ANY
                    | if directory {
                        libc::O_DIRECTORY
                    } else {
                        libc::O_EVTONLY
                    },
            )
            .open(path)
            .map_err(|_| "无法打开系统返回的确切对象，结果待核实。")?;
        let result = Self {
            path: path.to_owned(),
            held,
            witness: expected,
        };
        result.validate()?;
        Ok(result)
    }
    pub(super) fn target(&self) -> &File {
        &self.held
    }
    pub(super) fn local_materialized(&self) -> bool {
        let mut out = std::mem::MaybeUninit::<libc::statfs>::zeroed();
        let local = unsafe {
            libc::fstatfs(self.held.as_raw_fd(), out.as_mut_ptr()) == 0
                && out.assume_init().f_flags & libc::MNT_LOCAL as u32 != 0
        };
        local
            && self
                .held
                .metadata()
                .is_ok_and(|m| m.st_flags() & 0x40000000 == 0)
    }
    pub(super) fn validate(&self) -> Result<(), String> {
        let current = chain(&self.path)?;
        let meta = self.held.metadata().map_err(|_| "系统返回对象无法复核。")?;
        let same_node = current.nodes.last() == Some(&node(self.path.clone(), &meta));
        if current != self.witness
            || !same_node
            || witness(&meta, current.nodes.clone()) != current
            || !self.local_materialized()
        {
            return Err("系统返回对象、祖先或内容已变化，结果待核实。".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn exact_returned_object_matches_stable_evidence_and_refuses_links_and_hardlinks() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        std::fs::create_dir(home.join("parent")).unwrap();
        let file = home.join("parent/file");
        std::fs::write(&file, "fixture").unwrap();
        for path in [&file, &home.join("parent")] {
            let returned = ReturnedPath::open(path).unwrap();
            assert_eq!(returned.witness, crate::observation::witness(path).unwrap());
            returned.validate().unwrap();
        }
        symlink(home.join("parent"), home.join("alias")).unwrap();
        assert!(ReturnedPath::open(&home.join("alias/file")).is_err());
        symlink(&file, home.join("link")).unwrap();
        assert!(ReturnedPath::open(&home.join("link")).is_err());
        std::fs::hard_link(&file, home.join("shared")).unwrap();
        assert!(ReturnedPath::open(&file).is_err());
        for invalid in [
            Path::new("relative"),
            Path::new("/"),
            Path::new("/a/../b"),
            Path::new("/a/./b"),
        ] {
            assert!(ReturnedPath::open(invalid).is_err());
        }
    }

    #[test]
    fn exact_open_rejects_replaced_target_ancestor_or_late_ancestor_link() {
        for mode in ["target", "ancestor", "ancestorLink"] {
            let temp = tempfile::tempdir().unwrap();
            let home = temp.path().canonicalize().unwrap();
            let parent = home.join("parent");
            std::fs::create_dir(&parent).unwrap();
            let file = parent.join("file");
            std::fs::write(&file, "original").unwrap();
            let result = ReturnedPath::open_with(&file, || {
                if mode == "target" {
                    std::fs::rename(&file, parent.join("original")).unwrap();
                    std::fs::write(&file, "replacement").unwrap();
                } else {
                    std::fs::rename(&parent, home.join("detached")).unwrap();
                    if mode == "ancestorLink" {
                        symlink(home.join("detached"), &parent).unwrap();
                    } else {
                        std::fs::create_dir(&parent).unwrap();
                        std::fs::write(&file, "replacement").unwrap();
                    }
                }
            });
            assert!(result.is_err(), "{mode}");
        }
    }

    #[test]
    fn exact_returned_object_detects_later_content_and_ancestor_changes() {
        for mode in ["content", "ancestor"] {
            let temp = tempfile::tempdir().unwrap();
            let home = temp.path().canonicalize().unwrap();
            let parent = home.join("parent");
            std::fs::create_dir(&parent).unwrap();
            let file = parent.join("file");
            std::fs::write(&file, "original").unwrap();
            let returned = ReturnedPath::open(&file).unwrap();
            if mode == "content" {
                std::fs::write(&file, "changed after exact open").unwrap();
            } else {
                std::fs::rename(&parent, home.join("detached")).unwrap();
                std::fs::create_dir(&parent).unwrap();
                std::fs::write(&file, "unselected replacement").unwrap();
            }
            assert!(returned.validate().is_err(), "{mode}");
        }
    }
}
