//! Bounded, explicit read of pinned Mole's saved roots. Never discovers or writes roots.
use crate::{directory_probe, engine, observation::AnchoredPath};
use serde::Serialize;
use std::{
    ffi::CString,
    fs::File,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::Path,
};
const MAX_BYTES: u64 = 64 * 1024;
const MAX_PATHS: usize = 64;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    protocol: u32,
    revision: &'static str,
    source: String,
    checked_at: i64,
    state: &'static str,
    paths: Vec<String>,
    can_execute: bool,
}
fn valid_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 4096
        && path != "/"
        && !path.chars().any(char::is_control)
        && !path[1..].split('/').any(|s| ["", ".", ".."].contains(&s))
}
fn parse(bytes: &[u8], home: &str) -> Result<Vec<String>, String> {
    if bytes.len() as u64 > MAX_BYTES || !valid_path(home) {
        return Err("Mole 目录配置超过读取范围，未发布目录。".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "Mole 目录配置不是有效 UTF-8。")?;
    if text.lines().count() > 256 || (!text.is_empty() && !text.ends_with('\n')) {
        return Err("Mole 目录配置行数过多或末行不完整，未发布目录。".into());
    }
    let mut paths = vec![];
    for line in text.split_terminator('\n') {
        let line = line.trim_matches(|c: char| c.is_ascii_whitespace());
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let expanded = if line == "~" {
            home.to_owned()
        } else if let Some(suffix) = line.strip_prefix("~/") {
            format!("{home}/{suffix}")
        } else {
            line.to_owned()
        };
        let path = expanded.trim_end_matches('/');
        if !valid_path(path) {
            return Err(
                "Mole 目录配置含不支持的路径；请使用选择目录，不会运行配置中的命令。".into(),
            );
        }
        if !paths.iter().any(|p| p == path) {
            paths.push(path.to_owned());
        }
        if paths.len() > MAX_PATHS {
            return Err("Mole 目录配置超过 64 处，未发布部分列表。".into());
        }
    }
    Ok(paths)
}
pub fn read(home: &Path) -> Result<Report, String> {
    read_with(home, || {})
}
fn read_with(home: &Path, after_read: impl FnOnce()) -> Result<Report, String> {
    let home_text = home
        .to_str()
        .filter(|p| valid_path(p))
        .ok_or("个人目录无法确认。")?;
    let source = home.join(".config/mole/purge_paths");
    let report = |state, paths| Report {
        protocol: 1,
        revision: engine::REVISION,
        source: source.to_string_lossy().into_owned(),
        checked_at: crate::db::now(),
        state,
        paths,
        can_execute: false,
    };
    let mut path = home.to_path_buf();
    let mut held = AnchoredPath::open(home)?;
    for part in [".config", "mole", "purge_paths"] {
        if !held.local_materialized() {
            return Err("配置路径不是已下载的本机目录，未读取。".into());
        }
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                held.validate()?;
                return Ok(report("missing", vec![]));
            }
            Err(_) => return Err("Mole 目录配置无法访问，请使用选择目录。".into()),
            Ok(_) => {
                held = AnchoredPath::open(&path)?;
            }
        }
    }
    let before = held
        .target()
        .metadata()
        .map_err(|_| "配置元数据无法读取。")?;
    if !before.is_file()
        || before.nlink() != 1
        || !held.local_materialized()
        || before.len() > MAX_BYTES
    {
        return Err("只读取本机普通单链接、64 KiB 内的 Mole 目录配置。".into());
    }
    let name = CString::new("purge_paths").unwrap();
    let fd = unsafe {
        libc::openat(
            held.parent().as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err("Mole 目录配置无法只读打开。".into());
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let same = |file: &File| {
        file.metadata().is_ok_and(|m| {
            m.is_file()
                && m.dev() == before.dev()
                && m.ino() == before.ino()
                && m.len() == before.len()
                && m.nlink() == 1
                && m.mode() == before.mode()
                && m.mtime() == before.mtime()
                && m.mtime_nsec() == before.mtime_nsec()
                && m.ctime() == before.ctime()
                && m.ctime_nsec() == before.ctime_nsec()
        })
    };
    if !same(&file) || !directory_probe::local(&file) {
        return Err("配置在打开期间变化，未发布目录。".into());
    }
    let mut bytes = vec![];
    (&mut file)
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Mole 目录配置读取失败。")?;
    after_read();
    if !same(&file) {
        return Err("配置在读取期间变化，请重新读取。".into());
    }
    held.validate()?;
    Ok(report("ready", parse(&bytes, home_text)?))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};
    #[test]
    fn pinned_lines_expand_home_without_shell_or_target_access() {
        assert_eq!(
            parse(
                b"# title\n  ~/Code/ \r\n/Volumes/Work/Projects\n~/Code\n\n",
                "/Users/test"
            )
            .unwrap(),
            vec!["/Users/test/Code", "/Volumes/Work/Projects"]
        );
        assert_eq!(
            parse(b"/does-not-exist/name # literal\n", "/Users/test").unwrap(),
            vec!["/does-not-exist/name # literal"]
        );
        for input in [
            b"relative\n".as_slice(),
            b"$HOME/code\n",
            b"~other/code\n",
            b"/\n",
            b"/tmp/../work\n",
            b"/tmp//work\n",
            b"/tmp/no-final-newline",
            b"/tmp/a\0\n",
            b"\xff\n",
        ] {
            assert!(parse(input, "/Users/test").is_err(), "{input:?}");
        }
        assert!(parse(&vec![b'x'; MAX_BYTES as usize + 1], "/Users/test").is_err());
        assert!(parse("\n".repeat(257).as_bytes(), "/Users/test").is_err());
        assert!(parse(
            (0..65)
                .map(|n| format!("/tmp/root{n}\n"))
                .collect::<String>()
                .as_bytes(),
            "/Users/test"
        )
        .is_err());
    }
    #[test]
    fn actual_file_is_read_only_and_changes_links_and_special_inputs_are_rejected() {
        let t = tempfile::tempdir().unwrap();
        let home = fs::canonicalize(t.path()).unwrap();
        assert_eq!(read(&home).unwrap().state, "missing");
        let parent = home.join(".config/mole");
        fs::create_dir_all(&parent).unwrap();
        let file = parent.join("purge_paths");
        fs::write(&file, "~/not-a-real-directory\n").unwrap();
        let original = fs::read(&file).unwrap();
        let report = read(&home).unwrap();
        assert_eq!(
            report.paths,
            vec![home.join("not-a-real-directory").to_string_lossy()]
        );
        assert_eq!(fs::read(&file).unwrap(), original);
        assert!(read_with(&home, || fs::write(&file, "/changed/path\n").unwrap()).is_err());
        fs::hard_link(&file, parent.join("alias")).unwrap();
        assert!(read(&home).is_err());
        fs::remove_file(parent.join("alias")).unwrap();
        fs::remove_file(&file).unwrap();
        symlink(home.join("missing"), &file).unwrap();
        assert!(read(&home).is_err());
        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        assert!(read(&home).is_err());
        fs::remove_dir(&file).unwrap();
        let f = File::create(&file).unwrap();
        f.set_len(MAX_BYTES + 1).unwrap();
        assert!(read(&home).is_err());
        fs::remove_file(&file).unwrap();
        fs::remove_dir(&parent).unwrap();
        symlink(&home, &parent).unwrap();
        assert!(read(&home).is_err());
    }
}
