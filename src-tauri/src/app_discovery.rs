//! Bounded, read-only installation-root discovery. Never proof of global absence.
use serde::Serialize;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub code: &'static str,
    pub issue: Option<&'static str>,
    root_count: usize,
    entries: usize,
    candidates: usize,
    first_uncertain_path: Option<String>,
}
impl Observation {
    pub fn not_checked() -> Self {
        Self {
            code: "notChecked",
            issue: None,
            root_count: 0,
            entries: 0,
            candidates: 0,
            first_uncertain_path: None,
        }
    }
}
#[cfg(target_os = "macos")]
pub mod mac {
    use super::*;
    use std::{
        collections::HashSet,
        ffi::{CStr, CString},
        fs::File,
        os::fd::{AsRawFd, FromRawFd, IntoRawFd},
        path::{Component, Path, PathBuf},
        time::{Duration, Instant},
    };
    const FLAGS: i32 =
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
    const SF_DATALESS: u32 = 0x40000000;
    fn stat_at(file: &File, name: &CStr) -> Result<Option<libc::stat>, ()> {
        let mut s = std::mem::MaybeUninit::zeroed();
        if unsafe {
            libc::fstatat(
                file.as_raw_fd(),
                name.as_ptr(),
                s.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0
        {
            Ok(Some(unsafe { s.assume_init() }))
        } else if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
            Ok(None)
        } else {
            Err(())
        }
    }
    fn stat_fd(file: &File) -> Result<libc::stat, ()> {
        let mut s = std::mem::MaybeUninit::zeroed();
        if unsafe { libc::fstat(file.as_raw_fd(), s.as_mut_ptr()) } == 0 {
            Ok(unsafe { s.assume_init() })
        } else {
            Err(())
        }
    }
    type Object = (i32, u64, u16, u32, u32, u32);
    fn object(s: &libc::stat) -> Object {
        (
            s.st_dev, s.st_ino, s.st_mode, s.st_flags, s.st_uid, s.st_gid,
        )
    }
    fn stamp(s: &libc::stat) -> (Object, i64, i64, i64, i64, i64) {
        (
            object(s),
            s.st_size,
            s.st_mtime,
            s.st_mtime_nsec,
            s.st_ctime,
            s.st_ctime_nsec,
        )
    }
    fn local(file: &File) -> bool {
        let mut s = std::mem::MaybeUninit::<libc::statfs>::zeroed();
        unsafe {
            libc::fstatfs(file.as_raw_fd(), s.as_mut_ptr()) == 0
                && s.assume_init().f_flags & libc::MNT_LOCAL as u32 != 0
        }
    }
    // Own the mount snapshot: getmntinfo's shared buffer is unsafe across workers.
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct Mount {
        path: PathBuf,
        fsid: Vec<u8>,
        flags: u32,
        kind: String,
        source: Vec<i8>,
    }
    fn mount_table(scope: &Path) -> Result<Vec<Mount>, ()> {
        const LIMIT: usize = 256;
        let mut buffer: Vec<libc::statfs> =
            (0..LIMIT).map(|_| unsafe { std::mem::zeroed() }).collect();
        let count = unsafe {
            libc::getfsstat(
                buffer.as_mut_ptr(),
                std::mem::size_of_val(buffer.as_slice()) as i32,
                libc::MNT_NOWAIT,
            )
        };
        if count < 0 || count as usize >= LIMIT {
            return Err(());
        }
        let mut result = Vec::new();
        for item in buffer.into_iter().take(count as usize) {
            fn string(bytes: &[i8]) -> Result<String, ()> {
                let end = bytes.iter().position(|b| *b == 0).ok_or(())?;
                String::from_utf8(bytes[..end].iter().map(|b| *b as u8).collect()).map_err(|_| ())
            }
            let path = PathBuf::from(string(&item.f_mntonname)?);
            if !path.is_absolute()
                || path
                    .to_str()
                    .is_none_or(|s| s.chars().any(char::is_control))
            {
                return Err(());
            }
            if !scope.starts_with(&path) && !path.starts_with(scope) {
                continue;
            }
            let fsid = unsafe {
                std::slice::from_raw_parts(
                    (&item.f_fsid as *const libc::fsid_t).cast::<u8>(),
                    std::mem::size_of::<libc::fsid_t>(),
                )
                .to_vec()
            };
            result.push(Mount {
                path,
                fsid,
                flags: item.f_flags,
                kind: string(&item.f_fstypename)?,
                source: item.f_mntfromname.to_vec(),
            });
        }
        result.sort();
        Ok(result)
    }
    fn local_mount(path: &Path, mounts: &[Mount]) -> bool {
        let enclosing: Vec<_> = mounts
            .iter()
            .filter(|m| path.starts_with(&m.path))
            .collect();
        !enclosing.is_empty()
            && enclosing.iter().all(|m| {
                m.flags & libc::MNT_LOCAL as u32 != 0
                    && m.flags & libc::MNT_DONTBROWSE as u32 == 0
                    && m.kind != "autofs"
            })
    }
    fn internal_snapshot(name: &str, parent: &libc::stat, child: &libc::stat) -> bool {
        name == "com.apple.TimeMachine.localsnapshots"
            && child.st_mode & libc::S_IFMT == libc::S_IFDIR
            && parent.st_uid == 0
            && child.st_uid == 0
            && parent.st_dev == child.st_dev
    }
    enum Opened {
        Directory(File),
        Missing {
            parent: PathBuf,
            name: CString,
            object: libc::stat,
        },
    }
    fn open(path: &Path) -> Result<Opened, ()> {
        if !path.is_absolute()
            || path.as_os_str().len() > 4096
            || path.components().count() > 72
            || path
                .to_str()
                .is_none_or(|s| s.chars().any(char::is_control))
            || path
                .components()
                .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
        {
            return Err(());
        }
        let fd = unsafe { libc::open(c"/".as_ptr(), FLAGS) };
        if fd < 0 {
            return Err(());
        }
        let mut parent = unsafe { File::from_raw_fd(fd) };
        let mut parent_path = PathBuf::from("/");
        for part in path.components().filter_map(|c| {
            if let Component::Normal(v) = c {
                v.to_str()
            } else {
                None
            }
        }) {
            let name = CString::new(part).map_err(|_| ())?;
            let Some(before) = stat_at(&parent, &name)? else {
                return Ok(Opened::Missing {
                    parent: parent_path,
                    name,
                    object: stat_fd(&parent)?,
                });
            };
            if before.st_mode & libc::S_IFMT != libc::S_IFDIR || before.st_flags & SF_DATALESS != 0
            {
                return Err(());
            }
            let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), FLAGS) };
            if fd < 0 {
                return Err(());
            }
            let child = unsafe { File::from_raw_fd(fd) };
            let after = stat_fd(&child)?;
            if object(&before) != object(&after)
                || after.st_flags & SF_DATALESS != 0
                || !local(&child)
            {
                return Err(());
            }
            parent = child;
            parent_path.push(part);
        }
        Ok(Opened::Directory(parent))
    }
    enum Evidence {
        Directory(PathBuf, libc::stat),
        Missing(PathBuf, CString, libc::stat),
    }
    struct Stream(*mut libc::DIR);
    impl Drop for Stream {
        fn drop(&mut self) {
            unsafe {
                libc::closedir(self.0);
            }
        }
    }
    pub struct Check {
        pub paths: Vec<Option<String>>,
        observation: Observation,
        evidence: Vec<Evidence>,
        seen: HashSet<String>,
        started: Instant,
        mounts: Option<(PathBuf, Vec<Mount>)>,
    }
    pub fn roots(home: &Path) -> Vec<PathBuf> {
        vec![
            "/Applications".into(),
            home.join("Applications"),
            "/System/Applications".into(),
            "/Library/Input Methods".into(),
            home.join("Library/Input Methods"),
            home.join("Library/Application Support/Setapp/Applications"),
            home.join("Library/Containers/io.playcover.PlayCover/Applications"),
            "/opt/homebrew/Caskroom".into(),
            "/usr/local/Caskroom".into(),
        ]
    }
    impl Check {
        #[cfg(test)]
        pub(crate) fn for_test(roots: &[PathBuf]) -> Self {
            Self::scan(roots)
        }
        pub fn begin(home: &Path) -> Self {
            Self::scan(&roots(home))
        }
        fn scan(roots: &[PathBuf]) -> Self {
            let mut result = Self {
                paths: vec![],
                observation: Observation {
                    code: "observed",
                    issue: None,
                    root_count: 0,
                    entries: 0,
                    candidates: 0,
                    first_uncertain_path: None,
                },
                evidence: vec![],
                seen: HashSet::new(),
                started: Instant::now(),
                mounts: None,
            };
            for root in roots {
                if result.exhausted(root) {
                    break;
                }
                result.observation.root_count += 1;
                match open(root) {
                    Ok(Opened::Directory(file)) => result.walk(root, &file, 0),
                    Ok(Opened::Missing {
                        parent,
                        name,
                        object,
                    }) => result
                        .evidence
                        .push(Evidence::Missing(parent, name, object)),
                    Err(()) => result.partial("unavailable", root),
                }
            }
            result.observation.candidates = result.paths.len();
            result
        }
        pub fn begin_volumes() -> Self {
            Self::volumes(Path::new("/Volumes"), mount_table(Path::new("/Volumes")))
        }
        fn volumes(root: &Path, mounts: Result<Vec<Mount>, ()>) -> Self {
            let mut result = Self::scan(&[]);
            let Ok(mounts) = mounts else {
                result.partial("unavailable", root);
                return result;
            };
            result.mounts = Some((root.into(), mounts));
            let Ok(Opened::Directory(file)) = result.open(root) else {
                result.partial("unavailable", root);
                return result;
            };
            // Stages: /Volumes entries, each volume's immediate children, then
            // only Applications (three levels) or an immediate independent App.
            result.volume_level(root, &file, true);
            result.observation.candidates = result.paths.len();
            result
        }
        fn open(&self, path: &Path) -> Result<Opened, ()> {
            if self
                .mounts
                .as_ref()
                .is_some_and(|(_, mounts)| !local_mount(path, mounts))
            {
                return Err(());
            }
            open(path)
        }
        fn volume_level(&mut self, path: &Path, file: &File, container: bool) {
            if self.exhausted(path) {
                return;
            }
            let Ok(before) = stat_fd(file) else {
                self.partial("unavailable", path);
                return;
            };
            self.evidence.push(Evidence::Directory(path.into(), before));
            let Ok(clone) = file.try_clone() else {
                self.partial("unavailable", path);
                return;
            };
            let fd = clone.into_raw_fd();
            let pointer = unsafe { libc::fdopendir(fd) };
            if pointer.is_null() {
                unsafe {
                    libc::close(fd);
                }
                self.partial("unavailable", path);
                return;
            }
            let stream = Stream(pointer);
            loop {
                if self.exhausted(path) {
                    break;
                }
                unsafe {
                    *libc::__error() = 0;
                }
                let entry = unsafe { libc::readdir(stream.0) };
                if entry.is_null() {
                    if unsafe { *libc::__error() } != 0 {
                        self.partial("unavailable", path);
                    }
                    break;
                }
                let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
                if matches!(name.to_bytes(), b"." | b"..") {
                    continue;
                }
                self.observation.entries += 1;
                let Ok(text) = name.to_str() else {
                    self.partial("unavailable", path);
                    continue;
                };
                if text.chars().any(char::is_control) {
                    self.partial("unavailable", path);
                    continue;
                }
                let child = path.join(text);
                if self
                    .mounts
                    .as_ref()
                    .is_some_and(|(_, mounts)| !local_mount(&child, mounts))
                {
                    self.partial("unsupportedVolume", &child);
                    continue;
                }
                let Ok(Some(stat)) = stat_at(file, name) else {
                    self.partial("unavailable", &child);
                    continue;
                };
                let kind = stat.st_mode & libc::S_IFMT;
                if container {
                    if self.observation.root_count >= 64 {
                        self.partial("limitReached", &child);
                        break;
                    }
                    self.observation.root_count += 1;
                    // This is the exact verified internal namespace exception in Mole.
                    if internal_snapshot(text, &before, &stat) {
                        match self.open(&child) {
                            Ok(Opened::Directory(dir))
                                if stat_fd(&dir).is_ok_and(|s| stamp(&s) == stamp(&stat)) =>
                            {
                                self.evidence.push(Evidence::Directory(child, stat));
                            }
                            _ => self.partial("unavailable", &child),
                        }
                        continue;
                    }
                    if kind != libc::S_IFDIR && kind != libc::S_IFLNK {
                        continue;
                    }
                    match self.open(&child) {
                        Ok(Opened::Directory(dir)) => self.volume_level(&child, &dir, false),
                        _ => self.partial("unavailable", &child),
                    }
                } else if text.to_ascii_lowercase().ends_with(".app")
                    && (kind == libc::S_IFDIR || kind == libc::S_IFLNK)
                {
                    self.candidate(&child);
                } else if text == "Applications" && kind == libc::S_IFDIR {
                    match self.open(&child) {
                        Ok(Opened::Directory(dir)) => self.walk(&child, &dir, 0),
                        _ => self.partial("unavailable", &child),
                    }
                }
            }
            if stat_fd(file).map_or(true, |s| stamp(&s) != stamp(&before)) {
                self.partial("changed", path);
            }
        }
        fn candidate(&mut self, path: &Path) {
            let Some(value) = path.to_str().filter(|s| s.len() <= 4096) else {
                self.partial("unavailable", path);
                return;
            };
            if self.seen.insert(value.to_owned()) {
                self.paths.push(Some(value.to_owned()));
            }
        }
        fn verify_mounts(&mut self, current: Result<Vec<Mount>, ()>) {
            if let Some((scope, expected)) = &self.mounts {
                let scope = scope.clone();
                match current {
                    Ok(current) if &current == expected => {}
                    Ok(_) => self.partial("changed", &scope),
                    Err(()) => self.partial("unavailable", &scope),
                }
            }
        }
        fn partial(&mut self, issue: &'static str, path: &Path) {
            self.observation.code = "partial";
            if self.observation.issue.is_none() || issue == "changed" {
                self.observation.issue = Some(issue);
                self.observation.first_uncertain_path = path
                    .to_str()
                    .filter(|s| s.len() <= 4096 && !s.chars().any(char::is_control))
                    .map(str::to_owned);
            }
        }
        fn exhausted(&mut self, path: &Path) -> bool {
            if self.observation.entries >= 2048
                || self.paths.len() >= 128
                || self.evidence.len() >= 128
                || self.started.elapsed() > Duration::from_millis(1000)
            {
                self.partial("limitReached", path);
                true
            } else {
                false
            }
        }
        fn walk(&mut self, path: &Path, file: &File, depth: usize) {
            if self.exhausted(path) {
                return;
            }
            let Ok(before) = stat_fd(file) else {
                self.partial("unavailable", path);
                return;
            };
            self.evidence.push(Evidence::Directory(path.into(), before));
            let Ok(cloned) = file.try_clone() else {
                self.partial("unavailable", path);
                return;
            };
            let fd = cloned.into_raw_fd();
            let pointer = unsafe { libc::fdopendir(fd) };
            if pointer.is_null() {
                unsafe {
                    libc::close(fd);
                };
                self.partial("unavailable", path);
                return;
            }
            let stream = Stream(pointer);
            loop {
                if self.exhausted(path) {
                    return;
                }
                unsafe {
                    *libc::__error() = 0;
                }
                let entry = unsafe { libc::readdir(stream.0) };
                if entry.is_null() {
                    if unsafe { *libc::__error() } != 0 {
                        self.partial("unavailable", path);
                    }
                    break;
                }
                let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
                if name.to_bytes() == b"." || name.to_bytes() == b".." {
                    continue;
                }
                self.observation.entries += 1;
                let Ok(text) = name.to_str() else {
                    self.partial("unavailable", path);
                    continue;
                };
                if text.chars().any(char::is_control) {
                    self.partial("unavailable", path);
                    continue;
                }
                let child_path = path.join(text);
                if self
                    .mounts
                    .as_ref()
                    .is_some_and(|(_, mounts)| !local_mount(&child_path, mounts))
                {
                    self.partial("unsupportedVolume", &child_path);
                    continue;
                }
                let Ok(Some(stat)) = stat_at(file, name) else {
                    self.partial("unavailable", &child_path);
                    continue;
                };
                let kind = stat.st_mode & libc::S_IFMT;
                if text.to_ascii_lowercase().ends_with(".app")
                    && (kind == libc::S_IFDIR || kind == libc::S_IFLNK)
                {
                    self.candidate(&child_path);
                } else if kind == libc::S_IFDIR && depth + 1 < 3 && !text.starts_with('.') {
                    match self.open(&child_path) {
                        Ok(Opened::Directory(child)) => self.walk(&child_path, &child, depth + 1),
                        _ => self.partial("unavailable", &child_path),
                    }
                }
            }
            if stat_fd(file).map_or(true, |s| stamp(&s) != stamp(&before)) {
                self.partial("changed", path);
            }
        }
        pub fn finish(mut self) -> Observation {
            if let Some((scope, _)) = &self.mounts {
                self.verify_mounts(mount_table(scope));
            }
            let started = Instant::now();
            for item in std::mem::take(&mut self.evidence) {
                if started.elapsed() > Duration::from_millis(1000) {
                    let scope = self
                        .mounts
                        .as_ref()
                        .map(|(scope, _)| scope.clone())
                        .unwrap_or_else(|| PathBuf::from("/"));
                    self.partial("limitReached", &scope);
                    break;
                }
                let (path, valid) = match item {
                    Evidence::Directory(path, expected) => {
                        let valid = match self.open(&path) {
                            Ok(Opened::Directory(file)) => {
                                stat_fd(&file).is_ok_and(|s| stamp(&s) == stamp(&expected))
                            }
                            _ => false,
                        };
                        (path, valid)
                    }
                    Evidence::Missing(parent, name, expected) => {
                        let valid = match self.open(&parent) {
                            Ok(Opened::Directory(file)) => {
                                stat_fd(&file).is_ok_and(|s| object(&s) == object(&expected))
                                    && matches!(stat_at(&file, &name), Ok(None))
                            }
                            _ => false,
                        };
                        (parent.join(name.to_str().unwrap_or("")), valid)
                    }
                };
                if !valid {
                    self.partial("changed", &path);
                }
            }
            if let Some((scope, _)) = &self.mounts {
                self.verify_mounts(mount_table(scope));
            }
            self.observation
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{fs, os::unix::fs::symlink};
        #[test]
        fn snapshot_exception_requires_exact_namespace_root_ownership_and_same_device() {
            let mut parent: libc::stat = unsafe { std::mem::zeroed() };
            let mut child: libc::stat = unsafe { std::mem::zeroed() };
            parent.st_dev = 1;
            child.st_dev = 1;
            child.st_mode = libc::S_IFDIR;
            let name = "com.apple.TimeMachine.localsnapshots";
            assert!(internal_snapshot(name, &parent, &child));
            assert!(!internal_snapshot("other", &parent, &child));
            child.st_dev = 2;
            assert!(!internal_snapshot(name, &parent, &child));
            child.st_dev = 1;
            child.st_uid = 501;
            assert!(!internal_snapshot(name, &parent, &child));
            child.st_uid = 0;
            parent.st_uid = 501;
            assert!(!internal_snapshot(name, &parent, &child));
            parent.st_uid = 0;
            child.st_mode = libc::S_IFLNK;
            assert!(!internal_snapshot(name, &parent, &child));
        }
        #[test]
        fn volume_scope_finds_only_direct_apps_and_applications_children() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            for relative in [
                "Disk/Top.app/Contents/Helper.app",
                "Disk/Applications/Vendor/v1/Deep.app",
                "Disk/Applications/Vendor/v1/deeper/TooDeep.app",
                "Disk/Elsewhere/Missed.app",
                ".HiddenDisk/.Hidden.app",
                "Disk/Applications/.hidden/Ignore.app",
                "Disk/Applications/.Visible.APP",
            ] {
                fs::create_dir_all(root.join(relative)).unwrap();
            }
            let check = Check::volumes(&root, mount_table(&root));
            let mut names: Vec<_> = check
                .paths
                .iter()
                .flatten()
                .map(|p| Path::new(p).file_name().unwrap().to_str().unwrap())
                .collect();
            names.sort();
            assert_eq!(
                names,
                [".Hidden.app", ".Visible.APP", "Deep.app", "Top.app"]
            );
            assert_eq!(check.observation.root_count, 2);
            assert_eq!(check.finish().code, "observed");
        }
        #[test]
        fn volumes_reject_network_autofs_unknown_tables_and_linked_roots() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            fs::create_dir_all(root.join("Network/Applications/Blocked.app")).unwrap();
            let mut mounts = mount_table(&root).unwrap();
            let mut network = mounts[0].clone();
            network.path = root.join("Network");
            network.flags = 0;
            network.kind = "smbfs".into();
            mounts.push(network.clone());
            let mut check = Check::volumes(&root, Ok(mounts.clone()));
            assert!(check.paths.is_empty());
            assert_eq!(check.observation.issue, Some("unsupportedVolume"));
            check.verify_mounts(Ok(mounts.clone()));
            network.flags = libc::MNT_LOCAL as u32;
            network.kind = "autofs".into();
            assert!(!local_mount(&root.join("Network/Applications"), &[network]));
            let unknown = Check::volumes(&root, Err(()));
            assert_eq!(unknown.observation.issue, Some("unavailable"));
            assert!(unknown.paths.is_empty());
            let alias = root.join("Alias");
            symlink(root.join("Network"), &alias).unwrap();
            let check = Check::volumes(&root, mount_table(&root));
            assert_eq!(check.finish().code, "partial");
        }
        #[test]
        fn hidden_local_mounts_are_rejected_before_discovery_and_flag_changes_remain_unknown() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            fs::create_dir_all(root.join("Hidden/Applications/SystemOnly.app")).unwrap();
            fs::create_dir_all(root.join("User/Applications/Visible.app")).unwrap();
            let mut mounts = mount_table(&root).unwrap();
            let mut hidden = mounts[0].clone();
            hidden.path = root.join("Hidden");
            hidden.flags |= libc::MNT_LOCAL as u32 | libc::MNT_DONTBROWSE as u32;
            hidden.kind = "apfs".into();
            mounts.push(hidden);
            assert!(!local_mount(&root.join("Hidden/Applications"), &mounts));
            let mut check = Check::volumes(&root, Ok(mounts.clone()));
            assert_eq!(
                check.paths,
                vec![Some(
                    root.join("User/Applications/Visible.app")
                        .to_str()
                        .unwrap()
                        .into()
                )]
            );
            assert_eq!(check.observation.root_count, 1);
            assert_eq!(check.observation.issue, Some("unsupportedVolume"));
            assert_eq!(
                check.observation.first_uncertain_path.as_deref(),
                root.join("Hidden").to_str()
            );
            check.verify_mounts(Ok(mounts.clone()));
            assert_eq!(check.observation.issue, Some("unsupportedVolume"));
            mounts.last_mut().unwrap().flags &= !(libc::MNT_DONTBROWSE as u32);
            check.verify_mounts(Ok(mounts));
            assert_eq!(check.observation.issue, Some("changed"));
        }
        #[test]
        fn volume_mount_changes_replacements_and_budgets_remain_partial() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            fs::create_dir_all(root.join("Disk/Applications/One.app")).unwrap();
            let mut check = Check::volumes(&root, mount_table(&root));
            let mut changed = mount_table(&root).unwrap();
            changed[0].fsid.push(1);
            check.verify_mounts(Ok(changed));
            assert_eq!(check.observation.issue, Some("changed"));
            let check = Check::volumes(&root, mount_table(&root));
            fs::rename(root.join("Disk/Applications"), root.join("Saved")).unwrap();
            symlink(root.join("Saved"), root.join("Disk/Applications")).unwrap();
            assert_eq!(check.finish().issue, Some("changed"));
            fs::remove_file(root.join("Disk/Applications")).unwrap();
            for i in 0..65 {
                fs::create_dir(root.join(format!("Disk{i}"))).unwrap();
            }
            let check = Check::volumes(&root, mount_table(&root));
            assert_eq!(check.observation.root_count, 64);
            assert_eq!(check.finish().issue, Some("limitReached"));
        }
        #[test]
        #[ignore = "requires pinned Mole source; compares volume selection and nested app scope with original find"]
        fn prepared_volume_scope_matches_mole_find() {
            use std::process::Command;
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
            crate::app_uninstall_rules::load(&source).unwrap();
            let batch = fs::read_to_string(source.join("lib/uninstall/batch.sh")).unwrap();
            let offset = batch
                .find("uninstall_live_bundle_has_other_install() {")
                .unwrap();
            let start = offset + batch[offset..].find("-mindepth 1 -maxdepth 1").unwrap();
            let expression =
                &batch[start..start + batch[start..].find(" || volume_scan_rc=$?").unwrap()];
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            for path in [
                "Disk/Top.app/Contents/Helper.app",
                "Disk/Applications/Vendor/Deep.app",
                "Disk/Elsewhere/No.app",
                ".Hidden/.App.APP",
            ] {
                fs::create_dir_all(root.join(path)).unwrap();
            }
            symlink(root.join("Disk/Top.app"), root.join("Disk/Link.app")).unwrap();
            let script = format!(
                r#"set -euo pipefail
{}
shopt -s dotglob nullglob
for volume in "$1"/*; do
  while IFS= read -r -d '' scope; do
    find "$scope" -maxdepth 3 \( -type d -name '.*' ! -iname '*.app' ! -path "$scope" -prune \) -o \( \( -type d -o -type l \) -iname '*.app' -print0 \) | while IFS= read -r -d '' app; do
      if _uninstall_live_candidate_is_nested_app "$scope" "$app"; then continue; fi
      printf '%s\0' "$app"
    done
  done < <(find "$volume" {} -print0)
done"#,
                crate::app_background::test_function(
                    &batch,
                    "_uninstall_live_candidate_is_nested_app"
                ),
                expression
            );
            let output = Command::new("/bin/bash")
                .args(["-c", &script, "volume-scope"])
                .arg(&root)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("LC_ALL", "C")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let mut expected: Vec<_> = output
                .stdout
                .split(|b| *b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8(s.to_vec()).unwrap())
                .collect();
            // Upstream's relative predicate admits a helper when its starting
            // root is itself an App. Our supported scope is independent Apps;
            // the package inspector also rejects every nested App path.
            assert!(expected
                .iter()
                .any(|p| p.ends_with("Top.app/Contents/Helper.app")));
            expected.retain(|p| {
                !Path::new(p).ancestors().skip(1).any(|parent| {
                    parent
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("app"))
                })
            });
            let mut actual: Vec<_> = Check::volumes(&root, mount_table(&root))
                .paths
                .into_iter()
                .flatten()
                .collect();
            expected.sort();
            actual.sort();
            assert_eq!(actual, expected);
        }
        #[test]
        fn bounded_depth_hidden_and_nested_helpers_follow_mole_scope() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            for relative in [
                "Top.app/Contents/Helper.app",
                "Vendor/v1/Deep.app",
                "Vendor/v1/deeper/TooDeep.app",
                ".hidden/Hidden.app",
                ".Visible.app",
            ] {
                fs::create_dir_all(root.join(relative)).unwrap();
            }
            let check = Check::scan(std::slice::from_ref(&root));
            let paths = check.paths.clone();
            assert_eq!(paths.len(), 3);
            assert!(paths.iter().flatten().any(|p| p.ends_with("Deep.app")));
            assert!(!paths
                .iter()
                .flatten()
                .any(|p| p.ends_with("Helper.app") || p.ends_with("TooDeep.app")));
            assert_eq!(check.finish().code, "observed");
        }
        #[test]
        fn linked_roots_changes_missing_and_limits_do_not_prove_absence() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            let alias = root.join("Alias");
            symlink(&root, &alias).unwrap();
            assert_eq!(Check::scan(&[alias]).finish().issue, Some("unavailable"));
            let absent = root.join("Missing");
            let check = Check::scan(std::slice::from_ref(&absent));
            fs::create_dir(&absent).unwrap();
            assert_eq!(check.finish().issue, Some("changed"));
            let check = Check::scan(std::slice::from_ref(&root));
            fs::create_dir(root.join("Changed")).unwrap();
            assert_eq!(check.finish().issue, Some("changed"));
            for i in 0..129 {
                fs::create_dir(root.join(format!("App{i}.app"))).unwrap();
            }
            let check = Check::scan(std::slice::from_ref(&root));
            assert_eq!(check.paths.len(), 128);
            assert_eq!(check.finish().code, "partial");
        }
        #[test]
        fn unreadable_and_excessive_entries_remain_partial() {
            use std::os::unix::fs::PermissionsExt;
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            if unsafe { libc::geteuid() } != 0 {
                fs::set_permissions(&root, fs::Permissions::from_mode(0o000)).unwrap();
                let observation = Check::scan(std::slice::from_ref(&root)).finish();
                fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
                assert_eq!(observation.issue, Some("unavailable"));
            }
            for i in 0..2049 {
                fs::write(root.join(format!("entry{i}")), "").unwrap();
            }
            let observation = Check::scan(std::slice::from_ref(&root)).finish();
            assert_eq!(observation.entries, 2048);
            assert_eq!(observation.issue, Some("limitReached"));
            let mut check = Check::scan(&[]);
            check.started = Instant::now() - Duration::from_secs(2);
            assert!(check.exhausted(&root));
            assert_eq!(check.finish().issue, Some("limitReached"));
        }
        #[test]
        #[ignore = "requires pinned Mole source; compares bounded discovery with original find and nesting predicate"]
        fn prepared_root_scope_matches_mole_find_without_following_links() {
            use std::process::Command;
            let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
            crate::app_uninstall_rules::load(&source_root).unwrap();
            let batch = fs::read_to_string(source_root.join("lib/uninstall/batch.sh")).unwrap();
            for line in [
                "\"/Applications\"",
                "\"$HOME/Applications\"",
                "\"/System/Applications\"",
                "\"/Library/Input Methods\"",
                "\"$HOME/Library/Input Methods\"",
                "\"$HOME/Library/Application Support/Setapp/Applications\"",
                "\"$HOME/Library/Containers/io.playcover.PlayCover/Applications\"",
                "\"/opt/homebrew/Caskroom\"",
                "\"/usr/local/Caskroom\"",
            ] {
                assert!(batch.contains(&format!("    {line}\n")));
            }
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            for path in [
                "Top.app/Contents/Helper.app",
                "Vendor/v1/Deep.app",
                "Vendor/v1/deeper/TooDeep.app",
                ".hidden/Ignore.app",
                ".Visible.APP",
            ] {
                fs::create_dir_all(root.join(path)).unwrap();
            }
            symlink(root.join("Top.app"), root.join("Link.app")).unwrap();
            let script = format!(
                "set -euo pipefail\n{}\n{}",
                crate::app_background::test_function(
                    &batch,
                    "_uninstall_live_candidate_is_nested_app"
                ),
                r#"find "$1" -maxdepth 3 \( -type d -name '.*' ! -iname '*.app' ! -path "$1" -prune \) -o \( \( -type d -o -type l \) -iname '*.app' -print0 \) | while IFS= read -r -d '' app; do if _uninstall_live_candidate_is_nested_app "$1" "$app"; then continue; fi; printf '%s\0' "$app"; done"#
            );
            let output = Command::new("/bin/bash")
                .arg("-c")
                .arg(script)
                .arg("root-scope-test")
                .arg(&root)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("LC_ALL", "C")
                .env("HOME", &root)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let mut expected: Vec<_> = output
                .stdout
                .split(|b| *b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8(s.to_vec()).unwrap())
                .collect();
            expected.sort();
            let mut actual: Vec<_> = Check::scan(&[root]).paths.into_iter().flatten().collect();
            actual.sort();
            assert_eq!(actual, expected);
        }
        #[test]
        fn configured_roots_are_fixed_and_home_scoped() {
            let roots = roots(Path::new("/Users/fixture"));
            assert_eq!(roots.len(), 9);
            assert!(roots.contains(&PathBuf::from("/opt/homebrew/Caskroom")));
            assert!(roots.contains(&PathBuf::from("/Users/fixture/Applications")));
        }
    }
}
