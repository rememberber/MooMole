//! Main-ID ByHost files. Unlike embedded `${id}.*.plist`, Mole accepts `${id}.plist`.
use super::*;
const ROOT: &str = "Library/Preferences/ByHost";
fn matches(name: &str, id: &str) -> bool {
    name.starts_with(&format!("{id}.")) && name.ends_with(".plist")
}
pub(super) struct Check {
    state: ByHost,
    root: Probe,
    entries: Vec<Entry>,
    probes: Vec<Probe>,
}
impl Check {
    pub(super) fn begin(
        home: &Path,
        id: &str,
        depth: usize,
        pool: &mut HashMap<std::path::PathBuf, Arc<File>>,
    ) -> Self {
        let started = Instant::now();
        let (root, file) = Probe::directory(&home.join(ROOT), depth, pool);
        let mut check = Self {
            state: ByHost {
                code: "observed",
                issue: None,
                entries: 0,
                candidates: 0,
            },
            root,
            entries: vec![],
            probes: vec![],
        };
        if let Some(file) = file {
            check.scan(&file, id, started);
        } else if check.root.status == "unknown" {
            check.partial(check.root.issue.unwrap_or("unavailable"));
        }
        check
            .entries
            .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        for entry in &check.entries {
            if started.elapsed() > Duration::from_millis(500) {
                break;
            }
            let mut probe = Probe::begin(&home.join(&entry.relative_path), depth, pool);
            probe.require_type(libc::S_IFREG);
            check.probes.push(probe);
        }
        if check.probes.len() != check.entries.len() {
            check.partial("limitReached");
        }
        check
    }
    fn partial(&mut self, issue: &'static str) {
        self.state.code = "partial";
        if self.state.issue.is_none()
            || issue == "changed"
            || (issue == "limitReached" && self.state.issue != Some("changed"))
        {
            self.state.issue = Some(issue);
        }
    }
    fn scan(&mut self, file: &File, id: &str, started: Instant) {
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
            }
            self.partial("unavailable");
            return;
        }
        let stream = Stream(pointer);
        loop {
            if self.state.entries >= 1024
                || self.state.candidates >= 64
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
            self.state.entries += 1;
            let Ok(name) = name.to_str() else {
                self.partial("unsupported");
                continue;
            };
            if name.chars().any(char::is_control) {
                self.partial("unsupported");
                continue;
            }
            if !matches(name, id) {
                continue;
            }
            let Ok(Some(stat)) = stat_at(file, &CString::new(name).unwrap()) else {
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
            self.entries.push(Entry {
                kind: "byHost",
                relative_path: format!("{ROOT}/{name}"),
                status: "unknown",
                issue: Some("unavailable"),
                protection_reasons: vec![],
                mole_path_protection: crate::app_uninstall_rules::PathObservation::unknown(
                    "rulesUnavailable",
                ),
            });
            self.state.candidates += 1;
        }
    }
    pub(super) fn finish(mut self) -> (ByHost, Vec<Entry>) {
        let started = Instant::now();
        for (entry, probe) in self
            .entries
            .iter_mut()
            .zip(std::mem::take(&mut self.probes))
        {
            if started.elapsed() > Duration::from_millis(500) {
                break;
            }
            (entry.status, entry.issue) = probe.finish();
        }
        if self.entries.iter().any(|e| e.issue == Some("unavailable")) {
            self.partial("unavailable");
        }
        if started.elapsed() > Duration::from_millis(500) {
            self.partial("limitReached");
        }
        let (status, issue) = self.root.finish();
        if status == "unknown" {
            self.state.code = "partial";
            let issue = issue.unwrap_or("unavailable");
            if self.state.issue.is_none()
                || issue == "changed"
                || (issue == "limitReached" && self.state.issue != Some("changed"))
            {
                self.state.issue = Some(issue);
            }
        }
        if started.elapsed() > Duration::from_millis(500) && self.state.issue != Some("changed") {
            self.state.code = "partial";
            self.state.issue = Some("limitReached");
        }
        (self.state, self.entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};
    const ID: &str = "org.example.app";
    fn fixture() -> tempfile::TempDir {
        tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
    }
    fn check(home: &Path) -> Check {
        Check::begin(home, ID, home.components().count() - 1, &mut HashMap::new())
    }
    #[test]
    fn main_byhost_names_follow_prefix_rule_and_preserve_personal_settings() {
        let t = fixture();
        let root = t.path().join(ROOT);
        fs::create_dir_all(&root).unwrap();
        for name in [
            "org.example.app.plist",
            "org.example.app..plist",
            "org.example.app.DEVICE.plist",
            "org.example.apple.DEVICE.plist",
            "Org.example.app.DEVICE.plist",
            "TEAM.org.example.app.DEVICE.plist",
            "org.example.app.PLIST",
        ] {
            fs::write(root.join(name), "personal settings").unwrap();
        }
        fs::create_dir(root.join(format!("{ID}.directory.plist"))).unwrap();
        let (state, entries) = check(t.path()).finish();
        assert_eq!(state.code, "observed");
        assert_eq!(state.candidates, 3);
        assert!(entries
            .iter()
            .all(|e| e.status == "present" && e.kind == "byHost"));
        assert!(entries
            .iter()
            .any(|e| e.relative_path.ends_with("/org.example.app.plist")));
        assert_eq!(
            fs::read_to_string(root.join(format!("{ID}.DEVICE.plist"))).unwrap(),
            "personal settings"
        );
    }
    #[test]
    fn missing_roots_links_and_rebinding_keep_scope_or_locations_unknown() {
        let t = fixture();
        fs::create_dir_all(t.path().join("Library/Preferences")).unwrap();
        let initial = check(t.path());
        fs::create_dir(t.path().join(ROOT)).unwrap();
        assert_eq!(initial.finish().0.issue, Some("changed"));
        let path = t.path().join(ROOT).join(format!("{ID}.DEVICE.plist"));
        symlink("/tmp", &path).unwrap();
        let (state, entries) = check(t.path()).finish();
        assert_eq!(state.issue, Some("unsupported"));
        assert_eq!(entries[0].status, "unknown");
        fs::remove_file(&path).unwrap();
        fs::write(&path, "original").unwrap();
        let initial = check(t.path());
        fs::rename(&path, path.with_extension("saved")).unwrap();
        fs::write(&path, "replacement").unwrap();
        let (state, entries) = initial.finish();
        assert_eq!(state.issue, Some("changed"));
        assert_eq!(entries[0].issue, Some("changed"));
    }
    #[test]
    fn entry_candidate_and_elapsed_budgets_never_claim_complete_scope() {
        let t = fixture();
        let root = t.path().join(ROOT);
        fs::create_dir_all(&root).unwrap();
        for i in 0..65 {
            fs::write(root.join(format!("{ID}.{i}.plist")), "keep").unwrap();
        }
        let (state, entries) = check(t.path()).finish();
        assert_eq!(state.candidates, 64);
        assert_eq!(entries.len(), 64);
        assert_eq!(state.issue, Some("limitReached"));
        let t = fixture();
        let root = t.path().join(ROOT);
        fs::create_dir_all(&root).unwrap();
        for i in 0..1025 {
            fs::write(root.join(format!("other{i}")), "").unwrap();
        }
        let (state, entries) = check(t.path()).finish();
        assert_eq!(state.entries, 1024);
        assert!(entries.is_empty());
        assert_eq!(state.issue, Some("limitReached"));
        let mut initial = check(t.path());
        initial.state = ByHost::not_checked();
        initial.partial("unavailable");
        let (_, file) = Probe::directory(
            &root,
            t.path().components().count() - 1,
            &mut HashMap::new(),
        );
        initial.scan(&file.unwrap(), ID, Instant::now() - Duration::from_secs(1));
        assert_eq!(initial.state.issue, Some("limitReached"));
    }
    #[test]
    fn added_exact_paths_enforce_upstream_types_and_receive_latest_client_guards() {
        let t = fixture();
        let home = t.path();
        for (kind, root, suffix) in super::super::LOCATIONS.iter().skip(16) {
            let path = home.join(format!("{root}/{ID}{suffix}"));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if *kind == "recentDocuments" {
                fs::write(&path, "personal recent list").unwrap();
            } else {
                fs::create_dir(&path).unwrap();
            }
        }
        let file = home.join(ROOT).join(format!("{ID}.plist"));
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "device settings").unwrap();
        let mut result =
            crate::app_associations::mac::Check::begin(home, Some(ID), "matches", true).finish();
        result
            .attach_protection(
                home,
                Some(ID),
                "org.client.app",
                &[home.join("Library/Preferences").to_str().unwrap().into()],
            )
            .unwrap();
        assert!(result
            .entries
            .iter()
            .skip(16)
            .all(|e| e.status == "present"));
        assert!(result
            .entries
            .last()
            .unwrap()
            .protection_reasons
            .contains(&"clientDirectory"));
        let recent = result
            .entries
            .iter()
            .find(|e| e.relative_path.ends_with(".sfl2"))
            .unwrap()
            .relative_path
            .clone();
        assert_eq!(
            fs::read_to_string(home.join(&recent)).unwrap(),
            "personal recent list"
        );
        fs::remove_file(home.join(&recent)).unwrap();
        fs::create_dir(home.join(&recent)).unwrap();
        let prefs = home.join(format!("Library/Preferences/{ID}"));
        fs::remove_dir(&prefs).unwrap();
        fs::write(&prefs, "wrong type").unwrap();
        let result =
            crate::app_associations::mac::Check::begin(home, Some(ID), "matches", true).finish();
        for path in [&recent, &format!("Library/Preferences/{ID}")] {
            let entry = result
                .entries
                .iter()
                .find(|e| &e.relative_path == path)
                .unwrap();
            assert_eq!(entry.issue, Some("unsupported"));
        }
    }
    #[test]
    #[ignore = "requires fixed Mole source; compares isolated original ByHost find/filter only"]
    fn prepared_main_byhost_names_match_original_filter_including_unsuffixed_plist() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        crate::app_uninstall_rules::load(&resources).unwrap();
        let source = fs::read_to_string(resources.join("lib/core/app_protection.sh")).unwrap();
        assert!(source.contains("-name \"*.plist\" -print0"));
        assert!(
            source.contains("mole_name_starts_with_bundle_id_boundary \"$pref\" \"$bundle_id\"")
        );
        let base = fs::read_to_string(resources.join("lib/core/base.sh")).unwrap();
        let mut script = String::from("set -euo pipefail\n");
        for name in [
            "mole_is_reverse_dns_bundle_id",
            "mole_name_starts_with_bundle_id_boundary",
        ] {
            script.push_str(&crate::app_background::test_function(&base, name));
            script.push('\n');
        }
        script.push_str("bundle_id=$1; while IFS= read -r -d '' pref; do if mole_name_starts_with_bundle_id_boundary \"$pref\" \"$bundle_id\"; then printf '%s\\0' \"${pref##*/}\"; fi; done < <(find \"$2\" -maxdepth 1 -type f -name \"*.plist\" -print0)");
        let t = fixture();
        let root = t.path().join(ROOT);
        fs::create_dir_all(&root).unwrap();
        for name in [
            format!("{ID}.plist"),
            format!("{ID}..plist"),
            format!("{ID}.中文 DEVICE.plist"),
            format!("{ID}le.A.plist"),
            format!("{ID}.A.PLIST"),
            format!("TEAM.{ID}.A.plist"),
            format!("{}.A.plist", ID.to_uppercase()),
        ] {
            fs::write(root.join(name), "keep").unwrap();
        }
        let output = std::process::Command::new("/bin/bash")
            .args(["-c", &script, "main-byhost", ID, root.to_str().unwrap()])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        assert!(output.status.success());
        let mut expected = output
            .stdout
            .split(|b| *b == 0)
            .filter(|b| !b.is_empty())
            .map(|b| std::str::from_utf8(b).unwrap().to_owned())
            .collect::<Vec<_>>();
        expected.sort();
        let actual = check(t.path())
            .finish()
            .1
            .into_iter()
            .map(|e| e.relative_path.rsplit('/').next().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        eprintln!(
            "{} main ByHost candidates matched original filter",
            actual.len()
        );
    }
}
