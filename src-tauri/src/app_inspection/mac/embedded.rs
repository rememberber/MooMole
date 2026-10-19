//! Read-only subset of the pinned Mole embedded-ID discovery, never ownership.
use super::*;
use std::{ffi::CStr, os::fd::IntoRawFd, time::Instant};

fn reverse_dns(id: &str) -> bool {
    id.len() <= 255
        && id.contains('.')
        && id.split('.').all(|p| {
            !p.is_empty()
                && p.as_bytes()[0].is_ascii_alphanumeric()
                && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}
fn kind(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next()?;
    match name.rsplit_once('.')?.1.to_ascii_lowercase().as_str() {
        "appex" => Some("appex"),
        "xpc" => Some("xpc"),
        "app" if path.starts_with("Contents/Library/LoginItems/") => Some("loginItem"),
        _ => None,
    }
}
fn signature(m: &Metadata) -> ((u64, u64, u64, i64, i64, i64, i64), u32, u32, u32, u32) {
    (stamp(m), m.mode(), m.uid(), m.gid(), m.st_flags())
}
struct EvidenceItem {
    path: String,
    metadata: Metadata,
    directory: bool,
}
pub(super) struct Check<'a> {
    root: &'a File,
    root_meta: Metadata,
    observation: Embedded,
    evidence: Vec<EvidenceItem>,
    started: Instant,
    bytes: u64,
}
impl<'a> Check<'a> {
    pub(super) fn begin(
        bundle: &'a Bundle,
        primary: Option<&str>,
        identity: &str,
        ready: bool,
    ) -> Self {
        let issue = if identity == "different" {
            Some("identityChanged")
        } else if identity == "unavailable" || primary.is_none() {
            Some("identityUnavailable")
        } else if !ready {
            Some("rulesUnavailable")
        } else if !reverse_dns(primary.unwrap()) {
            Some("bundleIdUnsupported")
        } else {
            None
        };
        let mut check = Self {
            root: &bundle.root,
            root_meta: bundle.metadata.clone(),
            observation: Embedded {
                version: 1,
                revision: crate::engine::REVISION,
                code: if issue.is_some() {
                    "notChecked"
                } else {
                    "observed"
                },
                issue,
                entries: 0,
                directories: 0,
                plists: 0,
                bundles: vec![],
            },
            evidence: vec![],
            started: Instant::now(),
            bytes: 0,
        };
        if issue.is_none() {
            match child(&check.root, c"Contents", true) {
                Ok(file) => check.walk(file, "Contents".into(), 0, primary.unwrap()),
                Err(_) => check.partial("unavailable"),
            }
        }
        check
    }
    fn partial(&mut self, issue: &'static str) {
        self.observation.code = "partial";
        if self.observation.issue.is_none() || issue == "changed" {
            self.observation.issue = Some(issue);
        }
    }
    fn exhausted(&self) -> bool {
        self.observation.entries >= 2048
            || self.observation.directories >= 128
            || self.observation.plists >= 128
            || self.observation.bundles.len() >= 32
            || self.started.elapsed() > Duration::from_millis(500)
    }
    fn walk(&mut self, file: File, path: String, depth: usize, primary: &str) {
        if self.exhausted() {
            self.partial("limitReached");
            return;
        }
        let Ok(metadata) = file.metadata() else {
            self.partial("unavailable");
            return;
        };
        let mut fs = std::mem::MaybeUninit::<libc::statfs>::zeroed();
        if metadata.st_flags() & SF_DATALESS != 0
            || unsafe { libc::fstatfs(file.as_raw_fd(), fs.as_mut_ptr()) } != 0
            || unsafe { fs.assume_init() }.f_flags & libc::MNT_LOCAL as u32 == 0
        {
            self.partial("unsupported");
            return;
        }
        self.observation.directories += 1;
        self.evidence.push(EvidenceItem {
            path: path.clone(),
            metadata,
            directory: true,
        });
        struct Stream(*mut libc::DIR);
        impl Drop for Stream {
            fn drop(&mut self) {
                unsafe {
                    libc::closedir(self.0);
                }
            }
        }
        let Ok(clone) = file.try_clone() else {
            self.partial("unavailable");
            return;
        };
        let fd = clone.into_raw_fd();
        let raw = unsafe { libc::fdopendir(fd) };
        if raw.is_null() {
            unsafe {
                libc::close(fd);
            };
            self.partial("unavailable");
            return;
        }
        let stream = Stream(raw);
        loop {
            if self.exhausted() {
                self.partial("limitReached");
                break;
            }
            unsafe {
                *libc::__error() = 0;
            }
            let entry = unsafe { libc::readdir(stream.0) };
            if entry.is_null() {
                if unsafe { *libc::__error() } != 0 {
                    self.partial("unavailable");
                }
                break;
            }
            let raw_name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if matches!(raw_name.to_bytes(), b"." | b"..") {
                continue;
            }
            self.observation.entries += 1;
            let Ok(name) = raw_name.to_str() else {
                self.partial("unsupported");
                continue;
            };
            if name.chars().any(char::is_control) {
                self.partial("unsupported");
                continue;
            }
            let relative = format!("{path}/{name}");
            if relative.len() > 1024 {
                self.partial("unsupported");
                continue;
            }
            let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
            if unsafe {
                libc::fstatat(
                    file.as_raw_fd(),
                    raw_name.as_ptr(),
                    stat.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } != 0
            {
                self.partial("unavailable");
                continue;
            }
            let stat = unsafe { stat.assume_init() };
            match stat.st_mode & libc::S_IFMT {
                libc::S_IFDIR => {
                    if depth + 1 >= 12 {
                        self.partial("limitReached");
                        continue;
                    }
                    match child(&file, raw_name, true) {
                        Ok(next) => self.walk(next, relative, depth + 1, primary),
                        Err(_) => self.partial("unavailable"),
                    }
                }
                libc::S_IFREG
                    if name == "Info.plist" && path.ends_with("/Contents")
                        || relative == "Contents/Info.plist" =>
                {
                    self.observation.plists += 1;
                    let Some(bundle_path) = relative.strip_suffix("/Contents/Info.plist") else {
                        continue;
                    };
                    let Some(bundle_kind) = kind(bundle_path) else {
                        continue;
                    };
                    self.read(
                        &file,
                        raw_name,
                        &relative,
                        bundle_path,
                        bundle_kind,
                        primary,
                    );
                }
                libc::S_IFLNK => self.partial("unsupported"),
                libc::S_IFREG => {}
                _ => self.partial("unsupported"),
            }
        }
    }
    fn read(
        &mut self,
        parent: &File,
        name: &CStr,
        path: &str,
        bundle_path: &str,
        kind: &'static str,
        primary: &str,
    ) {
        let Ok(file) = child(parent, name, false) else {
            self.partial("unavailable");
            return;
        };
        let Ok(metadata) = file.metadata() else {
            self.partial("unavailable");
            return;
        };
        self.evidence.push(EvidenceItem {
            path: path.into(),
            metadata: metadata.clone(),
            directory: false,
        });
        if metadata.len() == 0 || metadata.len() > LIMIT {
            self.partial("unsupported");
            return;
        }
        if self.bytes + metadata.len() > 4 * LIMIT {
            self.partial("limitReached");
            return;
        }
        let mut bytes = vec![];
        let read_limit = LIMIT.min(4 * LIMIT - self.bytes);
        if (&file)
            .take(read_limit + 1)
            .read_to_end(&mut bytes)
            .is_err()
        {
            self.partial("unavailable");
            return;
        }
        self.bytes += bytes.len() as u64;
        if bytes.len() as u64 > read_limit {
            self.partial("limitReached");
            return;
        }
        if file
            .metadata()
            .map_or(true, |m| signature(&m) != signature(&metadata))
        {
            self.partial("changed");
            return;
        }
        let Some(id) = parse(&bytes)
            .ok()
            .and_then(|f| f.id)
            .filter(|s| reverse_dns(s))
        else {
            self.partial("unsupported");
            return;
        };
        if id == primary || id.starts_with("org.sparkle-project.") {
            return;
        }
        self.observation.bundles.push(EmbeddedBundle {
            relative_path: bundle_path.into(),
            bundle_id: id,
            kind,
        });
    }
    pub(super) fn basis(&self) -> (Vec<String>, &'static str, Option<&'static str>) {
        (
            self.observation
                .bundles
                .iter()
                .map(|b| b.bundle_id.clone())
                .collect(),
            self.observation.code,
            self.observation.issue,
        )
    }
    pub(super) fn finish(mut self) -> Embedded {
        if self.observation.code == "notChecked" {
            return self.observation;
        }
        let started = Instant::now();
        for item in &self.evidence {
            if started.elapsed() > Duration::from_millis(500) {
                self.observation.code = "partial";
                self.observation.issue = Some("limitReached");
                self.observation.bundles.clear();
                break;
            }
            let result = (|| {
                let mut parent = self.root.try_clone().map_err(|_| ())?;
                let parts: Vec<_> = item.path.split('/').collect();
                for (index, name) in parts.iter().enumerate() {
                    let name = std::ffi::CString::new(*name).map_err(|_| ())?;
                    parent = child(&parent, &name, index + 1 < parts.len() || item.directory)
                        .map_err(|_| ())?;
                }
                parent.metadata().map_err(|_| ())
            })();
            let issue = match result {
                Ok(ref m) if signature(m) == signature(&item.metadata) => None,
                Ok(_) => Some("changed"),
                Err(_) => Some("unavailable"),
            };
            if let Some(issue) = issue {
                self.observation.code = "partial";
                self.observation.issue = Some(issue);
                self.observation.bundles.clear();
                break;
            }
        }
        if self
            .root
            .metadata()
            .map_or(true, |m| signature(&m) != signature(&self.root_meta))
        {
            self.partial("changed");
            self.observation.bundles.clear();
        }
        if self.observation.issue == Some("changed") {
            self.observation.bundles.clear();
        }
        self.observation
            .bundles
            .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        self.observation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let app = temp.path().canonicalize().unwrap().join("Selected.app");
        write(&app, "org.example.main");
        (temp, app)
    }
    fn write(bundle: &Path, id: &str) {
        fs::create_dir_all(bundle.join("Contents")).unwrap();
        fs::write(bundle.join("Contents/Info.plist"),format!("<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>{id}</string></dict></plist>")).unwrap();
    }
    fn populate(app: &Path) {
        for (path, id) in [
            ("Contents/PlugIns/Share.appex", "org.example.share"),
            ("Contents/XPCServices/Service.XPC", "org.vendor.service"),
            ("Contents/Library/LoginItems/Helper.App", "org.vendor.login"),
            ("Contents/Other.app", "org.vendor.notlogin"),
            ("Contents/Frameworks/Test.framework", "org.vendor.framework"),
            ("Contents/PlugIns/Primary.appex", "org.example.main"),
            (
                "Contents/XPCServices/Sparkle.xpc",
                "org.sparkle-project.helper",
            ),
        ] {
            write(&app.join(path), id);
        }
    }
    fn observe(app: &Path) -> Embedded {
        let bundle = Bundle::open(app.to_str().unwrap()).unwrap();
        Check::begin(&bundle, Some("org.example.main"), "matches", true).finish()
    }
    #[test]
    fn eligible_types_exact_exclusions_and_binary_metadata_are_preserved() {
        let (_temp, app) = fixture();
        populate(&app);
        let info = app.join("Contents/XPCServices/Service.XPC/Contents/Info.plist");
        let bytes = fs::read(&info).unwrap();
        let binary = autoreleasepool(|_| unsafe {
            let object = NSPropertyListSerialization::propertyListWithData_options_format_error(
                &NSData::with_bytes(&bytes),
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
        });
        fs::write(info, binary).unwrap();
        let result = observe(&app);
        assert_eq!(result.code, "observed");
        assert_eq!(result.bundles.len(), 3);
        assert_eq!(result.plists, 8);
        assert_eq!(
            result.bundles.iter().map(|b| b.kind).collect::<Vec<_>>(),
            ["loginItem", "appex", "xpc"]
        );
        assert_eq!(result.bundles[2].bundle_id, "org.vendor.service");
        // Duplicate IDs at distinct embedded paths remain distinct observations.
        write(
            &app.join("Contents/PlugIns/Other.appex"),
            "org.example.share",
        );
        assert_eq!(observe(&app).bundles.len(), 4);
    }
    #[test]
    fn changes_links_and_malformed_metadata_never_prove_absence_or_ownership() {
        let (_temp, app) = fixture();
        populate(&app);
        let bundle = Bundle::open(app.to_str().unwrap()).unwrap();
        let check = Check::begin(&bundle, Some("org.example.main"), "matches", true);
        fs::write(
            app.join("Contents/PlugIns/Share.appex/Contents/Info.plist"),
            "changed",
        )
        .unwrap();
        let result = check.finish();
        assert_eq!(result.code, "partial");
        assert_eq!(result.issue, Some("changed"));
        assert!(result.bundles.is_empty());
        populate(&app);
        let check = Check::begin(&bundle, Some("org.example.main"), "matches", true);
        fs::create_dir(app.join("Contents/new-directory")).unwrap();
        assert!(check.finish().bundles.is_empty());
        std::os::unix::fs::symlink("/Applications", app.join("Contents/Linked")).unwrap();
        assert_eq!(observe(&app).issue, Some("unsupported"));
        fs::remove_file(app.join("Contents/Linked")).unwrap();
        let info = app.join("Contents/PlugIns/Share.appex/Contents/Info.plist");
        fs::hard_link(&info, app.join("Contents/extra-link")).unwrap();
        assert_eq!(observe(&app).issue, Some("unavailable"));
        fs::remove_file(app.join("Contents/extra-link")).unwrap();
        fs::write(info, "invalid").unwrap();
        let result = observe(&app);
        assert_eq!(result.issue, Some("unsupported"));
        assert_eq!(result.bundles.len(), 2); // Independent, unchanged identities remain visible.
    }
    #[test]
    fn budgets_and_unsupported_identity_skip_or_remain_partial() {
        let (_temp, app) = fixture();
        for i in 0..33 {
            write(
                &app.join(format!("Contents/P{i}.appex")),
                &format!("org.example.extension{i}"),
            );
        }
        let result = observe(&app);
        assert_eq!(result.bundles.len(), 32);
        assert_eq!(result.issue, Some("limitReached"));
        let bundle = Bundle::open(app.to_str().unwrap()).unwrap();
        for (id, identity, ready, issue) in [
            (
                Some("org.example.main"),
                "different",
                true,
                "identityChanged",
            ),
            (None, "unavailable", true, "identityUnavailable"),
            (
                Some("org.example.main"),
                "matches",
                false,
                "rulesUnavailable",
            ),
            (Some("org.bad_id"), "matches", true, "bundleIdUnsupported"),
        ] {
            let result = Check::begin(&bundle, id, identity, ready).finish();
            assert_eq!(result.code, "notChecked");
            assert_eq!(result.issue, Some(issue));
            assert_eq!(result.entries, 0);
            assert_eq!(result.directories, 0);
        }
        let (_temp, app) = fixture();
        for i in 0..2049 {
            fs::write(app.join(format!("Contents/file{i}")), "").unwrap();
        }
        let result = observe(&app);
        assert_eq!(result.entries, 2048);
        assert_eq!(result.issue, Some("limitReached"));
        let (_temp, app) = fixture();
        for i in 0..129 {
            fs::create_dir(app.join(format!("Contents/d{i}"))).unwrap();
        }
        let result = observe(&app);
        assert_eq!(result.directories, 128);
        assert_eq!(result.issue, Some("limitReached"));
        let (_temp, app) = fixture();
        for i in 0..50 {
            write(
                &app.join(format!("Contents/B{i}.framework")),
                "org.example.framework",
            );
        }
        // Injectable counters exercise the next-iteration plist and evidence budgets.
        let bundle = Bundle::open(app.to_str().unwrap()).unwrap();
        let mut check = Check::begin(&bundle, Some("org.example.main"), "matches", true);
        check.observation.plists = 128;
        check.walk(
            child(&bundle.root, c"Contents", true).unwrap(),
            "Contents".into(),
            0,
            "org.example.main",
        );
        assert_eq!(check.observation.issue, Some("limitReached"));
        let (_temp, app) = fixture();
        let deep = app.join("Contents").join(vec!["level"; 12].join("/"));
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(observe(&app).issue, Some("limitReached"));
        let bundle = Bundle::open(app.to_str().unwrap()).unwrap();
        let mut check = Check::begin(&bundle, Some("org.example.main"), "matches", true);
        check.started = Instant::now() - Duration::from_secs(1);
        check.walk(
            child(&bundle.root, c"Contents", true).unwrap(),
            "Contents".into(),
            0,
            "org.example.main",
        );
        assert_eq!(check.observation.issue, Some("limitReached"));
    }
    #[test]
    fn file_and_aggregate_read_budgets_are_bounded() {
        let (_temp, app) = fixture();
        let oversized = app.join("Contents/Huge.appex");
        write(&oversized, "org.example.huge");
        fs::write(
            oversized.join("Contents/Info.plist"),
            vec![b' '; LIMIT as usize + 1],
        )
        .unwrap();
        assert_eq!(observe(&app).issue, Some("unsupported"));
        fs::remove_dir_all(oversized).unwrap();
        for i in 0..5 {
            let extension = app.join(format!("Contents/P{i}.appex"));
            write(&extension, &format!("org.example.ext{i}"));
            let path = extension.join("Contents/Info.plist");
            let mut bytes = fs::read(&path).unwrap();
            bytes.extend(vec![b' '; 900_000]);
            fs::write(path, bytes).unwrap();
        }
        let bundle = Bundle::open(app.to_str().unwrap()).unwrap();
        let check = Check::begin(&bundle, Some("org.example.main"), "matches", true);
        assert!(check.bytes <= 4 * LIMIT + 1);
        assert_eq!(check.finish().issue, Some("limitReached"));
    }
    #[test]
    #[ignore = "requires prepared pinned Mole source and local plutil; reads isolated fixtures only"]
    fn prepared_original_embedded_ids_match_the_supported_subset() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        crate::app_uninstall_rules::load(&root).unwrap();
        let source = fs::read_to_string(root.join("lib/core/app_protection.sh")).unwrap();
        let base = fs::read_to_string(root.join("lib/core/base.sh")).unwrap();
        let (_temp, app) = fixture();
        populate(&app);
        let script=format!("set -euo pipefail\nMOLE_TIMEOUT_DISK_VERIFY_SEC=5\ncreate_temp_file() {{ mktemp; }}\n_mole_uninstall_materialize_find0() {{ local output=\"$1\"; shift; find \"$@\" > \"$output\"; }}\n{}\n{}\n_mole_uninstall_embedded_bundle_ids \"$1\" \"$2\"",
            crate::app_background::test_function(&base,"mole_is_reverse_dns_bundle_id"),
            crate::app_background::test_function(&source,"_mole_uninstall_embedded_bundle_ids"));
        let output = std::process::Command::new("/bin/bash")
            .args([
                "-c",
                &script,
                "embedded",
                app.to_str().unwrap(),
                "org.example.main",
            ])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .env("TMPDIR", app.parent().unwrap())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut expected = observe(&app)
            .bundles
            .into_iter()
            .map(|b| b.bundle_id)
            .collect::<Vec<_>>();
        expected.sort();
        expected.dedup();
        assert_eq!(
            String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            expected
        );
    }
}
