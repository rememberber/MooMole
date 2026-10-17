//! Bounded maintenance of our own update files, inside an already anchored cache.
use super::Cache;
use std::{
    fs::Metadata,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::Path,
    time::{Duration, Instant, SystemTime},
};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_ENTRIES: usize = 512;
const MAX_REMOVALS: usize = 64;
const MAX_TIME: Duration = Duration::from_secs(2);

#[derive(Debug, Default, PartialEq)]
pub struct Cleanup {
    pub scanned: usize,
    pub removed: usize,
}

fn retention(name: &str) -> Option<Duration> {
    fn uuid(raw: &str) -> bool {
        uuid::Uuid::parse_str(raw).is_ok_and(|id| {
            id.get_version_num() == 4
                && id.to_string() == raw
                && id.get_variant() == uuid::Variant::RFC4122
        })
    }
    if let Some(raw) = name
        .strip_prefix(".moomole-")
        .and_then(|s| s.strip_suffix(".download"))
    {
        return uuid(raw).then_some(DAY);
    }
    let raw = name.strip_prefix("MooMole-")?.strip_suffix(".dmg")?;
    // Split from the right: versions may themselves contain hyphens.
    let split = raw.len().checked_sub(37)?;
    if !raw.is_char_boundary(split) || raw.as_bytes()[split] != b'-' || !uuid(&raw[split + 1..]) {
        return None;
    }
    let version = raw[..split]
        .strip_suffix("-mac-arm64")
        .or_else(|| raw[..split].strip_suffix("-mac-x64"))?;
    (version.len() <= 128 && semver::Version::parse(version).is_ok()).then_some(DAY * 7)
}

fn eligible(meta: &Metadata, now: SystemTime, age: Duration) -> bool {
    meta.is_file()
        && meta.nlink() == 1
        && meta.uid() == unsafe { libc::geteuid() }
        && meta.mode() & 0o7777 == 0o600
        && meta
            .modified()
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|d| d >= age)
}
fn same(a: &Metadata, b: &Metadata) -> bool {
    (
        a.dev(),
        a.ino(),
        a.len(),
        a.mtime(),
        a.mtime_nsec(),
        a.ctime(),
        a.ctime_nsec(),
    ) == (
        b.dev(),
        b.ino(),
        b.len(),
        b.mtime(),
        b.mtime_nsec(),
        b.ctime(),
        b.ctime_nsec(),
    )
}

