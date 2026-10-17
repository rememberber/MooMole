//! Reuse only the exact asset selected by a fresh authoritative update check.
use super::{verify_file, Cache, Control, Ready};
use crate::updates::Asset;
use std::{
    os::unix::fs::MetadataExt,
    path::Path,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};
const MAX_AGE: Duration = Duration::from_secs(7 * 86400);
const UNAVAILABLE: &str = "本地更新包未能复用，请重新下载。";
struct Limits {
    entries: usize,
    candidates: usize,
    bytes: u64,
    deadline: Instant,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            entries: 512,
            candidates: 4,
            bytes: 2 * 1024 * 1024 * 1024,
            deadline: Instant::now() + Duration::from_secs(30),
        }
    }
}
impl Cache {
    /// Does not create, rewrite, quarantine, or delete any cached file.
    pub fn recover_existing(
        path: &Path,
        asset: &Asset,
        control: &Control,
    ) -> Result<Option<Ready>, String> {
        if let Err(e) = std::fs::symlink_metadata(path) {
            if e.kind() == std::io::ErrorKind::NotFound {
                return Ok(None);
            }
        }
        recover(
            Self::existing(path)?,
            asset,
            control,
            SystemTime::now(),
            Limits::default(),
        )
    }
}
fn recover(
    cache: Arc<Cache>,
    asset: &Asset,
    control: &Control,
    now: SystemTime,
    limits: Limits,
) -> Result<Option<Ready>, String> {
    let stop = || {
        if control.cancelled() {
            return Err("本地更新包复核已停止。".to_string());
        }
        if Instant::now() >= limits.deadline {
            return Err("本地更新包复核超时，请重新下载。".into());
        }
        Ok(())
    };
    stop()?;
    cache.attached()?;
    let prefix = format!(
        "{}-",
        asset.file_name.strip_suffix(".dmg").ok_or(UNAVAILABLE)?
    );
    let mut candidates = Vec::new();
    let mut invalid = false;
    let mut matches = 0;
    for (n, entry) in std::fs::read_dir(&cache.path)
        .map_err(|_| UNAVAILABLE)?
        .enumerate()
    {
        stop()?;
        if n == limits.entries {
            return Err("本地更新缓存条目过多，请重新下载或查看发布页。".into());
        }
        let name = entry.map_err(|_| UNAVAILABLE)?.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(&prefix) || super::maintenance::retention(name) != Some(MAX_AGE) {
            continue;
        }
        matches += 1;
        if matches > limits.candidates {
            return Err("本地同版更新包过多，请重新下载或查看发布页。".into());
        }
        let Ok(file) = cache.open_file(name, false) else {
            invalid = true;
            continue;
        };
        let meta = file.metadata().map_err(|_| UNAVAILABLE)?;
        if !meta.is_file()
            || meta.nlink() != 1
            || meta.uid() != unsafe { libc::geteuid() }
            || meta.mode() & 0o7777 != 0o600
            || meta.len() != asset.size
        {
            invalid = true;
            continue;
        }
        let modified = meta.modified().map_err(|_| UNAVAILABLE)?;
        match now.duration_since(modified) {
            Ok(age) if age < MAX_AGE => (),
            Ok(_) => continue,
            Err(_) => {
                invalid = true;
                continue;
            }
        }
        candidates.push((modified, name.to_owned(), (meta.dev(), meta.ino())));
    }
    cache.attached()?;
    // Deterministic newest-first order; any reused file must match the same digest.
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut bytes = 0u64;
    for (_, name, identity) in candidates {
        stop()?;
        bytes = bytes.checked_add(asset.size).ok_or(UNAVAILABLE)?;
        if bytes > limits.bytes {
            return Err("本地更新包复核超出读取预算，请重新下载。".into());
        }
        cache.attached()?;
        let result: Result<Ready, String> = (|| {
            let mut file = cache.open_file(&name, false)?;
            let verified = verify_file(
                &mut file,
                asset,
                identity,
                Some(control),
                Some(limits.deadline),
            )?;
            crate::update_quarantine::present(&cache.path.join(&name))?;
            cache.attached()?;
            let current = cache
                .open_file(&name, false)?
                .metadata()
                .map_err(|_| UNAVAILABLE)?;
            if !super::maintenance::same(&verified, &current)
                || current.mode() & 0o7777 != 0o600
                || current.nlink() != 1
                || current.uid() != unsafe { libc::geteuid() }
            {
                return Err(UNAVAILABLE.into());
            }
            Ok(Ready {
                cache: cache.clone(),
                name,
                asset: asset.clone(),
                identity,
            })
        })();
        match result {
            Ok(ready) => {
                stop()?;
                return Ok(Some(ready));
            }
            Err(_) => invalid = true,
        }
    }
    stop()?;
    if invalid {
        Err(UNAVAILABLE.into())
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::File,
        io::Write,
        os::unix::fs::{symlink, PermissionsExt},
    };
    async fn fixture() -> (tempfile::TempDir, Asset, std::path::PathBuf) {
        let (temp, ready) = crate::update_download::tests::installation_fixture().await;
        let path = ready.verified_path().unwrap();
        (temp, ready.asset, path)
    }
    #[tokio::test]
    async fn restart_reopens_verified_file_without_changing_bytes_age_or_creating_metadata() {
        let (_temp, asset, path) = fixture().await;
        let before = std::fs::metadata(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let ready = Cache::recover_existing(path.parent().unwrap(), &asset, &Control::default())
            .unwrap()
            .unwrap();
        assert_eq!(ready.verified_path().unwrap(), path);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let after = std::fs::metadata(&path).unwrap();
        assert!(super::super::maintenance::same(&before, &after));
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }
    #[tokio::test]
    async fn exact_selected_version_architecture_and_digest_are_required_and_partials_ignored() {
        let (_temp, asset, path) = fixture().await;
        let root = path.parent().unwrap();
        for file_name in [
            asset.file_name.replace("0.2.0", "0.3.0"),
            asset.file_name.replace("arm64", "x64"),
        ] {
            let mut changed = asset.clone();
            changed.file_name = file_name;
            assert!(Cache::recover_existing(root, &changed, &Control::default())
                .unwrap()
                .is_none());
        }
        let mut changed = asset.clone();
        changed.sha512 = "a".repeat(128);
        assert!(Cache::recover_existing(root, &changed, &Control::default()).is_err());
        let partial = root.join(format!(".moomole-{}.download", uuid::Uuid::new_v4()));
        std::fs::rename(&path, &partial).unwrap();
        assert!(Cache::recover_existing(root, &asset, &Control::default())
            .unwrap()
            .is_none());
        assert!(partial.is_file());
    }
    #[tokio::test]
    async fn links_directories_nonprivate_files_and_missing_quarantine_are_never_reused() {
        let (temp, asset, path) = fixture().await;
        let root = path.parent().unwrap();
        let outside = temp.path().join("outside");
        std::fs::rename(&path, &outside).unwrap();
        symlink(&outside, &path).unwrap();
        assert!(Cache::recover_existing(root, &asset, &Control::default()).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::hard_link(&outside, &path).unwrap();
        assert!(Cache::recover_existing(root, &asset, &Control::default()).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(Cache::recover_existing(root, &asset, &Control::default()).is_err());
        std::fs::remove_dir(&path).unwrap();
        std::fs::rename(&outside, &path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Cache::recover_existing(root, &asset, &Control::default()).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let body = std::fs::read(&path).unwrap();
        std::fs::rename(&path, &outside).unwrap();
        let cache = Cache::existing(root).unwrap();
        cache
            .open_file(path.file_name().unwrap().to_str().unwrap(), true)
            .unwrap()
            .write_all(&body)
            .unwrap();
        #[cfg(target_os = "macos")]
        {
            assert!(crate::update_quarantine::present(&path).is_err());
            assert!(Cache::recover_existing(root, &asset, &Control::default()).is_err());
        }
        assert_eq!(std::fs::read(&outside).unwrap(), body);
        assert!(path.is_file());
    }
    #[tokio::test]
    async fn expired_and_future_files_are_not_restored_and_age_is_not_extended() {
        let (_temp, asset, path) = fixture().await;
        let cache = Cache::existing(path.parent().unwrap()).unwrap();
        let now = SystemTime::now();
        let file = File::open(&path).unwrap();
        file.set_modified(now - MAX_AGE).unwrap();
        assert!(recover(
            cache.clone(),
            &asset,
            &Control::default(),
            now,
            Limits::default()
        )
        .unwrap()
        .is_none());
        assert!(path.is_file());
        file.set_modified(now + Duration::from_secs(60)).unwrap();
        assert!(recover(
            cache.clone(),
            &asset,
            &Control::default(),
            now,
            Limits::default()
        )
        .is_err());
        let age = now - MAX_AGE + Duration::from_secs(1);
        file.set_modified(age).unwrap();
        assert!(
            recover(cache, &asset, &Control::default(), now, Limits::default())
                .unwrap()
                .is_some()
        );
        assert_eq!(file.metadata().unwrap().modified().unwrap(), age);
    }
    #[tokio::test]
    async fn scan_candidate_byte_deadline_and_cancellation_limits_preserve_files() {
        let (_temp, asset, path) = fixture().await;
        let cache = Cache::existing(path.parent().unwrap()).unwrap();
        let now = SystemTime::now();
        for limits in [
            Limits {
                entries: 0,
                ..Limits::default()
            },
            Limits {
                candidates: 0,
                ..Limits::default()
            },
            Limits {
                bytes: asset.size - 1,
                ..Limits::default()
            },
            Limits {
                deadline: Instant::now(),
                ..Limits::default()
            },
        ] {
            assert!(recover(cache.clone(), &asset, &Control::default(), now, limits).is_err());
        }
        let stop = Control::default();
        stop.cancel();
        assert!(Cache::recover_existing(&cache.path, &asset, &stop).is_err());
        assert!(path.is_file());
        assert_eq!(std::fs::read_dir(&cache.path).unwrap().count(), 1);
    }
    #[tokio::test]
    async fn newer_corrupt_duplicate_falls_back_to_verified_copy_with_shared_read_budget() {
        let (_temp, asset, path) = fixture().await;
        let cache = Cache::existing(path.parent().unwrap()).unwrap();
        let name = format!(
            "{}-{}.dmg",
            asset.file_name.trim_end_matches(".dmg"),
            uuid::Uuid::new_v4()
        );
        let mut corrupt = cache.open_file(&name, true).unwrap();
        corrupt.write_all(&vec![0; asset.size as usize]).unwrap();
        let now = SystemTime::now();
        File::open(&path)
            .unwrap()
            .set_modified(now - Duration::from_secs(120))
            .unwrap();
        corrupt.set_modified(now - Duration::from_secs(60)).unwrap();
        assert!(recover(
            cache.clone(),
            &asset,
            &Control::default(),
            now,
            Limits {
                bytes: asset.size,
                ..Limits::default()
            }
        )
        .is_err());
        let ready = recover(
            cache.clone(),
            &asset,
            &Control::default(),
            now,
            Limits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(ready.verified_path().unwrap(), path);
        assert!(cache.path.join(name).is_file());
    }
    #[tokio::test]
    async fn missing_cache_is_not_created_and_detached_or_linked_cache_is_refused() {
        let (temp, asset, path) = fixture().await;
        let missing = temp.path().join("missing/child");
        assert!(
            Cache::recover_existing(&missing, &asset, &Control::default())
                .unwrap()
                .is_none()
        );
        assert!(!temp.path().join("missing").exists());
        let cache = Cache::existing(path.parent().unwrap()).unwrap();
        let moved = temp.path().join("moved");
        std::fs::rename(&cache.path, &moved).unwrap();
        symlink(&moved, &cache.path).unwrap();
        assert!(Cache::recover_existing(&cache.path, &asset, &Control::default()).is_err());
        assert!(recover(
            cache,
            &asset,
            &Control::default(),
            SystemTime::now(),
            Limits::default()
        )
        .is_err());
        assert!(moved.join(path.file_name().unwrap()).is_file());
    }
}
