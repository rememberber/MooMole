//! Scope-limited project-marker evidence. Never a Git/content/ownership clearance.
use super::{local, open_child, Catalog};
use serde::Serialize;
use std::{
    ffi::CString,
    fs::File,
    mem::MaybeUninit,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::Path,
};
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hint {
    pub state: &'static str,
    pub path: Option<String>,
    pub kind: Option<&'static str>,
    pub markers: Vec<String>,
}
impl Hint {
    pub fn unknown() -> Self {
        Self {
            state: "unknown",
            path: None,
            kind: None,
            markers: vec![],
        }
    }
    fn absent() -> Self {
        Self {
            state: "notFound",
            ..Self::unknown()
        }
    }
}
fn observe(directory: &File, name: &CString) -> Result<Option<libc::stat>, ()> {
    let mut stat = MaybeUninit::uninit();
    if unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        Ok(Some(unsafe { stat.assume_init() }))
    } else if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
        Ok(None)
    } else {
        Err(())
    }
}
fn same(a: &libc::stat, b: &libc::stat) -> bool {
    a.st_dev == b.st_dev
        && a.st_ino == b.st_ino
        && a.st_mode == b.st_mode
        && a.st_size == b.st_size
        && a.st_nlink == b.st_nlink
        && a.st_flags == b.st_flags
        && a.st_mtime == b.st_mtime
        && a.st_mtime_nsec == b.st_mtime_nsec
        && a.st_ctime == b.st_ctime
        && a.st_ctime_nsec == b.st_ctime_nsec
}
pub struct Probe {
    pub hint: Hint,
    values: Vec<(CString, Result<Option<libc::stat>, ()>)>,
}
impl Probe {
    pub fn inspect(directory: &File, path: &Path, reference: &Catalog) -> Self {
        let mut probe = Self {
            hint: Hint::absent(),
            values: vec![],
        };
        let device = directory.metadata().ok().map(|m| m.dev());
        let mut workspace = vec![];
        let mut project = vec![];
        let mut uncertain = false;
        for marker in reference
            .workspace_indicators
            .iter()
            .chain(&reference.project_indicators)
        {
            let name = CString::new(marker.as_str()).unwrap();
            if probe.values.iter().any(|(n, _)| *n == name) {
                continue;
            }
            let value = observe(directory, &name);
            match &value {
                Err(()) => uncertain = true,
                Ok(None) => {}
                Ok(Some(stat)) => {
                    let kind = stat.st_mode & libc::S_IFMT;
                    let physical = device == Some(stat.st_dev as u64)
                        && stat.st_flags & 0x40000000 == 0
                        && ((kind == libc::S_IFREG && stat.st_nlink == 1)
                            || (marker == ".git"
                                && kind == libc::S_IFDIR
                                && open_child(directory, &name, stat).is_ok_and(|f| local(&f))));
                    if !physical {
                        uncertain = true;
                    } else if reference.workspace_indicators.contains(marker) {
                        workspace.push(marker.clone());
                    } else {
                        project.push(marker.clone());
                    }
                }
            }
            probe.values.push((name, value));
        }
        probe.hint = if uncertain {
            Hint::unknown()
        } else if !workspace.is_empty() {
            Hint {
                state: "hint",
                path: path.to_str().map(String::from),
                kind: Some("workspace"),
                markers: workspace,
            }
        } else if !project.is_empty() {
            Hint {
                state: "hint",
                path: path.to_str().map(String::from),
                kind: Some("project"),
                markers: project,
            }
        } else {
            Hint::absent()
        };
        probe
    }
    pub fn validate(&self, directory: &File) -> bool {
        self.values
            .iter()
            .all(|(name, before)| match (before, observe(directory, name)) {
                (Ok(Some(a)), Ok(Some(b))) => same(a, &b),
                (Ok(None), Ok(None)) | (Err(()), Err(())) => true,
                _ => false,
            })
    }
}
pub fn choose(lineage: &[Hint]) -> Hint {
    if lineage.iter().any(|h| h.state == "unknown") {
        return Hint::unknown();
    }
    lineage
        .iter()
        .rev()
        .find(|h| h.kind == Some("workspace"))
        .or_else(|| lineage.iter().rev().find(|h| h.kind == Some("project")))
        .cloned()
        .unwrap_or_else(Hint::absent)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};
    #[test]
    fn nearest_workspace_overrides_nested_manifests_and_nested_worktrees_win() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        fs::create_dir(root.path().join(".git")).unwrap();
        fs::create_dir_all(root.path().join("web/sub/node_modules")).unwrap();
        fs::write(root.path().join("web/package.json"), "not parsed").unwrap();
        let r = super::super::scan(root.path(), &[]).unwrap();
        assert_eq!(
            r.items[0].project_hint.path.as_deref(),
            root.path().to_str()
        );
        assert_eq!(r.items[0].project_hint.kind, Some("workspace"));
        fs::write(
            root.path().join("web/.git"),
            "gitdir: never read or followed",
        )
        .unwrap();
        let r = super::super::scan(root.path(), &[]).unwrap();
        assert_eq!(
            r.items[0].project_hint.path.as_deref(),
            root.path().join("web").to_str()
        );
    }
    #[test]
    fn nearest_manifest_is_only_a_hint_and_scope_does_not_probe_outside_selected_root() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        fs::write(root.path().join("pnpm-workspace.yaml"), "ignored").unwrap();
        fs::create_dir_all(root.path().join("chosen/rust/deep/target")).unwrap();
        fs::create_dir_all(root.path().join("chosen/unknown/dist")).unwrap();
        fs::write(
            root.path().join("chosen/rust/Cargo.toml"),
            "invalid but present",
        )
        .unwrap();
        let r = super::super::scan(&root.path().join("chosen"), &[]).unwrap();
        assert_eq!(r.items[0].project_hint.kind, Some("project"));
        assert_eq!(r.items[0].project_hint.markers, ["Cargo.toml"]);
        assert_eq!(r.items[1].project_hint.state, "notFound");
        assert_eq!(r.items[1].project_hint.path, None);
        assert!(r.items.iter().all(|i| !i.can_execute));
    }
    #[test]
    fn links_shared_markers_special_types_and_changed_evidence_remain_unknown() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        fs::create_dir(root.path().join("dist")).unwrap();
        fs::write(root.path().join("package.json"), "{}").unwrap();
        symlink(root.path(), root.path().join(".git")).unwrap();
        assert_eq!(
            super::super::scan(root.path(), &[]).unwrap().items[0]
                .project_hint
                .state,
            "unknown"
        );
        fs::remove_file(root.path().join(".git")).unwrap();
        let directory = File::open(root.path()).unwrap();
        let probe = Probe::inspect(&directory, root.path(), &super::super::catalog().unwrap());
        fs::write(root.path().join("package.json"), "changed").unwrap();
        assert!(!probe.validate(&directory));
        fs::hard_link(root.path().join("package.json"), root.path().join("go.mod")).unwrap();
        assert_eq!(
            Probe::inspect(&directory, root.path(), &super::super::catalog().unwrap())
                .hint
                .state,
            "unknown"
        );
    }
}
