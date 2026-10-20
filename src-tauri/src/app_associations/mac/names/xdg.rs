//! Fixed Mole's standard and compound-name XDG templates; no custom XDG env roots.
pub(super) const BASE: [(&str, &str, &str); 4] = [
    ("nameXdgConfig", ".config", "app_name"),
    ("nameXdgCache", ".cache", "app_name"),
    ("nameXdgLowerCache", ".cache", "lowercase_name"),
    ("nameXdgShare", ".local/share", "app_name"),
];
pub(super) const SPACE: [(&str, &str, &str); 9] = [
    ("nameXdgCompactConfig", ".config", "lowercase_nospace"),
    ("nameXdgHyphenConfig", ".config", "lowercase_hyphen"),
    ("nameXdgUnderscoreConfig", ".config", "lowercase_underscore"),
    ("nameXdgCompactCache", ".cache", "lowercase_nospace"),
    ("nameXdgHyphenCache", ".cache", "lowercase_hyphen"),
    ("nameXdgUnderscoreCache", ".cache", "lowercase_underscore"),
    ("nameXdgCompactShare", ".local/share", "lowercase_nospace"),
    ("nameXdgHyphenShare", ".local/share", "lowercase_hyphen"),
    (
        "nameXdgUnderscoreShare",
        ".local/share",
        "lowercase_underscore",
    ),
];
pub(super) fn lower(name: &str) -> String {
    // The original $(echo "$name" | tr ...) produces empty text for a single
    // echo option word. No command is run here. Otherwise C tr changes ASCII only.
    if name.strip_prefix('-').is_some_and(|flags| {
        !flags.is_empty() && flags.bytes().all(|b| matches!(b, b'n' | b'e' | b'E'))
    }) {
        String::new()
    } else {
        name.to_ascii_lowercase()
    }
}
pub(super) fn paths(name: &str) -> Vec<(&'static str, String)> {
    BASE.into_iter()
        .chain(
            if super::space_variants(name) {
                SPACE.as_slice()
            } else {
                &[]
            }
            .iter()
            .copied(),
        )
        .map(|(kind, root, variable)| {
            let derived = match variable {
                "app_name" => name.to_owned(),
                "lowercase_name" => lower(name),
                "lowercase_nospace" => lower(&name.replace(' ', "")),
                "lowercase_hyphen" => lower(&name.replace(' ', "-")),
                "lowercase_underscore" => lower(&name.replace(' ', "_")),
                _ => unreachable!(),
            };
            (kind, format!("{root}/{derived}"))
        })
        .collect()
}
pub(super) fn independent_cli(path: &str) -> bool {
    let Some((parent, name)) = path.rsplit_once('/') else {
        return false;
    };
    matches!(parent, ".config" | ".cache" | ".local/share")
        && matches!(
            name.strip_prefix('.')
                .unwrap_or(name)
                .to_ascii_lowercase()
                .as_str(),
            "claude" | "opencode" | "codex" | "gemini"
        )
}
pub(super) fn shared_root(path: &str) -> bool {
    matches!(
        path.trim_end_matches('/').to_ascii_lowercase().as_str(),
        ".cache"
            | ".config"
            | ".local"
            | ".local/bin"
            | ".local/lib"
            | ".local/share"
            | ".local/state"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    #[test]
    fn cli_exclusion_is_case_insensitive_single_dot_and_xdg_parent_scoped() {
        let t = super::super::tests::temp();
        let home = t.path();
        for name in ["claude", "Claude", ".CoDeX", "OpenCode", "GEMINI", "Co dex"] {
            let library = home.join(format!("Library/Caches/{name}"));
            fs::create_dir_all(&library).unwrap();
            fs::write(library.join("personal"), "keep Library too").unwrap();
            let result = super::super::tests::observe(home, Some(name));
            assert_eq!(result.names.code, "observed");
            assert_eq!(
                result.names.cli_excluded,
                if name == "claude" || name == "Co dex" {
                    3
                } else {
                    4
                },
                "{name}"
            );
            assert!(result
                .entries
                .iter()
                .all(|entry| !independent_cli(&entry.relative_path)));
            assert!(result
                .entries
                .iter()
                .any(|e| e.relative_path == format!("Library/Caches/{name}")
                    && e.status == "present"));
            assert_eq!(
                fs::read_to_string(library.join("personal")).unwrap(),
                "keep Library too"
            );
        }
        for path in [".config/Codex", ".cache/.CLAUDE", ".local/share/Gemini"] {
            assert!(independent_cli(path));
        }
        for path in [
            ".cache/..codex",
            ".config/codex-beta",
            ".config/codex.tools",
            ".config/claude-backup",
            "Library/Caches/Codex",
            ".other/codex",
        ] {
            assert!(!independent_cli(path));
        }
        // No XDG directory is opened when all XDG clues are independently excluded.
        let isolated = super::super::tests::temp();
        for root in [".config", ".cache", ".local"] {
            std::os::unix::fs::symlink(home, isolated.path().join(root)).unwrap();
        }
        let result = super::super::tests::observe(isolated.path(), Some("Codex"));
        assert_eq!(result.names.candidates, 24);
        assert_eq!(result.names.cli_excluded, 4);
        assert!(result
            .entries
            .iter()
            .all(|e| !e.kind.starts_with("nameXdg")));
    }

    #[test]
    fn xdg_metadata_revalidation_and_client_protection_remain_independent() {
        let t = super::super::tests::temp();
        let home = t.path();
        for (_, relative) in paths("Suite Studio") {
            fs::create_dir_all(home.join(&relative)).unwrap();
            fs::write(home.join(relative).join("personal"), "keep tool data").unwrap();
        }
        let check = super::super::Check::begin(home, Some("Suite Studio"), &[], None);
        let target = home.join(".config/suitestudio");
        fs::rename(&target, home.join("retained-tool-data")).unwrap();
        std::os::unix::fs::symlink(home.join("retained-tool-data"), &target).unwrap();
        let (scope, entries) = check.finish();
        assert_eq!(scope.candidates, 49);
        assert_eq!(
            entries
                .iter()
                .find(|e| e.kind == "nameXdgCompactConfig")
                .unwrap()
                .issue,
            Some("changed")
        );
        let mut result = super::super::tests::observe(home, Some("Suite Studio"));
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
            .find(|e| e.kind == "nameXdgCompactConfig")
            .unwrap();
        assert_eq!(entry.issue, Some("unsupported"));
        assert!(entry.protection_reasons.contains(&"clientDirectory"));
        assert_eq!(entry.mole_path_protection.code, "unknown");
        assert_eq!(
            fs::read_to_string(home.join("retained-tool-data/personal")).unwrap(),
            "keep tool data"
        );
        let check = super::super::Check::begin(home, Some("Codex"), &[], None);
        assert_eq!(check.scope.cli_excluded, 4);
        let (scope, entries) =
            check.finish_started(std::time::Instant::now() - std::time::Duration::from_secs(1));
        assert_eq!(scope.code, "unknown");
        assert_eq!(scope.cli_excluded, 0);
        assert!(entries.is_empty());
    }

    #[test]
    #[ignore = "requires fixed Mole source; compares standard/compound XDG templates and both original exclusion helpers"]
    fn prepared_xdg_templates_and_exclusions_match_original_c_locale() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        crate::app_uninstall_rules::load(&resources).unwrap();
        let source = fs::read_to_string(resources.join("lib/core/app_protection.sh")).unwrap();
        let base_gate = "if [[ -n \"$app_name\" && ${#app_name} -ge 2 ]]; then";
        let space_gate = "if [[ ${#app_name} -gt 3 && \"$app_name\" =~ [[:space:]] ]]; then";
        let extract = |gate: &str| {
            source
                .split(gate)
                .nth(1)
                .unwrap()
                .split("user_patterns+=(")
                .nth(1)
                .unwrap()
                .lines()
                .map(str::trim)
                .take_while(|line| *line != ")")
                .filter(|line| line.starts_with("\"$HOME/."))
                .collect::<Vec<_>>()
        };
        let base = extract(base_gate);
        let space = extract(space_gate);
        assert_eq!(base.len(), BASE.len());
        assert_eq!(space.len(), SPACE.len());
        for ((_, root, variable), line) in BASE
            .iter()
            .chain(SPACE.iter())
            .zip(base.iter().chain(space.iter()))
        {
            assert_eq!(*line, format!("\"$HOME/{root}/${variable}\""));
        }
        let assignments = [
            "nospace_name",
            "underscore_name",
            "hyphen_name",
            "lowercase_name",
            "lowercase_nospace",
            "lowercase_hyphen",
            "lowercase_underscore",
        ]
        .map(|var| {
            source
                .lines()
                .map(str::trim)
                .find(|line| line.starts_with(&format!("local {var}=")))
                .unwrap()
                .trim_start_matches("local ")
        })
        .join("\n");
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
        let script = format!("set -euo pipefail\napp_name=$1\n{assignments}\n{helpers}\nuser_patterns=()\n{base_gate} user_patterns+=(\n{}\n); fi\n{space_gate} user_patterns+=(\n{}\n); fi\nfor expanded_path in \"${{user_patterns[@]+\"${{user_patterns[@]}}\"}}\"; do case \"$expanded_path\" in\n{common}\nesac\nif _mole_is_shared_home_state_root \"$expanded_path\"; then continue; fi\nif _path_belongs_to_independent_cli \"$expanded_path\"; then printf 'CLI:%s\\0' \"$expanded_path\"; continue; fi\nprintf '%s\\0' \"$expanded_path\"; done", base.join("\n"), space.join("\n")).replace("$HOME", "$MOOMOLE_TEST_ROOT");
        let t = super::super::tests::temp();
        let home = t.path();
        let names = [
            "Example",
            "example",
            "微信",
            "ÜBER",
            "İAPP",
            "Suite Studio",
            "Suite  Studio",
            "A B",
            "中 X",
            "Suite\u{a0}Studio",
            "Claude",
            "claude",
            ".CoDeX",
            "..codex",
            "Co dex",
            "OpenCode",
            "GEMINI",
            "codex-beta",
            "Local",
            "Config",
            "Cache",
            "Epic",
            "[*]?",
            "$(printf danger)",
            "App\\Name",
            "-n",
            "-e",
            "-E",
            "-neE",
            "-NE",
            "- e",
        ];
        for name in names {
            let output = std::process::Command::new("/bin/bash")
                .args(["-c", &script, "xdg-templates", name])
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
            let mut seen = std::collections::HashSet::new();
            let mut excluded = std::collections::HashSet::new();
            let mut expected = Vec::new();
            for record in output.stdout.split(|b| *b == 0).filter(|b| !b.is_empty()) {
                let value = std::str::from_utf8(record).unwrap();
                if let Some(path) = value.strip_prefix("CLI:") {
                    excluded.insert(path.to_owned());
                } else {
                    let relative = Path::new(value)
                        .strip_prefix(home)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned();
                    if seen.insert(relative.clone()) {
                        expected.push(relative);
                    }
                }
            }
            let (scope, entries) = super::super::Check::begin(home, Some(name), &[], None).finish();
            let actual = entries
                .into_iter()
                .filter(|entry| entry.kind.starts_with("nameXdg"))
                .map(|e| e.relative_path)
                .collect::<Vec<_>>();
            assert_eq!(scope.cli_excluded, excluded.len(), "{name}");
            assert_eq!(actual, expected, "{name}");
        }
        eprintln!(
            "{} names matched original 13 XDG templates and both exclusion helpers",
            names.len()
        );
    }
}
