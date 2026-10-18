//! An intentionally partial, read-only view of exact-ID Mole candidate locations.
//! Presence is not ownership; every location remains retained, including caches.
use serde::Serialize;
use std::path::Path;

const LOCATIONS: [(&str, &str, &str); 8] = [
    ("support", "Library/Application Support", ""),
    ("cache", "Library/Caches", ""),
    ("logs", "Library/Logs", ""),
    ("preferences", "Library/Preferences", ".plist"),
    (
        "savedState",
        "Library/Saved Application State",
        ".savedState",
    ),
    ("container", "Library/Containers", ""),
    ("webKit", "Library/WebKit", ""),
    ("httpStorage", "Library/HTTPStorages", ""),
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    version: u32,
    revision: &'static str,
    code: &'static str,
    issue: Option<&'static str>,
    entries: Vec<Entry>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    kind: &'static str,
    relative_path: String,
    status: &'static str,
    issue: Option<&'static str>,
}
impl Observation {
    fn unknown(issue: &'static str) -> Self {
        Self {
            version: 1,
            revision: crate::engine::REVISION,
            code: "unknown",
            issue: Some(issue),
            entries: vec![],
        }
    }
}

// Same accepted grammar as Mole's mole_is_reverse_dns_bundle_id; not proof of publisher.
fn valid_id(id: &str) -> bool {
    id.len() <= 255
        && id.contains('.')
        && id.split('.').all(|part| {
            !part.is_empty()
                && part.as_bytes()[0].is_ascii_alphanumeric()
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

#[cfg(target_os = "macos")]
pub mod mac {
    use super::*;
    use std::{
        ffi::CString,
        fs::File,
        os::fd::{AsRawFd, FromRawFd},
        path::Component,
        sync::Arc,
    };
    const SF_DATALESS: u32 = 0x40000000;
    #[derive(PartialEq, Eq)]
    struct Stamp(u64, u64, u16, u16, u32, i64, i64, i64, i64, i64);
    fn stamp(s: &libc::stat) -> Stamp {
        Stamp(
            s.st_dev as u64,
            s.st_ino,
            s.st_mode,
            s.st_nlink,
            s.st_flags,
            s.st_size,
            s.st_mtime,
            s.st_mtime_nsec,
            s.st_ctime,
            s.st_ctime_nsec,
        )
    }
    fn same(a: &libc::stat, b: &libc::stat, strict: bool) -> bool {
        if strict {
            stamp(a) == stamp(b)
        } else {
            (a.st_dev, a.st_ino, a.st_mode, a.st_flags)
                == (b.st_dev, b.st_ino, b.st_mode, b.st_flags)
        }
    }
    fn stat_at(parent: &File, name: &CString) -> Result<Option<libc::stat>, ()> {
        let mut out = std::mem::MaybeUninit::<libc::stat>::zeroed();
        if unsafe {
            libc::fstatat(
                parent.as_raw_fd(),
                name.as_ptr(),
                out.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0
        {
            Ok(Some(unsafe { out.assume_init() }))
        } else if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
            Ok(None)
        } else {
            Err(())
        }
    }
    fn fstat(file: &File) -> Result<libc::stat, ()> {
        let mut out = std::mem::MaybeUninit::<libc::stat>::zeroed();
        if unsafe { libc::fstat(file.as_raw_fd(), out.as_mut_ptr()) } != 0 {
            return Err(());
        }
        Ok(unsafe { out.assume_init() })
    }
    fn local(file: &File) -> bool {
        let mut out = std::mem::MaybeUninit::<libc::statfs>::zeroed();
        unsafe {
            libc::fstatfs(file.as_raw_fd(), out.as_mut_ptr()) == 0
                && out.assume_init().f_flags & libc::MNT_LOCAL as u32 != 0
        }
    }
    struct Link {
        parent: Arc<File>,
        name: CString,
        value: Option<libc::stat>,
        held: Option<Arc<File>>,
        strict: bool,
    }
    struct Probe {
        links: Vec<Link>,
        status: &'static str,
        issue: Option<&'static str>,
    }
    impl Probe {
        fn begin(path: &Path, home_depth: usize) -> Self {
            let mut probe = Self {
                links: vec![],
                status: "unknown",
                issue: Some("unavailable"),
            };
            let fd = unsafe {
                libc::open(
                    c"/".as_ptr(),
                    libc::O_RDONLY
                        | libc::O_CLOEXEC
                        | libc::O_DIRECTORY
                        | libc::O_NOFOLLOW
                        | libc::O_NONBLOCK,
                )
            };
            if fd < 0 {
                return probe;
            }
            let mut parent = Arc::new(unsafe { File::from_raw_fd(fd) });
            if !local(&parent) {
                return probe;
            }
            let parts: Vec<_> = path
                .components()
                .filter_map(|v| match v {
                    Component::Normal(v) => v.to_str(),
                    _ => None,
                })
                .collect();
            for (index, part) in parts.iter().enumerate() {
                let Ok(name) = CString::new(*part) else {
                    return probe;
                };
                let Ok(value) = stat_at(&parent, &name) else {
                    return probe;
                };
                let mut link = Link {
                    parent: parent.clone(),
                    name,
                    value,
                    held: None,
                    strict: index + 1 >= home_depth,
                };
                let Some(s) = link.value.as_ref() else {
                    probe.links.push(link);
                    probe.status = "notFound";
                    probe.issue = None;
                    return probe;
                };
                let last = index + 1 == parts.len();
                let kind = s.st_mode & libc::S_IFMT;
                if s.st_flags & SF_DATALESS != 0
                    || (!last && kind != libc::S_IFDIR)
                    || (last && (kind != libc::S_IFDIR && kind != libc::S_IFREG))
                    || (kind == libc::S_IFREG && s.st_nlink != 1)
                    || (last && fstat(&parent).map_or(true, |p| p.st_dev != s.st_dev))
                {
                    probe.links.push(link);
                    probe.issue = Some("unsupported");
                    return probe;
                }
                if !last {
                    let fd = unsafe {
                        libc::openat(
                            parent.as_raw_fd(),
                            link.name.as_ptr(),
                            libc::O_RDONLY
                                | libc::O_CLOEXEC
                                | libc::O_DIRECTORY
                                | libc::O_NOFOLLOW
                                | libc::O_NONBLOCK,
                        )
                    };
                    if fd < 0 {
                        probe.links.push(link);
                        return probe;
                    }
                    let file = Arc::new(unsafe { File::from_raw_fd(fd) });
                    if !local(&file)
                        || fstat(&file).map_or(true, |opened| !same(&opened, s, link.strict))
                    {
                        probe.links.push(link);
                        return probe;
                    }
                    link.held = Some(file.clone());
                    parent = file;
                }
                probe.links.push(link);
            }
            probe.status = "present";
            probe.issue = None;
            probe
        }
        fn finish(self) -> (&'static str, Option<&'static str>) {
            for link in self.links {
                let Ok(current) = stat_at(&link.parent, &link.name) else {
                    return ("unknown", Some("unavailable"));
                };
                if match (current.as_ref(), link.value.as_ref()) {
                    (Some(a), Some(b)) => !same(a, b, link.strict),
                    (None, None) => false,
                    _ => true,
                } || link.held.as_ref().is_some_and(|file| {
                    !local(file)
                        || fstat(file).map_or(true, |s| {
                            link.value
                                .as_ref()
                                .is_none_or(|v| !same(&s, v, link.strict))
                        })
                }) {
                    return ("unknown", Some("changed"));
                }
            }
            (self.status, self.issue)
        }
    }
    pub struct Check {
        observation: Observation,
        probes: Vec<Probe>,
    }
    impl Check {
        pub fn begin(home: &Path, id: Option<&str>, identity: &str, rules_ready: bool) -> Self {
            let issue = if identity == "different" {
                Some("identityChanged")
            } else if identity == "unavailable" || id.is_none() {
                Some("identityUnavailable")
            } else if !rules_ready {
                Some("rulesUnavailable")
            } else if !valid_id(id.unwrap()) {
                Some("bundleIdUnsupported")
            } else if home.to_str().is_none()
                || home == Path::new("/")
                || !home.is_absolute()
                || home.as_os_str().len() > 3072
                || home.components().count() > 64
                || home
                    .components()
                    .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
            {
                Some("homeUnavailable")
            } else {
                None
            };
            if let Some(issue) = issue {
                return Self {
                    observation: Observation::unknown(issue),
                    probes: vec![],
                };
            }
            let id = id.unwrap();
            let mut entries = vec![];
            let mut probes = vec![];
            for (kind, root, suffix) in LOCATIONS {
                let relative_path = format!("{root}/{id}{suffix}");
                probes.push(Probe::begin(
                    &home.join(&relative_path),
                    home.components()
                        .filter(|c| matches!(c, Component::Normal(_)))
                        .count(),
                ));
                entries.push(Entry {
                    kind,
                    relative_path,
                    status: "unknown",
                    issue: Some("unavailable"),
                });
            }
            Self {
                observation: Observation {
                    version: 1,
                    revision: crate::engine::REVISION,
                    code: "observed",
                    issue: None,
                    entries,
                },
                probes,
            }
        }
        pub fn finish(mut self) -> Observation {
            for (entry, probe) in self.observation.entries.iter_mut().zip(self.probes) {
                (entry.status, entry.issue) = probe.finish();
            }
            self.observation
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{fs, os::unix::fs::symlink};
        fn fixture() -> tempfile::TempDir {
            tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
        }
        fn observe(home: &Path) -> serde_json::Value {
            serde_json::to_value(
                Check::begin(home, Some("org.example.app"), "matches", true).finish(),
            )
            .unwrap()
        }
        #[test]
        fn exact_locations_do_not_enumerate_similar_or_shared_names() {
            let temp = fixture();
            let home = temp.path();
            fs::create_dir_all(home.join("Library/Caches/org.example.app.other")).unwrap();
            fs::create_dir_all(home.join("Library/Application Support/org.example.app")).unwrap();
            fs::create_dir_all(home.join("Library/Preferences")).unwrap();
            fs::write(
                home.join("Library/Preferences/org.example.app.plist"),
                "personal",
            )
            .unwrap();
            let v = observe(home);
            assert_eq!(v["entries"].as_array().unwrap().len(), 8);
            assert_eq!(v["entries"][0]["status"], "present");
            assert_eq!(v["entries"][1]["status"], "notFound");
            assert_eq!(v["entries"][3]["status"], "present");
            assert_eq!(
                fs::read_to_string(home.join("Library/Preferences/org.example.app.plist")).unwrap(),
                "personal"
            );
        }
        #[test]
        fn links_special_files_and_replacements_stay_unknown() {
            let temp = fixture();
            let home = temp.path();
            fs::create_dir_all(home.join("Library/Caches")).unwrap();
            symlink("/tmp", home.join("Library/Caches/org.example.app")).unwrap();
            assert_eq!(observe(home)["entries"][1]["issue"], "unsupported");
            let check = Check::begin(home, Some("org.example.app"), "matches", true);
            fs::rename(home.join("Library"), home.join("OldLibrary")).unwrap();
            fs::create_dir(home.join("Library")).unwrap();
            assert!(serde_json::to_value(check.finish()).unwrap()["entries"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e["status"] == "unknown"));
            fs::remove_dir(home.join("Library")).unwrap();
            symlink(home.join("OldLibrary"), home.join("Library")).unwrap();
            assert_eq!(observe(home)["entries"][0]["issue"], "unsupported");
        }
        #[test]
        fn hard_links_fifo_and_unreadable_ancestors_are_not_reported_missing() {
            use std::os::unix::fs::PermissionsExt;
            let temp = fixture();
            let home = temp.path();
            fs::create_dir_all(home.join("Library/Caches")).unwrap();
            fs::create_dir_all(home.join("Library/Preferences")).unwrap();
            let pref = home.join("Library/Preferences/org.example.app.plist");
            fs::write(&pref, "retained").unwrap();
            fs::hard_link(&pref, home.join("other-pref")).unwrap();
            assert_eq!(observe(home)["entries"][3]["issue"], "unsupported");
            let fifo = CString::new(
                home.join("Library/Caches/org.example.app")
                    .to_str()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
            assert_eq!(observe(home)["entries"][1]["issue"], "unsupported");
            if unsafe { libc::geteuid() } != 0 {
                fs::set_permissions(
                    home.join("Library/Caches"),
                    fs::Permissions::from_mode(0o000),
                )
                .unwrap();
                assert_eq!(observe(home)["entries"][1]["status"], "unknown");
                fs::set_permissions(
                    home.join("Library/Caches"),
                    fs::Permissions::from_mode(0o700),
                )
                .unwrap();
            }
            assert_eq!(fs::read_to_string(pref).unwrap(), "retained");
        }
        #[test]
        fn missing_paths_are_rechecked_and_untrusted_identity_never_probes() {
            let temp = fixture();
            let home = temp.path();
            let check = Check::begin(home, Some("org.example.app"), "matches", true);
            fs::create_dir(home.join("Library")).unwrap();
            assert_eq!(
                serde_json::to_value(check.finish()).unwrap()["entries"][0]["issue"],
                "changed"
            );
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
                (
                    Some("org.example_app"),
                    "matches",
                    true,
                    "bundleIdUnsupported",
                ),
            ] {
                let check = Check::begin(home, id, identity, rules);
                assert!(check.probes.is_empty());
                assert_eq!(
                    serde_json::to_value(check.finish()).unwrap()["issue"],
                    issue
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reverse_dns_boundaries() {
        for id in ["org.example.app", "1.a-b", "COM.Example.App"] {
            assert!(valid_id(id));
        }
        for id in [
            "", "unknown", "app", ".a", "a.", "a..b", "a._b", "a.-b", "a.b/c", "a.*", "a.é",
        ] {
            assert!(!valid_id(id));
        }
    }
    #[test]
    #[ignore = "requires the prepared, fixed Mole source"]
    fn exact_templates_are_in_pinned_mole_discovery() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        crate::app_uninstall_rules::load(&root).unwrap();
        let source = std::fs::read_to_string(root.join("lib/core/app_protection.sh")).unwrap();
        for (_, directory, suffix) in LOCATIONS {
            assert!(
                source.contains(&format!("$HOME/{directory}/$bundle_id{suffix}"))
                    || source.contains(&format!("~/{}/\"$bundle_id\"{}", directory, suffix))
            );
        }
        let base = std::fs::read_to_string(root.join("lib/core/base.sh")).unwrap();
        let script = format!("{}\nfor id do if mole_is_reverse_dns_bundle_id \"$id\"; then printf '1\\n'; else printf '0\\n'; fi; done", crate::app_background::test_function(&base, "mole_is_reverse_dns_bundle_id"));
        let ids = [
            "org.example.app",
            "1.a-b",
            "COM.Example.App",
            "",
            "unknown",
            "app",
            ".a",
            "a.",
            "a..b",
            "a._b",
            "a.-b",
            "a.b/c",
            "a.*",
            "a.é",
        ];
        let output = std::process::Command::new("/bin/bash")
            .arg("-c")
            .arg(script)
            .arg("association-id-check")
            .args(ids)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        assert!(output.status.success());
        let expected: String = ids
            .iter()
            .map(|id| if valid_id(id) { "1\n" } else { "0\n" })
            .collect();
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }
}
