//! Six base Library paths from pinned Mole, using its original list name literally.
//! A name hint never certifies identity, exclusive ownership or execution eligibility.
use super::*;
const LOCATIONS: [(&str, &str, &str); 6] = [
    ("nameSupport", "Library/Application Support", ""),
    ("nameCache", "Library/Caches", ""),
    ("nameLogs", "Library/Logs", ""),
    ("namePreferencesDirectory", "Library/Preferences", ""),
    ("namePreferences", "Library/Preferences", ".plist"),
    (
        "nameSavedState",
        "Library/Saved Application State",
        ".savedState",
    ),
];
fn supported(name: &str) -> bool {
    name.len() >= 2
        && name.len() <= 240
        && !matches!(name, "." | "..")
        && !name.contains('/')
        && !name.chars().any(char::is_control)
}
pub(super) struct Check {
    scope: NameDiscovery,
    entries: Vec<Entry>,
    probes: Vec<Probe>,
}
impl Check {
    pub(super) fn begin(home: &Path, name: Option<&str>, existing: &[Entry]) -> Self {
        let mut check = Self {
            scope: NameDiscovery::not_checked("nameUnavailable"),
            entries: vec![],
            probes: vec![],
        };
        let Some(name) = name else {
            return check;
        };
        if !supported(name) {
            check.scope.issue = Some("nameUnsupported");
            return check;
        }
        check.scope.code = "observed";
        check.scope.issue = None;
        check.scope.name = Some(name.into());
        let started = Instant::now();
        let mut pool = HashMap::new();
        for (kind, root, suffix) in LOCATIONS {
            let relative_path = format!("{root}/{name}{suffix}");
            // The original expanded-path common-root deny arm excludes ByHost.
            if relative_path == "Library/Preferences/ByHost"
                || existing.iter().any(|e| e.relative_path == relative_path)
            {
                continue;
            }
            if started.elapsed() > Duration::from_millis(500) {
                check.withdraw();
                return check;
            }
            check.probes.push(Probe::begin(
                &home.join(&relative_path),
                home.components().count() - 1,
                &mut pool,
            ));
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
        }
        if started.elapsed() > Duration::from_millis(500) {
            check.withdraw();
        } else {
            check.scope.candidates = check.entries.len();
        }
        check
    }
    fn withdraw(&mut self) {
        self.scope = NameDiscovery::not_checked("limitReached");
        self.scope.code = "unknown";
        self.entries.clear();
        self.probes.clear();
    }
    pub(super) fn finish(self) -> (NameDiscovery, Vec<Entry>) {
        self.finish_started(Instant::now())
    }
    fn finish_started(mut self, started: Instant) -> (NameDiscovery, Vec<Entry>) {
        for (entry, probe) in self
            .entries
            .iter_mut()
            .zip(std::mem::take(&mut self.probes))
        {
            if started.elapsed() > Duration::from_millis(500) {
                self.withdraw();
                return (self.scope, self.entries);
            }
            (entry.status, entry.issue) = probe.finish();
        }
        if started.elapsed() > Duration::from_millis(500) {
            self.withdraw();
        }
        (self.scope, self.entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};
    fn temp() -> tempfile::TempDir {
        tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
    }
    fn observe(home: &Path, name: Option<&str>) -> Observation {
        crate::app_associations::mac::Check::begin(home, Some("org.example.app"), "matches", true)
            .with_name(home, name)
            .finish()
    }
    #[test]
    fn literal_names_keep_spaces_unicode_globs_and_shell_text_without_expansion() {
        let t = temp();
        let home = t.path();
        for name in [
            "App Name",
            "微信",
            "中",
            "[*]?",
            "$(printf danger)",
            "App\\Name",
        ] {
            let path = home.join(format!("Library/Application Support/{name}"));
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("personal"), "keep me").unwrap();
            let result = observe(home, Some(name));
            assert_eq!(result.names.code, "observed");
            assert_eq!(result.names.name.as_deref(), Some(name));
            assert_eq!(result.names.candidates, 6);
            let entry = result
                .entries
                .iter()
                .find(|e| e.kind == "nameSupport")
                .unwrap();
            assert_eq!(
                entry.relative_path,
                format!("Library/Application Support/{name}")
            );
            assert_eq!(entry.status, "present");
            assert_eq!(
                fs::read_to_string(path.join("personal")).unwrap(),
                "keep me"
            );
        }
    }
    #[test]
    fn unavailable_unsupported_common_roots_and_exact_identifier_collisions_do_not_expand_scope() {
        let t = temp();
        let home = t.path();
        assert_eq!(observe(home, None).names.issue, Some("nameUnavailable"));
        for name in ["", "X", ".", "..", "App/Child", "App\nName"] {
            let result = observe(home, Some(name));
            assert_eq!(result.names.issue, Some("nameUnsupported"));
            assert!(result.entries.iter().all(|e| !e.kind.starts_with("name")));
        }
        assert_eq!(
            observe(home, Some(&"中".repeat(81))).names.issue,
            Some("nameUnsupported")
        );
        let result = observe(home, Some("org.example.app"));
        assert_eq!(result.names.code, "observed");
        assert_eq!(result.names.candidates, 0);
        assert_eq!(result.entries.len(), 21);
        let result = observe(home, Some("ByHost"));
        assert_eq!(result.names.candidates, 5);
        assert!(!result
            .entries
            .iter()
            .any(|e| e.relative_path == "Library/Preferences/ByHost"));
        for (identity, ready, issue) in [
            ("different", true, "identityChanged"),
            ("matches", false, "rulesUnavailable"),
        ] {
            let result = crate::app_associations::mac::Check::begin(
                home,
                Some("org.example.app"),
                identity,
                ready,
            )
            .with_name(home, Some("App Name"))
            .finish();
            assert!(result.entries.is_empty());
            assert_eq!(result.names.code, "notChecked");
            assert_eq!(result.names.issue, Some(issue));
        }
    }
    #[test]
    fn changes_links_and_latest_client_protection_preserve_unknown_and_retention() {
        let t = temp();
        let home = t.path();
        let root = home.join("Library/Application Support");
        let path = root.join("App Name");
        fs::create_dir_all(&path).unwrap();
        let check = crate::app_associations::mac::Check::begin(
            home,
            Some("org.example.app"),
            "matches",
            true,
        )
        .with_name(home, Some("App Name"));
        fs::rename(&path, root.join("saved")).unwrap();
        symlink(root.join("saved"), &path).unwrap();
        let result = check.finish();
        assert_eq!(
            result
                .entries
                .iter()
                .find(|e| e.kind == "nameSupport")
                .unwrap()
                .issue,
            Some("changed")
        );
        let mut result = observe(home, Some("App Name"));
        assert_eq!(
            result
                .entries
                .iter()
                .find(|e| e.kind == "nameSupport")
                .unwrap()
                .issue,
            Some("unsupported")
        );
        result
            .attach_protection(
                home,
                Some("org.example.app"),
                "org.client.app",
                &[home.join("Library").to_str().unwrap().into()],
            )
            .unwrap();
        result.attach_mole_paths(
            None,
            home,
            &mut crate::app_uninstall_rules::PathBudget::default(),
        );
        assert!(result
            .entries
            .iter()
            .filter(|e| e.kind.starts_with("name"))
            .all(|e| e.protection_reasons.contains(&"clientDirectory")
                && e.mole_path_protection.code == "unknown"));
    }
    #[test]
    fn finish_deadline_withdraws_all_name_evidence() {
        let t = temp();
        let check = Check::begin(t.path(), Some("App Name"), &[]);
        let (scope, entries) = check.finish_started(Instant::now() - Duration::from_secs(1));
        assert_eq!(scope.code, "unknown");
        assert_eq!(scope.issue, Some("limitReached"));
        assert_eq!(scope.candidates, 0);
        assert!(scope.name.is_none());
        assert!(entries.is_empty());
    }
    #[test]
    #[ignore = "requires fixed Mole source; compares only six literal name templates and common-root filter"]
    fn prepared_name_templates_and_common_root_filter_match_original_c_locale() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        crate::app_uninstall_rules::load(&resources).unwrap();
        let source = fs::read_to_string(resources.join("lib/core/app_protection.sh")).unwrap();
        let block = source
            .split("if [[ -n \"$app_name\" && ${#app_name} -ge 2 ]]; then")
            .nth(1)
            .unwrap()
            .split("user_patterns+=(")
            .nth(1)
            .unwrap();
        let templates = block
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with('"'))
            .take(6)
            .collect::<Vec<_>>();
        assert_eq!(templates.len(), 6);
        for ((_, root, suffix), line) in LOCATIONS.iter().zip(&templates) {
            assert_eq!(*line, format!("\"$HOME/{root}/$app_name{suffix}\""));
        }
        let common = source
            .split("case \"$expanded_path\" in")
            .nth(1)
            .unwrap()
            .split("esac")
            .next()
            .unwrap();
        let script=format!("set -euo pipefail\napp_name=$1\nuser_patterns=()\nif [[ -n \"$app_name\" && ${{#app_name}} -ge 2 ]]; then user_patterns+=(\n{}\n); fi\nfor expanded_path in \"${{user_patterns[@]+\"${{user_patterns[@]}}\"}}\"; do case \"$expanded_path\" in\n{}\nesac\nprintf '%s\\0' \"$expanded_path\"; done",templates.join("\n").replace("$HOME","$MOOMOLE_TEST_ROOT"),common.replace("$HOME","$MOOMOLE_TEST_ROOT"));
        let t = temp();
        let home = t.path();
        let names = [
            "App Name",
            "微信",
            "中",
            "X",
            "Go",
            "[*]?",
            "$(printf danger)",
            "App\\Name",
            "ByHost",
            ".hidden",
            "Beta Preview",
            "Name_With-Dash",
        ];
        for name in names {
            let output = std::process::Command::new("/bin/bash")
                .args(["-c", &script, "name-templates", name])
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
            let expected = output
                .stdout
                .split(|b| *b == 0)
                .filter(|b| !b.is_empty())
                .map(|b| {
                    Path::new(std::str::from_utf8(b).unwrap())
                        .strip_prefix(home)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned()
                })
                .collect::<Vec<_>>();
            let actual = Check::begin(home, Some(name), &[])
                .finish()
                .1
                .into_iter()
                .map(|e| e.relative_path)
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{name}");
        }
        eprintln!(
            "{} names matched original six templates and common-root filter",
            names.len()
        );
    }
}
