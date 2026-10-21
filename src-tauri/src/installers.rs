//! Explicit-scope metadata inventory, not an invocation of Mole's installer lifecycle.
use crate::{
    app_protection,
    directory_probe::{local, open_child, stat_at, Stream},
    engine,
    observation::AnchoredPath,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CStr, CString},
    fs::File,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::Path,
    time::{Duration, Instant},
};
pub mod connection;
pub mod usage;
pub mod zip;
const MAX_ENTRIES: usize = 10_000;
const MAX_ITEMS: usize = 256;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    version: String,
    revision: String,
    extensions: Vec<String>,
    max_depth: usize,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    path: String,
    name: String,
    extension: String,
    size_bytes: Option<u64>,
    measurement: &'static str,
    classification: &'static str,
    sharing: &'static str,
    protection: &'static str,
    usage: &'static str,
    can_execute: bool,
}
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Skipped {
    links: usize,
    unreadable: usize,
    non_local: usize,
    depth: usize,
    invalid_name: usize,
    changed: usize,
    unsupported_type: usize,
}
impl Skipped {
    fn any(&self) -> bool {
        self.links
            + self.unreadable
            + self.non_local
            + self.depth
            + self.invalid_name
            + self.changed
            + self.unsupported_type
            > 0
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    protocol: u32,
    revision: &'static str,
    root: String,
    checked_at: i64,
    completion: &'static str,
    stop_reason: Option<&'static str>,
    visited: usize,
    skipped: Skipped,
    items: Vec<Item>,
    can_execute: bool,
}
struct Scanner<'a> {
    reference: Catalog,
    client: &'a [String],
    report: Report,
    began: Instant,
    limit: usize,
    budget: Duration,
    device: u64,
}
fn same(a: &libc::stat, b: &libc::stat) -> bool {
    a.st_dev == b.st_dev
        && a.st_ino == b.st_ino
        && a.st_mode == b.st_mode
        && a.st_nlink == b.st_nlink
        && a.st_size == b.st_size
        && a.st_mtime == b.st_mtime
        && a.st_mtime_nsec == b.st_mtime_nsec
        && a.st_ctime == b.st_ctime
        && a.st_ctime_nsec == b.st_ctime_nsec
        && a.st_flags == b.st_flags
}
impl Scanner<'_> {
    fn ready(&mut self) -> bool {
        if self.report.stop_reason.is_some() {
            return false;
        }
        self.report.stop_reason = if self.began.elapsed() >= self.budget {
            Some("timeLimit")
        } else if self.report.visited >= self.limit {
            Some("entryLimit")
        } else if self.report.items.len() >= MAX_ITEMS {
            Some("candidateLimit")
        } else {
            None
        };
        self.report.stop_reason.is_none()
    }
    fn walk(&mut self, directory: &File, path: &Path, depth: usize) {
        if !self.ready() {
            return;
        }
        let Ok(before) = directory.metadata() else {
            self.report.skipped.unreadable += 1;
            return;
        };
        let dot = CString::new(".").unwrap();
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                dot.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            self.report.skipped.unreadable += 1;
            return;
        }
        let pointer = unsafe { libc::fdopendir(fd) };
        if pointer.is_null() {
            unsafe {
                libc::close(fd);
            }
            self.report.skipped.unreadable += 1;
            return;
        }
        let stream = Stream(pointer);
        loop {
            if !self.ready() {
                break;
            }
            unsafe {
                *libc::__error() = 0;
            }
            let raw = unsafe { libc::readdir(stream.0) };
            if raw.is_null() {
                if unsafe { *libc::__error() } != 0 {
                    self.report.skipped.unreadable += 1;
                }
                break;
            }
            let name = unsafe { CStr::from_ptr((*raw).d_name.as_ptr()) }.to_owned();
            if [b".".as_slice(), b"..".as_slice()].contains(&name.to_bytes()) {
                continue;
            }
            self.report.visited += 1;
            let Ok(text) = name.to_str() else {
                self.report.skipped.invalid_name += 1;
                continue;
            };
            let child_path = path.join(text);
            if text.chars().any(char::is_control) || child_path.as_os_str().len() > 4096 {
                self.report.skipped.invalid_name += 1;
                continue;
            }
            let Ok(stat) = stat_at(directory, &name) else {
                self.report.skipped.unreadable += 1;
                continue;
            };
            let kind = stat.st_mode & libc::S_IFMT;
            if kind == libc::S_IFLNK {
                self.report.skipped.links += 1;
                continue;
            }
            if stat.st_flags & 0x40000000 != 0 || stat.st_dev as u64 != self.device {
                self.report.skipped.non_local += 1;
                continue;
            }
            let extension = child_path
                .extension()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let matching = self.reference.extensions.contains(&extension);
            if kind == libc::S_IFDIR {
                // Bundle-style packages are outside the regular-file contract.
                if matching {
                    self.report.skipped.unsupported_type += 1;
                    continue;
                }
                if depth + 1 >= self.reference.max_depth {
                    self.report.skipped.depth += 1;
                    continue;
                }
                let Ok(child) = open_child(directory, &name, &stat) else {
                    self.report.skipped.unreadable += 1;
                    continue;
                };
                if !local(&child) {
                    self.report.skipped.non_local += 1;
                    continue;
                }
                self.walk(&child, &child_path, depth + 1);
                if !stat_at(directory, &name).is_ok_and(|now| same(&stat, &now)) {
                    self.report.skipped.changed += 1;
                }
            } else if matching && kind == libc::S_IFREG {
                if !stat_at(directory, &name).is_ok_and(|now| same(&stat, &now)) {
                    self.report.skipped.changed += 1;
                    continue;
                }
                let value = child_path.to_str().unwrap();
                let protection = app_protection::check(value, None, None, "", self.client)
                    .map(|v| {
                        if v.reasons.is_empty() {
                            "notMatched"
                        } else {
                            "matched"
                        }
                    })
                    .unwrap_or("unknown");
                let size = u64::try_from(stat.st_size)
                    .ok()
                    .filter(|v| *v <= 9_007_199_254_740_991);
                self.report.items.push(Item {
                    path: value.into(),
                    name: text.into(),
                    extension: extension.clone(),
                    size_bytes: size,
                    measurement: if size.is_some() {
                        "measured"
                    } else {
                        "unavailable"
                    },
                    classification: if extension == "zip" {
                        "archiveUnverified"
                    } else {
                        "nameOnly"
                    },
                    sharing: if stat.st_nlink > 1 {
                        "shared"
                    } else {
                        "single"
                    },
                    protection,
                    usage: "notChecked",
                    can_execute: false,
                });
            } else if matching {
                self.report.skipped.unsupported_type += 1;
            }
        }
        if !directory.metadata().is_ok_and(|after| {
            before.dev() == after.dev()
                && before.ino() == after.ino()
                && before.mtime() == after.mtime()
                && before.mtime_nsec() == after.mtime_nsec()
                && before.ctime() == after.ctime()
                && before.ctime_nsec() == after.ctime_nsec()
        }) {
            self.report.skipped.changed += 1;
        }
    }
}
pub fn scan(root: &Path, client: &[String]) -> Result<Report, String> {
    scan_with(root, client, MAX_ENTRIES, Duration::from_secs(3))
}
fn scan_with(
    root: &Path,
    client: &[String],
    limit: usize,
    budget: Duration,
) -> Result<Report, String> {
    let reference: Catalog =
        serde_json::from_str(include_str!("../../src/lib/installers-catalog.json"))
            .map_err(|_| "安装包来源说明无法读取。")?;
    if reference.revision != engine::REVISION
        || reference.version != "1.59.1"
        || reference.max_depth != 2
        || reference.extensions != ["dmg", "pkg", "mpkg", "iso", "xip", "zip"]
    {
        return Err("安装包来源基线无法确认。".into());
    }
    let text = root
        .to_str()
        .filter(|v| {
            v.starts_with('/')
                && v.len() <= 4096
                && !v.ends_with('/')
                && !v.chars().any(char::is_control)
                && !v[1..].split('/').any(|n| ["", ".", ".."].contains(&n))
        })
        .ok_or("请选择有效的绝对目录。")?;
    let held = AnchoredPath::open(root)?;
    if !held.target().metadata().is_ok_and(|m| m.is_dir()) || !held.local_materialized() {
        return Err("请选择可读取且已下载到本机的目录。".into());
    }
    let mut scanner = Scanner {
        reference,
        client,
        began: Instant::now(),
        limit,
        budget,
        device: held
            .target()
            .metadata()
            .map_err(|_| "目录无法读取。")?
            .dev(),
        report: Report {
            protocol: 1,
            revision: engine::REVISION,
            root: text.into(),
            checked_at: 0,
            completion: "complete",
            stop_reason: None,
            visited: 0,
            skipped: Skipped::default(),
            items: vec![],
            can_execute: false,
        },
    };
    scanner.walk(held.target(), root, 0);
    held.validate()
        .map_err(|_| "扫描期间所选目录或路径已变化，请重新扫描。")?;
    let r = &mut scanner.report;
    r.checked_at = crate::db::now();
    if r.stop_reason.is_some() || r.skipped.any() {
        r.completion = "partial";
    }
    r.items.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(scanner.report)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};
    #[test]
    fn all_extensions_are_only_name_clues_and_zip_content_is_never_assumed() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        for name in [
            "A.DMG",
            "b.pkg",
            "c.mpkg",
            "d.iso",
            "e.xip",
            "f.zip",
            "other.txt",
        ] {
            fs::write(root.path().join(name), "not an installer").unwrap();
        }
        let r = scan(
            root.path(),
            &[root.path().join("b.pkg").to_str().unwrap().into()],
        )
        .unwrap();
        assert_eq!(r.items.len(), 6);
        assert_eq!(r.completion, "complete");
        assert!(r
            .items
            .iter()
            .all(|i| i.size_bytes == Some(16) && !i.can_execute && i.usage == "notChecked"));
        assert_eq!(
            r.items
                .iter()
                .find(|i| i.extension == "zip")
                .unwrap()
                .classification,
            "archiveUnverified"
        );
        assert_eq!(
            r.items
                .iter()
                .find(|i| i.extension == "pkg")
                .unwrap()
                .protection,
            "matched"
        );
        let json = serde_json::to_value(r).unwrap();
        assert_eq!(json["canExecute"], false);
        assert!(json["items"][0].get("sizeBytes").is_some());
    }
    #[test]
    fn includes_hidden_children_but_links_bundles_and_depth_keep_inventory_partial() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        fs::create_dir_all(root.path().join(".hidden/deeper")).unwrap();
        fs::create_dir(root.path().join("bundle.pkg")).unwrap();
        fs::write(root.path().join(".hidden/file.dmg"), "").unwrap();
        fs::write(root.path().join(".hidden/deeper/missed.dmg"), "x").unwrap();
        symlink(root.path(), root.path().join("linked")).unwrap();
        let r = scan(root.path(), &[]).unwrap();
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.items[0].size_bytes, Some(0));
        assert_eq!(r.completion, "partial");
        assert_eq!(
            (r.skipped.depth, r.skipped.links, r.skipped.unsupported_type),
            (1, 1, 1)
        );
    }
    #[test]
    fn shared_files_are_visible_without_a_reclaim_estimate_and_alias_scopes_are_rejected() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        fs::write(root.path().join("source"), "shared").unwrap();
        fs::hard_link(root.path().join("source"), root.path().join("file.pkg")).unwrap();
        assert_eq!(
            scan(root.path(), &["relative".into()]).unwrap().items[0].sharing,
            "shared"
        );
        assert_eq!(
            scan(root.path(), &["relative".into()]).unwrap().items[0].protection,
            "unknown"
        );
        symlink(root.path(), root.path().join("alias")).unwrap();
        assert!(scan(&root.path().join("alias"), &[]).is_err());
        assert!(scan(Path::new("/"), &[]).is_err());
        assert!(scan(Path::new("relative"), &[]).is_err());
    }
    #[test]
    fn time_entry_and_result_limits_never_claim_complete() {
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        for n in 0..257 {
            fs::write(root.path().join(format!("{n}.dmg")), "x").unwrap();
        }
        assert_eq!(
            scan_with(root.path(), &[], 1, Duration::from_secs(3))
                .unwrap()
                .stop_reason,
            Some("entryLimit")
        );
        assert_eq!(
            scan_with(root.path(), &[], MAX_ENTRIES, Duration::ZERO)
                .unwrap()
                .stop_reason,
            Some("timeLimit")
        );
        let r = scan(root.path(), &[]).unwrap();
        assert_eq!(r.items.len(), MAX_ITEMS);
        assert_eq!(r.stop_reason, Some("candidateLimit"));
    }
}