impl Cache {
    /// Missing caches are a no-op: startup never creates a cache just to clean it.
    pub fn cleanup_existing(
        path: &Path,
        protected: Option<&str>,
        stop: impl Fn() -> bool,
    ) -> Result<Cleanup, String> {
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Cleanup::default()),
            _ => (),
        }
        Self::existing(path)?.cleanup(protected, SystemTime::now(), stop)
    }

    pub fn cleanup(
        &self,
        protected: Option<&str>,
        now: SystemTime,
        stop: impl Fn() -> bool,
    ) -> Result<Cleanup, String> {
        let deadline = Instant::now() + MAX_TIME;
        let stopped = || stop() || Instant::now() >= deadline;
        self.attached()?;
        let mut report = Cleanup::default();
        let mut candidates = Vec::new();
        // This enumeration never follows children. All file opens/unlinks below
        // are relative to the held directory fd, with attachment checks as well.
        for entry in std::fs::read_dir(&self.path).map_err(|_| "无法读取更新缓存。")? {
            if stopped() {
                return Ok(report);
            }
            if report.scanned == MAX_ENTRIES {
                // Refuse any mutations when the bounded scan cannot complete.
                return Err("更新缓存条目过多，本轮自动维护已跳过。".into());
            }
            report.scanned += 1;
            let entry = entry.map_err(|_| "无法读取更新缓存条目。")?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if Some(name) == protected {
                continue;
            }
            let Some(age) = retention(name) else { continue };
            let Ok(file) = self.open_file(name, false) else {
                continue;
            };
            let Ok(meta) = file.metadata() else { continue };
            if eligible(&meta, now, age) {
                candidates.push((name.to_owned(), meta, age));
            }
        }
        self.attached()?;
        for (name, before, age) in candidates {
            if stopped() || report.removed == MAX_REMOVALS {
                break;
            }
            self.attached()?;
            let Ok(file) = self.open_file(&name, false) else {
                continue;
            };
            let Ok(after) = file.metadata() else { continue };
            if !eligible(&after, now, age) || !same(&before, &after) {
                continue;
            }
            if stopped() {
                break;
            }
            self.attached()?;
            let Ok(current) = file.metadata() else {
                continue;
            };
            if !same(&after, &current) || !eligible(&current, now, age) {
                continue;
            }
            // Recheck the name against the open inode immediately before unlink.
            let name_c = std::ffi::CString::new(name).map_err(|_| "缓存文件名称无效。")?;
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            if unsafe {
                libc::fstatat(
                    self.directory.as_raw_fd(),
                    name_c.as_ptr(),
                    stat.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } != 0
            {
                continue;
            }
            let stat = unsafe { stat.assume_init() };
            if stat.st_dev as u64 != after.dev()
                || stat.st_ino as u64 != after.ino()
                || stat.st_mode & libc::S_IFMT != libc::S_IFREG
                || stat.st_nlink != 1
                || stat.st_uid != after.uid()
                || stat.st_mode & 0o7777 != 0o600
                || stat.st_size < 0
                || stat.st_size as u64 != after.len()
            {
                continue;
            }
            if unsafe { libc::unlinkat(self.directory.as_raw_fd(), name_c.as_ptr(), 0) } == 0 {
                report.removed += 1;
            } else if std::io::Error::last_os_error().kind() != std::io::ErrorKind::NotFound {
                return Err("部分过期更新缓存未能清理，将在后续重试。".into());
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::{
        cell::Cell,
        fs::OpenOptions,
        os::unix::fs::{symlink, PermissionsExt},
    };

    const ID: &str = "550e8400-e29b-41d4-a716-446655440000";
    fn dmg(n: usize) -> String {
        format!("MooMole-0.2.{n}-mac-arm64-{ID}.dmg")
    }
    fn partial() -> String {
        format!(".moomole-{ID}.download")
    }
    fn old(cache: &Cache, name: &str, time: SystemTime) {
        let file = cache.open_file(name, true).unwrap();
        file.set_modified(time).unwrap();
    }
    fn fixture() -> (tempfile::TempDir, std::sync::Arc<Cache>, SystemTime) {
        let temp = tempfile::tempdir().unwrap();
        let cache = Cache::open(&temp.path().join("cache")).unwrap();
        (temp, cache, SystemTime::now())
    }
    #[test]
    fn namespace_requires_exact_product_semver_architecture_and_canonical_v4_uuid() {
        assert_eq!(retention(&partial()), Some(DAY));
        assert_eq!(retention(&dmg(1)), Some(DAY * 7));
        assert_eq!(
            retention(&format!("MooMole-1.2.3-rc.1+build.4-mac-x64-{ID}.dmg")),
            Some(DAY * 7)
        );
        for name in [
            format!("MooTool-0.2.1-mac-arm64-{ID}.dmg"),
            format!("MooMole-00.2.1-mac-arm64-{ID}.dmg"),
            format!("MooMole-0.2.1-mac-x86_64-{ID}.dmg"),
            format!("MooMole-0.2.1-mac-arm64-{}.dmg", ID.to_uppercase()),
            dmg(1).replace("41d4", "11d4"),
            partial().replace("a716", "1716"),
            "MooMole-0.2.1-mac-arm64.dmg".into(),
            format!("{}/extra", partial()),
            format!("{}~", dmg(1)),
            format!(".moomole-{}.download", ID.replace('-', "")),
            format!("MooMole-你好-mac-arm64-{ID}.dmg"),
        ] {
            assert_eq!(retention(&name), None, "{name}");
        }
    }
    #[test]
    fn expiry_boundaries_protected_file_and_future_timestamps() {
        let (_temp, cache, now) = fixture();
        old(&cache, &partial(), now - DAY);
        old(&cache, &dmg(1), now - DAY * 7);
        old(&cache, &dmg(2), now - DAY * 7 + Duration::from_secs(1));
        old(&cache, &dmg(3), now + DAY);
        old(&cache, &dmg(4), now - DAY * 30);
        old(&cache, "keep.dmg", now - DAY * 30);
        let report = cache.cleanup(Some(&dmg(4)), now, || false).unwrap();
        assert_eq!(
            report,
            Cleanup {
                scanned: 6,
                removed: 2
            }
        );
        for name in [dmg(2), dmg(3), dmg(4), "keep.dmg".into()] {
            assert!(cache.path.join(name).is_file());
        }
        assert_eq!(cache.cleanup(None, now, || false).unwrap().removed, 1);
    }
    #[test]
    fn links_directories_and_nonprivate_files_are_retained_without_traversal() {
        let (temp, cache, now) = fixture();
        let outside = temp.path().join("outside");
        std::fs::write(&outside, b"keep").unwrap();
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o600)).unwrap();
        File::open(&outside)
            .unwrap()
            .set_modified(now - DAY * 30)
            .unwrap();
        symlink(&outside, cache.path.join(dmg(1))).unwrap();
        std::fs::hard_link(&outside, cache.path.join(dmg(2))).unwrap();
        std::fs::create_dir(cache.path.join(dmg(3))).unwrap();
        std::fs::write(cache.path.join(dmg(3)).join(partial()), b"nested").unwrap();
        for (n, mode) in [(4, 0o644), (5, 0o4600)] {
            old(&cache, &dmg(n), now - DAY * 30);
            std::fs::set_permissions(
                cache.path.join(dmg(n)),
                std::fs::Permissions::from_mode(mode),
            )
            .unwrap();
        }
        assert_eq!(cache.cleanup(None, now, || false).unwrap().removed, 0);
        assert_eq!(std::fs::read(&outside).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(&cache.path).unwrap().count(), 5);
    }
    #[test]
    fn missing_cache_is_not_created_and_directory_replacement_is_refused() {
        let (temp, cache, now) = fixture();
        let missing = temp.path().join("missing/child");
        assert_eq!(
            Cache::cleanup_existing(&missing, None, || false).unwrap(),
            Cleanup::default()
        );
        assert!(!temp.path().join("missing").exists());
        old(&cache, &dmg(1), now - DAY * 30);
        let moved = temp.path().join("moved");
        std::fs::rename(&cache.path, &moved).unwrap();
        symlink(&moved, &cache.path).unwrap();
        assert!(cache.cleanup(None, now, || false).is_err());
        assert!(Cache::cleanup_existing(&cache.path, None, || false).is_err());
        assert!(moved.join(dmg(1)).is_file());
        std::fs::remove_file(&cache.path).unwrap();
        std::fs::create_dir(&cache.path).unwrap();
        assert!(cache.cleanup(None, now, || false).is_err());
        std::fs::set_permissions(&cache.path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Cache::cleanup_existing(&cache.path, None, || false).is_err());
    }
    #[test]
    fn oversized_scan_is_aborted_before_any_deletion_and_removals_are_bounded() {
        let (_temp, cache, now) = fixture();
        for n in 0..MAX_ENTRIES + 1 {
            old(&cache, &dmg(n), now - DAY * 30);
        }
        assert!(cache.cleanup(None, now, || false).is_err());
        assert_eq!(
            std::fs::read_dir(&cache.path).unwrap().count(),
            MAX_ENTRIES + 1
        );
        std::fs::remove_file(cache.path.join(dmg(MAX_ENTRIES))).unwrap();
        let report = cache.cleanup(None, now, || false).unwrap();
        assert_eq!(report.scanned, MAX_ENTRIES);
        assert_eq!(report.removed, MAX_REMOVALS);
    }
    #[test]
    fn late_name_substitution_directory_change_and_stop_are_refused() {
        for kind in 0..4 {
            let (temp, cache, now) = fixture();
            old(&cache, &dmg(1), now - DAY * 30);
            let calls = Cell::new(0);
            let displaced = temp.path().join("original");
            let result = cache.cleanup(None, now, || {
                calls.set(calls.get() + 1);
                if calls.get() == 3 {
                    match kind {
                        0 | 1 => {
                            std::fs::rename(cache.path.join(dmg(1)), &displaced).unwrap();
                            if kind == 0 {
                                symlink(&displaced, cache.path.join(dmg(1))).unwrap();
                            } else {
                                old(&cache, &dmg(1), now - DAY * 30);
                            }
                        }
                        2 => {
                            std::fs::rename(&cache.path, &displaced).unwrap();
                            std::fs::create_dir(&cache.path).unwrap();
                        }
                        _ => return true,
                    }
                }
                false
            });
            if kind == 2 {
                assert!(result.is_err());
                assert!(displaced.join(dmg(1)).is_file());
            } else {
                assert_eq!(result.unwrap().removed, 0);
                assert!(cache.path.join(dmg(1)).exists());
            }
        }
    }

    #[test]
    fn shutdown_and_changed_candidate_are_retained() {
        let (_temp, cache, now) = fixture();
        old(&cache, &dmg(1), now - DAY * 30);
        assert_eq!(cache.cleanup(None, now, || true).unwrap().removed, 0);
        let calls = Cell::new(0);
        let report = cache
            .cleanup(None, now, || {
                calls.set(calls.get() + 1);
                if calls.get() == 2 {
                    // Mutation between scan and final file checks must be detected.
                    let file = OpenOptions::new()
                        .write(true)
                        .open(cache.path.join(dmg(1)))
                        .unwrap();
                    file.set_len(25).unwrap();
                }
                false
            })
            .unwrap();
        assert_eq!(report.removed, 0);
        assert_eq!(
            std::fs::metadata(cache.path.join(dmg(1))).unwrap().len(),
            25
        );
    }
}
