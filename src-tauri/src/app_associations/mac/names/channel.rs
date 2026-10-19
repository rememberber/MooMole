//! Pinned Mole's single channel-suffix base-name block, not a publisher identity.
pub(super) const LOCATIONS: [(&str, &str, &str); 9] = [
    ("nameChannelSupport", "Library/Application Support", ""),
    ("nameChannelCache", "Library/Caches", ""),
    ("nameChannelLogs", "Library/Logs", ""),
    ("nameChannelPreferencesDirectory", "Library/Preferences", ""),
    ("nameChannelPreferences", "Library/Preferences", ".plist"),
    (
        "nameChannelSavedState",
        "Library/Saved Application State",
        ".savedState",
    ),
    ("nameChannelXdgConfig", ".config", ""),
    ("nameChannelXdgCache", ".cache", ""),
    ("nameChannelXdgShare", ".local/share", ""),
];
pub(super) fn base(name: &str) -> Option<&str> {
    let base = crate::app_shared_rules::channel_base_name(name);
    (base != name && base.len() > 2).then_some(base)
}
pub(super) fn paths(name: &str) -> Vec<(&'static str, String)> {
    let Some(base) = base(name) else {
        return vec![];
    };
    LOCATIONS
        .into_iter()
        .map(|(kind, root, suffix)| {
            let leaf = if root.starts_with('.') {
                super::xdg::lower(base)
            } else {
                base.to_owned()
            };
            (kind, format!("{root}/{leaf}{suffix}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    #[test]
    fn channel_base_keeps_greedy_single_suffix_and_c_byte_gate() {
        for (name, expected) in [
            ("Zed Nightly", Some("Zed")),
            ("Zed  Nightly", Some("Zed ")),
            ("Zed Beta Nightly", Some("Zed Beta")),
            ("Firefox Developer Edition", Some("Firefox")),
            ("Safari Technology Preview", Some("Safari Technology")),
            ("App beta", None),
            ("App Beta ", None),
            ("AppBeta", None),
            ("AB Beta", None),
            ("中 Beta", Some("中")),
            ("Microsoft Edge", Some("Microsoft")),
            ("App Beta.app", None),
            ("App\u{a0}Beta", None),
            ("App Developer  Edition", None),
        ] {
            assert_eq!(base(name), expected, "{name}");
        }
    }

    #[test]
    fn channel_clues_preserve_metadata_data_and_protection_after_replacement() {
        let t = super::super::tests::temp();
        let home = t.path();
        for (index, (_, relative)) in paths("Suite Nightly").into_iter().enumerate() {
            let path = home.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if index % 2 == 0 {
                fs::create_dir(&path).unwrap();
                fs::write(path.join("personal"), "keep channel data").unwrap();
            } else {
                fs::write(path, "keep channel file").unwrap();
            }
        }
        let result = super::super::tests::observe(home, Some("Suite Nightly"));
        assert_eq!(result.names.candidates, 58);
        assert_eq!(result.names.base_name.as_deref(), Some("Suite"));
        assert!(result
            .entries
            .iter()
            .filter(|e| e.kind.starts_with("nameChannel"))
            .all(|e| e.status == "present"));
        let check = super::super::Check::begin(home, Some("Suite Nightly"), &[]);
        let target = home.join("Library/Application Support/Suite");
        fs::rename(&target, home.join("retained-channel-data")).unwrap();
        std::os::unix::fs::symlink(home.join("retained-channel-data"), &target).unwrap();
        let (_, entries) = check.finish();
        assert_eq!(
            entries
                .iter()
                .find(|e| e.kind == "nameChannelSupport")
                .unwrap()
                .issue,
            Some("changed")
        );
        let mut result = super::super::tests::observe(home, Some("Suite Nightly"));
        result
            .attach_protection(
                home,
                Some("org.example.app"),
                "org.client",
                &[target.to_string_lossy().into_owned()],
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
            .find(|e| e.kind == "nameChannelSupport")
            .unwrap();
        assert_eq!(entry.issue, Some("unsupported"));
        assert!(entry.protection_reasons.contains(&"clientDirectory"));
        assert_eq!(entry.mole_path_protection.code, "unknown");
        assert_eq!(
            fs::read_to_string(home.join("retained-channel-data/personal")).unwrap(),
            "keep channel data"
        );
    }

    #[test]
    fn cli_roots_exact_sources_and_scope_withdrawal_keep_channel_guards() {
        let t = super::super::tests::temp();
        let home = t.path();
        for (name, count, cli) in [
            ("Codex Nightly", 55, 3),
            ("ByHost Beta", 57, 0),
            ("Epic Beta", 57, 0),
            ("org.example.app Nightly", 52, 0),
        ] {
            let result = super::super::tests::observe(home, Some(name));
            assert_eq!(result.names.candidates, count, "{name}");
            assert_eq!(result.names.cli_excluded, cli, "{name}");
            assert!(result
                .entries
                .iter()
                .all(|e| !super::super::xdg::independent_cli(&e.relative_path)
                    && !super::super::excluded(&e.relative_path)));
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
        let check = super::super::Check::begin(home, Some("Codex Nightly"), &[]);
        let (scope, entries) =
            check.finish_started(std::time::Instant::now() - std::time::Duration::from_secs(1));
        assert_eq!(scope.code, "unknown");
        assert_eq!(scope.cli_excluded, 0);
        assert!(scope.base_name.is_none());
        assert!(entries.is_empty());
    }

    #[test]
    #[ignore = "requires fixed Mole source; compares original channel regex/gate/templates and exclusions"]
    fn prepared_channel_base_and_paths_match_original_c_locale() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        crate::app_uninstall_rules::load(&resources).unwrap();
        let source = fs::read_to_string(resources.join("lib/core/app_protection.sh")).unwrap();
        let prefix = "local base_name=\"$app_name\"";
        let normalization = format!(
            "{prefix}{}",
            source
                .split(prefix)
                .nth(1)
                .unwrap()
                .split("# Only use bundle_id")
                .next()
                .unwrap()
        )
        .lines()
        .map(|line| line.trim().strip_prefix("local ").unwrap_or(line.trim()))
        .collect::<Vec<_>>()
        .join("\n");
        let gate = "if [[ \"$base_name\" != \"$app_name\" && ${#base_name} -gt 2 ]]; then";
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
            .filter(|line| line.starts_with("\"$HOME/"))
            .collect::<Vec<_>>();
        assert_eq!(templates.len(), 9);
        for ((_, root, suffix), line) in LOCATIONS.iter().zip(&templates) {
            let variable = if root.starts_with('.') {
                "base_lowercase"
            } else {
                "base_name"
            };
            assert_eq!(*line, format!("\"$HOME/{root}/${variable}{suffix}\""));
        }
        let helpers = [
            "_mole_is_shared_home_state_root",
            "_path_belongs_to_independent_cli",
        ]
        .map(|name| {
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
        })
        .join("\n");
        let common = source
            .split("case \"$expanded_path\" in")
            .nth(1)
            .unwrap()
            .split("esac")
            .next()
            .unwrap();
        let script = format!("set -euo pipefail\napp_name=$1\n{normalization}\n{helpers}\nuser_patterns=()\n{gate} printf 'BASE:%s\\0' \"$base_name\"; user_patterns+=(\n{}\n); fi\nfor expanded_path in \"${{user_patterns[@]+\"${{user_patterns[@]}}\"}}\"; do case \"$expanded_path\" in\n{common}\nesac\nif _mole_is_shared_home_state_root \"$expanded_path\"; then continue; fi\nif _path_belongs_to_independent_cli \"$expanded_path\"; then printf 'CLI:%s\\0' \"$expanded_path\"; continue; fi\nprintf '%s\\0' \"$expanded_path\"; done", templates.join("\n")).replace("$HOME", "$MOOMOLE_TEST_ROOT");
        let t = super::super::tests::temp();
        let home = t.path();
        let mut names = [
            "Zed  Nightly",
            "Zed Beta Nightly",
            "Firefox Developer Edition",
            "Safari Technology Preview",
            "Microsoft Edge",
            "App beta",
            "App Beta ",
            "AppBeta",
            "AB Beta",
            "中 Beta",
            "App\u{a0}Beta",
            "App Developer  Edition",
            "Codex Nightly",
            ".CLAUDE Beta",
            "ByHost Beta",
            "Epic Beta",
            "$(printf danger) Beta",
            "[*]? Nightly",
            "App\\Name Preview",
            "    Beta",
            " .. Beta",
            "-eE Beta",
        ]
        .map(str::to_owned)
        .to_vec();
        names.extend(
            [
                "Nightly",
                "Beta",
                "Alpha",
                "Dev",
                "Canary",
                "Preview",
                "Insider",
                "Edge",
                "Stable",
                "Release",
                "RC",
                "LTS",
                "Developer Edition",
                "Technology Preview",
            ]
            .map(|suffix| format!("Demo {suffix}")),
        );
        for name in &names {
            let output = std::process::Command::new("/bin/bash")
                .args(["-c", &script, "channel-templates", name])
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("LC_ALL", "C")
                .env("MOOMOLE_TEST_ROOT", home)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let mut expected_base = None;
            let mut expected_cli = std::collections::HashSet::new();
            let mut expected = Vec::new();
            for record in output.stdout.split(|b| *b == 0).filter(|b| !b.is_empty()) {
                let value = std::str::from_utf8(record).unwrap();
                if let Some(base) = value.strip_prefix("BASE:") {
                    expected_base = Some(base.to_owned());
                } else if let Some(path) = value.strip_prefix("CLI:") {
                    expected_cli.insert(path.to_owned());
                } else {
                    expected.push(
                        Path::new(value)
                            .strip_prefix(home)
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .to_owned(),
                    );
                }
            }
            let (scope, entries) = super::super::Check::begin(home, Some(name), &[]).finish();
            assert_eq!(scope.base_name, expected_base, "{name}");
            assert_eq!(scope.cli_excluded, expected_cli.len(), "{name}");
            let actual = entries
                .into_iter()
                .filter(|e| e.kind.starts_with("nameChannel"))
                .map(|e| e.relative_path)
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{name}");
        }
        eprintln!(
            "{} names matched original channel base/gate, 9 templates and exclusions",
            names.len()
        );
    }
}
