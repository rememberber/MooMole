//! Conservative client guards, never Mole uninstall eligibility or data ownership.
use serde::Serialize;
use std::path::{Component, Path};

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Protection {
    pub reasons: Vec<&'static str>,
    pub matched_paths: Vec<String>,
}

fn absolute(raw: &str) -> bool {
    !raw.is_empty()
        && raw.len() <= 4096
        && !raw.chars().any(char::is_control)
        && Path::new(raw).is_absolute()
        && Path::new(raw)
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
}

pub fn check(
    path: &str,
    local_id: Option<&str>,
    current_app: Option<&Path>,
    current_id: &str,
    rules: &[String],
) -> Result<Protection, String> {
    if !absolute(path) || rules.len() > 100 || rules.iter().any(|rule| !absolute(rule)) {
        return Err("保护目录信息无法确认，请检查设置后重试。".into());
    }
    let path = Path::new(path);
    let mut report = Protection::default();
    if ["/System", "/Library/Apple/System"]
        .iter()
        .any(|root| path.starts_with(root))
    {
        report.reasons.push("systemLocation");
    }
    // Same ID is only a conservative guard, not publisher/signature verification.
    if current_app.is_some_and(|current| path == current)
        || (!current_id.is_empty() && local_id == Some(current_id))
    {
        report.reasons.push("currentApplication");
    }
    report.matched_paths = rules
        .iter()
        .filter(|rule| path.starts_with(rule) || Path::new(rule).starts_with(path))
        .cloned()
        .collect();
    report.matched_paths.sort();
    report.matched_paths.dedup();
    if !report.matched_paths.is_empty() {
        report.reasons.push("clientDirectory");
    }
    Ok(report)
}

pub fn containing_app(executable: &Path) -> Option<&Path> {
    executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "io.github.rememberber.moomole";
    fn report(path: &str, rules: &[&str]) -> Protection {
        check(
            path,
            None,
            None,
            ID,
            &rules.iter().map(|v| (*v).into()).collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn directory_guards_use_components_in_both_directions_and_keep_missing_rules() {
        for rule in [
            "/Applications",
            "/Applications/Example.app",
            "/Applications/Example.app/Contents/kept",
        ] {
            let result = report("/Applications/Example.app", &[rule, rule]);
            assert_eq!(result.reasons, ["clientDirectory"]);
            assert_eq!(result.matched_paths, [rule]);
        }
        assert!(report(
            "/Applications/Example.app",
            &["/Applications/Example.app.old"]
        )
        .reasons
        .is_empty());
        assert!(
            report("/Applications/Example.app", &["/Applications/Example"])
                .reasons
                .is_empty()
        );
        assert_eq!(
            report("/Applications/中文 应用.app", &["/"]).reasons,
            ["clientDirectory"]
        );
        for path in [
            "/System/Applications/Example.app",
            "/Library/Apple/System/Applications/Example.app",
        ] {
            assert_eq!(report(path, &[]).reasons, ["systemLocation"]);
        }
        for path in ["/Systematic/Example.app", "/Applications/Xcode.app"] {
            assert!(report(path, &[]).reasons.is_empty());
        }
    }
    #[test]
    fn current_location_and_same_id_are_independent_conservative_guards() {
        let current = Path::new("/Applications/MooMole.app");
        assert_eq!(
            check(current.to_str().unwrap(), None, Some(current), ID, &[])
                .unwrap()
                .reasons,
            ["currentApplication"]
        );
        assert_eq!(
            check(
                "/Volumes/Test/MooMole.app",
                Some(ID),
                Some(current),
                ID,
                &[]
            )
            .unwrap()
            .reasons,
            ["currentApplication"]
        );
        assert!(check(
            "/Applications/Other.app",
            Some("other.id"),
            Some(current),
            ID,
            &[]
        )
        .unwrap()
        .reasons
        .is_empty());
        assert_eq!(
            containing_app(&current.join("Contents/MacOS/moomole")),
            Some(current)
        );
        assert_eq!(containing_app(Path::new("/tmp/moomole")), None);
    }
    #[test]
    fn malformed_rules_fail_instead_of_claiming_no_match_and_guards_accumulate() {
        for rule in ["relative", "/safe/../other", "/bad\npath"] {
            assert!(check("/Applications/A.app", None, None, ID, &[rule.into()]).is_err());
        }
        assert!(check(
            "/Applications/A.app",
            None,
            None,
            ID,
            &vec!["/valid".into(); 101]
        )
        .is_err());
        assert_eq!(
            check("/System/A.app", Some(ID), None, ID, &["/System".into()])
                .unwrap()
                .reasons,
            ["systemLocation", "currentApplication", "clientDirectory"]
        );
    }
}
