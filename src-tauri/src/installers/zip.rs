//! On-demand, complete bounded ZipInfo listing; names are hints, never cleanup authority.
use crate::{
    engine::{self, Job, TaskKind},
    observation::AnchoredPath,
    process,
};
use serde::Serialize;
use std::{
    ffi::CString,
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{fs::MetadataExt, process::CommandExt},
    },
    path::Path,
    sync::Arc,
    time::Duration,
};
const OUTPUT_LIMIT: u64 = 256 * 1024;
const WINDOW: usize = 50;
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
    total_entries: Option<usize>,
    checked_entries: usize,
    matched_types: Vec<String>,
    can_execute: bool,
}
pub struct Prepared {
    scope: AnchoredPath,
    held: AnchoredPath,
    file: File,
    root: String,
    path: String,
}
impl Prepared {
    pub fn open(root: &Path, path: &Path) -> Result<Self, String> {
        let valid = |p: &Path| {
            p.to_str().is_some_and(|s| {
                s.len() <= 4096
                    && s.starts_with('/')
                    && !s.ends_with('/')
                    && !s.chars().any(char::is_control)
                    && !s[1..].split('/').any(|v| ["", ".", ".."].contains(&v))
            })
        };
        if !valid(root) || !valid(path) {
            return Err("请选择有效的扫描范围与 ZIP 文件。".into());
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "ZIP 不在所选范围内。")?;
        if relative.components().count() == 0
            || relative.components().count() > 2
            || !path
                .extension()
                .is_some_and(|v| v.eq_ignore_ascii_case("zip"))
        {
            return Err("只能检查所选范围两层内的 ZIP 文件。".into());
        }
        let scope = AnchoredPath::open(root)?;
        let held = AnchoredPath::open(path)?;
        let before = held
            .target()
            .metadata()
            .map_err(|_| "ZIP 元数据无法读取。")?;
        if !scope
            .target()
            .metadata()
            .is_ok_and(|m| m.is_dir() && m.dev() == before.dev())
            || !before.is_file()
            || !scope.local_materialized()
            || !held.local_materialized()
        {
            return Err("只能读取已下载到本机的普通 ZIP 文件。".into());
        }
        let name = CString::new(path.file_name().unwrap().as_encoded_bytes())
            .map_err(|_| "文件名称无效。")?;
        let fd = unsafe {
            libc::openat(
                held.parent().as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err("ZIP 无法只读打开，请检查访问权限。".into());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let after = file.metadata().map_err(|_| "ZIP 元数据无法读取。")?;
        if !crate::directory_probe::local(&file)
            || !after.is_file()
            || before.dev() != after.dev()
            || before.ino() != after.ino()
            || before.size() != after.size()
            || before.mtime() != after.mtime()
            || before.mtime_nsec() != after.mtime_nsec()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
            || before.nlink() != after.nlink()
        {
            return Err("打开期间 ZIP 已变化，请重新扫描。".into());
        }
        held.validate()?;
        Ok(Self {
            scope,
            held,
            file,
            root: root.to_str().unwrap().into(),
            path: path.to_str().unwrap().into(),
        })
    }
    fn finish(self, result: Result<Vec<u8>, String>) -> Result<Inspection, String> {
        self.scope
            .validate()
            .and_then(|_| self.held.validate())
            .map_err(|_| "检查期间 ZIP 或路径已变化；本次线索已撤回，请重新扫描。")?;
        let mut report = Inspection {
            protocol: 1,
            revision: engine::REVISION,
            root: self.root,
            path: self.path,
            checked_at: crate::db::now(),
            state: "unknown",
            reason: Some("listingUnavailable"),
            total_entries: None,
            checked_entries: 0,
            matched_types: vec![],
            can_execute: false,
        };
        if let Ok(bytes) = result {
            match classify(&bytes) {
                Ok((total, types)) => {
                    report.total_entries = Some(total);
                    report.checked_entries = total.min(WINDOW);
                    report.matched_types = types;
                    report.state = if report.matched_types.is_empty() {
                        "noHintInWindow"
                    } else {
                        "installerHint"
                    };
                    report.reason = None;
                }
                Err(reason) => report.reason = Some(reason),
            }
        }
        Ok(report)
    }
}
fn classify(bytes: &[u8]) -> Result<(usize, Vec<String>), &'static str> {
    if bytes.len() as u64 > OUTPUT_LIMIT {
        return Err("listingLimit");
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "listingUncertain")?;
    // ZipInfo's textual listing is not a structured archive index. Unusual
    // control/escape renderings remain unknown rather than granting a hint.
    if !text.ends_with('\n')
        || text.chars().any(|c| c.is_control() && c != '\n')
        || text.contains('^')
        || text.contains('\\')
    {
        return Err("listingUncertain");
    }
    let lines = text
        .strip_suffix('\n')
        .unwrap()
        .split('\n')
        .collect::<Vec<_>>();
    if lines.iter().any(|n| n.is_empty()) || lines.len() > 10_000 {
        return Err("listingLimit");
    }
    let mut matched = vec![];
    for kind in ["app", "pkg", "dmg", "xip"] {
        let suffix = format!(".{kind}");
        if lines
            .iter()
            .take(WINDOW)
            .any(|line| line.split('/').any(|part| part.ends_with(&suffix)))
        {
            matched.push(kind.into());
        }
    }
    Ok((lines.len(), matched))
}
pub async fn inspect(prepared: Prepared) -> Result<Inspection, String> {
    let fd = prepared.file.as_raw_fd();
    let mut command = tokio::process::Command::new("/usr/bin/zipinfo");
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .args(["-1", "/dev/fd/3"]);
    // Only async-signal-safe syscalls run after fork; the parent retains the
    // opened ZIP until the supervised child and all pipes have finished.
    unsafe {
        command.as_std_mut().pre_exec(move || {
            if libc::dup2(fd, 3) < 0 || libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let cpu = libc::rlimit {
                rlim_cur: 2,
                rlim_max: 2,
            };
            if libc::setrlimit(libc::RLIMIT_CPU, &cpu) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let result = process::output_bounded(
        command,
        Arc::new(Job::new(TaskKind::Analyze)),
        Duration::from_secs(2),
        OUTPUT_LIMIT,
    )
    .await;
    tauri::async_runtime::spawn_blocking(move || prepared.finish(result))
        .await
        .map_err(|_| "ZIP 检查未完成。".to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write, os::unix::fs::symlink};
    #[test]
    fn complete_listing_uses_the_original_case_sensitive_first_fifty_window() {
        assert_eq!(
            classify(b"notes.txt\nTool.app/Contents/Info.plist\nSetup.pkg\nDisk.dmg\nXcode.xip\n")
                .unwrap(),
            (
                5,
                vec!["app".into(), "pkg".into(), "dmg".into(), "xip".into()]
            )
        );
        let fifty = (0..50).map(|i| format!("{i}.txt\n")).collect::<String>();
        assert!(classify(format!("{fifty}Late.app\n").as_bytes())
            .unwrap()
            .1
            .is_empty());
        assert_eq!(
            classify(format!("{}Early.app\n{}", "x\n".repeat(49), "Later.txt\n").as_bytes())
                .unwrap()
                .1,
            ["app"]
        );
        assert!(classify(b"Tool.APP\nfile.app.txt\n").unwrap().1.is_empty());
        for invalid in [b"Tool.app".as_slice(), b"x\t.app\n", b"Tool^J.app\n", b"\n"] {
            assert!(classify(invalid).is_err());
        }
        assert!(classify(&vec![b'x'; OUTPUT_LIMIT as usize + 1]).is_err());
    }
    fn fixture_zip(path: &Path, names: &[&str]) {
        // Fixed uncompressed minimal ZIP fixture: no external writer dependency.
        let mut f = File::create(path).unwrap();
        let mut offsets = vec![];
        let mut offset = 0u32;
        for name in names {
            offsets.push(offset);
            for v in [0x04034b50u32, 20, 0, 0, 0, 0] {
                f.write_all(&v.to_le_bytes()).unwrap();
            }
            f.write_all(&0u16.to_le_bytes()).unwrap();
            f.write_all(&(name.len() as u16).to_le_bytes()).unwrap();
            f.write_all(&0u16.to_le_bytes()).unwrap();
            f.write_all(name.as_bytes()).unwrap();
            offset += 30 + name.len() as u32;
        }
        let start = offset;
        for (name, local) in names.iter().zip(offsets) {
            f.write_all(&0x02014b50u32.to_le_bytes()).unwrap();
            f.write_all(&[20, 0, 20, 0]).unwrap();
            f.write_all(&[0; 20]).unwrap();
            f.write_all(&(name.len() as u16).to_le_bytes()).unwrap();
            f.write_all(&[0; 12]).unwrap();
            f.write_all(&local.to_le_bytes()).unwrap();
            f.write_all(name.as_bytes()).unwrap();
            offset += 46 + name.len() as u32;
        }
        f.write_all(&0x06054b50u32.to_le_bytes()).unwrap();
        f.write_all(&[0; 4]).unwrap();
        f.write_all(&(names.len() as u16).to_le_bytes()).unwrap();
        f.write_all(&(names.len() as u16).to_le_bytes()).unwrap();
        f.write_all(&(offset - start).to_le_bytes()).unwrap();
        f.write_all(&start.to_le_bytes()).unwrap();
        f.write_all(&[0; 2]).unwrap();
    }
    #[tokio::test]
    async fn actual_system_listing_uses_held_file_and_keeps_corruption_unknown() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let path = root.path().join("sample.zip");
        fixture_zip(&path, &["Tool.app/Contents/Info.plist", "notes.txt"]);
        let report = inspect(Prepared::open(root.path(), &path).unwrap())
            .await
            .unwrap();
        assert_eq!(report.state, "installerHint");
        assert_eq!(report.total_entries, Some(2));
        assert_eq!(report.matched_types, ["app"]);
        assert!(!report.can_execute);
        fixture_zip(&path, &["ordinary.txt"]);
        assert_eq!(
            inspect(Prepared::open(root.path(), &path).unwrap())
                .await
                .unwrap()
                .state,
            "noHintInWindow"
        );
        fixture_zip(&path, &["notes\nFake.app/"]);
        assert_eq!(
            inspect(Prepared::open(root.path(), &path).unwrap())
                .await
                .unwrap()
                .state,
            "unknown"
        );
        fs::write(&path, "broken zip").unwrap();
        assert_eq!(
            inspect(Prepared::open(root.path(), &path).unwrap())
                .await
                .unwrap()
                .state,
            "unknown"
        );
    }
    #[tokio::test]
    async fn actual_limit_never_publishes_a_positive_prefix() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let path = root.path().join("large.zip");
        let mut names = vec!["First.app".to_string()];
        names.extend((0..10000).map(|i| format!("{i}.txt")));
        fixture_zip(&path, &names.iter().map(String::as_str).collect::<Vec<_>>());
        let report = inspect(Prepared::open(root.path(), &path).unwrap())
            .await
            .unwrap();
        assert_eq!(report.state, "unknown");
        assert_eq!(report.reason, Some("listingLimit"));
        assert!(report.matched_types.is_empty());
        names = vec!["First.app".to_string()];
        names.extend((0..4000).map(|i| format!("{i}-{}", "x".repeat(100))));
        fixture_zip(&path, &names.iter().map(String::as_str).collect::<Vec<_>>());
        let report = inspect(Prepared::open(root.path(), &path).unwrap())
            .await
            .unwrap();
        assert_eq!(report.state, "unknown");
        assert!(report.matched_types.is_empty());
        assert_eq!(report.total_entries, None);
    }
    #[tokio::test]
    async fn replacement_and_scope_aliases_never_publish_stale_hints() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        let path = root.path().join("sample.zip");
        fixture_zip(&path, &["Tool.app"]);
        let prepared = Prepared::open(root.path(), &path).unwrap();
        fs::rename(&path, root.path().join("old.zip")).unwrap();
        symlink(root.path().join("old.zip"), &path).unwrap();
        assert!(inspect(prepared).await.is_err());
        assert!(Prepared::open(root.path(), &path).is_err());
        assert!(Prepared::open(Path::new("/"), &path).is_err());
        assert!(Prepared::open(root.path(), &root.path().join("a/b/sample.zip")).is_err());
    }
}
