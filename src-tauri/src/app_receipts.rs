//! Read-only, bounded nonstandard App discovery from structured system receipts.
use serde::Serialize;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub code: &'static str,
    pub issue: Option<&'static str>,
    package_count: usize,
    checked_count: usize,
    entries: usize,
    candidates: usize,
}
impl Observation {
    pub fn not_checked() -> Self {
        Self {
            code: "notChecked",
            issue: None,
            package_count: 0,
            checked_count: 0,
            entries: 0,
            candidates: 0,
        }
    }
}
#[cfg(target_os = "macos")]
pub mod mac {
    use super::*;
    use objc2::{
        rc::{autoreleasepool, Retained},
        runtime::AnyObject,
    };
    use objc2_foundation::{
        NSArray, NSData, NSDictionary, NSPropertyListReadOptions, NSPropertyListSerialization,
        NSString,
    };
    use std::{
        collections::HashSet,
        ffi::CString,
        fs::{File, Metadata, OpenOptions},
        os::{
            fd::{AsRawFd, FromRawFd},
            macos::fs::MetadataExt as DarwinMetadataExt,
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
        path::{Component, Path, PathBuf},
        sync::Arc,
        time::{Duration, Instant},
    };
    const OUTPUT: u64 = 2 * 1024 * 1024;
    const SF_DATALESS: u32 = 0x40000000;
    type Stamp = (u64, u64, u32, u32, u32, u32, u64, i64, i64, i64, i64);
    fn stamp(m: &Metadata) -> Stamp {
        (
            m.dev(),
            m.ino(),
            m.mode(),
            m.uid(),
            m.gid(),
            m.st_flags(),
            m.len(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )
    }
    struct Database {
        path: PathBuf,
        directory: File,
        metadata: Metadata,
    }
    impl Database {
        fn open(path: &Path) -> Result<Self, ()> {
            if path.canonicalize().map_err(|_| ())? != path {
                return Err(());
            }
            let directory = OpenOptions::new()
                .read(true)
                .custom_flags(
                    libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
                .open(path)
                .map_err(|_| ())?;
            let metadata = directory.metadata().map_err(|_| ())?;
            let mut fs = std::mem::MaybeUninit::<libc::statfs>::zeroed();
            if !metadata.is_dir()
                || metadata.st_flags() & SF_DATALESS != 0
                || unsafe { libc::fstatfs(directory.as_raw_fd(), fs.as_mut_ptr()) } != 0
                || unsafe { fs.assume_init() }.f_flags & libc::MNT_LOCAL as u32 == 0
            {
                return Err(());
            }
            Ok(Self {
                path: path.into(),
                directory,
                metadata,
            })
        }
        fn file(&self, name: &str) -> Result<Stamp, ()> {
            let name = CString::new(name).map_err(|_| ())?;
            let mut before = std::mem::MaybeUninit::<libc::stat>::zeroed();
            if unsafe {
                libc::fstatat(
                    self.directory.as_raw_fd(),
                    name.as_ptr(),
                    before.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } != 0
            {
                return Err(());
            }
            let before = unsafe { before.assume_init() };
            if before.st_mode & libc::S_IFMT != libc::S_IFREG || before.st_flags & SF_DATALESS != 0
            {
                return Err(());
            }
            let fd = unsafe {
                libc::openat(
                    self.directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(());
            }
            let file = unsafe { File::from_raw_fd(fd) };
            let meta = file.metadata().map_err(|_| ())?;
            if !meta.is_file()
                || meta.nlink() != 1
                || meta.dev() != self.metadata.dev()
                || meta.ino() != before.st_ino
                || meta.st_flags() & SF_DATALESS != 0
            {
                return Err(());
            }
            Ok(stamp(&meta))
        }
        fn verify(&self) -> bool {
            self.path.canonicalize().is_ok_and(|p| p == self.path)
                && std::fs::symlink_metadata(&self.path)
                    .is_ok_and(|m| stamp(&m) == stamp(&self.metadata))
                && self
                    .directory
                    .metadata()
                    .is_ok_and(|m| stamp(&m) == stamp(&self.metadata))
        }
    }
    fn plist(bytes: &[u8]) -> Result<Retained<AnyObject>, &'static str> {
        if bytes.is_empty() || bytes.len() as u64 > OUTPUT {
            return Err("limitReached");
        }
        unsafe {
            NSPropertyListSerialization::propertyListWithData_options_format_error(
                &NSData::with_bytes(bytes),
                NSPropertyListReadOptions::Immutable,
                std::ptr::null_mut(),
            )
        }
        .map_err(|_| "unavailable")
    }
    fn text(value: &AnyObject) -> Result<String, &'static str> {
        value
            .downcast_ref::<NSString>()
            .map(|s| s.to_string())
            .ok_or("unsupportedReceipt")
    }
    fn packages(bytes: &[u8]) -> Result<Vec<String>, &'static str> {
        autoreleasepool(|_| {
            let object = plist(bytes)?;
            let array = object.downcast_ref::<NSArray>().ok_or("unavailable")?;
            if array.count() > 512 {
                return Err("limitReached");
            }
            let mut ids = Vec::new();
            for value in array {
                let id = text(&value)?;
                if id.is_empty()
                    || id.len() > 255
                    || id.starts_with('-')
                    || !id.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+')
                    })
                {
                    return Err("unsupportedReceipt");
                }
                ids.push(id);
            }
            ids.sort();
            if ids.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err("unsupportedReceipt");
            }
            Ok(ids)
        })
    }
    // Preserve Mole's case-arm order: an entire path ending .app is considered
    // first; otherwise take the first .app/ prefix. Do not normalize ../ paths.
    fn candidate(raw: &str) -> Result<Option<String>, &'static str> {
        if raw.len() > 4096 || raw.chars().any(char::is_control) {
            return Err("unsupportedReceipt");
        }
        let path = format!("/{}", raw.strip_prefix('/').unwrap_or(raw));
        if !path.starts_with("/usr/local/") && !path.starts_with("/opt/") {
            return Ok(None);
        }
        let lower = path.to_ascii_lowercase();
        let value = if lower.ends_with(".app") {
            path
        } else if let Some(index) = lower.find(".app/") {
            path[..index + 4].to_owned()
        } else {
            return Ok(None);
        };
        if Path::new(&value)
            .components()
            .any(|p| !matches!(p, Component::RootDir | Component::Normal(_)))
            || value
                .split('/')
                .skip(1)
                .any(|s| s.is_empty() || s == "." || s == "..")
        {
            return Err("unsupportedReceipt");
        }
        Ok(Some(value))
    }
    fn paths(bytes: &[u8], id: &str) -> Result<Vec<String>, &'static str> {
        autoreleasepool(|_| {
            let object = plist(bytes)?;
            let dictionary = object.downcast_ref::<NSDictionary>().ok_or("unavailable")?;
            for (key, expected) in [("pkgid", id), ("volume", "/")] {
                let value = dictionary
                    .objectForKey(&NSString::from_str(key))
                    .ok_or("unsupportedReceipt")?;
                if text(&value)? != expected {
                    return Err("unsupportedReceipt");
                }
            }
            let value = dictionary
                .objectForKey(&NSString::from_str("paths"))
                .ok_or("unsupportedReceipt")?;
            let dictionary = value
                .downcast_ref::<NSDictionary>()
                .ok_or("unsupportedReceipt")?;
            if dictionary.count() > 32768 {
                return Err("limitReached");
            }
            dictionary
                .allKeys()
                .iter()
                .map(|value| text(&value))
                .collect()
        })
    }
    #[derive(Clone, Copy)]
    enum Query<'a> {
        Packages,
        Export(&'a str),
    }
    fn query(q: Query<'_>, remaining: Duration) -> Result<Vec<u8>, &'static str> {
        if remaining.is_zero() {
            return Err("limitReached");
        }
        let mut command = tokio::process::Command::new("/usr/sbin/pkgutil");
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("LC_ALL", "C")
            .args(["--volume", "/"]);
        match q {
            Query::Packages => {
                command.arg("--pkgs-plist");
            }
            Query::Export(id) => {
                command.args(["--export-plist", id]);
            }
        }
        let job = Arc::new(crate::engine::Job::new(crate::engine::TaskKind::Apps));
        tokio::runtime::Handle::current()
            .block_on(crate::process::output_bounded(
                command,
                job,
                remaining.min(Duration::from_millis(750)),
                OUTPUT,
            ))
            .map_err(|_| "unavailable")
    }
    pub struct Check {
        pub paths: Vec<Option<String>>,
        observation: Observation,
        database: Option<Database>,
        files: Vec<(String, Stamp)>,
        packages: Option<Vec<String>>,
    }
    impl Check {
        pub fn begin() -> Self {
            Self::begin_with(Path::new("/private/var/db/receipts"), query)
        }
        fn partial(&mut self, issue: &'static str) {
            self.observation.code = "partial";
            if self.observation.issue.is_none() || issue == "changed" {
                self.observation.issue = Some(issue);
            }
        }
        fn begin_with(
            root: &Path,
            mut query: impl FnMut(Query<'_>, Duration) -> Result<Vec<u8>, &'static str>,
        ) -> Self {
            let mut result = Self {
                paths: vec![],
                observation: Observation::not_checked(),
                database: None,
                files: vec![],
                packages: None,
            };
            result.observation.code = "observed";
            let Ok(database) = Database::open(root) else {
                result.partial("unavailable");
                return result;
            };
            result.database = Some(database);
            let started = Instant::now();
            let budget = Duration::from_millis(1500);
            let list = match query(Query::Packages, budget).and_then(|b| packages(&b)) {
                Ok(ids) => ids,
                Err(issue) => {
                    result.partial(issue);
                    return result;
                }
            };
            result.observation.package_count = list.len();
            result.packages = Some(list.clone());
            let mut seen = HashSet::new();
            for id in list.iter().filter(|id| !id.starts_with("com.apple.")) {
                if result.observation.checked_count >= 64
                    || result.paths.len() >= 128
                    || result.observation.entries >= 32768
                    || started.elapsed() >= budget
                {
                    result.partial("limitReached");
                    break;
                }
                result.observation.checked_count += 1;
                let files = [format!("{id}.plist"), format!("{id}.bom")];
                let mut valid = true;
                for name in files {
                    match result.database.as_ref().unwrap().file(&name) {
                        Ok(stamp) => result.files.push((name, stamp)),
                        Err(()) => {
                            result.partial("unavailable");
                            valid = false;
                            break;
                        }
                    }
                }
                if !valid {
                    continue;
                }
                let mut entries =
                    match query(Query::Export(id), budget.saturating_sub(started.elapsed()))
                        .and_then(|bytes| paths(&bytes, id))
                    {
                        Ok(paths) => paths,
                        Err(issue) => {
                            result.partial(issue);
                            continue;
                        }
                    };
                entries.sort();
                for entry in entries {
                    if result.observation.entries >= 32768
                        || result.paths.len() >= 128
                        || started.elapsed() >= budget
                    {
                        result.partial("limitReached");
                        break;
                    }
                    result.observation.entries += 1;
                    match candidate(&entry) {
                        Ok(Some(path)) if seen.insert(path.clone()) => {
                            result.paths.push(Some(path))
                        }
                        Err(issue) => result.partial(issue),
                        _ => {}
                    }
                }
            }
            result.observation.candidates = result.paths.len();
            result
        }
        pub fn finish(self) -> Observation {
            self.finish_with(query)
        }
        fn finish_with(
            mut self,
            mut query: impl FnMut(Query<'_>, Duration) -> Result<Vec<u8>, &'static str>,
        ) -> Observation {
            let started = Instant::now();
            if let Some(expected) = &self.packages {
                match query(Query::Packages, Duration::from_millis(500))
                    .and_then(|bytes| packages(&bytes))
                {
                    Ok(list) if &list == expected => {}
                    Ok(_) => self.partial("changed"),
                    Err(issue) => self.partial(issue),
                }
            }
            if let Some(database) = &self.database {
                let valid = database.verify()
                    && self.files.iter().all(|(name, expected)| {
                        database
                            .file(name)
                            .is_ok_and(|current| &current == expected)
                    })
                    && database.verify();
                if !valid {
                    self.partial("changed");
                }
            }
            if started.elapsed() > Duration::from_millis(1000) {
                self.partial("limitReached");
            }
            self.observation
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{fs, os::unix::fs::symlink};
        fn xml(body: &str) -> Vec<u8> {
            format!(r#"<?xml version="1.0"?><plist version="1.0">{body}</plist>"#).into_bytes()
        }
        fn list(ids: &[&str]) -> Vec<u8> {
            xml(&format!(
                "<array>{}</array>",
                ids.iter()
                    .map(|id| format!("<string>{id}</string>"))
                    .collect::<String>()
            ))
        }
        fn export(id: &str, entries: &[&str]) -> Vec<u8> {
            xml(&format!("<dict><key>pkgid</key><string>{id}</string><key>volume</key><string>/</string><key>paths</key><dict>{}</dict></dict>",entries.iter().map(|path| format!("<key>{path}</key><dict/>")).collect::<String>()))
        }
        fn fixture(ids: &[&str]) -> (tempfile::TempDir, PathBuf) {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            for id in ids {
                for ext in ["plist", "bom"] {
                    fs::write(root.join(format!("{id}.{ext}")), "fixture").unwrap();
                }
            }
            (tmp, root)
        }
        #[test]
        fn receipt_scope_and_plist_boundaries_do_not_expand_paths_or_accept_options() {
            assert_eq!(
                candidate("usr/local/Vendor/Example.app/Contents/Info.plist").unwrap(),
                Some("/usr/local/Vendor/Example.app".into())
            );
            assert_eq!(
                candidate("/opt/Example.APP").unwrap(),
                Some("/opt/Example.APP".into())
            );
            assert_eq!(candidate("Applications/Example.app").unwrap(), None);
            assert_eq!(candidate("usr/locality/Example.app").unwrap(), None);
            for path in [
                "opt/../Example.app",
                "opt/./Example.app",
                "opt//Example.app",
                "opt/Bad\n.app",
            ] {
                assert!(candidate(path).is_err());
            }
            for bytes in [
                list(&["--forget"]),
                list(&["same", "same"]),
                list(&["../id"]),
                xml("<dict/>"),
                b"not a plist".to_vec(),
            ] {
                assert!(packages(&bytes).is_err());
            }
            assert!(paths(&export("other", &[]), "selected").is_err());
            assert!(paths(&xml("<dict><key>pkgid</key><string>selected</string><key>volume</key><string>/Volumes/Other</string><key>paths</key><dict/></dict>"), "selected").is_err());
        }
        #[test]
        fn apple_receipts_are_skipped_candidates_deduplicated_and_changes_detected() {
            let (_tmp, root) = fixture(&["org.fixture"]);
            let response = |q: Query<'_>, _: Duration| match q {
                Query::Packages => Ok(list(&["com.apple.ignored", "org.fixture"])),
                Query::Export(id) => {
                    assert_eq!(id, "org.fixture");
                    Ok(export(
                        id,
                        &[
                            "opt/Vendor/One.app",
                            "opt/Vendor/One.app/Contents/Info.plist",
                            "Applications/Else.app",
                        ],
                    ))
                }
            };
            let check = Check::begin_with(&root, response);
            assert_eq!(check.paths, [Some("/opt/Vendor/One.app".into())]);
            assert_eq!(check.observation.checked_count, 1);
            assert_eq!(check.finish_with(response).code, "observed");
            let check = Check::begin_with(&root, response);
            fs::write(root.join("org.fixture.bom"), "changed").unwrap();
            assert_eq!(check.finish_with(response).issue, Some("changed"));
            let check = Check::begin_with(&root, response);
            assert_eq!(
                check
                    .finish_with(|_, _| Ok(list(&["org.fixture", "new.install"])))
                    .issue,
                Some("changed")
            );
        }
        #[test]
        fn failures_links_limits_and_malformed_exports_preserve_uncertainty() {
            let (_tmp, root) = fixture(&["org.good", "org.bad"]);
            let response = |q: Query<'_>, _: Duration| match q {
                Query::Packages => Ok(list(&["org.good", "org.bad"])),
                Query::Export("org.bad") => Err("unavailable"),
                Query::Export(id) => Ok(export(id, &["opt/Good.app"])),
            };
            let check = Check::begin_with(&root, response);
            assert_eq!(check.paths, [Some("/opt/Good.app".into())]);
            assert_eq!(check.finish_with(response).issue, Some("unavailable"));
            fs::remove_file(root.join("org.good.bom")).unwrap();
            symlink(root.join("org.bad.bom"), root.join("org.good.bom")).unwrap();
            assert_eq!(
                Check::begin_with(&root, response)
                    .finish_with(response)
                    .code,
                "partial"
            );
            let ids: Vec<_> = (0..513).map(|i| format!("org.{i}")).collect();
            let refs: Vec<_> = ids.iter().map(String::as_str).collect();
            assert_eq!(packages(&list(&refs)).unwrap_err(), "limitReached");
            assert_eq!(
                Check::begin_with(&root, |_, _| Err("unavailable"))
                    .finish_with(response)
                    .issue,
                Some("unavailable")
            );
            let (_tmp, root) = fixture(&["org.many"]);
            let entries: Vec<_> = (0..129).map(|i| format!("opt/{i}.app")).collect();
            let refs: Vec<_> = entries.iter().map(String::as_str).collect();
            let response = |q: Query<'_>, _: Duration| match q {
                Query::Packages => Ok(list(&["org.many"])),
                Query::Export(id) => Ok(export(id, &refs)),
            };
            let check = Check::begin_with(&root, response);
            assert_eq!(check.paths.len(), 128);
            assert_eq!(check.finish_with(response).issue, Some("limitReached"));
        }
        #[test]
        fn receipt_and_entry_limits_do_not_become_an_empty_complete_scan() {
            let ids: Vec<_> = (0..65).map(|i| format!("org.fixture{i}")).collect();
            let refs: Vec<_> = ids.iter().map(String::as_str).collect();
            let (_tmp, root) = fixture(&refs);
            let response = |q: Query<'_>, _: Duration| match q {
                Query::Packages => Ok(list(&refs)),
                Query::Export(id) => Ok(export(id, &[])),
            };
            let observation = Check::begin_with(&root, response).finish_with(response);
            assert_eq!(observation.checked_count, 64);
            assert_eq!(observation.issue, Some("limitReached"));
            let entries: Vec<_> = (0..32769).map(|i| format!("file{i}")).collect();
            let refs: Vec<_> = entries.iter().map(String::as_str).collect();
            assert_eq!(
                paths(&export("org.fixture", &refs), "org.fixture").unwrap_err(),
                "limitReached"
            );
        }
        #[test]
        #[ignore = "requires pinned Mole source; compares original receipt path case arms without any lifecycle or real path writes"]
        fn prepared_receipt_path_selection_matches_original_case_arms() {
            use std::process::Command;
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
            crate::app_uninstall_rules::load(&source).unwrap();
            let text = fs::read_to_string(source.join("lib/core/pkg_receipts.sh")).unwrap();
            let start = text.find("            local stripped=").unwrap();
            let end = text[start..].find("            # Record the path").unwrap() + start;
            let values = [
                "usr/local/Foo.app",
                "/opt/Vendor/Foo.APP/Contents/Info.plist",
                "opt/Outer.app/Inner.app",
                "opt/Outer.app/Inner.app/file",
                "Applications/Foo.app",
                "opt/普通 名称.app/file",
                "usr/locality/No.app",
                "opt/No.appdata/file",
            ];
            let script = format!(
                r#"set -euo pipefail
main() {{
  local rel_path app_path
  for rel_path in "$@"; do
    app_path=""
{}
    printf '%s\0' "$app_path"
  done
}}
main "$@""#,
                &text[start..end]
            );
            let output = Command::new("/bin/bash")
                .args(["-c", &script, "receipt-path"])
                .args(values)
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
            let expected: Vec<_> = output
                .stdout
                .split(|b| *b == 0)
                .filter(|b| !b.is_empty())
                .map(|b| String::from_utf8(b.to_vec()).unwrap())
                .collect();
            let actual: Vec<_> = values
                .iter()
                .filter_map(|v| candidate(v).unwrap())
                .collect();
            assert_eq!(actual, expected);
        }
        #[tokio::test]
        #[ignore = "explicit read-only system receipt observation; does not write receipt caches or inspect target app contents"]
        async fn local_structured_receipts_smoke() {
            let observation = tokio::task::spawn_blocking(|| Check::begin().finish())
                .await
                .unwrap();
            assert!(matches!(observation.code, "observed" | "partial"));
            assert!(observation.package_count > 0);
            println!("{}", serde_json::to_string(&observation).unwrap());
        }
    }
}
