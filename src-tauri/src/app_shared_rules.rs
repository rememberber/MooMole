//! Retention-only semantic adapter for the pinned uninstall shared-remnant predicate.
//! A match never establishes ownership or creates a deletion candidate.
const SUFFIXES: [&str; 14] = [
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
];
pub(crate) fn channel_base_name(name: &str) -> &str {
    // POSIX ERE's greedy first capture keeps the longest prefix, strips one
    // separator byte, and removes only one suffix. LC_ALL=C counts bytes.
    let end = SUFFIXES
        .iter()
        .filter_map(|suffix| {
            let prefix = name.strip_suffix(suffix)?;
            let end = prefix.len().checked_sub(1)?;
            (end > 0
                && matches!(
                    prefix.as_bytes()[end],
                    b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c
                ))
            .then_some(end)
        })
        .max()
        .unwrap_or(name.len());
    &name[..end]
}
fn normalized_name(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or("");
    let name = if name.len() >= 4
        && name
            .get(name.len() - 4..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".app"))
    {
        &name[..name.len() - 4]
    } else {
        name
    };
    channel_base_name(name).to_ascii_lowercase()
}
pub fn relation(
    selected_id: &str,
    other_id: &str,
    selected_path: &str,
    other_path: &str,
) -> Option<&'static str> {
    let selected = selected_id.to_ascii_lowercase();
    let other = other_id.to_ascii_lowercase();
    if selected == other {
        return Some("sameIdentifier");
    }
    if selected
        .strip_prefix(&other)
        .is_some_and(|rest| rest.starts_with('.'))
        || other
            .strip_prefix(&selected)
            .is_some_and(|rest| rest.starts_with('.'))
    {
        return Some("relatedIdentifier");
    }
    let selected = normalized_name(selected_path);
    let other = normalized_name(other_path);
    (selected.len() >= 2 && other.len() >= 2 && selected == other).then_some("channelName")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retention_uses_id_boundaries_and_exact_channel_semantics() {
        assert_eq!(
            relation("COM.Example.App", "com.example.app", "/A.app", "/B.app"),
            Some("sameIdentifier")
        );
        assert_eq!(
            relation("org.app", "org.app.share", "/A.app", "/B.app"),
            Some("relatedIdentifier")
        );
        assert_eq!(
            relation("org.app.share", "org.app", "/A.app", "/B.app"),
            Some("relatedIdentifier")
        );
        assert_eq!(relation("org.app", "org.apple", "/A.app", "/B.app"), None);
        for (a, b, expected) in [
            ("Foo Beta", "Foo", true),
            ("Foo beta", "Foo", false),
            ("Foo  Beta", "Foo", false),
            ("Foo Technology Preview", "Foo", false),
            ("Foo Technology Preview", "Foo Technology", true),
            ("Foo Developer Edition", "Foo", true),
            ("A Beta", "A", false),
            ("中文 Beta", "中文", true),
            ("Ä Beta", "ä", false),
            ("Foo Beta Beta", "Foo Beta", false),
        ] {
            assert_eq!(
                relation("org.a", "org.b", &format!("/{a}.app"), &format!("/{b}.APP")).is_some(),
                expected,
                "{a} / {b}"
            );
        }
    }
    #[test]
    #[ignore = "requires prepared fixed Mole source; calls only extracted read-only predicates"]
    fn prepared_shared_predicate_matches_retention_adapter() {
        use std::{path::Path, process::Command};
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        crate::app_uninstall_rules::load(&root).unwrap();
        let batch = std::fs::read_to_string(root.join("lib/uninstall/batch.sh")).unwrap();
        let base = std::fs::read_to_string(root.join("lib/core/base.sh")).unwrap();
        let extract = crate::app_background::test_function;
        let script = format!("set -euo pipefail\n{}\n{}\n{}\nwhile (( $# >= 4 )); do if uninstall_bundles_share_remnants \"$1\" \"$2\" \"$3\" \"$4\"; then printf '1\\n'; else printf '0\\n'; fi; shift 4; done", extract(&base,"mole_ascii_lowercase"),extract(&batch,"_uninstall_strip_version_suffix_into"),extract(&batch,"uninstall_bundles_share_remnants"));
        let names = [
            "Foo",
            "Foo Beta",
            "foo.app",
            "Foo beta",
            "Foo  Beta",
            "Foo Technology Preview",
            "Foo Technology",
            "Foo Developer Edition",
            "Foo Beta Beta",
            "A Beta",
            "A",
            "中文 Beta",
            "中文",
            "Ä Beta",
            "ä",
            "Foo\tBeta",
            "Foo\u{b}Beta",
            "Foo\u{a0}Beta",
        ];
        let ids = [
            ("org.app", "ORG.APP"),
            ("org.app", "org.app.share"),
            ("org.app.share", "org.app"),
            ("org.app", "org.apple"),
            ("org.a", "org.b"),
        ];
        let mut args = vec![];
        let mut expected = String::new();
        for (a, b) in ids {
            for x in names {
                for y in names {
                    let x = format!("/Apps/{x}.app");
                    let y = format!("/Apps/{y}.APP");
                    expected.push_str(if relation(a, b, &x, &y).is_some() {
                        "1\n"
                    } else {
                        "0\n"
                    });
                    args.extend([a.to_owned(), b.to_owned(), x, y]);
                }
            }
        }
        let output = Command::new("/bin/bash")
            .arg("-c")
            .arg(script)
            .arg("shared-rule-test")
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }
}
