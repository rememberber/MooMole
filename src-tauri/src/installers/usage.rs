//! Positive-only visible open-handle observations; an empty view never proves idle.
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
    device: u64,
    inode: u64,
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
            return Err("请选择有效的扫描范围与安装包文件。".into());
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "文件不在所选范围内。")?;
        if !(1..=2).contains(&relative.components().count())
            || !path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                ["dmg", "iso", "pkg", "mpkg", "xip", "zip"]
                    .contains(&e.to_ascii_lowercase().as_str())
            })
        {
            return Err("只能检查所选范围两层内六类扩展名的文件。".into());
        }
        let scope = AnchoredPath::open(root)?;
        let held = AnchoredPath::open(path)?;
        let meta = held
            .target()
            .metadata()
            .map_err(|_| "文件元数据无法读取。")?;
        if !meta.is_file()
            || meta.nlink() != 1
            || !scope
                .target()
                .metadata()
                .is_ok_and(|m| m.is_dir() && m.dev() == meta.dev())
            || !scope.local_materialized()
            || !held.local_materialized()
        {
            return Err("只能检查本机已下载、非共享硬链接的普通文件。".into());
        }
        scope.validate()?;
        held.validate()?;
        Ok(Self {
            scope,
            held,
            root: root.to_str().unwrap().into(),
            path: path.to_str().unwrap().into(),
            device: meta.dev(),
            inode: meta.ino(),
        })
    }
    fn finish(self, result: Result<Vec<u8>, String>) -> Result<Inspection, String> {
        self.scope
            .validate()
            .and_then(|_| self.held.validate())
            .map_err(|_| "检查期间文件或路径已变化；占用观察已撤回，请重新扫描。")?;
        let (state, reason) = match result {
            Ok(bytes) => match classify(&bytes, self.device, self.inode, std::process::id()) {
                Ok(true) => ("busy", None),
                Ok(false) => ("unknown", Some("limitedView")),
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
// lsof -F0 machine fields, never human prose or names. Own O_EVTONLY
// descriptors are expected and must not make every observed target appear busy.
fn classify(bytes: &[u8], device: u64, inode: u64, own_pid: u32) -> Result<bool, &'static str> {
    if bytes.len() as u64 > OUTPUT_LIMIT {
        return Err("listingLimit");
    }
    if bytes.is_empty() {
        return Ok(false);
    }
    if !bytes.ends_with(b"\n") {
        return Err("listingUncertain");
    }
    let mut pid = None;
    let mut found = false;
    let mut records = 0;
    let mut files_in_process = 0;
    for line in bytes.strip_suffix(b"\n").unwrap().split(|b| *b == b'\n') {
        if !line.ends_with(&[0]) {
            return Err("listingUncertain");
        }
        let fields = line
            .strip_suffix(&[0])
            .unwrap()
            .split(|b| *b == 0)
            .collect::<Vec<_>>();
        records += 1;
        if records > 4096 {
            return Err("listingLimit");
        }
        if fields.len() == 1 && fields[0].starts_with(b"p") {
            if (pid.is_some() && files_in_process == 0)
                || !fields[0][1..].iter().all(u8::is_ascii_digit)
            {
                return Err("listingUncertain");
            }
            files_in_process = 0;
            let value = std::str::from_utf8(&fields[0][1..])
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|p| *p > 0);
            pid = Some(value.ok_or("listingUncertain")?);
            continue;
        }
        let current = pid.ok_or("listingUncertain")?;
        if fields.len() != 4
            || fields[0].first() != Some(&b'f')
            || fields[0].len() < 2
            || fields[0].len() > 33
            || !fields[0][1..].iter().all(u8::is_ascii_alphanumeric)
        {
            return Err("listingUncertain");
        }
        let mut kind = None;
        let mut dev = None;
        let mut ino = None;
        for field in &fields[1..] {
            match field.first() {
                Some(b't') if kind.is_none() => kind = Some(*field == b"tREG"),
                Some(b'D') if dev.is_none() => {
                    dev = Some(
                        std::str::from_utf8(&field[1..])
                            .ok()
                            .and_then(|s| s.strip_prefix("0x"))
                            .and_then(|s| u64::from_str_radix(s, 16).ok())
                            .ok_or("listingUncertain")?,
                    );
                }
                Some(b'i') if ino.is_none() => {
                    ino = Some(
                        std::str::from_utf8(&field[1..])
                            .ok()
                            .and_then(|s| s.parse::<u64>().ok())
                            .ok_or("listingUncertain")?,
                    );
                }
                _ => return Err("listingUncertain"),
            }
        }
        if kind != Some(true) || dev != Some(device) || ino != Some(inode) {
            return Err("listingUncertain");
        }
        files_in_process += 1;
        found |= current != own_pid;
    }
    if files_in_process == 0 {
        return Err("listingUncertain");
    }
    Ok(found)
}
pub async fn inspect(prepared: Prepared) -> Result<Inspection, String> {
    let mut command = tokio::process::Command::new("/usr/sbin/lsof");
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .args(["-nP", "-F0pftDi", "--", &prepared.path]);
    // Exact validated regular file only; no recursive walk, process names, sudo or shell.
    let result = process::output_bounded(
        command,
        Arc::new(engine::Job::new(engine::TaskKind::Analyze)),
        Duration::from_secs(2),
        OUTPUT_LIMIT,
    )
    .await;
    tauri::async_runtime::spawn_blocking(move || prepared.finish(result))
        .await
        .map_err(|_| "文件占用检查未完成。".to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, File},
        os::{
            fd::AsRawFd,
            unix::{fs::symlink, process::CommandExt},
        },
    };
    fn record(pid: u32, device: u64, inode: u64) -> Vec<u8> {
        format!("p{pid}\0\nf4\0tREG\0D0x{device:x}\0i{inode}\0\n").into_bytes()
    }
    #[test]
    fn complete_machine_fields_bind_file_identity_and_exclude_our_observation_handles() {
        assert_eq!(classify(&record(10, 20, 30), 20, 30, 10), Ok(false));
        assert_eq!(classify(&record(11, 20, 30), 20, 30, 10), Ok(true));
        assert_eq!(classify(b"", 20, 30, 10), Ok(false));
        let good = record(11, 20, 30);
        for bad in [
            record(11, 21, 30),
            record(11, 20, 31),
            b"lsof: warning\n".to_vec(),
            b"p11\0\nf4\0tREG\0i30\0\n".to_vec(),
            b"p11\0\nf4\0tREG\0D0x14\0D0x14\0\n".to_vec(),
            good[..good.len() - 1].to_vec(),
            [good.as_slice(), b"p12\0\n"].concat(),
            [good.as_slice(), b"unexpected tail\n"].concat(),
        ] {
            assert_eq!(classify(&bad, 20, 30, 10), Err("listingUncertain"));
        }
        assert_eq!(
            classify(&vec![b'x'; OUTPUT_LIMIT as usize + 1], 20, 30, 10),
            Err("listingLimit")
        );
        assert_eq!(
            classify(&record(11, 20, 30).repeat(2049), 20, 30, 10),
            Err("listingLimit")
        );
    }
    #[test]
    fn metadata_only_scope_and_changed_path_checks_remain_non_executable() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        for ext in ["DMG", "iso", "pkg", "mpkg", "zip", "xip"] {
            let path = root.path().join(format!("fixture.{ext}"));
            fs::write(&path, b"metadata only").unwrap();
            let r = Prepared::open(root.path(), &path)
                .unwrap()
                .finish(Ok(vec![]))
                .unwrap();
            assert_eq!(r.state, "unknown");
            assert_eq!(r.reason, Some("limitedView"));
            assert!(!r.can_execute);
        }
        let path = root.path().join("fixture.zip");
        let prepared = Prepared::open(root.path(), &path).unwrap();
        fs::write(&path, b"changed").unwrap();
        assert!(prepared.finish(Ok(vec![])).is_err());
        let alias = root.path().join("alias.zip");
        symlink(&path, &alias).unwrap();
        assert!(Prepared::open(root.path(), &alias).is_err());
        fs::remove_file(&alias).unwrap();
        fs::hard_link(&path, &alias).unwrap();
        assert!(Prepared::open(root.path(), &path).is_err());
        assert!(Prepared::open(root.path(), &root.path().join("a/b/fixture.zip")).is_err());
        assert!(Prepared::open(root.path(), &root.path().join("fixture.txt")).is_err());
    }
    #[tokio::test]
    async fn real_child_handle_is_visible_but_own_handles_and_query_failure_never_prove_idle() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let path = root.path().join("fixture.zip");
        fs::write(&path, b"fixture, not opened as an archive").unwrap();
        let file = File::open(&path).unwrap();
        let fd = file.as_raw_fd();
        let mut command = tokio::process::Command::new("/bin/sleep");
        command.arg("10").kill_on_drop(true);
        unsafe {
            command.as_std_mut().pre_exec(move || {
                if libc::dup2(fd, 3) < 0 || libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().unwrap();
        let report = inspect(Prepared::open(root.path(), &path).unwrap()).await;
        child.kill().await.unwrap();
        child.wait().await.unwrap();
        let r = report.unwrap();
        assert_eq!(r.state, "busy");
        assert_eq!(r.reason, None);
        assert!(!r.can_execute);
        let r = inspect(Prepared::open(root.path(), &path).unwrap())
            .await
            .unwrap();
        assert_eq!(r.state, "unknown");
        assert_eq!(r.reason, Some("limitedView"));
        let r = Prepared::open(root.path(), &path)
            .unwrap()
            .finish(Err("fixture failure".into()))
            .unwrap();
        assert_eq!(r.state, "unknown");
        assert_eq!(r.reason, Some("queryUnavailable"));
    }
}
