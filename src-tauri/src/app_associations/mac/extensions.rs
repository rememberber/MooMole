//! Exact embedded-ID data locations plus bounded ByHost metadata, all retained.
use super::*;
const LOCATIONS: [(&str, &str, &str); 8] = [
    ("scripts", "Library/Application Scripts", ""),
    (
        "fileProvider",
        "Library/Application Support/FileProvider",
        "",
    ),
    ("cache", "Library/Caches", ""),
    ("container", "Library/Containers", ""),
    ("httpStorage", "Library/HTTPStorages", ""),
    ("httpCookies", "Library/HTTPStorages", ".binarycookies"),
    ("preferences", "Library/Preferences", ".plist"),
    ("webKit", "Library/WebKit", ""),
];
fn by_host_matches(name: &str, id: &str) -> bool {
    name.strip_prefix(&format!("{id}."))
        .and_then(|rest| rest.strip_suffix(".plist"))
        .is_some()
}
pub struct Check {
    observation: ExtensionData,
    probes: Vec<Probe>,
    root: Option<Probe>,
}
fn add(entries: &mut Vec<ExtensionEntry>, relative_path: String, id: &str, kind: &'static str) {
    let source = ExtensionSource {
        bundle_id: id.into(),
        kind,
    };
    if let Some(entry) = entries
        .iter_mut()
        .find(|e| e.relative_path == relative_path)
    {
        entry.sources.push(source);
    } else {
        entries.push(ExtensionEntry {
            relative_path,
            sources: vec![source],
            status: "unknown",
            issue: Some("unavailable"),
            protection_reasons: vec![],
            mole_path_protection: crate::app_uninstall_rules::PathObservation::unknown(
                "rulesUnavailable",
            ),
        });
    }
}
impl Check {
    pub fn begin(
        home: &Path,
        mut ids: Vec<String>,
        code: &'static str,
        issue: Option<&'static str>,
    ) -> Self {
        let mut check = Self {
            observation: ExtensionData::empty(
                if code == "partial" {
                    "partial"
                } else {
                    "observed"
                },
                if code == "partial" {
                    Some("embeddedIncomplete")
                } else {
                    None
                },
            ),
            probes: vec![],
            root: None,
        };
        if code == "notChecked" {
            check.observation = ExtensionData::empty("notChecked", issue);
            return check;
        }
        if home
            .to_str()
            .is_none_or(|s| s.chars().any(char::is_control))
            || home == Path::new("/")
            || !home.is_absolute()
            || home.as_os_str().len() > 3072
            || home.components().count() > 64
            || home
                .components()
                .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
        {
            check.observation = ExtensionData::empty("notChecked", Some("homeUnavailable"));
            return check;
        }
        if ids.len() > 32
            || ids
                .iter()
                .any(|id| !valid_id(id) || id.starts_with("org.sparkle-project."))
        {
            check.observation = ExtensionData::empty("notChecked", Some("embeddedUnavailable"));
            return check;
        }
        ids.sort();
        ids.dedup();
        if ids.len() > 8 {
            ids.truncate(8);
            check.observation.code = "partial";
            check.observation.issue = Some("limitReached");
        }
        check.observation.identifiers = ids;
        if check.observation.identifiers.is_empty() {
            return check;
        }
        let depth = home
            .components()
            .filter(|c| matches!(c, Component::Normal(_)))
            .count();
        let mut pool = HashMap::new();
        for id in &check.observation.identifiers {
            for (kind, root, suffix) in LOCATIONS {
                add(
                    &mut check.observation.entries,
                    format!("{root}/{id}{suffix}"),
                    id,
                    kind,
                );
            }
        }
        // Freeze exact templates before discovery; both are revalidated at finish.
        let started = Instant::now();
        for entry in &check.observation.entries {
            if started.elapsed() > Duration::from_millis(500) {
                check.observation = ExtensionData::empty("unknown", Some("limitReached"));
                return check;
            }
            check.probes.push(Probe::begin(
                &home.join(&entry.relative_path),
                depth,
                &mut pool,
            ));
        }
        let root = "Library/Preferences/ByHost";
        let (probe, file) = Probe::directory(&home.join(root), depth, &mut pool);
        check.observation.by_host.code = "observed";
        if let Some(file) = file {
            check.scan(&file, started);
        } else if probe.status == "unknown" {
            check.partial(probe.issue.unwrap_or("unavailable"));
        }
        check.root = Some(probe);
        let exact = check.probes.len();
        check.observation.entries[exact..].sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        for entry in &check.observation.entries[exact..] {
            if started.elapsed() > Duration::from_millis(500) {
                check.observation = ExtensionData::empty("unknown", Some("limitReached"));
                return check;
            }
            let mut probe = Probe::begin(&home.join(&entry.relative_path), depth, &mut pool);
            if probe.status == "present"
                && probe
                    .links
                    .last()
                    .and_then(|link| link.value.as_ref())
                    .is_some_and(|s| s.st_mode & libc::S_IFMT != libc::S_IFREG)
            {
                probe.status = "unknown";
                probe.issue = Some("unsupported");
            }
            check.probes.push(probe);
        }
        check
    }
    fn partial(&mut self, issue: &'static str) {
        self.observation.by_host.code = "partial";
        if self.observation.by_host.issue.is_none() || issue == "changed" {
            self.observation.by_host.issue = Some(issue);
        }
        if self.observation.code == "observed" {
            self.observation.code = "partial";
            self.observation.issue = Some("byHostIncomplete");
        }
    }
    fn scan(&mut self, file: &File, started: Instant) {
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
        let pointer = unsafe { libc::fdopendir(fd) };
        if pointer.is_null() {
            unsafe {
                libc::close(fd);
            };
            self.partial("unavailable");
            return;
        }
        let stream = Stream(pointer);
        loop {
            if self.observation.by_host.entries >= 1024
                || self.observation.by_host.candidates >= 64
                || started.elapsed() > Duration::from_millis(500)
            {
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
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if matches!(name.to_bytes(), b"." | b"..") {
                continue;
            }
            self.observation.by_host.entries += 1;
            let Ok(name) = name.to_str() else {
                self.partial("unsupported");
                continue;
            };
            if name.chars().any(char::is_control) {
                self.partial("unsupported");
                continue;
            }
            let ids = self
                .observation
                .identifiers
                .iter()
                .filter(|id| by_host_matches(name, id))
                .cloned()
                .collect::<Vec<_>>();
            if ids.is_empty() {
                continue;
            }
            let name_c = CString::new(name).unwrap();
            let Ok(Some(stat)) = stat_at(file, &name_c) else {
                self.partial("unavailable");
                continue;
            };
            let kind = stat.st_mode & libc::S_IFMT;
            if kind != libc::S_IFREG && kind != libc::S_IFLNK {
                continue;
            }
            if kind == libc::S_IFLNK {
                self.partial("unsupported");
            }
            let path = format!("Library/Preferences/ByHost/{name}");
            for id in ids {
                add(&mut self.observation.entries, path.clone(), &id, "byHost");
            }
            self.observation.by_host.candidates += 1;
        }
    }
    pub fn finish(mut self) -> ExtensionData {
        let started = Instant::now();
        for (entry, probe) in self
            .observation
            .entries
            .iter_mut()
            .zip(std::mem::take(&mut self.probes))
        {
            if started.elapsed() > Duration::from_millis(500) {
                return ExtensionData::empty("unknown", Some("limitReached"));
            }
            (entry.status, entry.issue) = probe.finish();
        }
        if let Some(root) = self.root.take() {
            let (status, issue) = root.finish();
            if status == "unknown" {
                self.partial(issue.unwrap_or("unavailable"));
            }
        }
        self.observation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    const ID: &str = "org.example.extension";
    fn fixture() -> tempfile::TempDir {
        tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
    }
    fn observe(home: &Path, ids: Vec<String>) -> ExtensionData {
        Check::begin(home, ids, "observed", None).finish()
    }
    #[test]
    fn exact_templates_collisions_byhost_boundaries_and_protection_preserve_data() {
        let temp = fixture();
        let home = temp.path();
        for (_, root, suffix) in LOCATIONS {
            let path = home.join(format!("{root}/{ID}{suffix}"));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "personal data").unwrap();
        }
        let root = home.join("Library/Preferences/ByHost");
        fs::create_dir_all(&root).unwrap();
        for name in [
            format!("{ID}.DEVICE.plist"),
            format!("{ID}..plist"),
            format!("{ID}.binarycookies.DEVICE.plist"),
            format!("{ID}.plist"),
            format!("{ID}le.DEVICE.plist"),
            format!("{}.DEVICE.plist", ID.to_uppercase()),
        ] {
            fs::write(root.join(name), "device settings").unwrap();
        }
        fs::create_dir(root.join(format!("{ID}.DIRECTORY.plist"))).unwrap();
        let mut result = observe(
            home,
            vec![ID.into(), format!("{ID}.binarycookies"), ID.into()],
        );
        assert_eq!(result.code, "observed");
        assert_eq!(result.identifiers.len(), 2);
        assert_eq!(result.by_host.candidates, 3);
        assert_eq!(result.entries.len(), 18); // 15 unique exact + 3 ByHost.
        let collision = result
            .entries
            .iter()
            .find(|e| e.relative_path == format!("Library/HTTPStorages/{ID}.binarycookies"))
            .unwrap();
        assert_eq!(collision.sources.len(), 2);
        let multi = result
            .entries
            .iter()
            .find(|e| e.relative_path.ends_with(".binarycookies.DEVICE.plist"))
            .unwrap();
        assert_eq!(multi.sources.len(), 2);
        result
            .attach_protection(
                home,
                Some("org.example.main"),
                &format!("{ID}.binarycookies"),
                &[home
                    .join("Library/Application Scripts")
                    .to_str()
                    .unwrap()
                    .into()],
            )
            .unwrap();
        assert!(result
            .entries
            .iter()
            .find(|e| e.relative_path == format!("Library/Application Scripts/{ID}"))
            .unwrap()
            .protection_reasons
            .contains(&"clientDirectory"));
        assert!(result
            .entries
            .iter()
            .find(|e| e.relative_path.ends_with(".binarycookies.DEVICE.plist"))
            .unwrap()
            .protection_reasons
            .contains(&"currentApplication"));
        assert!(result.entries.iter().any(
            |e| e.status == "notFound" && e.protection_reasons.contains(&"currentApplication")
        ));
        result
            .attach_protection(home, Some("org.example.main"), "org.client.app", &[])
            .unwrap();
        assert!(result
            .entries
            .iter()
            .all(|e| e.protection_reasons.is_empty()));
        assert_eq!(
            fs::read_to_string(home.join(format!("Library/Caches/{ID}"))).unwrap(),
            "personal data"
        );
    }
    #[test]
    fn source_withdrawal_and_byhost_changes_remain_independent_of_valid_exact_locations() {
        let temp = fixture();
        let home = temp.path();
        fs::create_dir_all(home.join(format!("Library/Caches/{ID}"))).unwrap();
        fs::create_dir_all(home.join("Library/Preferences")).unwrap();
        let check = Check::begin(home, vec![ID.into()], "observed", None);
        fs::create_dir_all(home.join("Library/Preferences/ByHost")).unwrap();
        let mut result = check.finish();
        assert_eq!(result.by_host.issue, Some("changed"));
        assert_eq!(result.code, "partial");
        assert_eq!(
            result
                .entries
                .iter()
                .find(|e| e.relative_path == format!("Library/Caches/{ID}"))
                .unwrap()
                .status,
            "present"
        );
        result.bind(&[], "partial", Some("changed"));
        assert_eq!(result.code, "unknown");
        assert_eq!(result.issue, Some("embeddedChanged"));
        assert!(result.entries.is_empty());
        assert!(result.identifiers.is_empty());
        let mut result = observe(home, vec![ID.into()]);
        result.bind(&[], "partial", Some("unavailable"));
        assert_eq!(result.issue, Some("embeddedUnavailable"));
        assert!(result.entries.is_empty());
        let mut result = observe(home, vec![ID.into()]);
        result.bind(&[ID.into()], "partial", Some("unsupported"));
        assert_eq!(result.issue, Some("embeddedIncomplete"));
        assert_eq!(result.entries.len(), 8);
        let result = Check::begin(home, vec![], "notChecked", Some("rulesUnavailable")).finish();
        assert_eq!(result.code, "notChecked");
        assert_eq!(result.by_host.code, "notChecked");
    }
    #[test]
    fn byhost_links_and_replaced_files_never_gain_file_eligibility() {
        let temp = fixture();
        let home = temp.path();
        let root = home.join("Library/Preferences/ByHost");
        fs::create_dir_all(&root).unwrap();
        let path = root.join(format!("{ID}.DEVICE.plist"));
        std::os::unix::fs::symlink("/tmp", &path).unwrap();
        let result = observe(home, vec![ID.into()]);
        assert_eq!(result.by_host.issue, Some("unsupported"));
        assert_eq!(result.entries.last().unwrap().status, "unknown");
        fs::remove_file(&path).unwrap();
        fs::write(&path, "keep").unwrap();
        let check = Check::begin(home, vec![ID.into()], "observed", None);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let result = check.finish();
        assert_eq!(result.by_host.issue, Some("changed"));
        assert_eq!(result.entries.last().unwrap().issue, Some("changed"));
    }
    #[test]
    fn identifier_entry_and_candidate_budgets_keep_scope_partial() {
        let temp = fixture();
        let home = temp.path();
        let result = observe(
            home,
            (0..9).map(|i| format!("org.example.ext{i}")).collect(),
        );
        assert_eq!(result.identifiers.len(), 8);
        assert_eq!(result.entries.len(), 64);
        assert_eq!(result.issue, Some("limitReached"));
        let root = home.join("Library/Preferences/ByHost");
        fs::create_dir_all(&root).unwrap();
        for i in 0..65 {
            fs::write(root.join(format!("{ID}.{i}.plist")), "keep").unwrap();
        }
        let result = observe(home, vec![ID.into()]);
        assert_eq!(result.by_host.candidates, 64);
        assert_eq!(result.by_host.issue, Some("limitReached"));
        let temp = fixture();
        let home = temp.path();
        let root = home.join("Library/Preferences/ByHost");
        fs::create_dir_all(&root).unwrap();
        for i in 0..1025 {
            fs::write(root.join(format!("other{i}")), "").unwrap();
        }
        let result = observe(home, vec![ID.into()]);
        assert_eq!(result.by_host.entries, 1024);
        assert_eq!(result.by_host.candidates, 0);
        assert_eq!(result.by_host.issue, Some("limitReached"));
        assert_eq!(
            observe(Path::new("/"), vec![ID.into()]).issue,
            Some("homeUnavailable")
        );
        assert_eq!(observe(home, vec!["bad_id".into()]).code, "notChecked");
        let mut check = Check::begin(home, vec![ID.into()], "observed", None);
        let mut pool = HashMap::new();
        let (_, file) = Probe::directory(&root, home.components().count() - 1, &mut pool);
        check.observation.by_host = ByHost::not_checked();
        check.scan(&file.unwrap(), Instant::now() - Duration::from_secs(1));
        assert_eq!(check.observation.by_host.issue, Some("limitReached"));
    }
    #[test]
    #[ignore = "requires pinned Mole source; reads isolated fixtures and original find name expression only"]
    fn prepared_extension_templates_and_byhost_pattern_match_original_mole() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        crate::app_uninstall_rules::load(&root).unwrap();
        let source = fs::read_to_string(root.join("lib/core/app_protection.sh")).unwrap();
        let block = source
            .split("for embedded_candidate in ")
            .nth(1)
            .unwrap()
            .split("; do")
            .next()
            .unwrap();
        for (_, root, suffix) in LOCATIONS {
            assert!(block.contains(&format!("\"$HOME/{root}/$embedded_id{suffix}\"")));
        }
        assert_eq!(block.matches("$HOME/").count(), 8);
        let marker = "-name \"${embedded_id}.*.plist\" -print0";
        assert!(source.contains(marker));
        let temp = fixture();
        let names = [
            format!("{ID}.A.plist"),
            format!("{ID}..plist"),
            format!("{ID}.plist"),
            format!("{ID}le.A.plist"),
            format!("{ID}.A.PLIST"),
            format!("{ID}.nested.A.plist"),
        ];
        for name in &names {
            fs::write(temp.path().join(name), "").unwrap();
        }
        let script = format!("embedded_id=\"$1\"; find \"$2\" -maxdepth 1 -type f {marker}");
        let output = std::process::Command::new("/bin/bash")
            .args(["-c", &script, "byhost", ID, temp.path().to_str().unwrap()])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        assert!(output.status.success());
        let mut actual = output
            .stdout
            .split(|b| *b == 0)
            .filter(|b| !b.is_empty())
            .map(|b| {
                Path::new(std::str::from_utf8(b).unwrap())
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned()
            })
            .collect::<Vec<_>>();
        let mut expected = names
            .into_iter()
            .filter(|name| by_host_matches(name, ID))
            .collect::<Vec<_>>();
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected);
    }
}
