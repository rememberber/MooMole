//! Read-only DiskImages connection-list observation; never a removal prerequisite.
use crate::{engine, observation::AnchoredPath, process};
use serde::Serialize;
use std::{os::unix::fs::MetadataExt, path::Path, sync::Arc, time::Duration};
const OUTPUT_LIMIT: u64 = 256 * 1024;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    protocol: u32,
    revision: &'static str,
    root: String,
    path: String,
    checked_at: i64,
    state: &'static str,
    reason: Option<&'static str>,
    can_execute: bool,
}
pub struct Prepared {
    scope: AnchoredPath,
    held: AnchoredPath,
    root: String,
    path: String,
}
impl Prepared {
    pub fn open(root: &Path, path: &Path) -> Result<Self, String> {
        let valid = |p: &Path| {
            p.to_str().is_some_and(|s| {
                s.starts_with('/')
                    && !s.ends_with('/')
                    && s.len() <= 4096
                    && !s.chars().any(char::is_control)
                    && !s[1..].split('/').any(|v| ["", ".", ".."].contains(&v))
            })
        };
        if !valid(root) || !valid(path) {
            return Err("请选择有效的扫描范围与磁盘映像。".into());
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "映像不在所选范围内。")?;
        if !(1..=2).contains(&relative.components().count())
            || !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("dmg") || e.eq_ignore_ascii_case("iso"))
        {
            return Err("只能检查所选范围两层内的 DMG/ISO 文件。".into());
        }
        let scope = AnchoredPath::open(root)?;
        let held = AnchoredPath::open(path)?;
        let meta = held
            .target()
            .metadata()
            .map_err(|_| "映像元数据无法读取。")?;
        if !meta.is_file()
            || meta.nlink() != 1
            || !scope
                .target()
                .metadata()
                .is_ok_and(|m| m.is_dir() && m.dev() == meta.dev())
            || !scope.local_materialized()
            || !held.local_materialized()
        {
            return Err("只能检查本机已下载、非共享硬链接的普通映像文件。".into());
        }
        scope.validate()?;
        held.validate()?;
        Ok(Self {
            scope,
            held,
            root: root.to_str().unwrap().into(),
            path: path.to_str().unwrap().into(),
        })
    }
    fn finish(self, result: Result<Vec<u8>, String>) -> Result<Inspection, String> {
        self.scope
            .validate()
            .and_then(|_| self.held.validate())
            .map_err(|_| "检查期间映像或路径已变化；连接观察已撤回，请重新扫描。")?;
        let (state, reason) = match result {
            Ok(bytes) => match classify(&bytes, &self.path) {
                Ok(state) => (state, None),
                Err(reason) => ("unknown", Some(reason)),
            },
            Err(_) => ("unknown", Some("queryUnavailable")),
        };
        Ok(Inspection {
            protocol: 1,
            revision: engine::REVISION,
            root: self.root,
            path: self.path,
            checked_at: crate::db::now(),
            state,
            reason,
            can_execute: false,
        })
    }
}
fn classify(bytes: &[u8], target: &str) -> Result<&'static str, &'static str> {
    use objc2::rc::autoreleasepool;
    use objc2_foundation::{
        NSArray, NSData, NSDictionary, NSPropertyListReadOptions, NSPropertyListSerialization,
        NSString,
    };
    if bytes.is_empty() || bytes.len() as u64 > OUTPUT_LIMIT {
        return Err("listingLimit");
    }
    // A system plist needs no custom entity declarations or external expansion.
    if bytes
        .windows(8)
        .any(|v| v.eq_ignore_ascii_case(b"<!ENTITY"))
    {
        return Err("listingUncertain");
    }
    autoreleasepool(|_| {
        let object = unsafe {
            NSPropertyListSerialization::propertyListWithData_options_format_error(
                &NSData::with_bytes(bytes),
                NSPropertyListReadOptions::Immutable,
                std::ptr::null_mut(),
            )
        }
        .map_err(|_| "listingUncertain")?;
        let root = object
            .downcast_ref::<NSDictionary>()
            .ok_or("listingUncertain")?;
        let value = root
            .objectForKey(&NSString::from_str("images"))
            .ok_or("listingUncertain")?;
        let images = value.downcast_ref::<NSArray>().ok_or("listingUncertain")?;
        if images.count() > 128 {
            return Err("listingLimit");
        }
        let mut found = false;
        for image in images {
            let image = image
                .downcast_ref::<NSDictionary>()
                .ok_or("listingUncertain")?;
            let value = image
                .objectForKey(&NSString::from_str("image-path"))
                .ok_or("listingUncertain")?;
            let path = value
                .downcast_ref::<NSString>()
                .ok_or("listingUncertain")?
                .to_string();
            if path.len() > 4096
                || !path.starts_with('/')
                || path.ends_with('/')
                || path.chars().any(char::is_control)
                || path[1..].split('/').any(|v| ["", ".", ".."].contains(&v))
            {
                return Err("listingUncertain");
            }
            found |= path == target;
        }
        Ok(if found {
            "pathReported"
        } else {
            "pathNotListed"
        })
    })
}
pub async fn inspect(prepared: Prepared) -> Result<Inspection, String> {
    let mut command = tokio::process::Command::new("/usr/bin/hdiutil");
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .args(["info", "-plist"]);
    // No target argument, image read, attach/detach, alias resolution or shell.
    let result = process::output_bounded(
        command,
        Arc::new(engine::Job::new(engine::TaskKind::Analyze)),
        Duration::from_secs(2),
        OUTPUT_LIMIT,
    )
    .await;
    tauri::async_runtime::spawn_blocking(move || prepared.finish(result))
        .await
        .map_err(|_| "映像连接检查未完成。".to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, hard_link},
        os::unix::fs::symlink,
    };
    fn plist(body: &str) -> Vec<u8> {
        format!("<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>images</key><array>{body}</array></dict></plist>").into_bytes()
    }
    #[test]
    fn exact_connection_paths_include_unmounted_attachments_and_never_imply_safety() {
        let report = plist("<dict><key>image-path</key><string>/fixture/Tool.dmg</string><key>system-entities</key><array/></dict>");
        assert_eq!(classify(&report, "/fixture/Tool.dmg"), Ok("pathReported"));
        for path in [
            "/fixture/Tool.dmg-old",
            "/fixture/tool.dmg",
            "/else/Tool.dmg",
        ] {
            assert_eq!(classify(&report, path), Ok("pathNotListed"));
        }
        assert_eq!(
            classify(&plist(""), "/fixture/Tool.dmg"),
            Ok("pathNotListed")
        );
        for body in [
            "<string>bad</string>",
            "<dict/>",
            "<dict><key>image-path</key><integer>2</integer></dict>",
            "<dict><key>image-path</key><string>file:///fixture/Tool.dmg</string></dict>",
            "<dict><key>image-path</key><string>/fixture/../Tool.dmg</string></dict>",
        ] {
            assert_eq!(
                classify(&plist(body), "/fixture/Tool.dmg"),
                Err("listingUncertain")
            );
        }
        let too_many =
            "<dict><key>image-path</key><string>/fixture/a.dmg</string></dict>".repeat(129);
        assert_eq!(
            classify(&plist(&too_many), "/fixture/a.dmg"),
            Err("listingLimit")
        );
        assert_eq!(
            classify(&vec![b'x'; OUTPUT_LIMIT as usize + 1], "/fixture/a.dmg"),
            Err("listingLimit")
        );
        assert!(classify(b"<!ENTITY x SYSTEM 'file:///etc/passwd'>", "/fixture/a.dmg").is_err());
        assert!(classify(b"not a plist", "/fixture/a.dmg").is_err());
    }
    #[test]
    fn preparation_and_finish_reject_aliases_sharing_scope_and_file_changes() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let path = root.path().join("sample.DMG");
        fs::write(&path, b"not read or treated as an image format").unwrap();
        assert!(Prepared::open(root.path(), &path).is_ok());
        let before = Prepared::open(root.path(), &path).unwrap();
        fs::write(&path, b"changed").unwrap();
        assert!(before.finish(Ok(plist(""))).is_err());
        let alias = root.path().join("alias.dmg");
        symlink(&path, &alias).unwrap();
        assert!(Prepared::open(root.path(), &alias).is_err());
        fs::remove_file(&alias).unwrap();
        hard_link(&path, &alias).unwrap();
        assert!(Prepared::open(root.path(), &path).is_err());
        for invalid in [
            root.path().join("../sample.DMG"),
            root.path().join("a/b/sample.dmg"),
            root.path().join("sample.zip"),
        ] {
            assert!(Prepared::open(root.path(), &invalid).is_err());
        }
    }
    #[tokio::test]
    async fn actual_system_query_keeps_unlisted_path_and_query_failure_non_executable() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let path = root.path().join("fixture.iso");
        fs::write(&path, b"fixture; do not open or mount").unwrap();
        let r = inspect(Prepared::open(root.path(), &path).unwrap())
            .await
            .unwrap();
        assert_eq!(r.state, "pathNotListed");
        assert!(!r.can_execute);
        let r = Prepared::open(root.path(), &path)
            .unwrap()
            .finish(Err("fixture query failure".into()))
            .unwrap();
        assert_eq!(r.state, "unknown");
        assert_eq!(r.reason, Some("queryUnavailable"));
        assert!(!r.can_execute);
    }
}
