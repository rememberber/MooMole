//! Pinned Mole's guarded bundle-leaf derivation; never a publisher/ownership claim.
pub(super) const LOCATIONS: [(&str, &str, &str); 5] = [
    ("nameLeafSupport", "Library/Application Support", ""),
    ("nameLeafCache", "Library/Caches", ""),
    ("nameLeafLogs", "Library/Logs", ""),
    ("nameLeafPreferences", "Library/Preferences", ".plist"),
    (
        "nameLeafSavedState",
        "Library/Saved Application State",
        ".savedState",
    ),
];
fn split_rest(rest: &str) -> String {
    // Two ASCII passes mirror the original C-locale sed substitutions. Splitting
    // only the suffix preserves the supplied display name, including its spaces.
    let bytes = rest.as_bytes();
    let mut acronym = String::new();
    for (i, &byte) in bytes.iter().enumerate() {
        if i > 0
            && bytes[i - 1].is_ascii_uppercase()
            && byte.is_ascii_uppercase()
            && bytes.get(i + 1).is_some_and(u8::is_ascii_lowercase)
        {
            acronym.push(' ');
        }
        acronym.push(byte as char);
    }
    let mut spaced = String::new();
    let mut previous = b' ';
    for byte in acronym.bytes() {
        if (previous.is_ascii_lowercase() || previous.is_ascii_digit()) && byte.is_ascii_uppercase()
        {
            spaced.push(' ');
        }
        spaced.push(byte as char);
        previous = byte;
    }
    spaced
}
pub(super) fn variants(name: &str, id: Option<&str>) -> Vec<String> {
    let Some(id) = id.filter(|id| crate::app_associations::valid_id(id)) else {
        return vec![];
    };
    if !super::supported(name) || name.len() < 3 {
        return vec![];
    }
    let leaf = id.rsplit('.').next().unwrap();
    let compact = name.replace(' ', "");
    if leaf.len() < 8
        || compact.len() < 3
        || leaf == name
        || !leaf
            .as_bytes()
            .windows(2)
            .any(|p| p[0].is_ascii_lowercase() && p[1].is_ascii_uppercase())
        || leaf.len() <= compact.len()
        || !leaf
            .to_ascii_lowercase()
            .starts_with(&compact.to_ascii_lowercase())
    {
        return vec![];
    }
    // The valid reverse-DNS leaf is ASCII, so the matched prefix and slice are
    // C byte offsets even when the display name is not ASCII (which cannot match).
    let rest = &leaf[compact.len()..];
    if !(rest.as_bytes()[0].is_ascii_uppercase() || rest.as_bytes()[0].is_ascii_digit()) {
        return vec![];
    }
    vec![leaf.into(), format!("{name} {}", split_rest(rest))]
}
pub(super) fn paths(names: &[String]) -> Vec<(&'static str, String)> {
    names
        .iter()
        .flat_map(|name| {
            LOCATIONS.map(|(kind, root, suffix)| (kind, format!("{root}/{name}{suffix}")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};
    #[test]
    fn requires_display_name_extension_and_original_word_boundary() {
        for (name, id, expected) in [
            (
                "AyuGram",
                "one.ayugram.AyuGramDesktop",
                Some("AyuGram Desktop"),
            ),
            (
                "Ayu Gram",
                "one.ayugram.AyuGramDesktop",
                Some("Ayu Gram Desktop"),
            ),
            (
                "ayugram",
                "org.test.AyuGramHTTPServer",
                Some("ayugram HTTP Server"),
            ),
            (
                "AyuGram",
                "org.test.AyuGram2FAThing",
                Some("AyuGram 2 FA Thing"),
            ),
            ("Wrapper", "com.wrapper.GoogleChrome", None),
            ("64Gram", "org.fork.TelegramDesktop", None),
            ("AyuGram", "org.test.AyuGramdesktop", None),
            ("AyuGramDesktop", "org.test.AyuGramDesktop", None),
            ("AyuGram", "org.test.ayugramdesktop", None),
            ("AB", "org.test.ABDesktop", None),
            ("中", "org.test.AppDesktop", None),
            ("AyuGram", "org.test/Bad.AyuGramDesktop", None),
            ("AyuGram", "org..AyuGramDesktop", None),
            ("AyuGram", "org.test.AyuGram_Desktop", None),
        ] {
            let actual = variants(name, Some(id));
            assert_eq!(actual.get(1).map(String::as_str), expected, "{name}/{id}");
            if expected.is_some() {
                assert_eq!(actual[0], id.rsplit('.').next().unwrap());
            } else {
                assert!(actual.is_empty());
            }
        }
        assert!(variants("AyuGram", None).is_empty());
    }
    #[test]
    fn leaf_metadata_is_bound_to_identity_rechecked_and_never_changes_data() {
        let t = super::super::tests::temp();
        let home = t.path();
        let name = "AyuGram";
        let id = "one.ayugram.AyuGramDesktop";
        for (index, (_, relative)) in paths(&variants(name, Some(id))).iter().enumerate() {
            let path = home.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if index % 2 == 0 {
                fs::create_dir(&path).unwrap();
                fs::write(path.join("personal"), "keep all data").unwrap();
            } else {
                fs::write(path, "keep file").unwrap();
            }
        }
        let observe = || {
            crate::app_associations::mac::Check::begin(home, Some(id), "matches", true)
                .with_name(home, Some(name))
        };
        let result = observe().finish();
        assert_eq!(
            result.names.leaf_names,
            ["AyuGramDesktop", "AyuGram Desktop"]
        );
        assert_eq!(result.names.candidates, 38);
        assert_eq!(
            result
                .entries
                .iter()
                .filter(|e| e.kind.starts_with("nameLeaf") && e.status == "present")
                .count(),
            10
        );
        let check = observe();
        let target = home.join("Library/Application Support/AyuGramDesktop");
        fs::rename(&target, home.join("retained")).unwrap();
        std::os::unix::fs::symlink(home.join("retained"), &target).unwrap();
        let result = check.finish();
        assert_eq!(
            result
                .entries
                .iter()
                .find(|e| e.relative_path == "Library/Application Support/AyuGramDesktop")
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
            .find(|e| e.relative_path == "Library/Application Support/AyuGramDesktop")
            .unwrap();
        assert_eq!(row.issue, Some("unsupported"));
        assert!(row.protection_reasons.contains(&"clientDirectory"));
        assert_eq!(row.mole_path_protection.code, "unknown");
        assert_eq!(
            fs::read_to_string(home.join("retained/personal")).unwrap(),
            "keep all data"
        );
        let (scope, entries) = super::super::Check::begin(home, Some(name), &[], Some(id))
            .finish_started(std::time::Instant::now() - std::time::Duration::from_secs(1));
        assert_eq!(scope.code, "unknown");
        assert!(scope.leaf_names.is_empty() && entries.is_empty());
        let unknown = crate::app_associations::mac::Check::begin(home, Some(id), "different", true)
            .with_name(home, Some(name))
            .finish();
        assert!(unknown.entries.is_empty() && unknown.names.leaf_names.is_empty());
        let maximum = super::super::Check::begin(
            home,
            Some("Ayu Gram Nightly"),
            &[],
            Some("org.test.AyuGramNightlyDesktop"),
        )
        .finish();
        assert_eq!(maximum.0.candidates, 68);
        assert_eq!(
            maximum.0.leaf_names,
            ["AyuGramNightlyDesktop", "Ayu Gram Nightly Desktop"]
        );
        let long_id = format!("org.AyuGram{}", "Ab".repeat(120));
        let long_names = variants(name, Some(&long_id));
        assert_eq!(long_names.len(), 2);
        let (_, long_entries) =
            super::super::Check::begin(home, Some(name), &[], Some(&long_id)).finish();
        let long_saved = format!(
            "Library/Saved Application State/{}.savedState",
            long_names[0]
        );
        assert_eq!(
            long_entries
                .iter()
                .find(|e| e.relative_path == long_saved)
                .unwrap()
                .status,
            "unknown"
        );
        // A long derived component is kept literal; metadata can stay unknown,
        // never truncated into a different product's directory.
        let long = format!("org.{}", "AyuGramX".to_owned() + &"Ab".repeat(124));
        assert!(variants(name, Some(&long)).is_empty()); // ID exceeds the 255 byte bound.
    }
    #[test]
    #[ignore = "requires fixed Mole source; compares original leaf guards, sed and literal templates"]
    fn prepared_bundle_leaf_matches_original_c_locale() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        crate::app_uninstall_rules::load(&resources).unwrap();
        let source = fs::read_to_string(resources.join("lib/core/app_protection.sh")).unwrap();
        let gate = "if [[ \"$bundle_id_valid\" == \"true\" && ${#app_name} -ge 3 ]]; then";
        let block = format!(
            "{gate}{}",
            source
                .split(gate)
                .nth(1)
                .unwrap()
                .split("# Add all naming variants")
                .next()
                .unwrap()
        );
        let templates = block
            .split("user_patterns+=(")
            .nth(1)
            .unwrap()
            .lines()
            .map(str::trim)
            .take_while(|l| *l != ")")
            .filter(|l| l.starts_with("\"$HOME/"))
            .collect::<Vec<_>>();
        assert_eq!(templates.len(), 5);
        for ((_, root, suffix), line) in LOCATIONS.iter().zip(templates) {
            assert_eq!(
                line,
                format!("\"$HOME/{root}/$bundle_leaf_variant{suffix}\"")
            );
        }
        let base = fs::read_to_string(resources.join("lib/core/base.sh")).unwrap();
        let validator = format!(
            "mole_is_reverse_dns_bundle_id() {{{}\n}}",
            base.split("mole_is_reverse_dns_bundle_id() {")
                .nth(1)
                .unwrap()
                .split("\n}")
                .next()
                .unwrap()
        );
        let script = format!(
            r#"{validator}
observe() {{
 local app_name="$1" bundle_id="$2" bundle_id_valid=false
 local user_patterns=()
 mole_is_reverse_dns_bundle_id "$bundle_id" && bundle_id_valid=true
 {block}
 for path in "${{user_patterns[@]}}"; do printf '%s\0' "$path"; done
}}
observe "$1" "$2"
"#,
            block = block.replace("$HOME/", "$MOOMOLE_TEST_ROOT/")
        );
        let t = super::super::tests::temp();
        let mut cases = vec![];
        for name in [
            "AyuGram",
            "Ayu Gram",
            "ayugram",
            "AYUGRAM",
            "Wrapper",
            "64Gram",
            "Ayu",
            "AB",
            "中",
            "AyuGram ",
            "AyuGramDesktop",
            "-neE",
            "Ayu*",
            "Ayu\u{a0}Gram",
        ] {
            for leaf in [
                "AyuGramDesktop",
                "AyuGramHTTPServer",
                "AyuGram2FAThing",
                "AyuGramdesktop",
                "ayugramdesktop",
                "GoogleChrome",
                "TelegramDesktop",
                "AyuDesktop",
                "ABServer",
                "AyuGram-Desktop",
            ] {
                cases.push((name.to_owned(), format!("org.test.{leaf}")));
            }
        }
        cases.extend([
            ("AyuGram".into(), "org..AyuGramDesktop".into()),
            ("AyuGram".into(), "org.test/AyuGramDesktop".into()),
            ("AyuGram".into(), format!("org.AyuGram{}", "Ab".repeat(120))),
        ]);
        for (name, id) in &cases {
            let output = std::process::Command::new("/bin/bash")
                .args(["-c", &script, "leaf-parity", name, id])
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("LC_ALL", "C")
                .env("MOOMOLE_TEST_ROOT", t.path())
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
                        .strip_prefix(t.path())
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                paths(&variants(name, Some(id)))
                    .into_iter()
                    .map(|(_, p)| p)
                    .collect::<Vec<_>>(),
                expected,
                "{name}/{id}"
            );
        }
        eprintln!("{} original bundle-leaf name/ID pairs matched", cases.len());
    }
}
