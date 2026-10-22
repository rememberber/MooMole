//! Positive-only recent modification hints from the existing metadata walk.
//! Mirrors the pinned seven-day threshold, not a build/idle determination.
use crate::{directory_probe::stat_at, observation::AnchoredPath};
use serde::Serialize;
use std::{
    ffi::{CStr, CString, OsStr},
    fs::File,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::Path,
};
pub const WINDOW_DAYS: i64 = 7;
const WINDOW_MS: i64 = WINDOW_DAYS * 86400 * 1000;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    kind: &'static str,
    path: String,
    modified_at: i64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    window_days: i64,
    reference_at: i64,
    state: &'static str,
    reason: Option<&'static str>,
    evidence: Option<Evidence>,
}
struct Proof {
    parent: AnchoredPath,
    name: CString,
    stat: libc::stat,
    evidence: Evidence,
}
pub struct ActivityProbe {
    reference: i64,
    proof: Option<Proof>,
    uncertain: bool,
}
fn same(a: &libc::stat, b: &libc::stat) -> bool {
    a.st_dev == b.st_dev
        && a.st_ino == b.st_ino
        && a.st_mode == b.st_mode
        && a.st_size == b.st_size
        && a.st_nlink == b.st_nlink
        && a.st_flags == b.st_flags
        && a.st_mtime == b.st_mtime
        && a.st_mtime_nsec == b.st_mtime_nsec
        && a.st_ctime == b.st_ctime
        && a.st_ctime_nsec == b.st_ctime_nsec
}
impl ActivityProbe {
    pub fn new(path: &Path, reference: i64) -> Self {
        let mut probe = Self {
            reference,
            proof: None,
            uncertain: false,
        };
        // Only the artifact directory itself, then regular descendants (as in Mole).
        if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
            if let (Ok(held), Ok(name)) =
                (AnchoredPath::open(parent), CString::new(name.as_bytes()))
            {
                if let Ok(stat) = stat_at(held.target(), &name) {
                    probe.observe(held.target(), parent, &name, &stat, true);
                } else {
                    probe.uncertain = true;
                }
            } else {
                probe.uncertain = true;
            }
        }
        probe
    }
    pub fn observe(
        &mut self,
        directory: &File,
        path: &Path,
        name: &CStr,
        stat: &libc::stat,
        root: bool,
    ) {
        if self.proof.is_some() {
            return;
        }
        let kind = stat.st_mode & libc::S_IFMT;
        if (root && kind != libc::S_IFDIR) || (!root && kind != libc::S_IFREG) {
            return;
        }
        let Some(modified) = stat.st_mtime.checked_mul(1000) else {
            self.uncertain = true;
            return;
        };
        // Future/negative timestamps are never evidence of an old or recent artifact.
        if modified < 0 || modified > self.reference {
            self.uncertain = true;
            return;
        }
        if self.reference - modified >= WINDOW_MS {
            return;
        }
        let capture = (|| {
            if stat.st_flags & 0x40000000 != 0 || stat.st_nlink != 1 && !root {
                return Err(());
            }
            let full = path.join(OsStr::from_bytes(name.to_bytes()));
            let text = full
                .to_str()
                .filter(|s| s.len() <= 4096 && !s.chars().any(char::is_control))
                .ok_or(())?;
            let parent = AnchoredPath::open(path).map_err(|_| ())?;
            let original = directory.metadata().map_err(|_| ())?;
            let current = parent.target().metadata().map_err(|_| ())?;
            if !parent.local_materialized()
                || original.dev() != current.dev()
                || original.ino() != current.ino()
                || stat.st_dev as u64 != current.dev()
                || !stat_at(parent.target(), name).is_ok_and(|v| same(stat, &v))
            {
                return Err(());
            }
            Ok(Proof {
                parent,
                name: name.to_owned(),
                stat: *stat,
                evidence: Evidence {
                    kind: if root { "directory" } else { "file" },
                    path: text.into(),
                    modified_at: modified,
                },
            })
        })();
        match capture {
            Ok(proof) => self.proof = Some(proof),
            Err(()) => self.uncertain = true,
        }
    }
    pub fn finish(self, incomplete: bool, checked_at: i64) -> Activity {
        let report = |state, reason, evidence| Activity {
            window_days: WINDOW_DAYS,
            reference_at: self.reference,
            state,
            reason,
            evidence,
        };
        if checked_at < self.reference || checked_at - self.reference > 6000 {
            return report("unknown", Some("unavailable"), None);
        }
        if let Some(proof) = self.proof {
            if proof.parent.validate().is_ok()
                && stat_at(proof.parent.target(), &proof.name).is_ok_and(|v| same(&proof.stat, &v))
            {
                return report("recent", None, Some(proof.evidence));
            }
            return report("unknown", Some("unavailable"), None);
        }
        report(
            "unknown",
            Some(if self.uncertain {
                "unavailable"
            } else if incomplete {
                "incomplete"
            } else {
                "notObserved"
            }),
            None,
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn age(path: &Path, seconds: i64) {
        File::open(path)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds as u64))
            .unwrap();
    }
    fn observe(path: &Path, reference: i64) -> ActivityProbe {
        let mut probe = ActivityProbe::new(path.parent().unwrap(), reference);
        let parent = File::open(path.parent().unwrap()).unwrap();
        let name = CString::new(path.file_name().unwrap().as_bytes()).unwrap();
        let stat = stat_at(&parent, &name).unwrap();
        probe.observe(&parent, path.parent().unwrap(), &name, &stat, false);
        probe
    }
    #[test]
    fn threshold_and_future_times_never_publish_an_old_or_idle_verdict() {
        let t = tempfile::tempdir_in("/private/tmp").unwrap();
        let file = t.path().join("fixture");
        fs::write(&file, b"not read by probe").unwrap();
        let now = crate::db::now() / 1000;
        age(t.path(), now - 10 * 86400);
        for (delta, expected) in [
            (7 * 86400 - 1, "recent"),
            (7 * 86400, "unknown"),
            (10 * 86400, "unknown"),
            (-20, "unknown"),
        ] {
            age(&file, now - delta);
            let r = observe(&file, now * 1000).finish(false, now * 1000);
            assert_eq!(r.state, expected);
            if expected == "recent" {
                assert_eq!(r.evidence.unwrap().kind, "file");
            } else {
                assert!(r.evidence.is_none());
            }
        }
        age(&file, now - 10 * 86400);
        assert_eq!(
            observe(&file, now * 1000).finish(true, now * 1000).reason,
            Some("incomplete")
        );
        age(&file, now);
        assert_eq!(
            observe(&file, now * 1000)
                .finish(false, now * 1000 - 1)
                .reason,
            Some("unavailable")
        );
        assert_eq!(fs::read(&file).unwrap(), b"not read by probe");
    }
    #[test]
    fn directory_changes_file_replacement_and_links_withdraw_evidence() {
        let t = tempfile::tempdir_in("/private/tmp").unwrap();
        let dir = t.path().join("target");
        fs::create_dir(&dir).unwrap();
        let now = crate::db::now();
        assert_eq!(
            ActivityProbe::new(&dir, now)
                .finish(false, now)
                .evidence
                .unwrap()
                .kind,
            "directory"
        );
        let probe = ActivityProbe::new(&dir, now);
        fs::write(dir.join("file"), b"x").unwrap();
        assert_eq!(probe.finish(false, now).reason, Some("unavailable"));
        age(&dir, now / 1000 - 10 * 86400);
        let file = dir.join("file");
        let probe = observe(&file, now);
        fs::rename(&file, dir.join("old")).unwrap();
        fs::write(&file, b"x").unwrap();
        assert_eq!(probe.finish(false, now).reason, Some("unavailable"));
        age(&dir, now / 1000 - 10 * 86400);
        std::os::unix::fs::symlink(&file, dir.join("link")).unwrap();
        age(&dir, now / 1000 - 10 * 86400);
        assert_eq!(
            observe(&dir.join("link"), now).finish(false, now).state,
            "unknown"
        );
    }
}
