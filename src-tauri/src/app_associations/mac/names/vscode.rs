//! Pinned Mole's VS Code-specific literal directory block. This regex is a
//! discovery clue, not an authenticated product, vendor or exclusive ownership.
pub(super) fn channel(id: Option<&str>) -> Option<&'static str> {
    let id = id.filter(|id| crate::app_associations::valid_id(id))?;
    let (_, tail) = id.split_once("microsoft")?;
    if !tail.as_bytes().windows(6).any(|p| {
        matches!(p[0], b'v' | b'V')
            && matches!(p[1], b's' | b'S')
            && matches!(p[2], b'c' | b'C')
            && &p[3..] == b"ode"
    }) {
        return None;
    }
    Some(if id.contains("insiders") || id.contains("Insiders") {
        "insiders"
    } else {
        "stable"
    })
}
pub(super) fn paths(channel: Option<&str>) -> Vec<(&'static str, String)> {
    let Some(channel) = channel else {
        return vec![];
    };
    let insiders = channel == "insiders";
    [
        (
            "nameVSCodeShipItStable",
            "Library/Caches/com.microsoft.VSCode.ShipIt",
        ),
        (
            "nameVSCodeShipItInsiders",
            "Library/Caches/com.microsoft.VSCodeInsiders.ShipIt",
        ),
        (
            "nameVSCodeExtensions",
            if insiders {
                ".vscode-insiders"
            } else {
                ".vscode"
            },
        ),
        (
            "nameVSCodeSupport",
            if insiders {
                "Library/Application Support/Code - Insiders"
            } else {
                "Library/Application Support/Code"
            },
        ),
        (
            "nameVSCodeCache",
            if insiders {
                "Library/Caches/com.microsoft.VSCodeInsiders"
            } else {
                "Library/Caches/com.microsoft.VSCode"
            },
        ),
    ]
    .into_iter()
    .map(|(kind, path)| (kind, path.into()))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};
    #[test]
    fn original_regex_case_substrings_and_channels_are_preserved() {
        for (id, expected) in [
            ("com.microsoft.VSCode", Some("stable")),
            ("com.microsoft.VSCodeInsiders", Some("insiders")),
            ("com.microsoft.vscode-insiders", Some("insiders")),
            ("org.notmicrosoft.foo.vScodeSomething", Some("stable")),
            ("com.microsoft.VSCodeINSIDERS", Some("stable")),
            ("com.Microsoft.VSCode", None),
            ("com.microsoft.VSCODE", None),
            ("com.microsoft.code", None),
            ("com.microsoft.VSCode/other", None),
            ("com..microsoft.VSCode", None),
            ("com.cursor.editor", None),
        ] {
            assert_eq!(channel(Some(id)), expected, "{id}");
        }
        assert_eq!(channel(None), None);
    }
    #[test]
    fn directories_are_clues_rechecked_and_kept_with_personal_data() {
        let t = super::super::tests::temp();
        let home = t.path();
        for (_, relative) in paths(Some("stable"))
            .into_iter()
            .chain(paths(Some("insiders")))
        {
            let path = home.join(relative);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("personal"), "keep credentials and extensions").unwrap();
        }
        let id = "com.microsoft.VSCode";
        let observe = || {
            crate::app_associations::mac::Check::begin(home, Some(id), "matches", true)
                .with_name(home, Some("Code"))
        };
        let result = observe().finish();
        assert_eq!(result.names.vscode_channel, Some("stable"));
        assert_eq!(result.names.candidates, 31);
        assert_eq!(
            result
                .entries
                .iter()
                .filter(|e| e.kind.starts_with("nameVSCode"))
                .count(),
            3
        );
        assert_eq!(
            result
                .entries
                .iter()
                .filter(|e| e.relative_path == "Library/Caches/com.microsoft.VSCode")
                .count(),
            1
        );
        assert_eq!(
            result
                .entries
                .iter()
                .find(|e| e.relative_path == "Library/Caches/com.microsoft.VSCode")
                .unwrap()
                .kind,
            "cache"
        );
        assert_eq!(
            result
                .entries
                .iter()
                .find(|e| e.relative_path == "Library/Application Support/Code")
                .unwrap()
                .kind,
            "nameSupport"
        );
        assert!(!result
            .entries
            .iter()
            .any(|e| e.relative_path == ".vscode-insiders"));
        let check = observe();
        let target = home.join(".vscode");
        fs::rename(&target, home.join("retained-vscode")).unwrap();
        std::os::unix::fs::symlink(home.join("retained-vscode"), &target).unwrap();
        assert_eq!(
            check
                .finish()
                .entries
                .iter()
                .find(|e| e.relative_path == ".vscode")
                .unwrap()
                .issue,
            Some("changed")
        );
        let mut result = observe().finish();
        result
            .attach_protection(
                home,
                Some(id),
                "org.client",
                &[target.to_string_lossy().into_owned()],
            )
            .unwrap();
        result.attach_mole_paths(
            None,
            home,
            &mut crate::app_uninstall_rules::PathBudget::default(),
        );
        let row = result
            .entries
            .iter()
            .find(|e| e.relative_path == ".vscode")
            .unwrap();
        assert_eq!(row.issue, Some("unsupported"));
        assert!(row.protection_reasons.contains(&"clientDirectory"));
        assert_eq!(row.mole_path_protection.code, "unknown");
        assert_eq!(
            fs::read_to_string(home.join("retained-vscode/personal")).unwrap(),
            "keep credentials and extensions"
        );
        let check = super::super::Check::begin(home, Some("Code"), &[], Some(id));
        let (scope, entries) =
            check.finish_started(std::time::Instant::now() - std::time::Duration::from_secs(1));
        assert!(scope.vscode_channel.is_none() && entries.is_empty());
        let maximum = super::super::Check::begin(
            home,
            Some("Suite Nightly"),
            &[],
            Some("org.microsoft.SuiteNightlyVSCode"),
        )
        .finish();
        assert_eq!(maximum.0.candidates, 73);
        assert_eq!(maximum.0.vscode_channel, Some("stable"));
        let no_name = crate::app_associations::mac::Check::begin(home, Some(id), "matches", true)
            .with_name(home, None)
            .finish();
        assert_eq!(no_name.names.issue, Some("nameUnavailable"));
        assert!(no_name.names.vscode_channel.is_none());
    }
    #[test]
    fn files_and_links_never_satisfy_the_original_directory_condition() {
        let t = super::super::tests::temp();
        let home = t.path();
        fs::write(home.join(".vscode-insiders"), "not a directory").unwrap();
        let result = crate::app_associations::mac::Check::begin(
            home,
            Some("com.microsoft.VSCodeInsiders"),
            "matches",
            true,
        )
        .with_name(home, Some("Visual Studio Code Insiders"))
        .finish();
        assert_eq!(result.names.vscode_channel, Some("insiders"));
        let row = result
            .entries
            .iter()
            .find(|e| e.kind == "nameVSCodeExtensions")
            .unwrap();
        assert_eq!((row.status, row.issue), ("unknown", Some("unsupported")));
        assert_eq!(
            fs::read_to_string(home.join(".vscode-insiders")).unwrap(),
            "not a directory"
        );
        assert!(!result.entries.iter().any(|e| e.relative_path == ".vscode"));
    }
    #[test]
    #[ignore = "requires fixed Mole source; compares original VS Code branch predicates and directory templates"]
    fn prepared_vscode_paths_match_original_c_locale() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        crate::app_uninstall_rules::load(&resources).unwrap();
        let source = fs::read_to_string(resources.join("lib/core/app_protection.sh")).unwrap();
        let gate = "if [[ \"$bundle_id\" =~ microsoft.*[vV][sS][cC]ode ]]; then";
        let block = format!(
            "{gate}{}",
            source
                .split(gate)
                .nth(1)
                .unwrap()
                .split("# Docker:")
                .next()
                .unwrap()
        );
        let t = super::super::tests::temp();
        let home = t.path();
        for (_, relative) in paths(Some("stable"))
            .into_iter()
            .chain(paths(Some("insiders")))
        {
            fs::create_dir_all(home.join(relative)).unwrap();
        }
        let script = format!(
            r#"observe() {{
local bundle_id="$1" files_to_clean=()
{block}
for path in "${{files_to_clean[@]}}"; do printf '%s\0' "$path"; done
}}
observe "$1"
"#,
            block = block.replace("$HOME/", "$MOOMOLE_TEST_ROOT/")
        );
        let mut ids = vec![
            "com.microsoft.VSCode".to_owned(),
            "com.Microsoft.VSCode".into(),
            "org.microsofteditor.VSCodeInsidersExtra".into(),
            "com.cursor.editor".into(),
        ];
        for v in ["v", "V"] {
            for s in ["s", "S"] {
                for c in ["c", "C"] {
                    for suffix in [
                        "",
                        "Insiders",
                        "insiders",
                        "INSIDERS",
                        "-insiders",
                        "-preview",
                    ] {
                        ids.push(format!("org.microsoft.tool.{v}{s}{c}ode{suffix}"));
                    }
                }
            }
        }
        for id in &ids {
            let output = std::process::Command::new("/bin/bash")
                .args(["-c", &script, "vscode-parity", id])
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
                .map(|p| {
                    Path::new(std::str::from_utf8(p).unwrap())
                        .strip_prefix(home)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                paths(channel(Some(id)))
                    .into_iter()
                    .map(|(_, p)| p)
                    .collect::<Vec<_>>(),
                expected,
                "{id}"
            );
        }
        // Original -d excludes an ordinary file; our read-only observation keeps
        // the unsupported type explicit instead of treating it as absent.
        fs::remove_dir(home.join(".vscode-insiders")).unwrap();
        fs::write(home.join(".vscode-insiders"), "keep").unwrap();
        let output = std::process::Command::new("/bin/bash")
            .args(["-c", &script, "vscode-type", "com.microsoft.VSCodeInsiders"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .env("MOOMOLE_TEST_ROOT", home)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            output
                .stdout
                .split(|b| *b == 0)
                .filter(|b| !b.is_empty())
                .count(),
            4
        );
        eprintln!(
            "{} valid IDs matched original VS Code branch and templates",
            ids.len()
        );
    }
}
