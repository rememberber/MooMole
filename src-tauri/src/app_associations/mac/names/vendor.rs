//! Pinned Mole vendor/product name boundaries with bounded, metadata-only
//! two-level discovery. Every candidate remains retained; names prove no ownership.
use super::*;
pub(super) const ROOTS: [(&str, &str); 3] = [
    ("vendorSupport", "Library/Application Support"),
    ("vendorCache", "Library/Caches"),
    ("vendorLogs", "Library/Logs"),
];
const COMMON: &str = "music notes photos finder safari preview calendar contacts messages reminders clock weather stocks books news podcasts voice files store system helper agent daemon service update sync backup cloud manager monitor server client worker runner launcher driver plugin extension widget utility";
pub(super) fn tokens(id: Option<&str>, name: &str) -> Option<(String, String)> {
    if !super::supported(name)
        || name.len() < 4
        || COMMON.split(' ').any(|n| n == name.to_ascii_lowercase())
    {
        return None;
    }
    let id = id.filter(|id| crate::app_associations::valid_id(id))?;
    let mut parts = id.rsplit('.');
    let product = parts.next()?;
    let vendor = parts.next()?;
    (vendor.len() >= 3 && product.len() >= 3)
        .then(|| (vendor.to_ascii_lowercase(), product.to_ascii_lowercase()))
}
pub(super) fn matches(child: &str, name: &str, product: &str) -> bool {
    let child = child.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    [
        name.clone(),
        name.replace(' ', ""),
        name.replace(' ', "-"),
        name.replace(' ', "_"),
        product.into(),
    ]
    .iter()
    .any(|variant| {
        !variant.is_empty()
            && (child == *variant
                || child
                    .strip_prefix(variant)
                    .is_some_and(|rest| rest.starts_with([' ', '-', '_', '.'])))
    })
}
pub(super) struct Check {
    state: VendorDiscovery,
    directories: Vec<(&'static str, Probe)>,
    entries: Vec<Entry>,
    probes: Vec<Probe>,
}
impl Check {
    pub(super) fn begin(
        home: &Path,
        id: Option<&str>,
        name: &str,
        existing: &std::collections::HashSet<String>,
        started: Instant,
    ) -> Self {
        let mut check = Self {
            state: VendorDiscovery::not_checked("notApplicable"),
            directories: vec![],
            entries: vec![],
            probes: vec![],
        };
        let Some((vendor, product)) = tokens(id, name) else {
            return check;
        };
        check.state.code = "observed";
        check.state.issue = None;
        check.state.vendor_token = Some(vendor.clone());
        check.state.product_token = Some(product.clone());
        let depth = home.components().count() - 1;
        let mut pool = HashMap::new();
        for (kind, root) in ROOTS {
            if !check.within_budget(started, root) {
                break;
            }
            check.state.root_count += 1;
            let (probe, file) = Probe::directory(&home.join(root), depth, &mut pool);
            check.directories.push((root, probe));
            let Some(file) = file else {
                let p = &check.directories.last().unwrap().1;
                if p.status == "unknown" {
                    check.partial(p.issue.unwrap_or("unavailable"), root);
                }
                continue;
            };
            let parents = check.list(&file, root, started);
            for parent in parents {
                if parent.to_ascii_lowercase() != vendor {
                    continue;
                }
                if !check.within_budget(started, root) {
                    break;
                }
                let relative = format!("{root}/{parent}");
                let (probe, file) = Probe::directory(&home.join(&relative), depth, &mut pool);
                check.directories.push((root, probe));
                let Some(file) = file else {
                    // A matching ordinary file is not a nested directory. Links
                    // and unreadable/changing matching parents stay incomplete.
                    let p = &check.directories.last().unwrap().1;
                    if p.status == "unknown" {
                        check.partial(p.issue.unwrap_or("unavailable"), root);
                    }
                    continue;
                };
                for child in check.list(&file, root, started) {
                    if !matches(&child, name, &product) {
                        continue;
                    }
                    if !check.within_budget(started, root) {
                        break;
                    }
                    let relative_path = format!("{relative}/{child}");
                    if relative_path.len() > 1024
                        || home.join(&relative_path).as_os_str().len() > 4096
                    {
                        check.partial("unsupported", root);
                        continue;
                    }
                    let Ok(Some(stat)) = stat_at(&file, &CString::new(child).unwrap()) else {
                        check.partial("changed", root);
                        continue;
                    };
                    let kind_value = stat.st_mode & libc::S_IFMT;
                    if kind_value == libc::S_IFLNK {
                        check.partial("unsupported", root);
                        continue;
                    }
                    if kind_value != libc::S_IFDIR
                        || existing.contains(&relative_path)
                        || check
                            .entries
                            .iter()
                            .any(|e| e.relative_path == relative_path)
                    {
                        continue;
                    }
                    let mut probe = Probe::begin(&home.join(&relative_path), depth, &mut pool);
                    probe.require_type(libc::S_IFDIR);
                    check.probes.push(probe);
                    check.entries.push(Entry {
                        kind,
                        relative_path,
                        status: "unknown",
                        issue: Some("unavailable"),
                        protection_reasons: vec![],
                        mole_path_protection: crate::app_uninstall_rules::PathObservation::unknown(
                            "rulesUnavailable",
                        ),
                    });
                    check.state.candidates += 1;
                }
            }
        }
        if started.elapsed() > Duration::from_millis(500) {
            check.partial("limitReached", "Library/Application Support");
        }
        check.withdraw_incomplete();
        check
    }
    fn partial(&mut self, issue: &'static str, root: &'static str) {
        self.state.code = "partial";
        if self.state.issue.is_none()
            || issue == "changed"
            || (issue == "limitReached" && self.state.issue != Some("changed"))
        {
            self.state.issue = Some(issue);
            self.state.first_uncertain_root = Some(root);
        }
    }
    fn within_budget(&mut self, started: Instant, root: &'static str) -> bool {
        if self.state.entries >= 1024
            || self.state.candidates >= 64
            || started.elapsed() > Duration::from_millis(500)
        {
            self.partial("limitReached", root);
            false
        } else {
            true
        }
    }
    fn withdraw_incomplete(&mut self) {
        if self.state.code == "partial" {
            self.entries.clear();
            self.probes.clear();
            self.state.candidates = 0;
        }
    }
    fn list(&mut self, file: &File, root: &'static str, started: Instant) -> Vec<String> {
        struct Stream(*mut libc::DIR);
        impl Drop for Stream {
            fn drop(&mut self) {
                unsafe {
                    libc::closedir(self.0);
                }
            }
        }
        let Ok(clone) = file.try_clone() else {
            self.partial("unavailable", root);
            return vec![];
        };
        let fd = clone.into_raw_fd();
        let pointer = unsafe { libc::fdopendir(fd) };
        if pointer.is_null() {
            unsafe {
                libc::close(fd);
            }
            self.partial("unavailable", root);
            return vec![];
        }
        let stream = Stream(pointer);
        let mut names = vec![];
        loop {
            if !self.within_budget(started, root) {
                break;
            }
            unsafe {
                *libc::__error() = 0;
            }
            let entry = unsafe { libc::readdir(stream.0) };
            if entry.is_null() {
                if unsafe { *libc::__error() } != 0 {
                    self.partial("unavailable", root);
                }
                break;
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if matches!(name.to_bytes(), b"." | b"..") {
                continue;
            }
            self.state.entries += 1;
            let Ok(name) = name.to_str() else {
                self.partial("unsupported", root);
                continue;
            };
            if name.chars().any(char::is_control) {
                self.partial("unsupported", root);
                continue;
            }
            names.push(name.into());
        }
        names.sort();
        names
    }
    pub(super) fn finish(mut self, started: Instant) -> (VendorDiscovery, Vec<Entry>) {
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
        for (root, probe) in std::mem::take(&mut self.directories) {
            let (status, issue) = probe.finish();
            if status == "unknown" {
                self.partial(issue.unwrap_or("unavailable"), root);
            }
        }
        if started.elapsed() > Duration::from_millis(500) {
            self.partial("limitReached", "Library/Application Support");
        }
        if let Some((issue, root)) = self.entries.iter().find_map(|e| {
            e.issue.map(|issue| {
                (
                    issue,
                    ROOTS
                        .iter()
                        .find(|(_, root)| e.relative_path.starts_with(&format!("{root}/")))
                        .unwrap()
                        .1,
                )
            })
        }) {
            self.partial(issue, root);
        }
        self.withdraw_incomplete();
        self.entries
            .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        (self.state, self.entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};
    fn begin(home: &Path, name: &str, id: &str) -> Check {
        Check::begin(
            home,
            Some(id),
            name,
            &std::collections::HashSet::new(),
            Instant::now(),
        )
    }
    fn fixture() -> tempfile::TempDir {
        let t = super::super::tests::temp();
        for (_, root) in ROOTS {
            for child in [
                "Sibelius",
                "Sibelius Ultimate",
                "Sibelius-2026",
                "Sibelius_backup",
                "Sibelius.old",
                "SibeliusBackup",
                "Sibelius2026",
                "Unrelated",
            ] {
                let p = t.path().join(root).join("Avid").join(child);
                fs::create_dir_all(&p).unwrap();
                fs::write(p.join("personal"), "keep projects and licences").unwrap();
            }
        }
        t
    }
    #[test]
    fn token_and_literal_boundary_rules_reject_generic_or_weak_context() {
        for (id, name, expected) in [
            ("com.Avid.Sibelius", "Sibelius", Some(("avid", "sibelius"))),
            ("uk.co.avid.Product", "Sibelius", Some(("avid", "product"))),
            ("com.ab.Sibelius", "Sibelius", None),
            ("com.avid.xy", "Sibelius", None),
            ("com.avid.Sibelius", "Sib", None),
            ("com.avid.Sibelius", "Preview", None),
            ("com.avid.Sibelius", "MANAGER", None),
            ("com..Sibelius", "Sibelius", None),
            ("com.avid.Sibelius", "应用X", Some(("avid", "sibelius"))),
        ] {
            assert_eq!(
                tokens(Some(id), name)
                    .as_ref()
                    .map(|(a, b)| (a.as_str(), b.as_str())),
                expected
            );
        }
        for candidate in [
            "Sibelius",
            "SIBELIUS Ultimate",
            "Sibelius-2026",
            "Sibelius_backup",
            "Sibelius.old",
        ] {
            assert!(matches(candidate, "Sibelius", "other"), "{candidate}");
        }
        for candidate in [
            "SibeliusBackup",
            "Sibelius2026",
            "otherSibelius",
            "Sibelius\u{a0}Backup",
        ] {
            assert!(!matches(candidate, "Sibelius", "other"), "{candidate}");
        }
        assert!(matches(
            "Sibelius Ultimate.old",
            "Sibelius Ultimate",
            "other"
        ));
        assert!(matches(
            "SibeliusUltimate.old",
            "Sibelius Ultimate",
            "other"
        ));
        assert!(matches("Product-extra", "Unrelated Name", "product"));
        assert!(matches("Sib*elius-extra", "Sib*elius", "other"));
        assert!(!matches("SibXYZelius-extra", "Sib*elius", "other"));
    }
    #[test]
    fn bounded_two_level_metadata_preserves_data_exact_vendor_and_latest_protection() {
        let t = fixture();
        let home = t.path();
        fs::create_dir_all(home.join("Library/Application Support/NotAvid/Sibelius")).unwrap();
        fs::create_dir_all(home.join("Library/Application Support/Avid/Sibelius/deeper/Sibelius"))
            .unwrap();
        let (scope, entries) = begin(home, "Sibelius", "com.avid.Sibelius").finish(Instant::now());
        assert_eq!(scope.code, "observed");
        assert_eq!(scope.root_count, 3);
        assert_eq!(scope.candidates, 15);
        assert!(entries
            .iter()
            .all(|e| e.status == "present" && e.relative_path.split('/').count() == 4));
        assert!(!entries
            .iter()
            .any(|e| e.relative_path.contains("NotAvid")
                || e.relative_path.ends_with("SibeliusBackup")));
        let excluded = "Library/Application Support/Avid/Sibelius".to_owned();
        let (scope, entries) = Check::begin(
            home,
            Some("com.avid.Sibelius"),
            "Sibelius",
            &std::collections::HashSet::from([excluded.clone()]),
            Instant::now(),
        )
        .finish(Instant::now());
        assert_eq!(scope.candidates, 14);
        assert!(!entries.iter().any(|e| e.relative_path == excluded));
        let mut outer = crate::app_associations::mac::Check::begin(
            home,
            Some("com.avid.Sibelius"),
            "matches",
            true,
        )
        .with_name(home, Some("Sibelius"))
        .finish();
        assert_eq!(outer.names.vendor.candidates, 15);
        outer
            .attach_protection(
                home,
                Some("com.avid.Sibelius"),
                "org.client",
                &[home.join(&excluded).to_string_lossy().into_owned()],
            )
            .unwrap();
        outer.attach_mole_paths(
            None,
            home,
            &mut crate::app_uninstall_rules::PathBudget::default(),
        );
        let row = outer
            .entries
            .iter()
            .find(|e| e.relative_path == excluded)
            .unwrap();
        assert!(row.protection_reasons.contains(&"clientDirectory"));
        assert_eq!(row.mole_path_protection.code, "unknown");
        assert_eq!(
            fs::read_to_string(home.join(&excluded).join("personal")).unwrap(),
            "keep projects and licences"
        );
    }
    #[test]
    fn changes_links_invalid_names_and_limits_withdraw_entire_vendor_group() {
        let t = fixture();
        let home = t.path();
        let check = begin(home, "Sibelius", "com.avid.Sibelius");
        let path = home.join("Library/Caches/Avid/Sibelius");
        fs::rename(&path, home.join("retained-vendor")).unwrap();
        std::os::unix::fs::symlink(home.join("retained-vendor"), &path).unwrap();
        let (scope, entries) = check.finish(Instant::now());
        assert_eq!((scope.code, scope.issue), ("partial", Some("changed")));
        assert_eq!(scope.candidates, 0);
        assert!(entries.is_empty());
        let (scope, entries) = begin(home, "Sibelius", "com.avid.Sibelius").finish(Instant::now());
        assert_eq!(scope.issue, Some("unsupported"));
        assert!(entries.is_empty());
        assert_eq!(
            fs::read_to_string(home.join("retained-vendor/personal")).unwrap(),
            "keep projects and licences"
        );
        let check = begin(home, "Sibelius", "com.avid.Sibelius");
        let (scope, entries) = check.finish(Instant::now() - Duration::from_secs(1));
        assert!(entries.is_empty());
        assert_eq!(scope.candidates, 0);
        assert_eq!(scope.issue, Some("limitReached"));
        let t = super::super::tests::temp();
        for i in 0..65 {
            fs::create_dir_all(t.path().join(format!("Library/Logs/Avid/Sibelius-{i}"))).unwrap();
        }
        let (scope, entries) =
            begin(t.path(), "Sibelius", "com.avid.Sibelius").finish(Instant::now());
        assert_eq!(scope.issue, Some("limitReached"));
        assert!(entries.is_empty());
        let outer = crate::app_associations::mac::Check::begin(
            t.path(),
            Some("com.avid.Sibelius"),
            "matches",
            true,
        )
        .with_name(t.path(), Some("Sibelius"))
        .finish();
        assert_eq!(outer.names.code, "observed");
        assert_eq!(outer.names.candidates, 28);
        assert_eq!(outer.names.vendor.code, "partial");
        assert!(!outer.entries.iter().any(|e| e.kind.starts_with("vendor")));
        let t = super::super::tests::temp();
        for i in 0..1025 {
            fs::create_dir_all(t.path().join(format!("Library/Logs/other-{i}"))).unwrap();
        }
        let (scope, entries) =
            begin(t.path(), "Sibelius", "com.avid.Sibelius").finish(Instant::now());
        assert_eq!(scope.entries, 1024);
        assert_eq!(scope.issue, Some("limitReached"));
        assert!(entries.is_empty());
        let t = super::super::tests::temp();
        fs::create_dir_all(t.path().join("Library/Logs/Avid/Sibelius\nprivate")).unwrap();
        let (scope, entries) =
            begin(t.path(), "Sibelius", "com.avid.Sibelius").finish(Instant::now());
        assert_eq!(scope.issue, Some("unsupported"));
        assert!(entries.is_empty());
        let (scope, entries) =
            begin(t.path(), "Preview", "com.avid.Sibelius").finish(Instant::now());
        assert_eq!(scope.code, "notChecked");
        assert_eq!(scope.issue, Some("notApplicable"));
        assert_eq!(scope.root_count, 0);
        assert!(entries.is_empty());
    }
    #[test]
    #[ignore = "requires fixed Mole source; compares original vendor helper and actual depth-two discovery"]
    fn prepared_vendor_discovery_matches_original_c_locale() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        crate::app_uninstall_rules::load(&resources).unwrap();
        let source = fs::read_to_string(resources.join("lib/core/app_protection.sh")).unwrap();
        let original_common = source
            .split("case \"$lower_name\" in")
            .nth(1)
            .unwrap()
            .split(')')
            .next()
            .unwrap()
            .replace("\\\n", "");
        assert_eq!(
            original_common
                .split('|')
                .map(str::trim)
                .collect::<Vec<_>>(),
            COMMON.split(' ').collect::<Vec<_>>()
        );
        let base = fs::read_to_string(resources.join("lib/core/base.sh")).unwrap();
        let helper = |source: &str, name: &str| {
            format!(
                "{name}() {{{}\n}}",
                source
                    .split(&format!("{name}() {{"))
                    .nth(1)
                    .unwrap()
                    .split("\n}")
                    .next()
                    .unwrap()
            )
        };
        let mut functions = helper(&base, "mole_is_reverse_dns_bundle_id");
        for name in [
            "_mole_uninstall_lower",
            "_mole_uninstall_is_common_app_name",
            "_mole_uninstall_vendor_product_tokens",
            "_mole_uninstall_name_variant_matches",
            "find_vendor_nested_app_paths",
        ] {
            functions.push_str(&format!("\n{}", helper(&source, name)));
        }
        let script = format!(
            r#"{functions}
create_temp_file() {{ /usr/bin/mktemp "$MOOMOLE_TEST_ROOT/vendor.XXXXXX"; }}
_mole_uninstall_materialize_find0() {{ local output="$1"; shift; /usr/bin/find "$@" > "$output"; }}
MOLE_TIMEOUT_DISK_VERIFY_SEC=5
find_vendor_nested_app_paths "$1" "$2" "$MOOMOLE_TEST_ROOT/Library/Application Support" "$MOOMOLE_TEST_ROOT/Library/Caches" "$MOOMOLE_TEST_ROOT/Library/Logs"
"#
        );
        let t = fixture();
        let home = t.path();
        fs::create_dir_all(home.join("Library/Logs/Other/Sibelius")).unwrap();
        let mut cases = vec![];
        for id in [
            "com.avid.Sibelius",
            "com.AVID.Product",
            "uk.co.avid.Sibelius",
            "com.foo.Sibelius",
            "com.ab.Sibelius",
            "com.avid.xy",
            "com..Sibelius",
        ] {
            for name in [
                "Sibelius",
                "Sibelius Ultimate",
                "Sib  elius",
                "Sib*elius",
                "Preview",
                "MANAGER",
                "Sib",
                "应用X",
            ] {
                cases.push((id, name));
            }
        }
        for (id, name) in &cases {
            let output = std::process::Command::new("/bin/bash")
                .args(["-c", &script, "vendor-source", id, name])
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("LC_ALL", "C")
                .env("MOOMOLE_TEST_ROOT", home)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let mut expected = String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .map(|p| {
                    Path::new(p)
                        .strip_prefix(home)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned()
                })
                .collect::<Vec<_>>();
            expected.sort();
            let (scope, entries) = begin(home, name, id).finish(Instant::now());
            assert_ne!(scope.code, "partial");
            assert_eq!(
                entries
                    .into_iter()
                    .map(|e| e.relative_path)
                    .collect::<Vec<_>>(),
                expected,
                "{id}/{name}"
            );
        }
        eprintln!(
            "{} vendor/name contexts matched original helper and actual find depth",
            cases.len()
        );
    }
}
