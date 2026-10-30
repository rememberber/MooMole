//! Explicit one-item metadata observation. Never walks a directory, opens file
//! contents, asks a provider to download/evict, or issues mutation authority.
use crate::{observation::AnchoredPath, AppState};
use serde::Serialize;
use std::{
    mem::MaybeUninit,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::Path,
    time::Duration,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Observation {
    version: u32,
    path: String,
    checked_at: i64,
    is_directory: bool,
    logical_bytes: Option<u64>,
    allocated_bytes: Option<u64>,
    dataless: bool,
    location: &'static str,
}

fn location(path: &Path, home: &Path) -> &'static str {
    if path.starts_with(home.join("Library/Mobile Documents")) {
        "icloudLocation"
    } else if path.starts_with(home.join("Library/CloudStorage")) {
        "providerLocation"
    } else if path.starts_with(home.join("Library")) {
        "accountLibrary"
    } else if path.starts_with(home) {
        "unclassified"
    } else if ["/System", "/Library", "/private", "/usr", "/bin", "/sbin"]
        .iter()
        .any(|root| path.starts_with(root))
    {
        "systemLocation"
    } else {
        "unclassified"
    }
}

fn inspect(path: &str, home: &Path) -> Result<Observation, String> {
    if path.len() > 4096 || path.chars().any(char::is_control) {
        return Err("项目路径无效，请重新选择。".into());
    }
    let held = AnchoredPath::open(Path::new(path))?;
    // Unlike processing admission, this observation must allow dataless local
    // entries. Do not reuse local_materialized()/directory_probe::local().
    let mut fs = MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::fstatfs(held.target().as_raw_fd(), fs.as_mut_ptr()) } != 0
        || unsafe { fs.assume_init() }.f_flags & libc::MNT_LOCAL as u32 == 0
    {
        return Err("此项目不在可核对的本地卷上，未读取空间属性。".into());
    }
    let m = held.target().metadata().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let dataless = {
        use std::os::macos::fs::MetadataExt as _;
        m.st_flags() & 0x40000000 != 0 // SF_DATALESS, metadata only
    };
    #[cfg(not(target_os = "macos"))]
    let dataless = false;
    // st_blocks counts 512-byte units. Neither this nor logical length measures
    // exclusive APFS storage, purgeable bytes, snapshots, or reclaimable space.
    let safe = |n: u64| (n <= 9_007_199_254_740_991).then_some(n);
    let result = Observation {
        version: 1,
        path: path.into(),
        checked_at: crate::db::now(),
        is_directory: m.is_dir(),
        logical_bytes: (!m.is_dir()).then(|| safe(m.len())).flatten(),
        allocated_bytes: if m.is_dir() {
            None
        } else {
            m.blocks().checked_mul(512).and_then(safe)
        },
        dataless,
        location: location(Path::new(path), home),
    };
    held.validate()?;
    // Include allocation and flags in the final metadata stability check, as
    // providers and sparse allocation can change without a length change.
    let after = held.target().metadata().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt as _;
        if m.st_flags() != after.st_flags() {
            return Err("空间属性已变化，请重新读取。".into());
        }
    }
    if m.blocks() != after.blocks() {
        return Err("空间属性已变化，请重新读取。".into());
    }
    held.validate()?;
    Ok(result)
}

#[tauri::command]
pub(crate) async fn inspect_space_item(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Observation, String> {
    let permit = state
        .inspection_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "仍在读取其他项目，请稍后重试。")?;
    let home = state.home.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        inspect(&path, &home)
    });
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .map_err(|_| "空间属性读取超时，未取得可用结果。")?
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn open_storage_settings() -> Result<(), String> {
    // Fixed bundle ID, not an undocumented Settings-pane URL or user command.
    let mut command = tokio::process::Command::new("/usr/bin/open");
    command
        .args(["-b", "com.apple.systempreferences"])
        .kill_on_drop(true);
    let status = tokio::time::timeout(Duration::from_secs(5), command.status())
        .await
        .map_err(|_| "打开系统设置超时，请从 Apple 菜单打开。")?
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("无法打开系统设置，请从 Apple 菜单打开。".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sparse_file_is_observed_without_reading_contents_and_directories_are_not_totaled() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().canonicalize().unwrap();
        let file = root.join("large sparse.bin");
        let f = std::fs::File::create(&file).unwrap();
        f.set_len(64 * 1024 * 1024).unwrap();
        let before = f.metadata().unwrap();
        let r = inspect(file.to_str().unwrap(), &root).unwrap();
        assert_eq!(r.logical_bytes, Some(64 * 1024 * 1024));
        assert_eq!(r.allocated_bytes, Some(before.blocks() * 512));
        assert!(!r.dataless);
        assert_eq!(r.location, "unclassified");
        assert_eq!(before.ino(), f.metadata().unwrap().ino());
        assert_eq!(before.mtime(), f.metadata().unwrap().mtime());
        assert_eq!(before.atime(), f.metadata().unwrap().atime());
        let directory = inspect(root.to_str().unwrap(), &root).unwrap();
        assert!(directory.is_directory);
        assert!(directory.logical_bytes.is_none() && directory.allocated_bytes.is_none());
        assert!(inspect("/", &root).is_err());
        std::os::unix::fs::symlink(&file, root.join("link")).unwrap();
        assert!(inspect(root.join("link").to_str().unwrap(), &root).is_err());
        std::fs::hard_link(&file, root.join("shared")).unwrap();
        assert!(inspect(file.to_str().unwrap(), &root).is_err());
    }

    #[test]
    fn cloud_locations_are_component_based_hints_and_never_sync_claims() {
        let home = Path::new("/Users/fixture");
        for (p, expected) in [
            ("Library/Mobile Documents/a/file", "icloudLocation"),
            ("Library/CloudStorage/provider/file", "providerLocation"),
            ("Library/Mobile Documents Copy/a", "accountLibrary"),
            ("Library/CloudStorageOther/file", "accountLibrary"),
            ("Downloads/cloud name", "unclassified"),
        ] {
            assert_eq!(location(&home.join(p), home), expected);
        }
        assert_eq!(
            location(Path::new("/System/Library"), home),
            "systemLocation"
        );
        assert_eq!(location(Path::new("/LibraryOther"), home), "unclassified");
    }
}
