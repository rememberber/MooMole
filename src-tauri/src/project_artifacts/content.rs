//! Pinned purge name signals collected during the existing metadata walk.
//! No key contents, Git invocation, ownership verdict or execution clearance.
use crate::{directory_probe::stat_at, observation::AnchoredPath};
use serde::Serialize;
use std::{
    ffi::{CStr, CString, OsStr},
    fs::File,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::Path,
};
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    kind: &'static str,
    path: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentSignals {
    state: &'static str,
    reason: Option<&'static str>,
    evidence: Option<Evidence>,
}
impl ContentSignals {
    pub fn unknown(reason: &'static str) -> Self {
        Self {
            state: "unknown",
            reason: Some(reason),
            evidence: None,
        }
    }
}
struct Proof {
    parent: AnchoredPath,
    name: CString,
    stat: libc::stat,
    evidence: Evidence,
}
#[derive(Default)]
pub struct SignalProbe {
    proof: Option<Proof>,
    unavailable: bool,
}
fn same(a: &libc::stat, b: &libc::stat) -> bool {
    a.st_dev == b.st_dev
        && a.st_ino == b.st_ino
        && a.st_mode == b.st_mode
        && a.st_size == b.st_size
        && a.st_nlink == b.st_nlink
        && a.st_flags == b.st_flags
        && a.st_uid == b.st_uid
        && a.st_gid == b.st_gid
        && a.st_mtime == b.st_mtime
        && a.st_mtime_nsec == b.st_mtime_nsec
        && a.st_ctime == b.st_ctime
        && a.st_ctime_nsec == b.st_ctime_nsec
}
impl SignalProbe {
    pub fn observe(&mut self, directory: &File, path: &Path, name: &CStr, stat: &libc::stat) {
        if self.proof.is_some() {
            return;
        }
        let kind = if name.to_bytes() == b".git" {
            "repositoryMarker"
        } else if name.to_bytes().ends_with(b"-keypair.json") {
            "keypairName"
        } else {
            return;
        };
        // The marker itself may be a link/special node: only its name is evidence.
        // Anchor its physical parent and lstat it, without following or opening it.
        let candidate = (|| {
            let full = path.join(OsStr::from_bytes(name.to_bytes()));
            let text = full
                .to_str()
                .filter(|p| p.len() <= 4096 && !p.chars().any(char::is_control))
                .ok_or(())?;
            let parent = AnchoredPath::open(path).map_err(|_| ())?;
            let original = directory.metadata().map_err(|_| ())?;
            let current = parent.target().metadata().map_err(|_| ())?;
            if !parent.local_materialized()
                || original.dev() != current.dev()
                || original.ino() != current.ino()
                || !stat_at(parent.target(), name).is_ok_and(|v| same(stat, &v))
            {
                return Err(());
            }
            Ok(Proof {
                parent,
                name: name.to_owned(),
                stat: *stat,
                evidence: Evidence {
                    kind,
                    path: text.into(),
                },
            })
        })();
        match candidate {
            Ok(proof) => self.proof = Some(proof),
            Err(()) => self.unavailable = true,
        }
    }
    pub fn finish(self, incomplete: bool) -> ContentSignals {
        if let Some(proof) = self.proof {
            if proof.parent.validate().is_ok()
                && stat_at(proof.parent.target(), &proof.name).is_ok_and(|v| same(&proof.stat, &v))
            {
                return ContentSignals {
                    state: "found",
                    reason: None,
                    evidence: Some(proof.evidence),
                };
            }
            return ContentSignals::unknown("unavailable");
        }
        ContentSignals::unknown(if self.unavailable {
            "unavailable"
        } else if incomplete {
            "incomplete"
        } else {
            "notObserved"
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};
    fn observe(parent: &Path, name: &str) -> SignalProbe {
        let file = File::open(parent).unwrap();
        let name = CString::new(name).unwrap();
        let stat = stat_at(&file, &name).unwrap();
        let mut probe = SignalProbe::default();
        probe.observe(&file, parent, &name, &stat);
        probe
    }
    #[test]
    fn names_are_evidence_without_following_links_or_reading_key_contents() {
        let t = tempfile::tempdir_in("/private/tmp").unwrap();
        let path = t.path();
        fs::write(path.join("wallet-keypair.json"), b"fixture private bytes").unwrap();
        let report = observe(path, "wallet-keypair.json").finish(false);
        assert_eq!(report.state, "found");
        assert_eq!(report.evidence.unwrap().kind, "keypairName");
        assert_eq!(
            fs::read(path.join("wallet-keypair.json")).unwrap(),
            b"fixture private bytes"
        );
        symlink(path.join("does-not-exist"), path.join(".git")).unwrap();
        let report = observe(path, ".git").finish(true);
        assert_eq!(report.state, "found");
        assert_eq!(report.evidence.unwrap().kind, "repositoryMarker");
        fs::write(path.join("not-a-keypair.JSON"), b"x").unwrap();
        assert_eq!(
            observe(path, "not-a-keypair.JSON").finish(false).reason,
            Some("notObserved")
        );
        assert_eq!(
            SignalProbe::default().finish(true).reason,
            Some("incomplete")
        );
    }
    #[test]
    fn replaced_marker_and_renamed_parent_withdraw_the_signal() {
        let t = tempfile::tempdir_in("/private/tmp").unwrap();
        let parent = t.path().join("nested");
        fs::create_dir(&parent).unwrap();
        fs::write(parent.join(".git"), b"gitdir: ignored").unwrap();
        let probe = observe(&parent, ".git");
        fs::rename(parent.join(".git"), parent.join("old")).unwrap();
        fs::write(parent.join(".git"), b"gitdir: ignored").unwrap();
        let report = probe.finish(false);
        assert_eq!(report.state, "unknown");
        assert!(report.evidence.is_none());
        let probe = observe(&parent, ".git");
        fs::rename(&parent, t.path().join("renamed")).unwrap();
        assert_eq!(probe.finish(false).reason, Some("unavailable"));
    }
}
