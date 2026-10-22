//! Read-only, source-bound purge-name discovery. No purge entrypoint or deletion plan.
use crate::directory_probe::{local, open_child, stat_at, Stream};
use crate::{app_protection, engine, observation::AnchoredPath};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CStr, CString},
    fs::File,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::Path,
    time::Instant,
};
mod activity;
mod content;
mod context;
pub mod git_index;
pub mod measurement;
pub mod roots;
mod scope;
const MAX_ENTRIES: usize = 10_000;
const MAX_CANDIDATES: usize = 256;
const MAX_DEPTH: usize = 6;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    version: String,
    revision: String,
    targets: Vec<String>,
    workspace_indicators: Vec<String>,
    project_indicators: Vec<String>,
}
fn catalog() -> Result<Catalog, String> {
    let value: Catalog =
        serde_json::from_str(include_str!("../../src/lib/project-artifacts-catalog.json"))
            .map_err(|_| "工程产物说明无法读取。")?;
    if value.revision != engine::REVISION
        || value.version != "1.59.1"
        || value.targets.len() != 34
        || value.workspace_indicators.len() != 5
        || value.project_indicators.len() != 16
        || value
            .targets
            .iter()
            .chain(value.workspace_indicators.iter())
            .chain(value.project_indicators.iter())
            .any(|s| s.is_empty() || s.contains('/') || s.contains('\0'))
    {
        return Err("工程产物说明基线不匹配。".into());
    }
    Ok(value)
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    path: String,
    name: String,
    context: &'static str,
    project_path: Option<String>,
    protection: &'static str,
    project_hint: context::Hint,
    measurement: &'static str,
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
}
impl Skipped {
    fn any(&self) -> bool {
        self.links
            + self.unreadable
            + self.non_local
            + self.depth
            + self.invalid_name
            + self.changed
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
    items: Vec<Candidate>,
    can_execute: bool,
}
fn marker(parent: &File, name: &str) -> bool {
    stat_at(parent, &CString::new(name).unwrap()).is_ok_and(|m| {
        m.st_mode & libc::S_IFMT == libc::S_IFREG && m.st_nlink == 1 && m.st_flags & 0x40000000 == 0
    })
}
struct Scanner<'a> {
    began: Instant,
    targets: &'a [String],
    reference: &'a Catalog,
    client: &'a [String],
    visited: usize,
    items: Vec<Candidate>,
    skipped: Skipped,
    stop: Option<&'static str>,
    entry_limit: usize,
}
impl Scanner<'_> {
    fn ready(&mut self) -> bool {
        if self.stop.is_some() {
            return false;
        }
        self.stop = if self.began.elapsed().as_secs() >= 4 {
            Some("timeLimit")
        } else if self.visited >= self.entry_limit {
            Some("entryLimit")
        } else if self.items.len() >= MAX_CANDIDATES {
            Some("candidateLimit")
        } else {
            None
        };
        self.stop.is_none()
    }
    fn walk(&mut self, directory: &File, path: &Path, depth: usize, ancestors: &[context::Hint]) {
        if !self.ready() {
            return;
        }
        let Ok(before) = directory.metadata() else {
            self.skipped.unreadable += 1;
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
            self.skipped.unreadable += 1;
            return;
        }
        let pointer = unsafe { libc::fdopendir(fd) };
        if pointer.is_null() {
            unsafe {
                libc::close(fd);
            }
            self.skipped.unreadable += 1;
            return;
        }
        let stream = Stream(pointer);
        let first_item = self.items.len();
        let probe = context::Probe::inspect(directory, path, self.reference);
        let mut lineage = ancestors.to_vec();
        lineage.push(probe.hint.clone());
        let node_context = marker(directory, "package.json");
        let rust_context = marker(directory, "Cargo.toml");
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
                    self.skipped.unreadable += 1;
                }
                break;
            }
            let name = unsafe { CStr::from_ptr((*raw).d_name.as_ptr()) }.to_owned();
            if [b".".as_slice(), b"..".as_slice()].contains(&name.to_bytes()) {
                continue;
            }
            self.visited += 1;
            let Ok(text) = name.to_str() else {
                self.skipped.invalid_name += 1;
                continue;
            };
            if text.chars().any(char::is_control) {
                self.skipped.invalid_name += 1;
                continue;
            }
            let target = self.targets.iter().any(|n| n == text);
            if !target && (text.starts_with('.') || ["Library", "Applications"].contains(&text)) {
                continue;
            }
            let Ok(stat) = stat_at(directory, &name) else {
                self.skipped.unreadable += 1;
                continue;
            };
            if stat.st_mode & libc::S_IFMT == libc::S_IFLNK {
                self.skipped.links += 1;
                continue;
            }
            if stat.st_mode & libc::S_IFMT != libc::S_IFDIR {
                continue;
            }
            if stat.st_flags & 0x40000000 != 0 {
                self.skipped.non_local += 1;
                continue;
            }
            let child_path = path.join(text);
            if child_path.as_os_str().len() > 4096 {
                self.skipped.invalid_name += 1;
                continue;
            }
            let Ok(child) = open_child(directory, &name, &stat) else {
                self.skipped.unreadable += 1;
                continue;
            };
            if !local(&child) {
                self.skipped.non_local += 1;
                continue;
            }
            if target {
                let context = if text == "node_modules" && node_context {
                    "nodeContext"
                } else if text == "target" && rust_context {
                    "rustContext"
                } else {
                    "nameOnly"
                };
                let Some(value) = child_path.to_str() else {
                    self.skipped.invalid_name += 1;
                    continue;
                };
                let protection = app_protection::check(value, None, None, "", self.client)
                    .map(|r| {
                        if r.reasons.is_empty() {
                            "notMatched"
                        } else {
                            "matched"
                        }
                    })
                    .unwrap_or("unknown");
                self.items.push(Candidate {
                    path: value.into(),
                    name: text.into(),
                    context,
                    project_path: if context == "nameOnly" {
                        None
                    } else {
                        path.to_str().map(String::from)
                    },
                    protection,
                    project_hint: context::choose(&lineage),
                    measurement: "notMeasured",
                    can_execute: false,
                });
            } else if depth + 1 < MAX_DEPTH {
                self.walk(&child, &child_path, depth + 1, &lineage);
            } else {
                self.skipped.depth += 1;
            }
            if !stat_at(directory, &name).is_ok_and(|now| {
                now.st_dev == stat.st_dev
                    && now.st_ino == stat.st_ino
                    && now.st_mode == stat.st_mode
            }) {
                self.skipped.changed += 1;
            }
        }
        // One change event per directory keeps the existing bounded counter,
        // whether marker evidence, directory metadata, or both changed.
        if !probe.validate(directory)
            || !directory.metadata().is_ok_and(|after| {
                before.dev() == after.dev()
                    && before.ino() == after.ino()
                    && before.mtime() == after.mtime()
                    && before.mtime_nsec() == after.mtime_nsec()
                    && before.ctime() == after.ctime()
                    && before.ctime_nsec() == after.ctime_nsec()
            })
        {
            self.skipped.changed += 1;
            for item in &mut self.items[first_item..] {
                item.project_hint = context::Hint::unknown();
            }
        }
    }
}
pub fn scan(root: &Path, client: &[String]) -> Result<Report, String> {
    scan_with_limit(root, client, MAX_ENTRIES)
}
fn scan_with_limit(root: &Path, client: &[String], entry_limit: usize) -> Result<Report, String> {
    let reference = catalog()?;
    let text = root
        .to_str()
        .filter(|v| v.len() <= 4096 && !v.chars().any(char::is_control))
        .ok_or("请选择有效的绝对目录路径。")?;
    if root
        .file_name()
        .and_then(|v| v.to_str())
        .is_some_and(|v| reference.targets.iter().any(|n| n == v))
    {
        return Err("请选择项目或项目所在目录，而不是产物目录本身。".into());
    }
    let held = AnchoredPath::open(root)?;
    if !held.target().metadata().is_ok_and(|m| m.is_dir()) || !held.local_materialized() {
        return Err("请选择已下载到本机、可读取的项目目录。".into());
    }
    let mut scanner = Scanner {
        began: Instant::now(),
        targets: &reference.targets,
        reference: &reference,
        client,
        visited: 0,
        items: vec![],
        skipped: Skipped::default(),
        stop: None,
        entry_limit,
    };
    scanner.walk(held.target(), root, 0, &[]);
    held.validate()
        .map_err(|_| "扫描期间所选目录已变化，请重新扫描。")?;
    scanner.items.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Report {
        protocol: 2,
        revision: engine::REVISION,
        root: text.into(),
        checked_at: crate::db::now(),
        completion: if scanner.stop.is_some() || scanner.skipped.any() {
            "partial"
        } else {
            "complete"
        },
        stop_reason: scanner.stop,
        visited: scanner.visited,
        skipped: scanner.skipped,
        items: scanner.items,
        can_execute: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};
    fn temporary_root() -> tempfile::TempDir {
        tempfile::tempdir_in("/private/tmp").unwrap()
    }
    #[test]
    fn discovers_pinned_names_prunes_artifact_contents_and_keeps_contexts_unverified() {
        let root = temporary_root();
        for path in [
            "web/node_modules/nested/target",
            "rust/target",
            "other/target",
            "other/coverage",
            "ignored/.git/build",
            "hidden/.hidden/dist",
        ] {
            fs::create_dir_all(root.path().join(path)).unwrap();
        }
        fs::write(root.path().join("web/package.json"), "{}").unwrap();
        fs::write(root.path().join("rust/Cargo.toml"), "fixture").unwrap();
        let report = scan(
            root.path(),
            &[root.path().join("rust").to_str().unwrap().into()],
        )
        .unwrap();
        assert_eq!(report.items.len(), 4);
        assert!(!report.can_execute);
        assert_eq!(
            report
                .items
                .iter()
                .filter(|v| v.context == "nameOnly")
                .count(),
            2
        );
        assert_eq!(
            report
                .items
                .iter()
                .find(|v| v.context == "nodeContext")
                .unwrap()
                .name,
            "node_modules"
        );
        let rust = report
            .items
            .iter()
            .find(|v| v.context == "rustContext")
            .unwrap();
        assert_eq!(rust.protection, "matched");
        assert!(report
            .items
            .iter()
            .all(|v| !v.can_execute && v.measurement == "notMeasured"));
        assert!(!serde_json::to_string(&report)
            .unwrap()
            .contains("sizeBytes"));
        let wire = serde_json::to_value(&report).unwrap();
        assert_eq!(wire["items"][0]["canExecute"], false);
        assert!(wire["items"][0].get("projectPath").is_some());
        assert!(wire["skipped"].get("nonLocal").is_some());
        assert!(wire["items"][0].get("can_execute").is_none());
    }
    #[test]
    fn skips_links_preserves_unknown_limits_and_rejects_artifact_roots() {
        let root = temporary_root();
        let outside = temporary_root();
        fs::create_dir(outside.path().join("dist")).unwrap();
        symlink(outside.path(), root.path().join("linked")).unwrap();
        let report = scan(root.path(), &[]).unwrap();
        assert!(report.items.is_empty());
        assert_eq!(report.completion, "partial");
        assert_eq!(report.skipped.links, 1);
        fs::create_dir(root.path().join("node_modules")).unwrap();
        assert!(scan(&root.path().join("node_modules"), &[]).is_err());
        let report = scan_with_limit(root.path(), &[], 1).unwrap();
        assert_eq!(report.stop_reason, Some("entryLimit"));
        assert_eq!(report.completion, "partial");
        assert!(scan(Path::new("/"), &[]).is_err());
        assert!(scan(Path::new("relative"), &[]).is_err());
    }
    #[test]
    fn direct_project_hidden_artifacts_and_linked_markers_do_not_overstate_ownership() {
        let root = temporary_root();
        fs::write(root.path().join("package.json"), "{}").unwrap();
        fs::create_dir(root.path().join("node_modules")).unwrap();
        fs::create_dir(root.path().join(".venv")).unwrap();
        fs::create_dir_all(root.path().join("nested/target")).unwrap();
        symlink(
            root.path().join("package.json"),
            root.path().join("nested/Cargo.toml"),
        )
        .unwrap();
        let report = scan(root.path(), &["relative".into()]).unwrap();
        assert_eq!(report.items.len(), 3);
        assert_eq!(
            report
                .items
                .iter()
                .find(|i| i.name == "node_modules")
                .unwrap()
                .context,
            "nodeContext"
        );
        assert_eq!(
            report
                .items
                .iter()
                .find(|i| i.name == "target")
                .unwrap()
                .context,
            "nameOnly"
        );
        assert!(report.items.iter().all(|i| i.protection == "unknown"));
    }
    #[test]
    fn depth_and_result_limits_keep_partial_results_without_entering_artifacts() {
        let root = temporary_root();
        let mut path = root.path().to_path_buf();
        for _ in 0..7 {
            path = path.join("nested");
            fs::create_dir(&path).unwrap();
        }
        fs::create_dir(path.join("target")).unwrap();
        let report = scan(root.path(), &[]).unwrap();
        assert!(report.items.is_empty());
        assert!(report.skipped.depth > 0);
        assert_eq!(report.completion, "partial");
        let root = temporary_root();
        for n in 0..257 {
            fs::create_dir_all(root.path().join(format!("p{n}/node_modules"))).unwrap();
        }
        let report = scan(root.path(), &[]).unwrap();
        assert_eq!(report.items.len(), MAX_CANDIDATES);
        assert_eq!(report.stop_reason, Some("candidateLimit"));
    }
    #[test]
    fn replaced_child_does_not_follow_an_outside_link() {
        let root = temporary_root();
        let outside = temporary_root();
        fs::create_dir(root.path().join("child")).unwrap();
        let held = AnchoredPath::open(root.path()).unwrap();
        let name = CString::new("child").unwrap();
        let stat = stat_at(held.target(), &name).unwrap();
        fs::rename(root.path().join("child"), root.path().join("old")).unwrap();
        symlink(outside.path(), root.path().join("child")).unwrap();
        assert!(open_child(held.target(), &name, &stat).is_err());
    }
}
