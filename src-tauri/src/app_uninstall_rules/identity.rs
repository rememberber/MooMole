//! Conservative metadata-only subset of Mole's existing-ancestor identity test.
//! Symlinks, missing/unreadable paths and nonlocal layouts remain unknown.
use super::path::IdentityBudget;
use std::path::Path;

pub(super) fn within(
    path: &Path,
    root: &Path,
    budget: &mut IdentityBudget,
) -> Result<bool, &'static str> {
    #[cfg(target_os = "macos")]
    {
        check(path, root, budget, || {})
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, root, budget);
        Err("identityUnverified")
    }
}

#[cfg(target_os = "macos")]
fn check(
    path: &Path,
    root: &Path,
    budget: &mut IdentityBudget,
    hook: impl FnOnce(),
) -> Result<bool, &'static str> {
    use crate::observation::AnchoredPath;
    if !budget.take() {
        return Err("identityLimitReached");
    }
    let root = AnchoredPath::open(root).map_err(|_| "identityUnverified")?;
    if !root.local_materialized() {
        return Err("identityUnverified");
    }
    let target = root.witness.nodes.last().ok_or("identityUnverified")?;
    let mut probe = path;
    loop {
        if !budget.take() {
            return Err("identityLimitReached");
        }
        if let Ok(observed) = AnchoredPath::open(probe) {
            if !observed.local_materialized() {
                return Err("identityUnverified");
            }
            let contains = observed
                .witness
                .nodes
                .iter()
                .any(|n| n.dev == target.dev && n.ino == target.ino);
            hook();
            observed.validate().map_err(|_| "identityUnverified")?;
            root.validate().map_err(|_| "identityUnverified")?;
            if budget.expired() {
                return Err("identityLimitReached");
            }
            // A positive ancestor remains valid even when a deeper component was
            // missing/unsupported: upstream tests every lexical ancestor too.
            if contains {
                return Ok(true);
            }
            // Only a complete existing input can disprove this restricted match.
            return if probe == path {
                Ok(false)
            } else {
                Err("identityUnverified")
            };
        }
        probe = probe
            .parent()
            .filter(|p| *p != Path::new("/"))
            .ok_or("identityUnverified")?;
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};
    fn temp() -> tempfile::TempDir {
        tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
    }
    #[test]
    fn existing_and_missing_descendants_use_device_inode_and_keep_unknowns() {
        let t = temp();
        let root = t.path().join("protected");
        fs::create_dir_all(root.join("child")).unwrap();
        let outside = t.path().join("outside");
        fs::create_dir(&outside).unwrap();
        for path in [&root, &root.join("child"), &root.join("missing/leaf")] {
            assert_eq!(
                within(path, &root, &mut IdentityBudget::default()),
                Ok(true)
            );
        }
        assert_eq!(
            within(&outside, &root, &mut IdentityBudget::default()),
            Ok(false)
        );
        assert_eq!(
            within(
                &outside.join("missing"),
                &root,
                &mut IdentityBudget::default()
            ),
            Err("identityUnverified")
        );
        let alias = t.path().join("alias");
        symlink(&root, &alias).unwrap();
        assert_eq!(
            within(&alias.join("child"), &root, &mut IdentityBudget::default()),
            Err("identityUnverified")
        );
        assert_eq!(
            within(
                &outside,
                &t.path().join("absent"),
                &mut IdentityBudget::default()
            ),
            Err("identityUnverified")
        );
        // Case aliases are identified by the volume, never by lowercasing names.
        let upper = t.path().join("PROTECTED/child");
        let expected = if upper.exists() {
            Ok(true)
        } else {
            Err("identityUnverified")
        };
        assert_eq!(
            within(&upper, &root, &mut IdentityBudget::default()),
            expected
        );
    }
    #[test]
    fn late_rebinding_and_shared_checkpoint_limits_withdraw_results() {
        for which in ["root", "path"] {
            let t = temp();
            let root = t.path().join("root");
            let path = root.join("child");
            fs::create_dir_all(&path).unwrap();
            assert_eq!(
                check(&path, &root, &mut IdentityBudget::default(), || {
                    let changed = if which == "root" { &root } else { &path };
                    fs::rename(changed, t.path().join("saved")).unwrap();
                    fs::create_dir(changed).unwrap();
                }),
                Err("identityUnverified")
            );
        }
        let t = temp();
        for mut budget in [
            IdentityBudget {
                started: std::time::Instant::now(),
                attempts: 64,
            },
            IdentityBudget {
                started: std::time::Instant::now() - std::time::Duration::from_secs(1),
                attempts: 0,
            },
        ] {
            assert_eq!(
                within(t.path(), t.path(), &mut budget),
                Err("identityLimitReached")
            );
        }
    }
    #[test]
    #[ignore = "requires fixed Mole filesystem helpers; isolated metadata-only comparison"]
    fn prepared_ancestor_identity_matches_original_for_supported_local_paths() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        super::super::load(&resources).unwrap();
        let source = fs::read_to_string(resources.join("lib/core/file_ops.sh")).unwrap();
        let mut script = String::from("set -euo pipefail\n");
        for name in [
            "_mole_path_is_same_existing_file",
            "_mole_path_is_within_existing_root",
        ] {
            script.push_str(&crate::app_background::test_function(&source, name));
            script.push('\n');
        }
        script.push_str("root=$1; shift; for path do if _mole_path_is_within_existing_root \"$path\" \"$root\"; then printf 1; else printf 0; fi; done");
        let t = temp();
        let root = t.path().join("protected[*] 中文");
        fs::create_dir_all(root.join("child")).unwrap();
        let outside = t.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let mut paths = vec![
            root.clone(),
            root.join("child"),
            root.join("missing/nested"),
            outside,
        ];
        let case = t.path().join("PROTECTED[*] 中文/child");
        if case.exists() {
            paths.push(case);
        }
        let output = std::process::Command::new("/bin/bash")
            .args(["-c", &script, "identity"])
            .arg(&root)
            .args(&paths)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        assert!(output.status.success());
        let actual: String = paths
            .iter()
            .map(|p| {
                if within(p, &root, &mut IdentityBudget::default()).unwrap() {
                    '1'
                } else {
                    '0'
                }
            })
            .collect();
        assert_eq!(output.stdout, actual.as_bytes());
        eprintln!(
            "{} supported existing-ancestor identity paths matched original Mole",
            paths.len()
        );
    }
}
