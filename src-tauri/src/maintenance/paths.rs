//! Metadata-only file observations. No preference file contents or write probes.
use super::*;
use crate::{app_protection, permissions};
use std::os::macos::fs::MetadataExt as _;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Observation {
    pub location: &'static str,
    pub state: &'static str,
    pub client_protection: &'static str,
    pub mole_protection: &'static str,
    pub access: &'static str,
}

pub(super) fn locations(id: &str) -> &'static [&'static str] {
    match id {
        "prevent_network_dsstore" => &["Library/Preferences/com.apple.desktopservices.plist"],
        _ => &[
            "Library/Preferences/.GlobalPreferences.plist",
            "Library/Preferences/com.apple.frameworks.diskimages.plist",
        ],
    }
}

// Mole's C-locale [[ target == $pattern ]] for a bounded subset (* and ?).
// Brackets, escaping and extended patterns remain unknown, never clear.
fn glob(pattern: &str, target: &str, budget: &mut usize) -> Option<bool> {
    if pattern.len() > 512
        || target.len() > 4096
        || pattern.contains(['[', '\\', '(', ')', '|'])
        || pattern.chars().any(char::is_control)
    {
        return None;
    }
    let cost = pattern.len().checked_mul(target.len().max(1))?;
    *budget = budget.checked_sub(cost)?;
    let mut previous = vec![false; target.len() + 1];
    previous[0] = true;
    for byte in pattern.bytes() {
        let mut next = vec![false; target.len() + 1];
        if byte == b'*' {
            next[0] = previous[0]
        }
        for (index, actual) in target.bytes().enumerate() {
            next[index + 1] = match byte {
                b'*' => previous[index + 1] || next[index],
                b'?' => previous[index],
                _ => previous[index] && byte == actual,
            };
        }
        previous = next;
    }
    Some(previous[target.len()])
}

fn normalized(value: &str) -> String {
    let mut out = value.strip_suffix('/').unwrap_or(value).to_string();
    while out.contains("//") {
        out = out.replace("//", "/")
    }
    out
}
pub(super) fn mole(target: &str, patterns: Option<&[String]>, budget: &mut usize) -> &'static str {
    let Some(patterns) = patterns else {
        return "unknown";
    };
    let target = normalized(target);
    let mut unknown = false;
    for pattern in patterns {
        let pattern = normalized(pattern);
        if pattern == target
            || pattern.starts_with(&format!("{target}/"))
            || (!pattern.contains(['*', '?', '[', '\\', '(', ')', '|'])
                && target.starts_with(&format!("{pattern}/")))
        {
            return "matched";
        }
        match glob(&pattern, &target, budget) {
            Some(true) => return "matched",
            None => unknown = true,
            Some(false) => {}
        }
    }
    if unknown {
        "unknown"
    } else {
        "notMatched"
    }
}

pub(super) fn observe(
    home: &Path,
    location: &'static str,
    patterns: Option<&[String]>,
    client: &[String],
    budget: &mut usize,
    began: Instant,
) -> Observation {
    let path = home.join(location);
    let text = path.to_str();
    let mut out = Observation {
        location,
        state: "unknown",
        client_protection: text
            .and_then(|path| app_protection::check(path, None, None, "", client).ok())
            .map(|result| {
                if result.reasons.is_empty() {
                    "notMatched"
                } else {
                    "matched"
                }
            })
            .unwrap_or("unknown"),
        mole_protection: text
            .map(|value| mole(value, patterns, budget))
            .unwrap_or("unknown"),
        access: "notChecked",
    };
    if began.elapsed().as_secs() >= 4 {
        return out;
    }
    if std::fs::symlink_metadata(&path)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        // Only a verified, local immediate parent can support a missing-file
        // observation. Do not create a file to test permissions.
        if let Some(parent) = path
            .parent()
            .and_then(|parent| AnchoredPath::open(parent).ok())
        {
            if parent.local_materialized()
                && parent.target().metadata().is_ok_and(|m| m.is_dir())
                && parent.validate().is_ok()
            {
                let name = CString::new(path.file_name().unwrap().to_str().unwrap()).unwrap();
                let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
                let absent = unsafe {
                    libc::fstatat(
                        parent.target().as_raw_fd(),
                        name.as_ptr(),
                        stat.as_mut_ptr(),
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                } != 0
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT);
                if absent && parent.validate().is_ok() {
                    out.state = "missing"
                }
            }
        }
        return out;
    }
    let Ok(held) = AnchoredPath::open(&path) else {
        return out;
    };
    if !held.local_materialized() || !held.target().metadata().is_ok_and(|m| m.is_file()) {
        return out;
    }
    out.state = "present";
    out.access = access(&held);
    if held.validate().is_err() {
        out.state = "unknown";
        out.access = "unknown"
    }
    out
}

