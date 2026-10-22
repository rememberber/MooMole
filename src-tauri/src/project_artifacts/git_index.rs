//! A deliberately limited, read-only Git index observer. Never invokes Git/config/hooks.
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
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
#[link(name = "System")]
unsafe extern "C" {
    fn CC_SHA1(data: *const libc::c_void, len: libc::c_uint, digest: *mut u8) -> *mut u8;
}
fn sha1(bytes: &[u8]) -> Result<[u8; 20], &'static str> {
    let mut digest = [0u8; 20];
    if bytes.len() > MAX_BYTES
        || unsafe {
            CC_SHA1(
                bytes.as_ptr().cast(),
                bytes.len() as u32,
                digest.as_mut_ptr(),
            )
        }
        .is_null()
    {
        return Err("invalidIndex");
    }
    Ok(digest)
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    protocol: u32,
    revision: &'static str,
    root: String,
    path: String,
    checked_at: i64,
    state: &'static str,
    reason: Option<&'static str>,
    repository: Option<String>,
    records: Option<usize>,
    example: Option<String>,
    can_execute: bool,
}
struct Matches {
    count: usize,
    example: String,
}
fn word(bytes: &[u8], pos: usize) -> Result<u32, &'static str> {
    Ok(u32::from_be_bytes(
        bytes
            .get(pos..pos + 4)
            .ok_or("invalidIndex")?
            .try_into()
            .unwrap(),
    ))
}
fn relative_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 4096
        && !name.chars().any(char::is_control)
        && !name
            .split('/')
            .any(|s| s.is_empty() || [".", "..", ".git"].contains(&s))
}
fn under(name: &str, relative: &str) -> bool {
    relative.is_empty()
        || name.eq_ignore_ascii_case(relative)
        || name
            .get(..relative.len())
            .is_some_and(|s| s.eq_ignore_ascii_case(relative))
            && name.as_bytes().get(relative.len()) == Some(&b'/')
}
// Positive records are published only after ALL entries/extensions and the checksum
// have been consumed. Required extensions (split/sparse) are not interpreted.
fn parse(bytes: &[u8], relative: &str) -> Result<Option<Matches>, &'static str> {
    if bytes.len() > MAX_BYTES {
        return Err("sizeLimit");
    }
    if bytes.len() < 32 || bytes.get(..4) != Some(b"DIRC") {
        return Err("invalidIndex");
    }
    let version = word(bytes, 4)?;
    if ![2, 3].contains(&version) {
        return Err("unsupportedIndex");
    }
    let end = bytes.len() - 20;
    if sha1(&bytes[..end])?.as_slice() != &bytes[end..] {
        return Err("invalidIndex");
    }
    let count = word(bytes, 8)? as usize;
    if count > MAX_ENTRIES {
        return Err("entryLimit");
    }
    let mut pos = 12;
    let mut previous: Option<(&str, u16)> = None;
    let mut found: Option<Matches> = None;
    for _ in 0..count {
        let start = pos;
        if pos + 62 > end {
            return Err("invalidIndex");
        }
        let mode = word(bytes, pos + 24)?;
        if ![0o100644, 0o100755, 0o120000, 0o160000].contains(&mode) {
            return Err("unsupportedIndex");
        }
        let flags = u16::from_be_bytes(bytes[pos + 60..pos + 62].try_into().unwrap());
        pos += 62;
        if flags & 0x4000 != 0 {
            if version != 3 || pos + 2 > end {
                return Err("invalidIndex");
            }
            let extended = u16::from_be_bytes(bytes[pos..pos + 2].try_into().unwrap());
            if extended & !0x6000 != 0 {
                return Err("unsupportedIndex");
            }
            pos += 2;
        }
        let length = bytes[pos..end]
            .iter()
            .position(|b| *b == 0)
            .ok_or("invalidIndex")?;
        let name =
            std::str::from_utf8(&bytes[pos..pos + length]).map_err(|_| "unsupportedIndex")?;
        if !relative_name(name) || (flags & 0x0fff) as usize != length.min(0x0fff) {
            return Err("invalidIndex");
        }
        let stage = (flags >> 12) & 3;
        if previous.is_some_and(|old| old >= (name, stage)) {
            return Err("invalidIndex");
        }
        previous = Some((name, stage));
        pos += length + 1;
        let padded = start + ((pos - start + 7) & !7);
        if padded > end || bytes[pos..padded].iter().any(|b| *b != 0) {
            return Err("invalidIndex");
        }
        pos = padded;
        if under(name, relative) {
            match &mut found {
                Some(m) => m.count += 1,
                None => {
                    found = Some(Matches {
                        count: 1,
                        example: name.into(),
                    })
                }
            }
        }
    }
    let mut extensions = 0;
    while pos < end {
        if pos + 8 > end {
            return Err("invalidIndex");
        }
        if !bytes[pos].is_ascii_uppercase() {
            return Err("unsupportedIndex");
        }
        let size = word(bytes, pos + 4)? as usize;
        pos = pos
            .checked_add(8)
            .and_then(|p| p.checked_add(size))
            .filter(|p| *p <= end)
            .ok_or("invalidIndex")?;
        extensions += 1;
        if extensions > 64 {
            return Err("unsupportedIndex");
        }
    }
    Ok(found)
}
pub fn inspect(root: &Path, path: &Path) -> Result<Report, String> {
    let (scope, target) = super::scope::open(root, path)?;
    let result = read(root, path);
    scope
        .validate()
        .and_then(|_| target.validate())
        .map_err(|_| "Git 检查期间目录或路径已变化，本次观察已撤回。")?;
    let mut report = Report {
        protocol: 1,
        revision: engine::REVISION,
        root: root.to_str().unwrap().into(),
        path: path.to_str().unwrap().into(),
        checked_at: crate::db::now(),
        state: "unknown",
        reason: None,
        repository: None,
        records: None,
        example: None,
        can_execute: false,
    };
    match result {
        Ok((repository, Some(found))) => {
            report.state = "recorded";
            report.repository = Some(repository);
            report.records = Some(found.count);
            report.example = Some(found.example);
        }
        Ok((_, None)) => report.reason = Some("notListed"),
        Err(reason) => report.reason = Some(reason),
    }
    Ok(report)
}
fn read(root: &Path, path: &Path) -> Result<(String, Option<Matches>), &'static str> {
    let mut repository = path;
    loop {
        match std::fs::symlink_metadata(repository.join(".git")) {
            Ok(m) if !m.is_dir() => return Err("unsupportedRepository"),
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("unavailable"),
        }
        if repository == root {
            return Err("noRepository");
        }
        repository = repository
            .parent()
            .filter(|p| p.starts_with(root))
            .ok_or("noRepository")?;
    }
    let repository_held = AnchoredPath::open(repository).map_err(|_| "unavailable")?;
    let git = AnchoredPath::open(&repository.join(".git")).map_err(|_| "unsupportedRepository")?;
    if !git.local_materialized() {
        return Err("unavailable");
    }
    let index =
        AnchoredPath::open(&repository.join(".git/index")).map_err(|_| "indexUnavailable")?;
    let before = index.target().metadata().map_err(|_| "indexUnavailable")?;
    if !before.is_file() || before.nlink() != 1 || !index.local_materialized() {
        return Err("indexUnavailable");
    }
    if before.len() > MAX_BYTES as u64 {
        return Err("sizeLimit");
    }
    let name = CString::new("index").unwrap();
    let fd = unsafe {
        libc::openat(
            index.parent().as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err("indexUnavailable");
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let same = || {
        file.metadata().is_ok_and(|m| {
            m.dev() == before.dev()
                && m.ino() == before.ino()
                && m.size() == before.size()
                && m.mtime() == before.mtime()
                && m.mtime_nsec() == before.mtime_nsec()
                && m.ctime() == before.ctime()
                && m.ctime_nsec() == before.ctime_nsec()
                && m.is_file()
                && m.nlink() == 1
        })
    };
    if !same() || !directory_probe::local(&file) {
        return Err("indexUnavailable");
    }
    let mut bytes = vec![];
    (&mut file)
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "indexUnavailable")?;
    let matches = parse(
        &bytes,
        path.strip_prefix(repository).unwrap().to_str().unwrap(),
    )?;
    // No source configuration, objects, worktree pointer, hook or file payload is read.
    index
        .validate()
        .and_then(|_| git.validate())
        .and_then(|_| repository_held.validate())
        .map_err(|_| "unavailable")?;
    let mut nearer = path;
    while nearer != repository {
        match std::fs::symlink_metadata(nearer.join(".git")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err("unavailable"),
        }
        nearer = nearer.parent().ok_or("unavailable")?;
    }
    Ok((repository.to_str().unwrap().into(), matches))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{symlink, PermissionsExt},
        process::Command,
    };
    fn fixture(paths: &[&str], version: u32, extensions: &[u8]) -> Vec<u8> {
        let mut bytes = b"DIRC".to_vec();
        bytes.extend_from_slice(&version.to_be_bytes());
        bytes.extend_from_slice(&(paths.len() as u32).to_be_bytes());
        for path in paths {
            let start = bytes.len();
            bytes.resize(start + 40, 0);
            bytes[start + 24..start + 28].copy_from_slice(&0o100644u32.to_be_bytes());
            bytes.extend_from_slice(&[1; 20]);
            let flags = (path.len().min(0xfff) as u16) | if version == 3 { 0x4000 } else { 0 };
            bytes.extend_from_slice(&flags.to_be_bytes());
            if version == 3 {
                bytes.extend_from_slice(&0x2000u16.to_be_bytes());
            }
            bytes.extend_from_slice(path.as_bytes());
            bytes.push(0);
            while (bytes.len() - start) % 8 != 0 {
                bytes.push(0);
            }
        }
        bytes.extend_from_slice(extensions);
        bytes.extend_from_slice(&sha1(&bytes).unwrap());
        bytes
    }
    #[test]
    fn validates_complete_index_before_publishing_literal_case_folded_records() {
        assert_eq!(
            sha1(b"abc").unwrap(),
            [
                0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50,
                0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d
            ]
        );
        for version in [2, 3] {
            let bytes = fixture(
                &["Target[x]/a.rs", "target-other/x", "target[x]/b.rs"],
                version,
                b"TREE\0\0\0\0",
            );
            let result = parse(&bytes, "target[x]").unwrap().unwrap();
            assert_eq!(result.count, 2);
            assert_eq!(result.example, "Target[x]/a.rs");
            assert!(parse(&bytes, "target").unwrap().is_none());
            assert_eq!(
                parse(&fixture(&["target"], version, b""), "TARGET")
                    .unwrap()
                    .unwrap()
                    .count,
                1
            );
            let mut corrupt = bytes.clone();
            corrupt[20] ^= 1;
            assert_eq!(parse(&corrupt, "target[x]").err(), Some("invalidIndex"));
        }
        for extension in [
            b"link\0\0\0\0".as_slice(),
            b"sdir\0\0\0\0",
            b"TREE\0\0\0\x05oops",
            b"BAD",
        ] {
            assert!(parse(&fixture(&["target/a"], 2, extension), "target").is_err());
        }
        for names in [
            vec!["target/b", "target/a"],
            vec!["target/a", "target/a"],
            vec!["target/../a"],
            vec!["target/.git/x"],
        ] {
            assert!(parse(&fixture(&names, 2, b""), "target").is_err());
        }
        assert_eq!(
            parse(&fixture(&[], 4, b""), "target").err(),
            Some("unsupportedIndex")
        );
        let mut count = fixture(&[], 2, b"");
        count[8..12].copy_from_slice(&20_001u32.to_be_bytes());
        let last = count.len() - 20;
        let digest = sha1(&count[..last]).unwrap();
        count[last..].copy_from_slice(&digest);
        assert_eq!(parse(&count, "target").err(), Some("entryLimit"));
        assert_eq!(
            parse(&vec![0; MAX_BYTES + 1], "target").err(),
            Some("sizeLimit")
        );
        for end in 0..fixture(&["target/a"], 2, b"").len() {
            assert!(parse(&fixture(&["target/a"], 2, b"")[..end], "target").is_err());
        }
    }
    #[test]
    fn real_git_index_is_read_without_git_hooks_config_or_source_contents() {
        let t = tempfile::tempdir_in("/private/tmp").unwrap();
        let root = t.path();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("target/tracked.rs"), "fixture source").unwrap();
        let git = |args: &[&str]| {
            let result = Command::new("/usr/bin/git")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", root)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .arg("-C")
                .arg(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        };
        git(&["init", "-q", "--template="]);
        git(&["add", "--", "target/tracked.rs"]);
        let index = root.join(".git/index");
        let original = fs::read(&index).unwrap();
        let sentinel = root.join("hook-ran");
        let hook = root.join(".git/test-hook");
        fs::write(
            &hook,
            format!("#!/bin/sh\n/usr/bin/touch {}\n", sentinel.display()),
        )
        .unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            root.join(".git/config"),
            format!(
                "[core]\nrepositoryformatversion = 0\nfsmonitor = {}\n",
                hook.display()
            ),
        )
        .unwrap();
        let result = inspect(root, &root.join("target")).unwrap();
        assert_eq!(result.state, "recorded");
        assert_eq!(result.records, Some(1));
        assert_eq!(result.example.as_deref(), Some("target/tracked.rs"));
        assert!(!result.can_execute);
        assert_eq!(fs::read(&index).unwrap(), original);
        assert!(!sentinel.exists());
        fs::remove_file(root.join("target/tracked.rs")).unwrap();
        assert_eq!(
            inspect(root, &root.join("target")).unwrap().state,
            "recorded"
        );
        fs::create_dir_all(root.join("below/target")).unwrap();
        assert_eq!(
            inspect(&root.join("below"), &root.join("below/target"))
                .unwrap()
                .reason,
            Some("noRepository")
        );
        fs::write(&index, fixture(&["outside/file"], 2, b"")).unwrap();
        assert_eq!(
            inspect(root, &root.join("target")).unwrap().reason,
            Some("notListed")
        );
        fs::remove_file(&index).unwrap();
        symlink(root.join("missing"), &index).unwrap();
        assert_eq!(
            inspect(root, &root.join("target")).unwrap().reason,
            Some("indexUnavailable")
        );
    }
    #[test]
    fn repository_links_worktree_pointers_and_shared_indexes_are_unknown() {
        let t = tempfile::tempdir_in("/private/tmp").unwrap();
        let root = t.path();
        fs::create_dir(root.join("target")).unwrap();
        fs::write(root.join(".git"), "gitdir: /outside\n").unwrap();
        assert_eq!(
            inspect(root, &root.join("target")).unwrap().reason,
            Some("unsupportedRepository")
        );
        fs::remove_file(root.join(".git")).unwrap();
        symlink(root.join("outside"), root.join(".git")).unwrap();
        assert_eq!(
            inspect(root, &root.join("target")).unwrap().reason,
            Some("unsupportedRepository")
        );
        fs::remove_file(root.join(".git")).unwrap();
        fs::create_dir(root.join(".git")).unwrap();
        fs::write(root.join(".git/index"), fixture(&["target/a"], 2, b"")).unwrap();
        fs::hard_link(root.join(".git/index"), root.join("alias")).unwrap();
        assert_eq!(
            inspect(root, &root.join("target")).unwrap().reason,
            Some("indexUnavailable")
        );
    }
}
