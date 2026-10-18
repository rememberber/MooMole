//! Bounded read-only evidence for an existing Mole list entry, never execution eligibility.
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    protocol: u32,
    checked_at: i64,
    bundle_id: Option<String>,
    version: Option<String>,
    build: Option<String>,
    identity: &'static str,
    running: Running,
    protection: crate::app_protection::Protection,
    mole_protection: crate::app_uninstall_rules::Observation,
    official_uninstaller: crate::app_uninstall_rules::OfficialObservation,
    background: crate::app_background::Observation,
    associations: crate::app_associations::Observation,
    installations: Installations,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Installations {
    version: u32,
    source: &'static str,
    revision: &'static str,
    code: &'static str,
    issue: Option<&'static str>,
    copies: Vec<InstalledCopy>,
    roots: crate::app_discovery::Observation,
    volumes: crate::app_discovery::Observation,
    receipts: crate::app_receipts::Observation,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstalledCopy {
    path: String,
    bundle_id: String,
    version: Option<String>,
    build: Option<String>,
    relation: &'static str,
}
impl Inspection {
    pub fn attach_protection(
        &mut self,
        path: &str,
        current_app: Option<&std::path::Path>,
        current_id: &str,
        home: &std::path::Path,
        rules: &[String],
    ) -> Result<(), String> {
        self.protection = crate::app_protection::check(
            path,
            self.bundle_id.as_deref(),
            current_app,
            current_id,
            rules,
        )?;
        self.associations
            .attach_protection(home, self.bundle_id.as_deref(), current_id, rules)?;
        Ok(())
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Running {
    code: &'static str,
    pids: Vec<i32>,
    other_copies: usize,
    reason: String,
}
impl Running {
    fn unknown(reason: &str) -> Self {
        Self {
            code: "unknown",
            pids: vec![],
            other_copies: 0,
            reason: reason.into(),
        }
    }
}
pub async fn load(
    path: String,
    expected: String,
    name: Option<String>,
    candidates: Option<Vec<Option<String>>>,
    engine_root: std::path::PathBuf,
    home: std::path::PathBuf,
    slots: Arc<Semaphore>,
) -> Result<Inspection, String> {
    let slot = slots
        .try_acquire_owned()
        .map_err(|_| "已有应用状态检查正在进行，请稍后重试。")?;
    let worker = tokio::task::spawn_blocking(move || {
        let _slot = slot;
        let policy = crate::app_uninstall_rules::load(&engine_root);
        let mut result = inspect(&path, &expected, &home, policy.is_ok(), candidates)?;
        result.mole_protection = crate::app_uninstall_rules::observe(
            policy.as_ref().ok(),
            result.bundle_id.as_deref(),
            result.identity,
        );
        result.official_uninstaller = crate::app_uninstall_rules::observe_official(
            policy.as_ref().ok(),
            result.bundle_id.as_deref(),
            result.identity,
            name.as_deref(),
            &path,
        );
        Ok(result)
    });
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .map_err(|_| "应用状态读取超时，结果无法确认；请稍后重试。")?
        .map_err(|_| "应用状态读取未完成，请重试。")?
}
fn inspect(
    path: &str,
    expected: &str,
    home: &std::path::Path,
    rules_ready: bool,
    candidates: Option<Vec<Option<String>>>,
) -> Result<Inspection, String> {
    #[cfg(target_os = "macos")]
    {
        mac::inspect(path, expected, home, rules_ready, candidates)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, expected, home, rules_ready, candidates);
        Err("当前系统不支持此应用状态检查。".into())
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2::{rc::autoreleasepool, runtime::AnyObject};
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::{
        NSData, NSDictionary, NSNumber, NSPropertyListReadOptions, NSPropertyListSerialization,
        NSString,
    };
    use std::{
        fs::{File, Metadata, OpenOptions},
        io::Read,
        os::{
            fd::{AsRawFd, FromRawFd},
            macos::fs::MetadataExt as DarwinMetadataExt,
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
        path::{Component, Path, PathBuf},
    };

    const LIMIT: u64 = 1024 * 1024;
    const SF_DATALESS: u32 = 0x40000000; // macOS sys/stat.h
    const UNREADABLE: &str = "无法读取应用身份；请确认应用仍在原位置且可访问。";
    const CHANGED: &str = "应用或身份文件在检查期间发生变化，请重新读取列表。";
    fn stamp(m: &Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
        (
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )
    }
    fn same_object(a: &Metadata, b: &Metadata) -> bool {
        (a.dev(), a.ino()) == (b.dev(), b.ino())
    }
    struct Bundle {
        path: PathBuf,
        root: File,
        metadata: Metadata,
    }
    impl Bundle {
        fn open(raw: &str) -> Result<Self, String> {
            let path = Path::new(raw);
            if raw.len() > 4096
                || raw.contains('\0')
                || !path.is_absolute()
                || path.extension().is_none_or(|v| v != "app")
                || path
                    .components()
                    .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
            {
                return Err(UNREADABLE.into());
            }
            if path.canonicalize().map_err(|_| UNREADABLE)? != path {
                return Err("应用路径包含链接或别名，请重新读取列表。".into());
            }
            let root = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(path)
                .map_err(|_| UNREADABLE)?;
            let metadata = root.metadata().map_err(|_| UNREADABLE)?;
            let mut fs = std::mem::MaybeUninit::<libc::statfs>::zeroed();
            if !metadata.is_dir()
                || metadata.st_flags() & SF_DATALESS != 0
                || unsafe { libc::fstatfs(root.as_raw_fd(), fs.as_mut_ptr()) } != 0
                || unsafe { fs.assume_init() }.f_flags & libc::MNT_LOCAL as u32 == 0
            {
                return Err("本次仅检查可读取的本地应用目录；网络卷或云占位目标暂不支持。".into());
            }
            Ok(Self {
                path: path.into(),
                root,
                metadata,
            })
        }
        fn verify(&self) -> bool {
            std::fs::symlink_metadata(&self.path)
                .is_ok_and(|m| m.is_dir() && stamp(&m) == stamp(&self.metadata))
                && self
                    .root
                    .metadata()
                    .is_ok_and(|m| stamp(&m) == stamp(&self.metadata))
                && self.path.canonicalize().is_ok_and(|p| p == self.path)
        }
    }
    fn child(parent: &File, name: &std::ffi::CStr, directory: bool) -> Result<File, String> {
        let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
        if unsafe {
            libc::fstatat(
                parent.as_raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(UNREADABLE.into());
        }
        let stat = unsafe { stat.assume_init() };
        let kind = if directory {
            libc::S_IFDIR
        } else {
            libc::S_IFREG
        };
        if stat.st_mode & libc::S_IFMT != kind || stat.st_flags & SF_DATALESS != 0 {
            return Err(UNREADABLE.into());
        }
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if directory { libc::O_DIRECTORY } else { 0 };
        let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(UNREADABLE.into());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let meta = file.metadata().map_err(|_| UNREADABLE)?;
        if (!directory && meta.nlink() != 1)
            || meta.dev() != parent.metadata().map_err(|_| UNREADABLE)?.dev()
        {
            return Err("应用身份文件的链接或挂载布局暂不支持检查。".into());
        }
        if (meta.dev(), meta.ino()) != (stat.st_dev as u64, stat.st_ino) {
            return Err(CHANGED.into());
        }
        Ok(file)
    }
    struct Evidence {
        contents: File,
        plist: File,
        contents_meta: Metadata,
        plist_meta: Metadata,
    }
    impl Evidence {
        fn read(bundle: &Bundle) -> Result<(Self, Vec<u8>), String> {
            let contents = child(&bundle.root, c"Contents", true)?;
            let plist = child(&contents, c"Info.plist", false)?;
            let contents_meta = contents.metadata().map_err(|_| UNREADABLE)?;
            let plist_meta = plist.metadata().map_err(|_| UNREADABLE)?;
            if plist_meta.len() == 0 || plist_meta.len() > LIMIT {
                return Err("应用身份文件为空或超过检查上限。".into());
            }
            let mut bytes = Vec::new();
            (&plist)
                .take(LIMIT + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| UNREADABLE)?;
            if bytes.len() as u64 > LIMIT {
                return Err("应用身份文件超过检查上限。".into());
            }
            Ok((
                Self {
                    contents,
                    plist,
                    contents_meta,
                    plist_meta,
                },
                bytes,
            ))
        }
        fn verify(&self, bundle: &Bundle) -> bool {
            let verified = (|| -> Result<bool, String> {
                let contents = child(&bundle.root, c"Contents", true)?;
                let plist = child(&contents, c"Info.plist", false)?;
                Ok(
                    stamp(&contents.metadata().map_err(|_| CHANGED)?) == stamp(&self.contents_meta)
                        && stamp(&plist.metadata().map_err(|_| CHANGED)?)
                            == stamp(&self.plist_meta)
                        && self
                            .contents
                            .metadata()
                            .is_ok_and(|m| stamp(&m) == stamp(&self.contents_meta))
                        && self
                            .plist
                            .metadata()
                            .is_ok_and(|m| stamp(&m) == stamp(&self.plist_meta)),
                )
            })();
            bundle.verify() && matches!(verified, Ok(true))
        }
    }
    #[derive(Debug)]
    struct Fields {
        id: Option<String>,
        version: Option<String>,
        build: Option<String>,
        background: crate::app_background::Marker,
    }
    fn background_marker(value: Option<&AnyObject>) -> crate::app_background::Marker {
        use crate::app_background::Marker;
        use core_foundation_sys::{
            base::CFGetTypeID,
            number::{
                CFBooleanGetTypeID, CFBooleanGetValue, CFNumberGetTypeID, CFNumberIsFloatType,
            },
        };
        let Some(value) = value else {
            return Marker::Absent;
        };
        let enabled = if let Some(string) = value.downcast_ref::<NSString>() {
            let string = string.to_string();
            if string.len() > 128 || string.chars().any(char::is_control) {
                return Marker::Unknown;
            }
            matches!(string.as_str(), "1" | "YES" | "yes" | "TRUE" | "true")
        } else if let Some(number) = value.downcast_ref::<NSNumber>() {
            let pointer = value as *const AnyObject as *const std::ffi::c_void;
            // Foundation plist numbers/booleans are toll-free bridged CF values.
            unsafe {
                if CFGetTypeID(pointer) == CFBooleanGetTypeID() {
                    CFBooleanGetValue(pointer.cast())
                } else if CFGetTypeID(pointer) == CFNumberGetTypeID()
                    && CFNumberIsFloatType(pointer.cast()) == 0
                {
                    number.stringValue().to_string() == "1"
                } else {
                    return Marker::Unknown;
                }
            }
        } else {
            return Marker::Unknown;
        };
        if enabled {
            Marker::Enabled
        } else {
            Marker::Disabled
        }
    }
    fn text(value: &AnyObject, max: usize) -> Option<String> {
        let value = value.downcast_ref::<NSString>()?.to_string();
        (!value.is_empty() && value.len() <= max && !value.chars().any(char::is_control))
            .then_some(value)
    }
    fn valid_id(id: &str) -> bool {
        !id.is_empty()
            && id.len() <= 255
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    }
    fn parse(bytes: &[u8]) -> Result<Fields, String> {
        autoreleasepool(|_| {
            let object = unsafe {
                NSPropertyListSerialization::propertyListWithData_options_format_error(
                    &NSData::with_bytes(bytes),
                    NSPropertyListReadOptions::Immutable,
                    std::ptr::null_mut(),
                )
            }
            .map_err(|_| "应用身份文件格式无法识别。")?;
            let dictionary = object
                .downcast_ref::<NSDictionary>()
                .ok_or("应用身份文件不是字典。")?;
            let field = |key: &str, limit| {
                dictionary
                    .objectForKey(&NSString::from_str(key))
                    .and_then(|v| text(&v, limit))
            };
            Ok(Fields {
                id: field("CFBundleIdentifier", 255).filter(|v| valid_id(v)),
                version: field("CFBundleShortVersionString", 128),
                build: field("CFBundleVersion", 128),
                background: background_marker(
                    dictionary
                        .objectForKey(&NSString::from_str("LSBackgroundOnly"))
                        .as_deref(),
                ),
            })
        })
    }
    #[derive(Debug)]
    struct Process {
        path: Option<String>,
        id: Option<String>,
        pid: i32,
        terminated: bool,
    }
    fn processes(id: &str) -> Result<Vec<Process>, String> {
        autoreleasepool(|_| {
            let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(
                &NSString::from_str(id),
            );
            if apps.count() > 32 {
                return Err("同标识运行实例超过检查上限。".into());
            }
            Ok(apps
                .iter()
                .map(|app| {
                    let path = app
                        .bundleURL()
                        .filter(|u| u.isFileURL())
                        .and_then(|u| u.path())
                        .and_then(|v| text(&v, 4096));
                    Process {
                        path,
                        id: app.bundleIdentifier().and_then(|v| text(&v, 255)),
                        pid: app.processIdentifier(),
                        terminated: app.isTerminated(),
                    }
                })
                .collect())
        })
    }
    fn classify(bundle: &Bundle, id: &str, records: &[Process]) -> Running {
        let mut pids = vec![];
        let mut copies = std::collections::HashSet::new();
        let mut unknown = false;
        for process in records {
            if process.terminated {
                continue;
            }
            if process.pid <= 0 || process.id.as_deref() != Some(id) {
                unknown = true;
                continue;
            }
            let Some(path) = process.path.as_deref() else {
                unknown = true;
                continue;
            };
            let Ok(candidate) = Bundle::open(path) else {
                unknown = true;
                continue;
            };
            if candidate.path == bundle.path && same_object(&candidate.metadata, &bundle.metadata) {
                if !pids.contains(&process.pid) {
                    pids.push(process.pid);
                }
            } else {
                copies.insert(candidate.path);
            }
        }
        pids.sort_unstable();
        let (code, reason) = if !pids.is_empty() {
            (
                "observed",
                "本次发现此安装位置的主应用运行；未检查辅助进程或后台服务。",
            )
        } else if unknown {
            (
                "unknown",
                "部分运行实例的位置或身份无法确认；不能判断此应用是否仍在使用。",
            )
        } else {
            (
                "notObserved",
                "本次未发现此安装位置的匹配主应用；辅助进程、后台服务和文件占用未检查。",
            )
        };
        Running {
            code,
            pids,
            other_copies: copies.len(),
            reason: reason.into(),
        }
    }
    // Launch Services is an additional, explicitly partial discovery source.
    // It cannot replace Mole's roots, receipt, volume or shared-remnant scans.
    mod copies {
        use super::*;
        use objc2_app_kit::NSWorkspace;
        const LIMIT: usize = 32;
        pub(super) fn registered(id: &str) -> Result<Vec<Option<String>>, ()> {
            autoreleasepool(|_| {
                let urls = NSWorkspace::sharedWorkspace()
                    .URLsForApplicationsWithBundleIdentifier(&NSString::from_str(id));
                Ok(urls
                    .iter()
                    .take(LIMIT + 1)
                    .map(|url| {
                        url.isFileURL()
                            .then(|| url.path().and_then(|p| text(&p, 4096)))
                            .flatten()
                    })
                    .collect())
            })
        }
        struct Candidate {
            path: PathBuf,
            root_meta: Metadata,
            contents_meta: Metadata,
            plist_meta: Metadata,
            held: Option<(Bundle, Evidence)>,
            copy: Option<InstalledCopy>,
        }
        impl Candidate {
            fn verify(&self) -> bool {
                if let Some((bundle, evidence)) = &self.held {
                    return evidence.verify(bundle);
                }
                let Ok(mut bundle) = Bundle::open(self.path.to_str().unwrap_or("")) else {
                    return false;
                };
                let Ok(contents) = child(&bundle.root, c"Contents", true) else {
                    return false;
                };
                let Ok(plist) = child(&contents, c"Info.plist", false) else {
                    return false;
                };
                bundle.metadata = self.root_meta.clone();
                Evidence {
                    contents,
                    plist,
                    contents_meta: self.contents_meta.clone(),
                    plist_meta: self.plist_meta.clone(),
                }
                .verify(&bundle)
            }
        }
        pub(super) struct Check {
            observation: Installations,
            candidates: Vec<Candidate>,
        }
        impl Check {
            #[cfg(test)]
            pub(super) fn begin(
                selected: &Bundle,
                id: Option<&str>,
                identity: &str,
                rules_ready: bool,
                query: impl FnOnce(&str) -> Result<Vec<Option<String>>, ()>,
            ) -> Self {
                Self::begin_sources(selected, id, identity, rules_ready, query, Some(vec![]))
            }
            #[cfg(test)]
            pub(super) fn begin_sources(
                selected: &Bundle,
                id: Option<&str>,
                identity: &str,
                rules_ready: bool,
                query: impl FnOnce(&str) -> Result<Vec<Option<String>>, ()>,
                mole_paths: Option<Vec<Option<String>>>,
            ) -> Self {
                Self::begin_all_sources(
                    selected,
                    id,
                    identity,
                    rules_ready,
                    query,
                    mole_paths,
                    vec![],
                )
            }
            pub(super) fn begin_all_sources(
                selected: &Bundle,
                id: Option<&str>,
                identity: &str,
                rules_ready: bool,
                query: impl FnOnce(&str) -> Result<Vec<Option<String>>, ()>,
                mole_paths: Option<Vec<Option<String>>>,
                root_paths: Vec<Option<String>>,
            ) -> Self {
                let issue = if identity == "different" {
                    Some("identityChanged")
                } else if identity == "unavailable" || id.is_none() {
                    Some("identityUnavailable")
                } else if !rules_ready {
                    Some("rulesUnavailable")
                } else {
                    None
                };
                let mut result = Self {
                    observation: Installations {
                        version: 5,
                        source: "moleListRegistryRootsVolumesAndReceipts",
                        revision: crate::engine::REVISION,
                        code: "unknown",
                        issue,
                        copies: vec![],
                        roots: crate::app_discovery::Observation::not_checked(),
                        volumes: crate::app_discovery::Observation::not_checked(),
                        receipts: crate::app_receipts::Observation::not_checked(),
                    },
                    candidates: vec![],
                };
                if issue.is_some() {
                    return result;
                }
                let id = id.unwrap();
                result.observation.code = "observed";
                let paths = match query(id) {
                    Ok(paths) => {
                        if paths.len() > LIMIT {
                            result.partial("limitReached");
                        }
                        paths
                    }
                    Err(()) => {
                        result.partial("registryUnavailable");
                        vec![]
                    }
                };
                let mole_paths = match mole_paths {
                    Some(paths) => {
                        if paths.len() > 64 {
                            result.partial("limitReached");
                        }
                        paths
                    }
                    None => {
                        result.partial("listUnavailable");
                        vec![]
                    }
                };
                let mut seen_paths = std::collections::HashSet::new();
                let mut seen_objects = std::collections::HashSet::new();
                seen_objects.insert((selected.metadata.dev(), selected.metadata.ino()));
                for path in paths
                    .into_iter()
                    .take(LIMIT)
                    .chain(mole_paths.into_iter().take(64))
                    .chain(root_paths.into_iter().take(384))
                {
                    let Some(path) = path else {
                        result.partial("candidateUnavailable");
                        continue;
                    };
                    if path == selected.path.to_string_lossy() || !seen_paths.insert(path.clone()) {
                        continue;
                    }
                    if path.chars().any(char::is_control)
                        || Path::new(&path)
                            .ancestors()
                            .skip(1)
                            .any(|p| p.extension().is_some_and(|s| s.eq_ignore_ascii_case("app")))
                    {
                        result.partial("candidateUnavailable");
                        continue;
                    }
                    let Ok(bundle) = Bundle::open(&path) else {
                        result.partial("candidateUnavailable");
                        continue;
                    };
                    if !seen_objects.insert((bundle.metadata.dev(), bundle.metadata.ino())) {
                        continue;
                    }
                    let Ok((evidence, bytes)) = Evidence::read(&bundle) else {
                        result.partial("candidateUnavailable");
                        continue;
                    };
                    let Ok(fields) = parse(&bytes) else {
                        result.partial("candidateUnavailable");
                        continue;
                    };
                    let copy = match fields.id {
                        Some(other) => crate::app_shared_rules::relation(
                            id,
                            &other,
                            selected.path.to_str().unwrap_or(""),
                            &path,
                        )
                        .map(|relation| InstalledCopy {
                            path,
                            bundle_id: other,
                            version: fields.version,
                            build: fields.build,
                            relation,
                        }),
                        None => {
                            result.partial("candidateUnavailable");
                            None
                        }
                    };
                    if copy.is_some()
                        && result
                            .candidates
                            .iter()
                            .filter(|c| c.copy.is_some())
                            .count()
                            >= 32
                    {
                        result.partial("limitReached");
                        break;
                    }
                    let candidate = Candidate {
                        path: bundle.path.clone(),
                        root_meta: bundle.metadata.clone(),
                        contents_meta: evidence.contents_meta.clone(),
                        plist_meta: evidence.plist_meta.clone(),
                        held: if copy.is_some() {
                            Some((bundle, evidence))
                        } else {
                            None
                        },
                        copy,
                    };
                    result.candidates.push(candidate);
                }
                result
            }
            fn partial(&mut self, issue: &'static str) {
                self.observation.code = "partial";
                self.observation.issue = Some(issue);
            }
            pub(super) fn finish(mut self) -> Installations {
                let candidates = std::mem::take(&mut self.candidates);
                for candidate in candidates {
                    if !candidate.verify() {
                        self.partial("candidatesChanged");
                        continue;
                    }
                    if let Some(copy) = candidate.copy {
                        self.observation.copies.push(copy);
                    }
                }
                self.observation.copies.sort_by(|a, b| a.path.cmp(&b.path));
                self.observation
            }
        }
        #[cfg(test)]
        mod tests {
            use super::*;
            fn fixture(root: &Path, name: &str, id: &str) -> PathBuf {
                let app = root.join(format!("{name}.app"));
                std::fs::create_dir_all(app.join("Contents")).unwrap();
                std::fs::write(app.join("Contents/Info.plist"), format!("<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>{id}</string><key>CFBundleShortVersionString</key><string>2.3</string></dict></plist>")).unwrap();
                app
            }
            fn value(check: Check) -> serde_json::Value {
                serde_json::to_value(check.finish()).unwrap()
            }
            #[test]
            fn root_only_candidates_are_verified_and_scope_changes_remain_partial() {
                let tmp = tempfile::tempdir().unwrap();
                let root = tmp.path().canonicalize().unwrap();
                let app = fixture(&root, "Selected", "org.example.app");
                let selected = Bundle::open(app.to_str().unwrap()).unwrap();
                let other = fixture(&root, "Other", "org.example.app.share");
                let scan = crate::app_discovery::mac::Check::for_test(std::slice::from_ref(&root));
                let check = Check::begin_all_sources(
                    &selected,
                    Some("org.example.app"),
                    "matches",
                    true,
                    |_| Ok(vec![]),
                    Some(vec![]),
                    scan.paths.clone(),
                );
                std::fs::write(root.join("neighbor"), "fixture").unwrap();
                let v = value(check);
                assert_eq!(v["copies"].as_array().unwrap().len(), 1);
                assert_eq!(v["copies"][0]["path"], other.to_str().unwrap());
                assert_eq!(scan.finish().issue, Some("changed"));
            }
            #[test]
            fn negative_snapshots_release_handles_and_recheck_changes_and_match_limits() {
                let tmp = tempfile::tempdir().unwrap();
                let root = tmp.path().canonicalize().unwrap();
                let app = fixture(&root, "Selected", "org.example.app");
                let selected = Bundle::open(app.to_str().unwrap()).unwrap();
                let other = fixture(&root, "Other", "org.unrelated.app");
                let check = Check::begin_sources(
                    &selected,
                    Some("org.example.app"),
                    "matches",
                    true,
                    |_| Ok(vec![Some(other.to_str().unwrap().into())]),
                    Some(vec![]),
                );
                assert!(check.candidates.iter().all(|c| c.held.is_none()));
                std::fs::write(other.join("Contents/Info.plist"), "changed").unwrap();
                assert_eq!(value(check)["issue"], "candidatesChanged");
                let paths = (0..33)
                    .map(|i| {
                        Some(
                            fixture(&root, &format!("Copy{i}"), "org.example.app")
                                .to_str()
                                .unwrap()
                                .to_owned(),
                        )
                    })
                    .collect();
                let v = value(Check::begin_all_sources(
                    &selected,
                    Some("org.example.app"),
                    "matches",
                    true,
                    |_| Ok(vec![]),
                    Some(vec![]),
                    paths,
                ));
                assert_eq!(v["copies"].as_array().unwrap().len(), 32);
                assert_eq!(v["issue"], "limitReached");
            }
            #[test]
            fn combines_bound_mole_paths_with_registry_and_explains_retention_matches() {
                let temp = tempfile::tempdir().unwrap();
                let root = temp.path().canonicalize().unwrap();
                let app = fixture(&root, "Example", "org.example.app");
                let selected = Bundle::open(app.to_str().unwrap()).unwrap();
                let derived = fixture(&root, "Extension", "org.example.app.share");
                let channel = fixture(&root, "Example Beta", "org.other.app");
                let unrelated = fixture(&root, "Other", "org.unrelated.app");
                let paths = [&app, &derived, &channel, &unrelated]
                    .map(|p| Some(p.to_str().unwrap().into()));
                let v = value(Check::begin_sources(
                    &selected,
                    Some("org.example.app"),
                    "matches",
                    true,
                    |_| Ok(vec![Some(derived.to_str().unwrap().into())]),
                    Some(paths.into()),
                ));
                assert_eq!(v["code"], "observed");
                assert_eq!(v["copies"].as_array().unwrap().len(), 2);
                assert!(v["copies"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|e| e["relation"] == "relatedIdentifier"));
                assert!(v["copies"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|e| e["relation"] == "channelName"));
            }
            #[test]
            fn source_failures_and_mole_limits_preserve_other_verified_results() {
                let temp = tempfile::tempdir().unwrap();
                let root = temp.path().canonicalize().unwrap();
                let app = fixture(&root, "Selected", "org.example.app");
                let selected = Bundle::open(app.to_str().unwrap()).unwrap();
                let other = fixture(&root, "Other", "org.example.app.share");
                let path = Some(other.to_str().unwrap().to_owned());
                let v = value(Check::begin_sources(
                    &selected,
                    Some("org.example.app"),
                    "matches",
                    true,
                    |_| Err(()),
                    Some(vec![path.clone()]),
                ));
                assert_eq!(v["issue"], "registryUnavailable");
                assert_eq!(v["copies"].as_array().unwrap().len(), 1);
                let v = value(Check::begin_sources(
                    &selected,
                    Some("org.example.app"),
                    "matches",
                    true,
                    |_| Ok(vec![path]),
                    None,
                ));
                assert_eq!(v["issue"], "listUnavailable");
                assert_eq!(v["copies"].as_array().unwrap().len(), 1);
                let v = value(Check::begin_sources(
                    &selected,
                    Some("org.example.app"),
                    "matches",
                    true,
                    |_| Ok(vec![]),
                    Some(vec![Some(app.to_str().unwrap().into()); 65]),
                ));
                assert_eq!(v["issue"], "limitReached");
                assert_eq!(v["code"], "partial");
                let v = value(Check::begin_sources(
                    &selected,
                    Some("org.example.app"),
                    "matches",
                    true,
                    |_| Ok(vec![]),
                    Some(vec![None]),
                ));
                assert_eq!(v["issue"], "candidateUnavailable");
            }
            #[test]
            fn verifies_ids_case_insensitively_deduplicates_and_keeps_nonrunning_copies() {
                let temp = tempfile::tempdir().unwrap();
                let root = temp.path().canonicalize().unwrap();
                let selected_path = fixture(&root, "Selected", "org.example.app");
                let selected = Bundle::open(selected_path.to_str().unwrap()).unwrap();
                let other = fixture(&root, "Other", "ORG.EXAMPLE.APP");
                let wrong = fixture(&root, "Wrong", "org.other.app");
                let paths = vec![
                    Some(selected_path.to_str().unwrap().into()),
                    Some(other.to_str().unwrap().into()),
                    Some(other.to_str().unwrap().into()),
                    Some(wrong.to_str().unwrap().into()),
                ];
                let v = value(Check::begin(
                    &selected,
                    Some("org.example.app"),
                    "matches",
                    true,
                    |_| Ok(paths),
                ));
                assert_eq!(v["code"], "observed");
                assert_eq!(v["copies"].as_array().unwrap().len(), 1);
                assert_eq!(v["copies"][0]["bundleId"], "ORG.EXAMPLE.APP");
                assert_eq!(v["copies"][0]["version"], "2.3");
            }
            #[test]
            fn stale_linked_nested_or_unreadable_candidates_keep_verified_copies_and_partial_scope()
            {
                use std::os::unix::fs::symlink;
                let temp = tempfile::tempdir().unwrap();
                let root = temp.path().canonicalize().unwrap();
                let app = fixture(&root, "Selected", "org.example.app");
                let selected = Bundle::open(app.to_str().unwrap()).unwrap();
                let good = fixture(&root, "Good", "org.example.app");
                let alias = root.join("Alias.app");
                symlink(&good, &alias).unwrap();
                let nested = fixture(&good, "Helper", "org.example.app");
                let bad = fixture(&root, "Bad", "org.example.app");
                std::fs::write(bad.join("Contents/Info.plist"), "bad").unwrap();
                let paths = [good, alias, nested, bad, root.join("Missing.app")]
                    .map(|p| Some(p.to_str().unwrap().into()));
                let v = value(Check::begin(
                    &selected,
                    Some("org.example.app"),
                    "matches",
                    true,
                    |_| Ok(paths.into()),
                ));
                assert_eq!(v["code"], "partial");
                assert_eq!(v["issue"], "candidateUnavailable");
                assert_eq!(v["copies"].as_array().unwrap().len(), 1);
            }
            #[test]
            fn candidate_changes_withdraw_only_affected_copies_and_recheck_negative_ids() {
                let temp = tempfile::tempdir().unwrap();
                let root = temp.path().canonicalize().unwrap();
                let app = fixture(&root, "Selected", "org.example.app");
                let selected = Bundle::open(app.to_str().unwrap()).unwrap();
                let good = fixture(&root, "Good", "org.example.app");
                let changed = fixture(&root, "Changed", "org.example.app");
                let wrong = fixture(&root, "Wrong", "org.other.app");
                let paths = [&good, &changed, &wrong].map(|p| Some(p.to_str().unwrap().into()));
                let check =
                    Check::begin(&selected, Some("org.example.app"), "matches", true, |_| {
                        Ok(paths.into())
                    });
                std::fs::write(changed.join("Contents/Info.plist"), "changed").unwrap();
                std::fs::write(wrong.join("Contents/Info.plist"), "changed").unwrap();
                let v = value(check);
                assert_eq!(v["issue"], "candidatesChanged");
                assert_eq!(v["copies"].as_array().unwrap().len(), 1);
            }
            #[test]
            fn missing_scope_and_limits_never_claim_exhaustive_absence_or_probe_untrusted_identity()
            {
                let temp = tempfile::tempdir().unwrap();
                let root = temp.path().canonicalize().unwrap();
                let app = fixture(&root, "Selected", "org.example.app");
                let selected = Bundle::open(app.to_str().unwrap()).unwrap();
                for (id, identity, rules, issue) in [
                    (
                        Some("org.example.app"),
                        "different",
                        true,
                        "identityChanged",
                    ),
                    (None, "unavailable", true, "identityUnavailable"),
                    (
                        Some("org.example.app"),
                        "matches",
                        false,
                        "rulesUnavailable",
                    ),
                ] {
                    let v = value(Check::begin(&selected, id, identity, rules, |_| {
                        panic!("must not query")
                    }));
                    assert_eq!(v["issue"], issue);
                    assert_eq!(v["code"], "unknown");
                }
                let v = value(Check::begin(
                    &selected,
                    Some("org.example.app"),
                    "matches",
                    true,
                    |_| Err(()),
                ));
                assert_eq!(v["issue"], "registryUnavailable");
                let v = value(Check::begin(
                    &selected,
                    Some("org.example.app"),
                    "unreported",
                    true,
                    |_| Ok(vec![]),
                ));
                assert_eq!(v["code"], "observed");
                assert!(v["copies"].as_array().unwrap().is_empty());
                let v = value(Check::begin(
                    &selected,
                    Some("org.example.app"),
                    "matches",
                    true,
                    |_| Ok(vec![Some(app.to_str().unwrap().into()); 33]),
                ));
                assert_eq!(v["issue"], "limitReached");
                assert_eq!(v["code"], "partial");
            }
        }
    }

    #[cfg(test)]
    fn inspect_context(
        path: &str,
        expected: &str,
        query: impl FnOnce(&str) -> Result<Vec<Process>, String>,
        hook: impl FnOnce(),
        home: &Path,
        rules_ready: bool,
    ) -> Result<Inspection, String> {
        inspect_context_with_registry(path, expected, query, hook, home, rules_ready, |_| {
            Ok(vec![])
        })
    }
    #[cfg(test)]
    fn inspect_context_with_registry(
        path: &str,
        expected: &str,
        query: impl FnOnce(&str) -> Result<Vec<Process>, String>,
        hook: impl FnOnce(),
        home: &Path,
        rules_ready: bool,
        registry: impl FnOnce(&str) -> Result<Vec<Option<String>>, ()>,
    ) -> Result<Inspection, String> {
        inspect_context_with_sources(
            path,
            expected,
            query,
            hook,
            home,
            rules_ready,
            registry,
            Some(vec![]),
            false,
        )
    }
    fn inspect_context_with_sources(
        path: &str,
        expected: &str,
        query: impl FnOnce(&str) -> Result<Vec<Process>, String>,
        hook: impl FnOnce(),
        home: &Path,
        rules_ready: bool,
        registry: impl FnOnce(&str) -> Result<Vec<Option<String>>, ()>,
        candidates: Option<Vec<Option<String>>>,
        scan_roots: bool,
    ) -> Result<Inspection, String> {
        let bundle = Bundle::open(path)?;
        let (evidence, bytes) = Evidence::read(&bundle)?;
        let fields = parse(&bytes)?;
        let identity = match fields.id.as_deref() {
            None => "unavailable",
            Some(_) if expected.is_empty() => "unreported",
            Some(id) if id == expected => "matches",
            Some(_) => "different",
        };
        let background = crate::app_background::mac::Check::begin(
            fields.background,
            identity,
            &bundle.path,
            &crate::app_background::mac::Scope::new(home),
            rules_ready,
        );
        let associations = crate::app_associations::mac::Check::begin(
            home,
            fields.id.as_deref(),
            identity,
            rules_ready,
        );
        let root_scan = (scan_roots
            && rules_ready
            && matches!(identity, "matches" | "unreported")
            && fields.id.is_some())
        .then(|| crate::app_discovery::mac::Check::begin(home));
        let volume_scan = root_scan
            .as_ref()
            .map(|_| crate::app_discovery::mac::Check::begin_volumes());
        let receipt_scan = root_scan
            .as_ref()
            .map(|_| crate::app_receipts::mac::Check::begin());
        let mut root_paths = root_scan
            .as_ref()
            .map(|check| check.paths.clone())
            .unwrap_or_default();
        root_paths.extend(
            volume_scan
                .as_ref()
                .map(|check| check.paths.clone())
                .unwrap_or_default(),
        );
        root_paths.extend(
            receipt_scan
                .as_ref()
                .map(|check| check.paths.clone())
                .unwrap_or_default(),
        );
        let installations = copies::Check::begin_all_sources(
            &bundle,
            fields.id.as_deref(),
            identity,
            rules_ready,
            registry,
            candidates,
            root_paths,
        );
        let running = match fields.id.as_deref() {
            _ if identity == "different" => {
                Running::unknown("本地应用标识与 Mole 清单不同，请重新读取列表。")
            }
            None => Running::unknown("本地应用标识无法读取，未查询运行实例。"),
            Some(id) => match query(id) {
                Ok(records) => classify(&bundle, id, &records),
                Err(reason) => Running::unknown(&reason),
            },
        };
        hook();
        let background = background.finish();
        let associations = associations.finish();
        let mut installations = installations.finish();
        installations.roots = root_scan
            .map(|scan| scan.finish())
            .unwrap_or_else(crate::app_discovery::Observation::not_checked);
        installations.volumes = volume_scan
            .map(|scan| scan.finish())
            .unwrap_or_else(crate::app_discovery::Observation::not_checked);
        installations.receipts = receipt_scan
            .map(|scan| scan.finish())
            .unwrap_or_else(crate::app_receipts::Observation::not_checked);
        if installations.receipts.code == "partial" && installations.code == "observed" {
            installations.code = "partial";
            installations.issue = Some("receiptsIncomplete");
        }
        if installations.volumes.code == "partial" && installations.code == "observed" {
            installations.code = "partial";
            installations.issue = Some("volumesIncomplete");
        }
        if installations.roots.code == "partial" && installations.code == "observed" {
            installations.code = "partial";
            installations.issue = Some("rootsIncomplete");
        }
        if !evidence.verify(&bundle) {
            return Err(CHANGED.into());
        }
        Ok(Inspection {
            protocol: 12,
            checked_at: crate::db::now(),
            bundle_id: fields.id,
            version: fields.version,
            build: fields.build,
            identity,
            running,
            protection: crate::app_protection::Protection::default(),
            mole_protection: crate::app_uninstall_rules::Observation::unknown("rulesUnavailable"),
            official_uninstaller: crate::app_uninstall_rules::OfficialObservation::unknown(
                "rulesUnavailable",
            ),
            background,
            associations,
            installations,
        })
    }
    #[cfg(test)]
    fn inspect_with(
        path: &str,
        expected: &str,
        query: impl FnOnce(&str) -> Result<Vec<Process>, String>,
        hook: impl FnOnce(),
    ) -> Result<Inspection, String> {
        inspect_context(
            path,
            expected,
            query,
            hook,
            Path::new("/unneeded-test-home"),
            true,
        )
    }
    pub(super) fn inspect(
        path: &str,
        expected: &str,
        home: &Path,
        rules_ready: bool,
        candidates: Option<Vec<Option<String>>>,
    ) -> Result<Inspection, String> {
        inspect_context_with_sources(
            path,
            expected,
            processes,
            || {},
            home,
            rules_ready,
            copies::registered,
            candidates,
            true,
        )
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        const ID: &str = "org.example.inspection";
        fn fixture(root: &Path, name: &str, body: &str) -> PathBuf {
            let app = root.join(format!("{name}.app"));
            std::fs::create_dir_all(app.join("Contents")).unwrap();
            std::fs::write(
                app.join("Contents/Info.plist"),
                format!(
                    "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>{body}</dict></plist>"
                ),
            )
            .unwrap();
            app
        }
        fn fields() -> String {
            format!("<key>CFBundleIdentifier</key><string>{ID}</string><key>CFBundleShortVersionString</key><string>2.3</string><key>CFBundleVersion</key><string>42</string>")
        }
        fn background_cases() -> Vec<(&'static str, crate::app_background::Marker)> {
            use crate::app_background::Marker::*;
            vec![
                ("", Absent),
                ("<true/>", Enabled),
                ("<false/>", Disabled),
                ("<integer>1</integer>", Enabled),
                ("<integer>0</integer>", Disabled),
                ("<integer>2</integer>", Disabled),
                ("<integer>-1</integer>", Disabled),
                ("<string>YES</string>", Enabled),
                ("<string>yes</string>", Enabled),
                ("<string>TRUE</string>", Enabled),
                ("<string>true</string>", Enabled),
                ("<string>1</string>", Enabled),
                ("<string>True</string>", Disabled),
                ("<string>false</string>", Disabled),
                ("<string></string>", Disabled),
                ("<string> true </string>", Disabled),
                ("<string>真</string>", Disabled),
                ("<real>1.0</real>", Unknown),
                ("<array><true/></array>", Unknown),
                ("<dict/>", Unknown),
                ("<data>MQ==</data>", Unknown),
                ("<string>true\n</string>", Unknown),
            ]
        }
        fn binary(bytes: &[u8]) -> Vec<u8> {
            autoreleasepool(|_| unsafe {
                let object =
                    NSPropertyListSerialization::propertyListWithData_options_format_error(
                        &NSData::with_bytes(bytes),
                        NSPropertyListReadOptions::Immutable,
                        std::ptr::null_mut(),
                    )
                    .unwrap();
                NSPropertyListSerialization::dataWithPropertyList_format_options_error(
                    &object,
                    objc2_foundation::NSPropertyListFormat::BinaryFormat_v1_0,
                    0,
                )
                .unwrap()
                .to_vec()
            })
        }
        #[test]
        fn background_metadata_preserves_bool_integer_and_string_types_in_xml_and_binary() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            for (index, (xml, expected)) in background_cases().iter().enumerate() {
                let body = format!(
                    "{}{}",
                    fields(),
                    if xml.is_empty() {
                        String::new()
                    } else {
                        format!("<key>LSBackgroundOnly</key>{xml}")
                    }
                );
                let app = fixture(&root, &format!("Marker{index}"), &body);
                let bytes = std::fs::read(app.join("Contents/Info.plist")).unwrap();
                assert_eq!(parse(&bytes).unwrap().background, *expected, "{xml}");
                assert_eq!(
                    parse(&binary(&bytes)).unwrap().background,
                    *expected,
                    "binary {xml}"
                );
            }
        }
        #[test]
        fn full_inspection_rechecks_registered_copy_evidence_and_keeps_selected_identity() {
            let temp = tempfile::tempdir().unwrap();
            let home = temp.path().canonicalize().unwrap();
            let app = fixture(&home, "SelectedCopy", &fields());
            let other = fixture(&home, "OtherCopy", &fields());
            let result = inspect_context_with_registry(
                app.to_str().unwrap(),
                ID,
                |_| Ok(vec![]),
                || {},
                &home,
                true,
                |_| Ok(vec![Some(other.to_str().unwrap().into())]),
            )
            .unwrap();
            assert_eq!(
                serde_json::to_value(result).unwrap()["installations"]["copies"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            let result = inspect_context_with_registry(
                app.to_str().unwrap(),
                ID,
                |_| Ok(vec![]),
                || {
                    std::fs::write(other.join("Contents/Info.plist"), "changed").unwrap();
                },
                &home,
                true,
                |_| Ok(vec![Some(other.to_str().unwrap().into())]),
            )
            .unwrap();
            let result = serde_json::to_value(result).unwrap();
            assert_eq!(result["installations"]["issue"], "candidatesChanged");
            assert!(result["installations"]["copies"]
                .as_array()
                .unwrap()
                .is_empty());
            assert_eq!(result["identity"], "matches");
            assert_eq!(result["version"], "2.3");
        }
        #[test]
        fn associations_are_bound_to_fresh_identity_and_withdraw_changed_paths() {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().canonicalize().unwrap();
            let app = fixture(&home, "Association", &fields());
            let cache = home.join(format!("Library/Caches/{ID}"));
            std::fs::create_dir_all(&cache).unwrap();
            let result = inspect_context(
                app.to_str().unwrap(),
                ID,
                |_| Ok(vec![]),
                || {},
                &home,
                true,
            )
            .unwrap();
            let result = serde_json::to_value(result).unwrap();
            assert_eq!(result["associations"]["entries"][1]["status"], "present");
            let result = inspect_context(
                app.to_str().unwrap(),
                ID,
                |_| Ok(vec![]),
                || {
                    std::fs::rename(&cache, home.join("retained-cache")).unwrap();
                    std::fs::create_dir(&cache).unwrap();
                },
                &home,
                true,
            )
            .unwrap();
            let result = serde_json::to_value(result).unwrap();
            assert_eq!(result["associations"]["entries"][1]["issue"], "changed");
            assert_eq!(result["identity"], "matches");
            assert_eq!(result["version"], "2.3");
            let result = inspect_context(
                app.to_str().unwrap(),
                "org.other.app",
                |_| panic!("changed identity must not query runtime"),
                || {},
                &home,
                true,
            )
            .unwrap();
            let result = serde_json::to_value(result).unwrap();
            assert_eq!(result["associations"]["issue"], "identityChanged");
            assert!(result["associations"]["entries"]
                .as_array()
                .unwrap()
                .is_empty());
        }
        #[test]
        fn background_context_keeps_local_identity_when_only_the_parent_observation_changes() {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().canonicalize().unwrap();
            let applications = home.join("Applications");
            std::fs::create_dir(&applications).unwrap();
            let app = fixture(
                &applications,
                "Background",
                &format!("{}<key>LSBackgroundOnly</key><true/>", fields()),
            );
            let result = inspect_context(
                app.to_str().unwrap(),
                ID,
                |_| Ok(vec![]),
                || {},
                &home,
                true,
            )
            .unwrap();
            let result = serde_json::to_value(result).unwrap();
            assert_eq!(result["background"]["code"], "directRoot");
            assert_eq!(result["background"]["rootKind"], "userApplications");
            let result = inspect_context(
                app.to_str().unwrap(),
                ID,
                |_| Ok(vec![]),
                || {
                    std::fs::write(applications.join("unrelated-neighbor"), "fixture").unwrap();
                },
                &home,
                true,
            )
            .unwrap();
            let result = serde_json::to_value(result).unwrap();
            assert_eq!(result["background"]["code"], "unknown");
            assert_eq!(result["background"]["issue"], "rootsChanged");
            assert_eq!(result["identity"], "matches");
            assert_eq!(result["version"], "2.3");
        }
        #[test]
        #[ignore = "Explicit read-only marker comparison with pinned function and isolated XML/binary fixtures"]
        fn prepared_background_marker_matches_mole_on_supported_types() {
            let engine = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
            crate::app_uninstall_rules::load(&engine).unwrap();
            let source = std::fs::read_to_string(engine.join("bin/uninstall.sh")).unwrap();
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            let mut cases = vec![];
            for (index, (xml, expected)) in background_cases().iter().enumerate() {
                if *expected == crate::app_background::Marker::Unknown {
                    continue;
                }
                let body = format!(
                    "{}{}",
                    fields(),
                    if xml.is_empty() {
                        String::new()
                    } else {
                        format!("<key>LSBackgroundOnly</key>{xml}")
                    }
                );
                let app = fixture(&root, &format!("Marker{index}"), &body);
                let bytes = std::fs::read(app.join("Contents/Info.plist")).unwrap();
                cases.push((app, *expected));
                let app = fixture(&root, &format!("Binary{index}"), &body);
                std::fs::write(app.join("Contents/Info.plist"), binary(&bytes)).unwrap();
                cases.push((app, *expected));
            }
            let script = format!("set -euo pipefail\n{}\nfor target do if uninstall_app_is_background_only \"$target\"; then printf '1\\n'; else status=$?; [[ $status == 1 ]] || exit 2; printf '0\\n'; fi; done",crate::app_background::test_function(&source,"uninstall_app_is_background_only"));
            let output = std::process::Command::new("/bin/bash")
                .env_clear()
                .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
                .env("HOME", &root)
                .env("TMPDIR", &root)
                .arg("-c")
                .arg(script)
                .arg("moomole-marker-test")
                .args(cases.iter().map(|(path, _)| path))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let actual = String::from_utf8(output.stdout).unwrap();
            assert_eq!(actual.lines().count(), cases.len());
            for ((path, expected), code) in cases.iter().zip(actual.lines()) {
                assert_eq!(
                    *expected == crate::app_background::Marker::Enabled,
                    code == "1",
                    "{}",
                    path.display()
                );
            }
            println!(
                "Compared {} typed XML/binary markers against pinned Mole",
                cases.len()
            );
        }
        fn process(path: &Path, pid: i32) -> Process {
            Process {
                path: Some(path.to_str().unwrap().into()),
                id: Some(ID.into()),
                pid,
                terminated: false,
            }
        }
        #[test]
        fn xml_and_binary_metadata_preserve_strings_and_unknown_fields_do_not_become_claims() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let app = fixture(&root, "身份 空格\n[*]", &fields());
            let result = inspect_with(app.to_str().unwrap(), ID, |_| Ok(vec![]), || {}).unwrap();
            assert_eq!(result.identity, "matches");
            assert_eq!(result.bundle_id.as_deref(), Some(ID));
            assert_eq!(result.version.as_deref(), Some("2.3"));
            assert_eq!(result.build.as_deref(), Some("42"));
            assert_eq!(result.running.code, "notObserved");
            autoreleasepool(|_| {
                let bytes = std::fs::read(app.join("Contents/Info.plist")).unwrap();
                let object = unsafe {
                    NSPropertyListSerialization::propertyListWithData_options_format_error(
                        &NSData::with_bytes(&bytes),
                        NSPropertyListReadOptions::Immutable,
                        std::ptr::null_mut(),
                    )
                }
                .unwrap();
                let binary = unsafe {
                    NSPropertyListSerialization::dataWithPropertyList_format_options_error(
                        &object,
                        objc2_foundation::NSPropertyListFormat::BinaryFormat_v1_0,
                        0,
                    )
                }
                .unwrap()
                .to_vec();
                assert!(binary.starts_with(b"bplist00"));
                assert_eq!(parse(&binary).unwrap().id.as_deref(), Some(ID));
            });
            let invalid=fixture(&root,"invalid","<key>CFBundleIdentifier</key><integer>17</integer><key>CFBundleVersion</key><true/>");
            let result = inspect_with(
                invalid.to_str().unwrap(),
                ID,
                |_| panic!("unknown ID must not query"),
                || {},
            )
            .unwrap();
            assert!(result.bundle_id.is_none());
            assert!(result.build.is_none());
            assert_eq!(result.running.code, "unknown");
            assert!(parse(b"not a plist").is_err());
            assert!(!valid_id("org.example\ncommand"));
        }
        #[test]
        fn plist_external_entities_and_metadata_hardlinks_are_not_followed() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let outside = root.join("outside.xml");
            std::fs::write(&outside, b"outside-marker").unwrap();
            let malicious=format!("<?xml version=\"1.0\"?><!DOCTYPE plist [<!ENTITY ext SYSTEM \"file://{}\">]><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>&ext;</string></dict></plist>",outside.display());
            assert!(parse(malicious.as_bytes()).is_err());
            let app = fixture(&root, "hardlink", &fields());
            let metadata = app.join("Contents/Info.plist");
            let saved = root.join("saved.plist");
            std::fs::rename(&metadata, &saved).unwrap();
            std::fs::hard_link(&saved, &metadata).unwrap();
            assert!(inspect_with(
                app.to_str().unwrap(),
                ID,
                |_| panic!("do not query linked metadata"),
                || {}
            )
            .unwrap_err()
            .contains("暂不支持"));
            assert_eq!(std::fs::read(outside).unwrap(), b"outside-marker");
        }

        #[test]
        fn identity_mismatch_skips_runtime_query_and_missing_mole_id_stays_unreported() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let app = fixture(&root, "test", &fields());
            let mismatch = inspect_with(
                app.to_str().unwrap(),
                "other.id",
                |_| panic!("mismatch must not query"),
                || {},
            )
            .unwrap();
            assert_eq!(mismatch.identity, "different");
            assert_eq!(mismatch.running.code, "unknown");
            let missing = inspect_with(app.to_str().unwrap(), "", |_| Ok(vec![]), || {}).unwrap();
            assert_eq!(missing.identity, "unreported");
            assert_eq!(missing.running.code, "notObserved");
        }
        #[test]
        fn running_matches_installation_location_not_just_bundle_id_and_retains_unknowns() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let app = fixture(&root, "first", &fields());
            let other = fixture(&root, "second", &fields());
            let bundle = Bundle::open(app.to_str().unwrap()).unwrap();
            let found = classify(
                &bundle,
                ID,
                &[
                    process(&app, 17),
                    process(&other, 20),
                    process(&other, 21),
                    process(&app, 17),
                ],
            );
            assert_eq!(found.code, "observed");
            assert_eq!(found.pids, vec![17]);
            assert_eq!(found.other_copies, 1);
            let only_other = classify(&bundle, ID, &[process(&other, 20)]);
            assert_eq!(only_other.code, "notObserved");
            assert_eq!(only_other.other_copies, 1);
            let mut invalid = process(&app, 0);
            assert_eq!(classify(&bundle, ID, &[invalid]).code, "unknown");
            invalid = process(&app, 17);
            invalid.path = None;
            assert_eq!(classify(&bundle, ID, &[invalid]).code, "unknown");
            invalid = process(&app, 17);
            invalid.id = Some("other.id".into());
            assert_eq!(classify(&bundle, ID, &[invalid]).code, "unknown");
            invalid = process(&app, 17);
            invalid.terminated = true;
            assert_eq!(classify(&bundle, ID, &[invalid]).code, "notObserved");
            let result = inspect_with(
                app.to_str().unwrap(),
                ID,
                |_| Err("source unavailable".into()),
                || {},
            )
            .unwrap();
            assert_eq!(result.running.code, "unknown");
        }
        #[test]
        fn linked_special_missing_and_excessive_metadata_are_refused_without_following_targets() {
            use std::os::unix::fs::symlink;
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let app = fixture(&root, "test", &fields());
            let link = root.join("link.app");
            symlink(&app, &link).unwrap();
            assert!(Bundle::open(link.to_str().unwrap()).is_err());
            let metadata = app.join("Contents/Info.plist");
            let outside = root.join("outside");
            std::fs::rename(&metadata, &outside).unwrap();
            symlink(&outside, &metadata).unwrap();
            assert!(inspect_with(app.to_str().unwrap(), ID, |_| panic!(), || {}).is_err());
            assert!(std::fs::read(&outside).unwrap().len() > 0);
            std::fs::remove_file(&metadata).unwrap();
            std::fs::write(&metadata, vec![b'x'; LIMIT as usize + 1]).unwrap();
            assert!(inspect_with(app.to_str().unwrap(), ID, |_| panic!(), || {}).is_err());
            std::fs::remove_file(&metadata).unwrap();
            let name = std::ffi::CString::new(metadata.to_str().unwrap()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            assert!(inspect_with(app.to_str().unwrap(), ID, |_| panic!(), || {}).is_err());
            assert!(Bundle::open("relative.app").is_err());
            assert!(Bundle::open(root.join("missing.app").to_str().unwrap()).is_err());
        }
        #[test]
        fn mutations_of_metadata_contents_or_the_bundle_withdraw_all_positive_evidence() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            for mode in ["file", "contents", "bundle"] {
                let app = fixture(&root, mode, &fields());
                let result = inspect_with(
                    app.to_str().unwrap(),
                    ID,
                    |_| Ok(vec![process(&app, 17)]),
                    || match mode {
                        "file" => {
                            std::fs::write(app.join("Contents/Info.plist"), b"changed").unwrap()
                        }
                        "contents" => {
                            std::fs::rename(app.join("Contents"), root.join("old-contents"))
                                .unwrap();
                            std::fs::create_dir(app.join("Contents")).unwrap();
                        }
                        _ => {
                            std::fs::rename(&app, root.join("old-bundle")).unwrap();
                            std::fs::create_dir(&app).unwrap();
                        }
                    },
                );
                assert_eq!(result.unwrap_err(), CHANGED);
            }
        }
    }
}