fn access(held: &AnchoredPath) -> &'static str {
    let Ok(target) = held.target().metadata() else {
        return "unknown";
    };
    let Ok(parent) = held.parent().metadata() else {
        return "unknown";
    };
    let user = unsafe { libc::geteuid() };
    if target.uid() != user
        || parent.uid() != user
        || (target.st_flags() | parent.st_flags())
            & (libc::UF_IMMUTABLE | libc::SF_IMMUTABLE | libc::UF_APPEND | libc::SF_APPEND)
            != 0
    {
        return "restricted";
    }
    // Share only volume/ACL primitives; cleanup's move/delete check has different
    // semantics and cannot be presented as preference write permission.
    let (Ok(target_volume), Ok(parent_volume)) = (
        permissions::read_only(held.target()),
        permissions::read_only(held.parent()),
    ) else {
        return "unknown";
    };
    if target_volume || parent_volume {
        return "restricted";
    }
    let (Ok(target_acl), Ok(parent_acl)) = (
        permissions::has_acl(held.target()),
        permissions::has_acl(held.parent()),
    ) else {
        return "unknown";
    };
    if target_acl || parent_acl {
        return "unknown";
    }
    let Some(name) = held
        .witness
        .nodes
        .last()
        .and_then(|node| node.path.file_name())
        .and_then(|name| name.to_str())
        .and_then(|name| CString::new(name).ok())
    else {
        return "unknown";
    };
    for (name, mode) in [
        (c".", libc::W_OK | libc::X_OK),
        (name.as_c_str(), libc::R_OK | libc::W_OK),
    ] {
        if unsafe {
            libc::faccessat(
                held.parent().as_raw_fd(),
                name.as_ptr(),
                mode,
                libc::AT_EACCESS,
            )
        } != 0
        {
            return match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::EACCES | libc::EPERM | libc::EROFS) => "restricted",
                _ => "unknown",
            };
        }
    }
    // Reobserve volatile metadata/volume/ACL and names. Still not atomic and
    // never proof that cfprefsd can commit a change or that TCC allows one.
    let metadata_same = |file: &std::fs::File, before: &std::fs::Metadata| {
        file.metadata().is_ok_and(|after| {
            after.mode() == before.mode()
                && after.uid() == before.uid()
                && after.gid() == before.gid()
                && after.st_flags() == before.st_flags()
                && after.ctime() == before.ctime()
                && after.ctime_nsec() == before.ctime_nsec()
        })
    };
    if !metadata_same(held.target(), &target)
        || !metadata_same(held.parent(), &parent)
        || permissions::read_only(held.target()) != Ok(false)
        || permissions::read_only(held.parent()) != Ok(false)
        || permissions::has_acl(held.target()) != Ok(false)
        || permissions::has_acl(held.parent()) != Ok(false)
        || held.validate().is_err()
    {
        return "unknown";
    }
    "noRestrictionObserved"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{symlink, PermissionsExt},
    };
    #[test]
    fn whitelist_paths_respect_components_normalization_and_bounded_c_globs() {
        for (pattern, target, result) in [
            ("/prefs/a.plist", "/prefs/a.plist", "matched"),
            ("/prefs/", "/prefs/a.plist", "matched"),
            ("/prefs/a.plist", "/prefs", "matched"),
            ("/prefs//a*", "/prefs/a.plist", "matched"),
            ("*", "/prefs/.hidden", "matched"),
            ("/prefs/*", "/prefs/sub/a.plist", "matched"),
            ("/prefs/?", "/prefs/中", "notMatched"),
            ("/prefs/???", "/prefs/中", "matched"),
            ("/prefs/a", "/prefs/abc", "notMatched"),
            ("~/prefs", "/home/prefs/a.plist", "notMatched"),
            ("$HOME/prefs", "/home/prefs/a.plist", "notMatched"),
            ("/prefs/[ab]", "/prefs/a", "unknown"),
            ("/prefs/a\\*", "/prefs/abc", "unknown"),
        ] {
            assert_eq!(
                mole(target, Some(&[pattern.into()]), &mut 1_000_000),
                result,
                "{pattern} / {target}"
            );
        }
        assert_eq!(mole("/prefs/a", None, &mut 1_000_000), "unknown");
        assert_eq!(mole("/prefs/a", Some(&["*".into()]), &mut 0), "unknown");
        assert_eq!(
            mole("/prefs/a", Some(&["[x]".into(), "/prefs/a".into()]), &mut 0),
            "matched"
        );
    }
    #[test]
    fn metadata_and_client_guards_do_not_create_missing_files_or_follow_links() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let location = locations("prevent_network_dsstore")[0];
        let path = root.path().join(location);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let probe = |client: &[String]| {
            observe(
                root.path(),
                location,
                Some(&[]),
                client,
                &mut 1_000_000,
                Instant::now(),
            )
        };
        let missing = probe(&[]);
        assert_eq!((missing.state, missing.access), ("missing", "notChecked"));
        assert!(!path.exists());
        fs::write(&path, "opaque fixture, never parsed").unwrap();
        assert_eq!(probe(&[]).state, "present");
        assert_eq!(
            probe(&[path.parent().unwrap().to_string_lossy().into_owned()]).client_protection,
            "matched"
        );
        assert_eq!(
            probe(&[format!("{}.old", path.display())]).client_protection,
            "notMatched"
        );
        assert_eq!(
            probe(&[format!("{}/child", path.display())]).client_protection,
            "matched"
        );
        assert_eq!(probe(&["relative".into()]).client_protection, "unknown");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        if unsafe { libc::geteuid() } != 0 {
            assert_eq!(probe(&[]).access, "restricted")
        }
        fs::remove_file(&path).unwrap();
        symlink("missing", &path).unwrap();
        assert_eq!(
            (probe(&[]).state, probe(&[]).access),
            ("unknown", "notChecked")
        );
    }
    #[test]
    fn ancestor_links_and_exhausted_probe_time_keep_path_unknown() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let other = tempfile::tempdir_in("/private/tmp").unwrap();
        symlink(other.path(), root.path().join("Library")).unwrap();
        let location = locations("prevent_network_dsstore")[0];
        assert_eq!(
            observe(
                root.path(),
                location,
                Some(&[]),
                &[],
                &mut 1_000_000,
                Instant::now()
            )
            .state,
            "unknown"
        );
        assert_eq!(
            observe(
                root.path(),
                location,
                Some(&[]),
                &[],
                &mut 1_000_000,
                Instant::now() - std::time::Duration::from_secs(5)
            )
            .access,
            "notChecked"
        );
    }
    #[test]
    #[ignore = "Requires pinned Mole source; compares extracted pure whitelist predicate on isolated string inputs"]
    fn bounded_subset_matches_pinned_mole_path_predicate() {
        let source = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole/lib/core/app_protection.sh"),
        )
        .unwrap();
        let body = source
            .split_once("is_path_whitelisted() {")
            .unwrap()
            .1
            .split_once("\n}\n")
            .unwrap()
            .0;
        let script = format!("is_path_whitelisted() {{{body}\n}}\nWHITELIST_PATTERNS=(\"$1\")\nis_path_whitelisted \"$2\"");
        for (pattern, target) in [
            ("/prefs/", "/prefs/a.plist"),
            ("/prefs/a.plist", "/prefs"),
            ("/prefs//a*", "/prefs/a.plist"),
            ("*", "/prefs/.hidden"),
            ("/prefs/*", "/prefs/sub/a.plist"),
            ("/prefs/?", "/prefs/中"),
            ("/prefs/???", "/prefs/中"),
            ("/prefs/a", "/prefs/abc"),
            ("~/prefs", "/home/prefs/a.plist"),
        ] {
            let status = std::process::Command::new("/bin/bash")
                .args([
                    "--noprofile",
                    "--norc",
                    "-c",
                    &script,
                    "fixture",
                    pattern,
                    target,
                ])
                .env_clear()
                .env("LC_ALL", "C")
                .status()
                .unwrap();
            assert_eq!(
                mole(target, Some(&[pattern.into()]), &mut 1_000_000) == "matched",
                status.success()
            );
        }
    }
}
