//! Bounded user name paths from pinned Mole, including Library and guarded XDG variants.
//! A name hint never certifies identity, exclusive ownership or execution eligibility.
use super::*;
mod channel;
mod leaf;
mod vendor;
mod vscode;
mod xdg;
const LOCATIONS: [(&str, &str, &str); 24] = [
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
    ("nameService", "Library/Services", ".workflow"),
    ("nameQuickLook", "Library/QuickLook", ".qlgenerator"),
    ("nameInternetPlugin", "Library/Internet Plug-Ins", ".plugin"),
    (
        "nameAudioComponent",
        "Library/Audio/Plug-Ins/Components",
        ".component",
    ),
    ("nameVst", "Library/Audio/Plug-Ins/VST", ".vst"),
    ("nameVst3", "Library/Audio/Plug-Ins/VST3", ".vst3"),
    (
        "nameDigidesign",
        "Library/Audio/Plug-Ins/Digidesign",
        ".dpm",
    ),
    ("namePreferencePane", "Library/PreferencePanes", ".prefPane"),
    ("nameInputMethod", "Library/Input Methods", ".app"),
    ("nameScreenSaver", "Library/Screen Savers", ".saver"),
    ("nameFramework", "Library/Frameworks", ".framework"),
    (
        "nameContextPlugin",
        "Library/Contextual Menu Items",
        ".plugin",
    ),
    ("nameSpotlight", "Library/Spotlight", ".mdimporter"),
    ("nameColorPicker", "Library/ColorPickers", ".colorPicker"),
    ("nameWorkflow", "Library/Workflows", ".workflow"),
    (
        "nameAddressBookPlugin",
        "Library/Address Book Plug-Ins",
        ".bundle",
    ),
    ("nameAccessibility", "Library/Accessibility", ".bundle"),
    ("nameMailPlugin", "Library/Mail/Bundles", ".mailbundle"),
];
// Order follows the original compound-name block; only Library entries are adapted.
const SPACE_LOCATIONS: [(&str, &str, &str, char); 12] = [
    (
        "nameCompactSupport",
        "Library/Application Support",
        "",
        '\0',
    ),
    ("nameCompactCache", "Library/Caches", "", '\0'),
    ("nameCompactLogs", "Library/Logs", "", '\0'),
    (
        "nameCompactPreferencesDirectory",
        "Library/Preferences",
        "",
        '\0',
    ),
    (
        "nameCompactPreferences",
        "Library/Preferences",
        ".plist",
        '\0',
    ),
    (
        "nameCompactSavedState",
        "Library/Saved Application State",
        ".savedState",
        '\0',
    ),
    (
        "nameUnderscoreSupport",
        "Library/Application Support",
        "",
        '_',
    ),
    ("nameHyphenSupport", "Library/Application Support", "", '-'),
    (
        "nameUnderscorePreferencesDirectory",
        "Library/Preferences",
        "",
        '_',
    ),
    (
        "nameUnderscorePreferences",
        "Library/Preferences",
        ".plist",
        '_',
    ),
    (
        "nameHyphenPreferencesDirectory",
        "Library/Preferences",
        "",
        '-',
    ),
    (
        "nameHyphenPreferences",
        "Library/Preferences",
        ".plist",
        '-',
    ),
];
fn space_variants(name: &str) -> bool {
    // Mole runs in the C locale. Control whitespace is already rejected below;
    // Unicode whitespace is not C [:space:], and only literal ASCII spaces change.
    name.len() > 3 && name.contains(' ')
}
fn component(name: &str) -> bool {
    !name.is_empty() && !matches!(name, "." | "..")
}
fn excluded(path: &str) -> bool {
    // Reachable common-root and shared-home-state exclusions from pinned Mole.
    xdg::shared_root(path)
        || matches!(
            path,
            "Library/Preferences/ByHost" | "Library/Application Support/Epic"
        )
}
fn paths(name: &str) -> Vec<(&'static str, String)> {
    let mut paths = LOCATIONS
        .into_iter()
        .map(|(kind, root, suffix)| (kind, format!("{root}/{name}{suffix}")))
        .collect::<Vec<_>>();
    if space_variants(name) {
        for (kind, root, suffix, separator) in SPACE_LOCATIONS {
            let derived = if separator == '\0' {
                name.replace(' ', "")
            } else {
                name.replace(' ', &separator.to_string())
            };
            paths.push((kind, format!("{root}/{derived}{suffix}")));
        }
    }
    paths.extend(xdg::paths(name));
    paths.extend(channel::paths(name));
    paths
}
fn supported(name: &str) -> bool {
    name.len() >= 2
        && name.len() <= 240
        && !matches!(name, "." | "..")
        && !name.contains('/')
        && !name.chars().any(char::is_control)
        // Reject the entire name scope if its compact component could escape a root.
        && (!space_variants(name) || component(&name.replace(' ', "")))
}
pub(super) struct Check {
    scope: NameDiscovery,
    entries: Vec<Entry>,
    probes: Vec<Probe>,
    vendor: Option<vendor::Check>,
}
impl Check {
    pub(super) fn begin(
        home: &Path,
        name: Option<&str>,
        existing: &[Entry],
        id: Option<&str>,
    ) -> Self {
        let mut check = Self {
            scope: NameDiscovery::not_checked("nameUnavailable"),
            entries: vec![],
            probes: vec![],
            vendor: None,
        };
        let Some(name) = name else {
            return check;
        };
        if !supported(name) {
            check.scope.issue = Some("nameUnsupported");
            check.scope.vendor = VendorDiscovery::not_checked("nameUnsupported");
            return check;
        }
        check.scope.code = "observed";
        check.scope.issue = None;
        check.scope.name = Some(name.into());
        check.scope.base_name = channel::base(name).map(str::to_owned);
        check.scope.leaf_names = leaf::variants(name, id);
        check.scope.vscode_channel = vscode::channel(id);
        let started = Instant::now();
        let mut pool = HashMap::new();
        let mut seen = std::collections::HashSet::new();
        for (kind, relative_path) in paths(name)
            .into_iter()
            .chain(leaf::paths(&check.scope.leaf_names))
            .chain(vscode::paths(check.scope.vscode_channel))
        {
            if !seen.insert(relative_path.clone())
                || excluded(&relative_path)
                || existing.iter().any(|e| e.relative_path == relative_path)
            {
                continue;
            }
            if xdg::independent_cli(&relative_path) {
                check.scope.cli_excluded += 1;
                continue;
            }
            if started.elapsed() > Duration::from_millis(500) {
                check.withdraw();
                return check;
            }
            let mut probe = Probe::begin(
                &home.join(&relative_path),
                home.components().count() - 1,
                &mut pool,
            );
            if kind.starts_with("nameVSCode") {
                probe.require_type(libc::S_IFDIR);
            }
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
        }
        if started.elapsed() > Duration::from_millis(500) {
            check.withdraw();
        } else {
            check.scope.candidates = check.entries.len();
            let exclusions = existing
                .iter()
                .chain(&check.entries)
                .map(|e| e.relative_path.clone())
                .collect::<std::collections::HashSet<_>>();
            check.vendor = Some(vendor::Check::begin(home, id, name, &exclusions, started));
        }
        check
    }
    fn withdraw(&mut self) {
        self.scope = NameDiscovery::not_checked("limitReached");
        self.scope.code = "unknown";
        self.entries.clear();
        self.probes.clear();
        self.vendor = None;
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
        if let Some(vendor) = self.vendor.take() {
            let (scope, entries) = vendor.finish(started);
            self.scope.vendor = scope;
            self.entries.extend(entries);
        }
        (self.scope, self.entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};
    pub(super) fn temp() -> tempfile::TempDir {
        tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
    }
    pub(super) fn observe(home: &Path, name: Option<&str>) -> Observation {
        crate::app_associations::mac::Check::begin(home, Some("org.example.app"), "matches", true)
            .with_name(home, name)
            .finish()
    }
    #[test]
    fn literal_names_keep_spaces_unicode_globs_and_shell_text_without_expansion() {
        let t = temp();
        let home = t.path();
        for (name, count) in [
            ("App Name", 49),
            ("微信", 27),
            ("中", 27),
            ("[*]?", 27),
            ("$(printf danger)", 48),
            ("App\\Name", 28),
        ] {
            let path = home.join(format!("Library/Application Support/{name}"));
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("personal"), "keep me").unwrap();
            let result = observe(home, Some(name));
            assert_eq!(result.names.code, "observed");
            assert_eq!(result.names.name.as_deref(), Some(name));
            assert_eq!(result.names.candidates, count);
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
    fn all_literal_components_observe_metadata_without_loading_or_reading_contents() {
        let t = temp();
        let home = t.path();
        let name = "Suite Studio";
        for (index, (_, root, suffix)) in LOCATIONS.iter().enumerate() {
            let path = home.join(format!("{root}/{name}{suffix}"));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if index % 2 == 0 {
                fs::create_dir(&path).unwrap();
                fs::write(path.join("personal"), "keep component data").unwrap();
            } else {
                fs::write(path, "keep ordinary file").unwrap();
            }
        }
        let result = observe(home, Some(name));
        assert_eq!(
            result.names.candidates,
            if space_variants(name) {
                49
            } else if name.bytes().any(|b| b.is_ascii_uppercase()) {
                28
            } else {
                27
            }
        );
        for (kind, root, suffix) in LOCATIONS {
            let entry = result.entries.iter().find(|e| e.kind == kind).unwrap();
            assert_eq!(entry.relative_path, format!("{root}/{name}{suffix}"));
            assert_eq!(entry.status, "present", "{kind}");
            let path = home.join(&entry.relative_path);
            assert_eq!(
                fs::read_to_string(if path.is_dir() {
                    path.join("personal")
                } else {
                    path
                })
                .unwrap(),
                if LOCATIONS.iter().position(|(k, _, _)| *k == kind).unwrap() % 2 == 0 {
                    "keep component data"
                } else {
                    "keep ordinary file"
                }
            );
        }
        let check = Check::begin(home, Some(name), &[], None);
        let path = home.join(format!("Library/Audio/Plug-Ins/VST3/{name}.vst3"));
        let saved = home.join("retained-component");
        fs::rename(&path, &saved).unwrap();
        symlink(&saved, &path).unwrap();
        let (_, entries) = check.finish();
        assert_eq!(
            entries.iter().find(|e| e.kind == "nameVst3").unwrap().issue,
            Some("changed")
        );
        let (_, entries) = Check::begin(home, Some(name), &[], None).finish();
        assert_eq!(
            entries.iter().find(|e| e.kind == "nameVst3").unwrap().issue,
            Some("unsupported")
        );
    }
    #[test]
    fn space_variants_preserve_metadata_content_and_withdraw_on_replacement() {
        let t = temp();
        let home = t.path();
        let name = "Suite  Studio";
        for (_, path) in paths(name) {
            fs::create_dir_all(home.join(&path)).unwrap();
            fs::write(home.join(path).join("personal"), "keep data").unwrap();
        }
        let result = observe(home, Some(name));
        assert_eq!(result.names.candidates, 49);
        assert!(result
            .entries
            .iter()
            .filter(|e| e.kind.starts_with("name"))
            .all(|e| e.status == "present"));
        for (_, path) in paths(name) {
            assert_eq!(
                fs::read_to_string(home.join(path).join("personal")).unwrap(),
                "keep data"
            );
        }
        assert!(result
            .entries
            .iter()
            .any(|e| e.relative_path == "Library/Application Support/Suite__Studio"));
        assert!(result
            .entries
            .iter()
            .any(|e| e.relative_path == "Library/Preferences/Suite--Studio.plist"));
        let check = Check::begin(home, Some(name), &[], None);
        let path = home.join("Library/Application Support/SuiteStudio");
        let saved = home.join("saved-compact-data");
        fs::rename(&path, &saved).unwrap();
        symlink(&saved, &path).unwrap();
        let (_, entries) = check.finish();
        assert_eq!(
            entries
                .iter()
                .find(|e| e.kind == "nameCompactSupport")
                .unwrap()
                .issue,
            Some("changed")
        );
        let (_, entries) = Check::begin(home, Some(name), &[], None).finish();
        assert_eq!(
            entries
                .iter()
                .find(|e| e.kind == "nameCompactSupport")
                .unwrap()
                .issue,
            Some("unsupported")
        );
    }
    #[test]
    fn space_gate_common_and_shared_roots_and_exact_sources_are_preserved() {
        let t = temp();
        let home = t.path();
        for (name, count) in [
            ("A B", 28),
            ("中 X", 49),
            ("Suite\u{a0}Studio", 28),
            ("By Host", 48),
            ("Epic", 27),
            ("Ep ic", 48),
            ("org.example. app", 42),
            (" X  ", 49),
        ] {
            let result = observe(home, Some(name));
            assert_eq!(result.names.code, "observed", "{name}");
            assert_eq!(result.names.candidates, count, "{name}");
            assert!(result.entries.iter().all(|e| !excluded(&e.relative_path)));
            assert_eq!(
                result
                    .entries
                    .iter()
                    .map(|e| &e.relative_path)
                    .collect::<std::collections::HashSet<_>>()
                    .len(),
                result.entries.len()
            );
        }
        for name in ["    ", " .  ", " .. ", "A\tB", "A\rB", "A\nB"] {
            let result = observe(home, Some(name));
            assert_eq!(result.names.issue, Some("nameUnsupported"), "{name:?}");
            assert_eq!(result.entries.len(), 21);
            assert!(result.names.name.is_none());
        }
        let mut result = observe(home, Some("Suite Studio"));
        result
            .attach_protection(
                home,
                Some("org.example.app"),
                "org.client",
                &[home
                    .join("Library/Application Support/Suite_Studio")
                    .to_string_lossy()
                    .into_owned()],
            )
            .unwrap();
        result.attach_mole_paths(
            None,
            home,
            &mut crate::app_uninstall_rules::PathBudget::default(),
        );
        let entry = result
            .entries
            .iter()
            .find(|e| e.kind == "nameUnderscoreSupport")
            .unwrap();
        assert!(entry.protection_reasons.contains(&"clientDirectory"));
        assert_eq!(entry.mole_path_protection.code, "unknown");
    }
    #[test]
    fn maximum_utf8_name_leaves_room_for_plugin_suffixes() {
        let t = temp();
        let name = "中".repeat(80);
        let path = t
            .path()
            .join(format!("Library/QuickLook/{name}.qlgenerator"));
        fs::create_dir_all(&path).unwrap();
        let (scope, entries) = Check::begin(t.path(), Some(&name), &[], None).finish();
        assert_eq!(scope.candidates, 27);
        assert_eq!(
            entries
                .iter()
                .find(|e| e.kind == "nameQuickLook")
                .unwrap()
                .status,
            "present"
        );
        assert!(LOCATIONS
            .iter()
            .all(|(_, _, suffix)| name.len() + suffix.len() <= 255));
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
        assert_eq!(result.names.candidates, 20);
        assert_eq!(result.entries.len(), 41);
        let result = observe(home, Some("ByHost"));
        assert_eq!(result.names.candidates, 27);
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
            .filter(|e| e.kind.starts_with("name") && !e.kind.starts_with("nameXdg"))
            .all(|e| e.protection_reasons.contains(&"clientDirectory")
                && e.mole_path_protection.code == "unknown"));
    }
    #[test]
    fn finish_deadline_withdraws_all_name_evidence() {
        let t = temp();
        let check = Check::begin(t.path(), Some("App Name"), &[], None);
        let (scope, entries) = check.finish_started(Instant::now() - Duration::from_secs(1));
        assert_eq!(scope.code, "unknown");
        assert_eq!(scope.issue, Some("limitReached"));
        assert_eq!(scope.candidates, 0);
        assert!(scope.name.is_none());
        assert!(entries.is_empty());
    }
    #[test]
    #[ignore = "requires fixed Mole source; compares all 24 literal Library name templates and common-root filter"]
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
            .take_while(|line| *line != ")")
            .filter(|line| line.starts_with("\"$HOME/Library/"))
            .collect::<Vec<_>>();
        assert_eq!(templates.len(), 24);
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
        let shared = source
            .split("_mole_is_shared_home_state_root() {")
            .nth(1)
            .unwrap()
            .split("\n}")
            .next()
            .unwrap();
        let script=format!("set -euo pipefail\napp_name=$1\n_mole_is_shared_home_state_root() {{{shared}\n}}\nuser_patterns=()\nif [[ -n \"$app_name\" && ${{#app_name}} -ge 2 ]]; then user_patterns+=(\n{}\n); fi\nfor expanded_path in \"${{user_patterns[@]+\"${{user_patterns[@]}}\"}}\"; do case \"$expanded_path\" in\n{}\nesac\nif _mole_is_shared_home_state_root \"$expanded_path\"; then continue; fi\nprintf '%s\\0' \"$expanded_path\"; done",templates.join("\n"),common).replace("$HOME", "$MOOMOLE_TEST_ROOT");
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
            "Epic",
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
            let actual = Check::begin(home, Some(name), &[], None)
                .finish()
                .1
                .into_iter()
                .filter(|e| LOCATIONS.iter().any(|(kind, _, _)| *kind == e.kind))
                .map(|e| e.relative_path)
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{name}");
        }
        eprintln!(
            "{} names matched original 24 Library templates and common-root filter",
            names.len()
        );
    }
    #[test]
    #[ignore = "requires fixed Mole source; compares compound Library templates, ASCII-space gate and shared-root exclusion"]
    fn prepared_space_variants_match_original_c_locale_with_shared_root_filter() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        crate::app_uninstall_rules::load(&resources).unwrap();
        let source = fs::read_to_string(resources.join("lib/core/app_protection.sh")).unwrap();
        let gate = "if [[ ${#app_name} -gt 3 && \"$app_name\" =~ [[:space:]] ]]; then";
        let templates = source
            .split(gate)
            .nth(1)
            .unwrap()
            .split("user_patterns+=(")
            .nth(1)
            .unwrap()
            .lines()
            .map(str::trim)
            .take_while(|line| *line != ")")
            .filter(|line| line.starts_with("\"$HOME/Library/"))
            .collect::<Vec<_>>();
        assert_eq!(templates.len(), 12);
        for ((_, root, suffix, separator), line) in SPACE_LOCATIONS.iter().zip(&templates) {
            let var = match separator {
                '\0' => "nospace_name",
                '_' => "underscore_name",
                '-' => "hyphen_name",
                _ => unreachable!(),
            };
            assert_eq!(*line, format!("\"$HOME/{root}/${var}{suffix}\""));
        }
        let assignments = ["nospace_name", "underscore_name", "hyphen_name"]
            .map(|var| {
                source
                    .lines()
                    .map(str::trim)
                    .find(|line| line.starts_with(&format!("local {var}=")))
                    .unwrap()
                    .trim_start_matches("local ")
            })
            .join("\n");
        let shared = source
            .split("_mole_is_shared_home_state_root() {")
            .nth(1)
            .unwrap()
            .split("\n}")
            .next()
            .unwrap();
        let common = source
            .split("case \"$expanded_path\" in")
            .nth(1)
            .unwrap()
            .split("esac")
            .next()
            .unwrap();
        let script = format!("set -euo pipefail\napp_name=$1\n{assignments}\n_mole_is_shared_home_state_root() {{{shared}\n}}\nuser_patterns=()\n{gate} user_patterns+=(\n{}\n); fi\nfor expanded_path in \"${{user_patterns[@]+\"${{user_patterns[@]}}\"}}\"; do case \"$expanded_path\" in\n{common}\nesac\nif _mole_is_shared_home_state_root \"$expanded_path\"; then continue; fi\nprintf '%s\\0' \"$expanded_path\"; done", templates.join("\n")).replace("$HOME", "$MOOMOLE_TEST_ROOT");
        let t = temp();
        let home = t.path();
        let names = [
            "Suite Studio",
            "Suite  Studio",
            "A B",
            "中 X",
            "By Host",
            "Ep ic",
            "Suite\u{a0}Studio",
            "Suite　Studio",
            "$(printf danger)",
            "[A *]?",
            " X  ",
            "A\\ B",
        ];
        for name in names {
            let output = std::process::Command::new("/bin/bash")
                .args(["-c", &script, "space-templates", name])
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
            let actual = Check::begin(home, Some(name), &[], None)
                .finish()
                .1
                .into_iter()
                .filter(|e| {
                    SPACE_LOCATIONS
                        .iter()
                        .any(|(kind, _, _, _)| *kind == e.kind)
                })
                .map(|e| e.relative_path)
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{name}");
        }
        // The raw Epic location is filtered by this same upstream shared-root guard.
        assert!(source.contains("\"$HOME/Library/Application Support/Epic\""));
        eprintln!(
            "{} names matched original 12 compound Library templates and filters",
            names.len()
        );
    }
}
