//! Client supplement: pinned Mole name boundaries applied to verified embedded IDs.
//! A directory-name candidate never certifies group membership or ownership.
use super::*;
const ROOT: &str = "Library/Group Containers";
pub(super) struct Check {
    observation: GroupDiscovery,
    root: Option<Probe>,
    entries: Vec<ExtensionEntry>,
    probes: Vec<Probe>,
}
impl Check {
    pub(super) fn begin(
        home: &Path,
        ids: &[String],
        depth: usize,
        pool: &mut HashMap<std::path::PathBuf, Arc<File>>,
        started: Instant,
    ) -> Self {
        let mut check = Self {
            observation: GroupDiscovery::not_checked(),
            root: None,
            entries: vec![],
            probes: vec![],
        };
        if ids.is_empty() {
            return check;
        }
        check.observation.scope.code = "observed";
        if started.elapsed() > Duration::from_millis(500) {
            check.partial("limitReached");
            return check;
        }
        let (root, file) = Probe::directory(&home.join(ROOT), depth, pool);
        if let Some(file) = file {
            check.scan(&file, ids, started);
        } else if root.status == "unknown" {
            check.partial(root.issue.unwrap_or("unavailable"));
        }
        check.root = Some(root);
        check
            .entries
            .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        for entry in &check.entries {
            if started.elapsed() > Duration::from_millis(500) {
                break;
            }
            let mut probe = Probe::begin(&home.join(&entry.relative_path), depth, pool);
            probe.require_type(libc::S_IFDIR);
            check.probes.push(probe);
        }
        if check.probes.len() != check.entries.len() {
            check.partial("limitReached");
        }
        check
    }
    fn partial(&mut self, issue: &'static str) {
        let scope = &mut self.observation.scope;
        scope.code = "partial";
        if scope.issue.is_none()
            || issue == "changed"
            || (issue == "limitReached" && scope.issue != Some("changed"))
        {
            scope.issue = Some(issue);
        }
    }
    fn scan(&mut self, file: &File, ids: &[String], started: Instant) {
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
            if self.observation.scope.entries >= 1024
                || self.observation.scope.candidates >= 64
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
            self.observation.scope.entries += 1;
            let Ok(name) = name.to_str() else {
                self.partial("unsupported");
                continue;
            };
            if name.chars().any(char::is_control) {
                self.partial("unsupported");
                continue;
            }
            let matching = ids
                .iter()
                .filter(|id| name_matches(name, id))
                .collect::<Vec<_>>();
            if matching.is_empty() {
                continue;
            }
            let Ok(Some(stat)) = stat_at(file, &CString::new(name).unwrap()) else {
                self.partial("unavailable");
                continue;
            };
            let kind = stat.st_mode & libc::S_IFMT;
            if kind != libc::S_IFDIR && kind != libc::S_IFLNK {
                continue;
            }
            if kind == libc::S_IFLNK {
                self.partial("unsupported");
            }
            for id in matching {
                add(
                    &mut self.entries,
                    format!("{ROOT}/{name}"),
                    id,
                    "groupContainer",
                );
            }
            self.observation.scope.candidates += 1;
        }
    }
    pub(super) fn finish(
        mut self,
        started: Instant,
    ) -> Result<(GroupDiscovery, Vec<ExtensionEntry>), ()> {
        for (entry, probe) in self
            .entries
            .iter_mut()
            .zip(std::mem::take(&mut self.probes))
        {
            if started.elapsed() > Duration::from_millis(500) {
                return Err(());
            }
            (entry.status, entry.issue) = probe.finish();
        }
        if self.entries.iter().any(|e| e.issue == Some("unavailable")) {
            self.partial("unavailable");
        }
        if let Some(root) = self.root.take() {
            let (status, issue) = root.finish();
            if status == "unknown" {
                self.partial(issue.unwrap_or("unavailable"));
            }
        }
        if started.elapsed() > Duration::from_millis(500) {
            return Err(());
        }
        Ok((self.observation, self.entries))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};
    const ID: &str = "org.other.helper";
    fn temp() -> tempfile::TempDir {
        tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
    }
    fn observe(home: &Path) -> ExtensionData {
        super::super::Check::begin(
            home,
            vec![ID.into(), format!("{ID}.share")],
            "observed",
            None,
        )
        .finish()
    }
    fn grouped(result: &ExtensionData) -> Vec<&ExtensionEntry> {
        result
            .entries
            .iter()
            .filter(|e| e.sources.iter().any(|s| s.kind == "groupContainer"))
            .collect()
    }
    #[test]
    fn verified_ids_map_case_sensitive_directory_names_and_merge_all_sources() {
        let t = temp();
        let home = t.path();
        let root = home.join(ROOT);
        fs::create_dir_all(&root).unwrap();
        for name in [
            format!("TEAM.{ID}.share"),
            ID.into(),
            format!("TEAM.{ID}apple"),
            format!("TEAM.{}", ID.to_uppercase()),
            format!("nested/TEAM.{ID}"),
        ] {
            fs::create_dir_all(root.join(name)).unwrap();
        }
        fs::write(root.join(format!("{ID}.file")), "not a directory").unwrap();
        let personal = root.join(format!("TEAM.{ID}.share/personal"));
        fs::write(&personal, "shared personal data").unwrap();
        let result = observe(home);
        let entries = grouped(&result);
        assert_eq!(result.code, "observed");
        assert_eq!(result.groups.source, "clientEmbeddedIdNameBoundary");
        assert_eq!(result.groups.scope.candidates, 2);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.status == "present"));
        let shared = entries
            .iter()
            .find(|e| e.relative_path.ends_with(".share"))
            .unwrap();
        assert_eq!(shared.sources.len(), 2);
        assert_eq!(shared.sources[0].bundle_id, ID);
        assert_eq!(shared.sources[1].bundle_id, format!("{ID}.share"));
        assert_eq!(
            fs::read_to_string(personal).unwrap(),
            "shared personal data"
        );
    }
    #[test]
    fn latest_protection_and_source_withdrawal_remain_independent_of_mole_unknown() {
        let t = temp();
        let home = t.path();
        let root = home.join(ROOT);
        fs::create_dir_all(root.join(format!("TEAM.{ID}.share"))).unwrap();
        let mut result = observe(home);
        result
            .attach_protection(
                home,
                Some("org.main.app"),
                &format!("{ID}.share"),
                &[root.to_str().unwrap().into()],
            )
            .unwrap();
        result.attach_mole_paths(
            None,
            home,
            &mut crate::app_uninstall_rules::PathBudget::default(),
        );
        let entry = grouped(&result)[0];
        assert!(entry.protection_reasons.contains(&"clientDirectory"));
        assert!(entry.protection_reasons.contains(&"currentApplication"));
        assert_eq!(entry.mole_path_protection.code, "unknown");
        result.bind(&[ID.into()], "partial", Some("unavailable"));
        assert_eq!(result.code, "unknown");
        assert_eq!(result.issue, Some("embeddedUnavailable"));
        assert!(result.entries.is_empty());
        assert_eq!(result.groups.scope.code, "notChecked");
        let mut result = observe(home);
        result.bind(&[], "partial", Some("changed"));
        assert!(result.entries.is_empty());
        assert_eq!(result.groups.scope.candidates, 0);
        assert_eq!(result.issue, Some("embeddedChanged"));
        let result = super::super::Check::begin(home, vec![], "observed", None).finish();
        assert_eq!(result.groups.scope.code, "notChecked");
    }
    #[test]
    fn missing_roots_links_and_rebinding_preserve_uncertainty() {
        let t = temp();
        let home = t.path();
        fs::create_dir_all(home.join("Library")).unwrap();
        let check = super::super::Check::begin(home, vec![ID.into()], "observed", None);
        fs::create_dir(home.join(ROOT)).unwrap();
        let result = check.finish();
        assert_eq!(result.groups.scope.issue, Some("changed"));
        assert_eq!(result.issue, Some("byHostIncomplete"));
        assert_eq!(result.by_host.issue, Some("changed"));
        let path = home.join(ROOT).join(format!("TEAM.{ID}"));
        symlink("/tmp", &path).unwrap();
        let result = observe(home);
        assert_eq!(result.groups.scope.issue, Some("unsupported"));
        assert_eq!(result.issue, Some("groupsIncomplete"));
        assert_eq!(grouped(&result)[0].status, "unknown");
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let check = super::super::Check::begin(home, vec![ID.into()], "observed", None);
        fs::rename(&path, home.join("saved")).unwrap();
        fs::create_dir(&path).unwrap();
        let result = check.finish();
        assert_eq!(result.groups.scope.issue, Some("changed"));
        assert_eq!(grouped(&result)[0].issue, Some("changed"));
    }
    #[test]
    fn candidate_entry_and_shared_time_budgets_never_claim_complete_groups() {
        let t = temp();
        let home = t.path();
        let root = home.join(ROOT);
        fs::create_dir_all(&root).unwrap();
        for i in 0..65 {
            fs::create_dir(root.join(format!("{ID}.{i}"))).unwrap();
        }
        let result = observe(home);
        assert_eq!(result.groups.scope.candidates, 64);
        assert_eq!(grouped(&result).len(), 64);
        assert_eq!(result.groups.scope.issue, Some("limitReached"));
        assert_eq!(result.code, "partial");
        let t = temp();
        let home = t.path();
        let root = home.join(ROOT);
        fs::create_dir_all(&root).unwrap();
        for i in 0..1025 {
            fs::create_dir(root.join(format!("other{i}"))).unwrap();
        }
        let result = observe(home);
        assert_eq!(result.groups.scope.entries, 1024);
        assert_eq!(result.groups.scope.candidates, 0);
        assert_eq!(result.groups.scope.issue, Some("limitReached"));
        let check = Check::begin(
            home,
            &[ID.into()],
            home.components().count() - 1,
            &mut HashMap::new(),
            Instant::now() - Duration::from_secs(1),
        );
        assert_eq!(check.observation.scope.issue, Some("limitReached"));
        assert!(check.entries.is_empty());
        assert!(check
            .finish(Instant::now() - Duration::from_secs(1))
            .is_err());
    }
}
