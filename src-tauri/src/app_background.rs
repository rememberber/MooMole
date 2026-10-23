//! Read-only adaptation of Mole's background-only / direct search-root condition.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Marker {
    Absent,
    Enabled,
    Disabled,
    Unknown,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    version: u32,
    revision: &'static str,
    pub code: &'static str,
    marker: Marker,
    root_kind: Option<&'static str>,
    reason: Option<&'static str>,
    issue: Option<&'static str>,
}
impl Observation {
    pub fn unknown(marker: Marker, issue: &'static str) -> Self {
        Self {
            version: 1,
            revision: crate::engine::REVISION,
            code: "unknown",
            marker,
            root_kind: None,
            reason: None,
            issue: Some(issue),
        }
    }
    fn known(
        marker: Marker,
        code: &'static str,
        root_kind: Option<&'static str>,
        reason: Option<&'static str>,
    ) -> Self {
        Self {
            version: 1,
            revision: crate::engine::REVISION,
            code,
            marker,
            root_kind,
            reason,
            issue: None,
        }
    }
}

#[cfg(target_os = "macos")]
pub mod mac {
    use super::*;
    use std::{
        fs::{self, File, Metadata, OpenOptions},
        os::{
            fd::AsRawFd,
            macos::fs::MetadataExt as DarwinMetadataExt,
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
        path::{Component, Path, PathBuf},
    };

    pub struct Scope {
        applications: PathBuf,
        home: PathBuf,
        input_methods: PathBuf,
        volumes: PathBuf,
    }
    impl Scope {
        pub fn new(home: &Path) -> Self {
            Self {
                applications: "/Applications".into(),
                home: home.into(),
                input_methods: "/Library/Input Methods".into(),
                volumes: "/Volumes".into(),
            }
        }
        fn fixed(&self) -> [(PathBuf, &'static str); 5] {
            [
                (self.applications.clone(), "systemApplications"),
                (self.home.join("Applications"), "userApplications"),
                (self.input_methods.clone(), "systemInputMethods"),
                (self.home.join("Library/Input Methods"), "userInputMethods"),
                (
                    self.home
                        .join("Library/Containers/io.playcover.PlayCover/Applications"),
                    "playCover",
                ),
            ]
        }
    }
    fn stamp(m: &Metadata) -> (u64, u64, i64, i64, i64, i64) {
        (
            m.dev(),
            m.ino(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )
    }
    fn absent(error: &std::io::Error) -> bool {
        matches!(error.raw_os_error(), Some(libc::ENOENT | libc::ENOTDIR))
    }
    fn local(file: &File, meta: &Metadata) -> bool {
        let mut fs = std::mem::MaybeUninit::<libc::statfs>::zeroed();
        meta.is_dir()
            && meta.st_flags() & 0x40000000 == 0
            && unsafe { libc::fstatfs(file.as_raw_fd(), fs.as_mut_ptr()) } == 0
            && unsafe { fs.assume_init() }.f_flags & libc::MNT_LOCAL as u32 != 0
    }
    struct Directory {
        original: PathBuf,
        resolved: PathBuf,
        file: File,
        meta: Metadata,
    }
    impl Directory {
        fn capture(path: &Path) -> Result<Self, ()> {
            let resolved = path.canonicalize().map_err(|_| ())?;
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(
                    libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
                .open(&resolved)
                .map_err(|_| ())?;
            let meta = file.metadata().map_err(|_| ())?;
            if !local(&file, &meta) {
                return Err(());
            }
            Ok(Self {
                original: path.into(),
                resolved,
                file,
                meta,
            })
        }
        fn same(&self, other: &Self) -> bool {
            (self.meta.dev(), self.meta.ino()) == (other.meta.dev(), other.meta.ino())
        }
        fn verify(&self) -> bool {
            self.original
                .canonicalize()
                .is_ok_and(|p| p == self.resolved)
                && fs::symlink_metadata(&self.resolved)
                    .is_ok_and(|m| m.is_dir() && stamp(&m) == stamp(&self.meta))
                && self
                    .file
                    .metadata()
                    .is_ok_and(|m| stamp(&m) == stamp(&self.meta) && local(&self.file, &m))
        }
        fn readable(&self) -> Result<bool, ()> {
            let result = unsafe {
                libc::faccessat(
                    self.file.as_raw_fd(),
                    c".".as_ptr(),
                    libc::R_OK,
                    libc::AT_EACCESS,
                )
            };
            if result == 0 {
                Ok(true)
            } else if matches!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::EACCES | libc::EPERM)
            ) {
                Ok(false)
            } else {
                Err(())
            }
        }
    }
    enum Dependency {
        Directory(Directory),
        NotDirectory(PathBuf),
    }
    impl Dependency {
        fn capture(path: &Path) -> Result<Self, ()> {
            match fs::metadata(path) {
                Ok(m) if m.is_dir() => Directory::capture(path).map(Self::Directory),
                Ok(_) => Ok(Self::NotDirectory(path.into())),
                Err(e) if absent(&e) => Ok(Self::NotDirectory(path.into())),
                Err(_) => Err(()),
            }
        }
        fn verify(&self) -> bool {
            match self {
                Self::Directory(dir) => dir.verify(),
                Self::NotDirectory(path) => match fs::metadata(path) {
                    Ok(m) => !m.is_dir(),
                    Err(e) => absent(&e),
                },
            }
        }
    }
    pub struct Check {
        result: Observation,
        parent: Option<Directory>,
        dependencies: Vec<Dependency>,
        readable: Option<bool>,
    }
    impl Check {
        pub fn begin(
            marker: Marker,
            identity: &str,
            path: &Path,
            scope: &Scope,
            rules_ready: bool,
        ) -> Self {
            let mut check = Self {
                result: Observation::unknown(marker, "rootsUnavailable"),
                parent: None,
                dependencies: vec![],
                readable: None,
            };
            let issue = match identity {
                "different" => Some("identityChanged"),
                "unavailable" => Some("identityUnavailable"),
                _ if !rules_ready => Some("rulesUnavailable"),
                _ if marker == Marker::Unknown => Some("markerUnknown"),
                _ => None,
            };
            if let Some(issue) = issue {
                check.result = Observation::unknown(marker, issue);
                return check;
            }
            if marker != Marker::Enabled {
                check.result = Observation::known(marker, "notRequired", None, None);
                return check;
            }
            let Some(parent) = path.parent() else {
                return check;
            };
            if !scope.home.is_absolute()
                || scope
                    .home
                    .to_str()
                    .is_none_or(|v| v.len() > 4096 || v.chars().any(char::is_control))
                || scope
                    .home
                    .components()
                    .any(|c| matches!(c, Component::ParentDir))
            {
                return check;
            }
            let Ok(directory) = Directory::capture(parent) else {
                return check;
            };
            check.parent = Some(directory);
            for (root, kind) in scope.fixed() {
                if parent == root {
                    check.result = Observation::known(marker, "directRoot", Some(kind), None);
                    return check;
                }
            }
            let external = parent.file_name().is_some_and(|v| v == "Applications")
                && parent.parent().and_then(Path::parent) == Some(scope.volumes.as_path())
                && parent
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|v| !v.to_string_lossy().starts_with('.'));
            if !external {
                check.result = Observation::known(marker, "restricted", None, Some("outsideRoots"));
                return check;
            }
            let directory = check.parent.as_ref().unwrap();
            let Ok(readable) = directory.readable() else {
                return check;
            };
            check.readable = Some(readable);
            if !readable {
                check.result =
                    Observation::known(marker, "restricted", None, Some("unreadableRoot"));
                return check;
            }
            for root in [&scope.applications, &scope.home.join("Applications")] {
                let Ok(dependency) = Dependency::capture(root) else {
                    return check;
                };
                let same =
                    matches!(&dependency, Dependency::Directory(other) if directory.same(other));
                check.dependencies.push(dependency);
                if same {
                    check.result =
                        Observation::known(marker, "restricted", None, Some("duplicateRoot"));
                    return check;
                }
            }
            check.result =
                Observation::known(marker, "directRoot", Some("externalApplications"), None);
            check
        }
        pub fn finish(mut self) -> Observation {
            if !self.parent.as_ref().is_none_or(Directory::verify)
                || !self.dependencies.iter().all(Dependency::verify)
                || self.readable.is_some_and(|value| {
                    self.parent
                        .as_ref()
                        .is_none_or(|dir| dir.readable() != Ok(value))
                })
            {
                self.result = Observation::unknown(self.result.marker, "rootsChanged");
            }
            self.result
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        fn fixture() -> (tempfile::TempDir, Scope) {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            let scope = Scope {
                applications: root.join("Applications"),
                home: root.join("Home"),
                input_methods: root.join("Input Methods"),
                volumes: root.join("Volumes"),
            };
            for (path, _) in scope.fixed() {
                fs::create_dir_all(path).unwrap();
            }
            fs::create_dir_all(&scope.volumes).unwrap();
            (tmp, scope)
        }
        fn app(path: PathBuf) -> PathBuf {
            fs::create_dir_all(&path).unwrap();
            path
        }
        fn check(path: &Path, scope: &Scope) -> Check {
            Check::begin(Marker::Enabled, "matches", path, scope, true)
        }
        #[test]
        fn fixed_playcover_and_external_roots_are_exact_parents_not_prefixes_or_nested_paths() {
            let (_tmp, scope) = fixture();
            for (path, kind) in [
                (scope.applications.join("A.app"), "systemApplications"),
                (scope.home.join("Applications/A.app"), "userApplications"),
                (scope.input_methods.join("A.app"), "systemInputMethods"),
                (
                    scope.home.join("Library/Input Methods/A.app"),
                    "userInputMethods",
                ),
                (
                    scope
                        .home
                        .join("Library/Containers/io.playcover.PlayCover/Applications/A.app"),
                    "playCover",
                ),
                (
                    scope.volumes.join("中文 外接盘/Applications/A.app"),
                    "externalApplications",
                ),
            ] {
                let value = check(&app(path), &scope).finish();
                assert_eq!(value.code, "directRoot");
                assert_eq!(value.root_kind, Some(kind));
            }
            for path in [
                scope.applications.join("Nested/A.app"),
                scope.home.join("Applications-old/A.app"),
                scope.volumes.join(".hidden/Applications/A.app"),
                scope.volumes.join("Disk/Applications/Nested/A.app"),
            ] {
                let value = check(&app(path), &scope).finish();
                assert_eq!(value.code, "restricted");
                assert_eq!(value.reason, Some("outsideRoots"));
            }
        }
        #[test]
        fn aliases_to_either_standard_app_directory_are_excluded_from_external_roots() {
            let (_tmp, scope) = fixture();
            for (volume, target) in [
                ("Alias", &scope.applications),
                ("HomeAlias", &scope.home.join("Applications")),
            ] {
                let volume = scope.volumes.join(volume);
                fs::create_dir(&volume).unwrap();
                std::os::unix::fs::symlink(target, volume.join("Applications")).unwrap();
                let value = check(&app(volume.join("Applications/A.app")), &scope).finish();
                assert_eq!(value.code, "restricted");
                assert_eq!(value.reason, Some("duplicateRoot"));
            }
        }
        #[test]
        fn parent_standard_root_and_previously_missing_root_changes_withdraw_the_observation() {
            for change in ["parent", "standard", "missing"] {
                let (_tmp, scope) = fixture();
                let path = app(scope.volumes.join("Disk/Applications/A.app"));
                let user_apps = scope.home.join("Applications");
                if change == "missing" {
                    fs::remove_dir(&user_apps).unwrap();
                }
                let pending = check(&path, &scope);
                if change == "parent" {
                    let parent = path.parent().unwrap();
                    fs::rename(parent, parent.with_file_name("Saved")).unwrap();
                    fs::create_dir(parent).unwrap();
                } else if change == "standard" {
                    fs::rename(
                        &scope.applications,
                        scope.applications.with_file_name("Saved"),
                    )
                    .unwrap();
                    fs::create_dir(&scope.applications).unwrap();
                } else {
                    fs::create_dir(user_apps).unwrap();
                }
                let value = pending.finish();
                assert_eq!(value.code, "unknown");
                assert_eq!(value.issue, Some("rootsChanged"));
                assert!(value.root_kind.is_none());
            }
        }
        #[test]
        fn missing_metadata_rules_or_roots_remain_unknown_and_nonbackground_skips_filesystem_queries(
        ) {
            let (_tmp, scope) = fixture();
            let nonexistent = scope.home.join("Missing/A.app");
            for marker in [Marker::Absent, Marker::Disabled] {
                let pending = Check::begin(marker, "matches", &nonexistent, &scope, true);
                assert!(pending.parent.is_none());
                assert_eq!(pending.finish().code, "notRequired");
            }
            for (marker, identity, ready, issue) in [
                (Marker::Enabled, "different", true, "identityChanged"),
                (Marker::Enabled, "unavailable", true, "identityUnavailable"),
                (Marker::Unknown, "matches", true, "markerUnknown"),
                (Marker::Absent, "matches", false, "rulesUnavailable"),
                (Marker::Enabled, "matches", true, "rootsUnavailable"),
            ] {
                let value = Check::begin(marker, identity, &nonexistent, &scope, ready).finish();
                assert_eq!(value.code, "unknown");
                assert_eq!(value.issue, Some(issue));
            }
            // A looping alias is an unavailable dependency, never evidence of absence.
            fs::remove_dir(&scope.applications).unwrap();
            std::os::unix::fs::symlink(&scope.applications, &scope.applications).unwrap();
            let value = check(&app(scope.volumes.join("Disk/Applications/A.app")), &scope).finish();
            assert_eq!(value.code, "unknown");
            assert_eq!(value.issue, Some("rootsUnavailable"));
        }
        #[test]
        #[ignore = "Explicit read-only root membership comparison using pinned functions and isolated directory mappings"]
        fn prepared_search_roots_match_mole_in_isolated_directories() {
            let engine = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
            crate::app_uninstall_rules::load(&engine).unwrap();
            let source = fs::read_to_string(engine.join("bin/uninstall.sh")).unwrap();
            let helpers = fs::read_to_string(engine.join("lib/core/app_protection.sh")).unwrap();
            let (_tmp, scope) = fixture();
            let mut paths: Vec<_> = scope
                .fixed()
                .into_iter()
                .map(|(p, _)| app(p.join("A.app")))
                .collect();
            paths.extend([
                app(scope.applications.join("Nested/A.app")),
                app(scope.volumes.join("中文 Disk/Applications/A.app")),
                app(scope.volumes.join(".hidden/Applications/A.app")),
                app(scope.volumes.join("Disk/Applications/Nested/A.app")),
            ]);
            for (volume, target) in [
                ("Alias", &scope.applications),
                ("HomeAlias", &scope.home.join("Applications")),
            ] {
                let volume = scope.volumes.join(volume);
                fs::create_dir(&volume).unwrap();
                std::os::unix::fs::symlink(target, volume.join("Applications")).unwrap();
                paths.push(app(volume.join("Applications/A.app")));
            }
            let roots = super::super::test_function(&source, "uninstall_print_app_search_dirs")
                .replace("\"/Applications\"", "\"$BG_APPLICATIONS\"")
                .replace("\"/Library/Input Methods\"", "\"$BG_INPUT_METHODS\"")
                .replace("/Volumes/*/Applications", "\"$BG_VOLUMES\"/*/Applications");
            let script = format!("set -euo pipefail\n{}\n{}\n{}\nfor target do if uninstall_app_is_directly_in_search_root \"$target\"; then printf '1\\n'; else status=$?; [[ $status == 1 ]] || exit 2; printf '0\\n'; fi; done", super::super::test_function(&helpers,"mole_playcover_apps_dir"), roots, super::super::test_function(&source,"uninstall_app_is_directly_in_search_root"));
            let output = std::process::Command::new("/bin/bash")
                .env_clear()
                .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
                .env("HOME", &scope.home)
                .env("TMPDIR", &scope.home)
                .env("BG_APPLICATIONS", &scope.applications)
                .env("BG_INPUT_METHODS", &scope.input_methods)
                .env("BG_VOLUMES", &scope.volumes)
                .arg("-c")
                .arg(script)
                .arg("moomole-root-test")
                .args(&paths)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let actual = String::from_utf8(output.stdout).unwrap();
            assert_eq!(actual.lines().count(), paths.len());
            for (path, code) in paths.iter().zip(actual.lines()) {
                let value = check(path, &scope).finish();
                assert_ne!(value.code, "unknown", "{}", path.display());
                assert_eq!(
                    value.code == "directRoot",
                    code == "1",
                    "{}",
                    path.display()
                );
            }
            println!(
                "Compared {} isolated root contexts against pinned Mole",
                paths.len()
            );
        }
    }
}

#[cfg(test)]
pub(crate) fn test_function<'a>(source: &'a str, name: &str) -> &'a str {
    let needle = format!("\n{name}() {{\n");
    let start = source.find(&needle).expect("pinned function header") + 1;
    let end = source[start..].find("\n}\n").expect("pinned function end") + 3;
    &source[start..start + end]
}
