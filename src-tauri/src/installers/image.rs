//! On-demand DMG internal checksum verification; no mount, install or trust decision.
use super::file::FileProbe;
use crate::{engine, process};
use serde::Serialize;
use std::{
    os::{fd::AsRawFd, unix::process::CommandExt},
    path::Path,
    sync::Arc,
    time::Duration,
};
const OUTPUT_LIMIT: u64 = 32 * 1024;
const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    protocol: u32,
    revision: &'static str,
    root: String,
    path: String,
    checked_at: i64,
    file_stamp: String,
    state: &'static str,
    reason: Option<&'static str>,
    can_execute: bool,
}
pub struct Prepared(FileProbe);
impl Prepared {
    pub fn open(root: &Path, path: &Path) -> Result<Self, String> {
        FileProbe::open(root, path, "dmg").map(Self)
    }
    fn finish(self, result: Result<(), &'static str>) -> Result<Inspection, String> {
        let probe = self.0;
        probe
            .scope
            .validate()
            .and_then(|_| probe.held.validate())
            .map_err(|_| "校验期间 DMG 或路径已变化；本次结果已撤回，请重新扫描。")?;
        Ok(Inspection {
            protocol: 1,
            revision: engine::REVISION,
            root: probe.root,
            path: probe.path,
            checked_at: crate::db::now(),
            file_stamp: probe.stamp,
            state: if result.is_ok() { "passed" } else { "unknown" },
            reason: result.err(),
            can_execute: false,
        })
    }
}
fn checksum_report(bytes: &[u8]) -> Result<(), &'static str> {
    use objc2::rc::autoreleasepool;
    use objc2_foundation::{
        NSData, NSDictionary, NSPropertyListReadOptions, NSPropertyListSerialization, NSString,
    };
    if bytes.is_empty() || bytes.len() as u64 > OUTPUT_LIMIT {
        return Err("reportUncertain");
    }
    if bytes
        .windows(8)
        .any(|w| w.eq_ignore_ascii_case(b"<!ENTITY"))
    {
        return Err("reportUncertain");
    }
    autoreleasepool(|_| {
        let value = unsafe {
            NSPropertyListSerialization::propertyListWithData_options_format_error(
                &NSData::with_bytes(bytes),
                NSPropertyListReadOptions::Immutable,
                std::ptr::null_mut(),
            )
        }
        .map_err(|_| "reportUncertain")?;
        let root = value
            .downcast_ref::<NSDictionary>()
            .ok_or("reportUncertain")?;
        if root.count() != 2 {
            return Err("reportUncertain");
        }
        let mut checksums = vec![];
        for key in ["calculated", "expected"] {
            let value = root
                .objectForKey(&NSString::from_str(key))
                .ok_or("reportUncertain")?;
            let dict = value
                .downcast_ref::<NSDictionary>()
                .ok_or("reportUncertain")?;
            if dict.count() != 2 {
                return Err("reportUncertain");
            }
            let kind = dict
                .objectForKey(&NSString::from_str("Checksum Type"))
                .ok_or("reportUncertain")?;
            let kind = kind
                .downcast_ref::<NSString>()
                .ok_or("reportUncertain")?
                .to_string();
            if kind != "CRC32" {
                return Err("checksumUnconfirmed");
            }
            let value = dict
                .objectForKey(&NSString::from_str("Checksum Value"))
                .ok_or("reportUncertain")?;
            let text = value
                .downcast_ref::<NSString>()
                .ok_or("reportUncertain")?
                .to_string();
            if text.len() != 9
                || !text.starts_with('$')
                || !text.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
            {
                return Err("reportUncertain");
            }
            checksums.push(text.to_ascii_uppercase());
        }
        if checksums[0] != checksums[1] {
            return Err("checksumUnconfirmed");
        }
        Ok(())
    })
}
pub async fn inspect(prepared: Prepared) -> Result<Inspection, String> {
    let size = prepared
        .0
        .file
        .metadata()
        .map_err(|_| "DMG 元数据无法读取。")?
        .len();
    let result = if size > MAX_FILE_BYTES {
        Err("fileSizeLimit")
    } else {
        let fd = prepared.0.file.as_raw_fd();
        let mut command = tokio::process::Command::new("/usr/bin/hdiutil");
        command
            .current_dir("/")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .args(["verify", "-nocache", "-plist", "-stdinpass", "/dev/fd/3"]);
        unsafe {
            command.as_std_mut().pre_exec(move || {
                if libc::dup2(fd, 3) < 0 || libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let cpu = libc::rlimit {
                    rlim_cur: 4,
                    rlim_max: 4,
                };
                if libc::setrlimit(libc::RLIMIT_CPU, &cpu) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        match process::output_bounded(
            command,
            Arc::new(engine::Job::new(engine::TaskKind::Analyze)),
            Duration::from_secs(4),
            OUTPUT_LIMIT,
        )
        .await
        {
            Ok(bytes) => checksum_report(&bytes),
            Err(_) => Err("verificationUnavailable"),
        }
    };
    tauri::async_runtime::spawn_blocking(move || prepared.finish(result))
        .await
        .map_err(|_| "DMG 校验未完成。".to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, File},
        os::unix::fs::symlink,
    };
    fn plist(kind: &str, expected: &str, calculated: &str) -> Vec<u8> {
        format!("<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>expected</key><dict><key>Checksum Type</key><string>{kind}</string><key>Checksum Value</key><string>{expected}</string></dict><key>calculated</key><dict><key>Checksum Type</key><string>{kind}</string><key>Checksum Value</key><string>{calculated}</string></dict></dict></plist>").into_bytes()
    }
    #[test]
    fn only_complete_recognized_matching_crc_reports_pass_without_granting_trust() {
        assert_eq!(
            checksum_report(&plist("CRC32", "$123456AB", "$123456ab")),
            Ok(())
        );
        for bytes in [
            plist("CRC32", "$123456AB", "$123456AC"),
            plist("none", "$123456AB", "$123456AB"),
            plist("CRC32", "123456AB", "123456AB"),
            plist("CRC32", "$123456AZ", "$123456AZ"),
            b"<plist><dict/></plist>".to_vec(),
            b"<plist><array/></plist>".to_vec(),
            b"not a plist".to_vec(),
            b"<!ENTITY x SYSTEM 'file:///etc/passwd'>".to_vec(),
            vec![b'x'; OUTPUT_LIMIT as usize + 1],
        ] {
            assert!(checksum_report(&bytes).is_err());
        }
        let bytes = plist("CRC32", "$123456AB", "$123456AB");
        assert!(checksum_report(&bytes[..bytes.len() - 10]).is_err());
    }
    #[test]
    fn metadata_scope_sharing_and_late_changes_reject_before_publishing() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let path = root.path().join("fixture.DMG");
        fs::write(&path, b"metadata fixture").unwrap();
        let prepared = Prepared::open(root.path(), &path).unwrap();
        fs::write(&path, b"changed fixture").unwrap();
        assert!(prepared.finish(Ok(())).is_err());
        let alias = root.path().join("alias.dmg");
        symlink(&path, &alias).unwrap();
        assert!(Prepared::open(root.path(), &alias).is_err());
        fs::remove_file(&alias).unwrap();
        fs::hard_link(&path, &alias).unwrap();
        assert!(Prepared::open(root.path(), &path).is_err());
        assert!(Prepared::open(Path::new("/"), &path).is_err());
        for path in [
            root.path().join("a/b/file.dmg"),
            root.path().join("fixture.iso"),
        ] {
            assert!(Prepared::open(root.path(), &path).is_err());
        }
    }
    #[tokio::test]
    async fn actual_invalid_image_and_oversize_never_report_passed_or_extract() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let path = root.path().join("fixture.dmg");
        fs::write(&path, b"not a disk image").unwrap();
        let r = inspect(Prepared::open(root.path(), &path).unwrap())
            .await
            .unwrap();
        assert_eq!(r.state, "unknown");
        assert_eq!(r.reason, Some("verificationUnavailable"));
        assert!(!r.can_execute);
        assert_eq!(r.file_stamp.len(), 64);
        File::create(&path)
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        let r = inspect(Prepared::open(root.path(), &path).unwrap())
            .await
            .unwrap();
        assert_eq!(r.state, "unknown");
        assert_eq!(r.reason, Some("fileSizeLimit"));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[tokio::test]
    #[ignore = "requires MOOMOLE_IMAGE_FIXTURE naming an existing self-built local DMG; only mutates a private copy"]
    async fn actual_local_dmg_matches_checksum_and_private_corruption_stays_unknown() {
        let source = std::env::var("MOOMOLE_IMAGE_FIXTURE").unwrap();
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let path = root.path().join("fixture.dmg");
        fs::copy(source, &path).unwrap();
        let r = inspect(Prepared::open(root.path(), &path).unwrap())
            .await
            .unwrap();
        assert_eq!(r.state, "passed");
        assert_eq!(r.reason, None);
        assert!(!r.can_execute);
        let mut bytes = fs::read(&path).unwrap();
        let midpoint = bytes.len() / 2;
        bytes[midpoint] ^= 1;
        fs::write(&path, bytes).unwrap();
        let r = inspect(Prepared::open(root.path(), &path).unwrap())
            .await
            .unwrap();
        assert_eq!(r.state, "unknown");
        assert!(!r.can_execute);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
