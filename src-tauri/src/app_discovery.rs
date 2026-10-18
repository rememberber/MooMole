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
    fn object(s: &libc::stat) -> (i32, u64, u16, u32) {
        (s.st_dev, s.st_ino, s.st_mode, s.st_flags)
    }
    fn stamp(s: &libc::stat) -> (i32, u64, u16, u32, i64, i64, i64, i64, i64) {
        (
            s.st_dev,
            s.st_ino,
            s.st_mode,
            s.st_flags,
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
                let Ok(Some(stat)) = stat_at(file, name) else {
                    self.partial("unavailable", &child_path);
                    continue;
                };
                let kind = stat.st_mode & libc::S_IFMT;
                if text.to_ascii_lowercase().ends_with(".app")
                    && (kind == libc::S_IFDIR || kind == libc::S_IFLNK)
                {
                    if child_path.as_os_str().len() > 4096 {
                        self.partial("unavailable", path);
                        continue;
                    }
                    let value = child_path.to_str().unwrap().to_owned();
                    if self.seen.insert(value.clone()) {
                        self.paths.push(Some(value));
                    }
                } else if kind == libc::S_IFDIR && depth + 1 < 3 && !text.starts_with('.') {
                    match open(&child_path) {
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
            let started = Instant::now();
            for item in std::mem::take(&mut self.evidence) {
                if started.elapsed() > Duration::from_millis(1000) {
                    self.partial("limitReached", Path::new("/"));
                    break;
                }
                let (path, valid) = match item {
                    Evidence::Directory(path, expected) => {
                        let valid = match open(&path) {
                            Ok(Opened::Directory(file)) => {
                                stat_fd(&file).is_ok_and(|s| stamp(&s) == stamp(&expected))
                            }
                            _ => false,
                        };
                        (path, valid)
                    }
                    Evidence::Missing(parent, name, expected) => {
                        let valid = match open(&parent) {
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
            self.observation
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{fs, os::unix::fs::symlink};
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
