//! Opt-in recurring personal-directory observations. Results never populate a
//! modification Store: processing always starts with a new interactive scan.
use super::*;
use crate::{
    db,
    engine::{Job, TaskKind},
    observation::{AnchoredPath, Node},
};
use std::{
    collections::BTreeMap,
    os::unix::fs::MetadataExt,
    sync::{atomic::Ordering, Arc, Mutex},
};
mod journal;
pub(crate) mod runtime;
const MAX_WATCHES: usize = 8;
const MAX_RUNS: usize = 64;
const MAX_CHANGES: usize = 100;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Watch {
    id: String,
    root: String,
    enabled: bool,
    interval_minutes: u32,
    updated_at: i64,
    last_attempt_at: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    config: Watch,
    generation: String,
    nodes: Vec<Node>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Edit {
    id: Option<String>,
    root: String,
    enabled: bool,
    interval_minutes: u32,
}
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Trigger {
    Manual,
    Scheduled,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum State {
    Running,
    Completed,
    Partial,
    Cancelled,
    Failed,
    Superseded,
    Interrupted,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Comparison {
    First,
    Compared,
    Reset,
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ChangeKind {
    Added,
    Changed,
    NotSeen,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Change {
    path: String,
    kind: ChangeKind,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Summary {
    complete: bool,
    visited: usize,
    skipped: usize,
    files: usize,
    logical_bytes: u64,
    comparison: Comparison,
    added: Option<usize>,
    changed: Option<usize>,
    not_seen: Option<usize>,
    logical_delta_bytes: Option<i64>,
    changes: Vec<Change>,
    changes_truncated: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Record {
    version: u32,
    id: String,
    watch_id: String,
    generation: String,
    root: String,
    trigger: Trigger,
    started_at: i64,
    finished_at: Option<i64>,
    state: State,
    summary: Option<Summary>,
    error: Option<String>,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fingerprint {
    dev: u64,
    ino: u64,
    size: u64,
    mtime: i64,
    mtime_ns: i64,
    ctime: i64,
    ctime_ns: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Baseline {
    version: u32,
    root: String,
    nodes: Vec<Node>,
    rules: Vec<String>,
    files: BTreeMap<String, Fingerprint>,
}
struct Observed {
    baseline: Baseline,
    visited: usize,
    skipped: usize,
    complete: bool,
    cancelled: bool,
}
fn valid_interval(value: u32) -> bool {
    [15, 60, 360, 1440].contains(&value)
}
fn valid_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok()
}
fn valid_root(value: &str) -> bool {
    value.len() <= 4096 && ordinary(Path::new(value))
}
fn bind(root: &str, rules: &[String]) -> Result<Vec<Node>, String> {
    if !valid_root(root)
        || protected(Path::new(root), &[])?
        || rules.iter().any(|rule| Path::new(root).starts_with(rule))
    {
        return Err("请选择未受保护的普通个人目录。".into());
    }
    let held = AnchoredPath::open(Path::new(root))?;
    let m = held.target().metadata().map_err(|e| e.to_string())?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || !held.local_materialized() {
        return Err("定期扫描仅支持本账户拥有、已落地的物理目录。".into());
    }
    held.validate()?;
    Ok(held.witness.nodes)
}
fn validate_binding(watch: &Saved, rules: &[String]) -> Result<(), String> {
    if bind(&watch.config.root, rules)? != watch.nodes {
        return Err("目录或祖先身份已变化；请重新选择并保存扫描范围。".into());
    }
    Ok(())
}
fn due(watch: &Saved, now: i64) -> bool {
    watch.config.enabled
        && watch.config.last_attempt_at.is_none_or(|last| {
            now < last
                || now.saturating_sub(last) >= i64::from(watch.config.interval_minutes) * 60_000
        })
}
fn observe(watch: &Saved, rules: &[String], job: Arc<Job>) -> Result<Observed, String> {
    validate_binding(watch, rules)?;
    let scan = super::scan::collect(Path::new(&watch.config.root), rules, job.clone())?;
    validate_binding(watch, rules)?;
    let mut files = BTreeMap::new();
    for item in scan.items {
        let w = item.witness;
        let node = w.nodes.last().ok_or("扫描项目缺少身份。")?;
        if files
            .insert(
                item.path,
                Fingerprint {
                    dev: node.dev,
                    ino: node.ino,
                    size: w.size,
                    mtime: w.mtime,
                    mtime_ns: w.mtime_ns,
                    ctime: w.ctime,
                    ctime_ns: w.ctime_ns,
                },
            )
            .is_some()
        {
            return Err("扫描返回重复路径。".into());
        }
    }
    Ok(Observed {
        baseline: Baseline {
            version: 1,
            root: watch.config.root.clone(),
            nodes: watch.nodes.clone(),
            rules: rules.to_vec(),
            files,
        },
        visited: scan.report.visited,
        skipped: scan.report.skipped,
        complete: scan.report.complete,
        cancelled: job.cancelled.load(Ordering::SeqCst) || scan.report.cancelled,
    })
}
fn logical(files: &BTreeMap<String, Fingerprint>) -> Result<u64, String> {
    files.values().try_fold(0u64, |total, f| {
        total
            .checked_add(f.size)
            .filter(|n| *n <= 9_007_199_254_740_991)
            .ok_or("所见文件大小超过精确计量范围。".into())
    })
}
fn summarize(current: &Observed, previous: Option<&Baseline>) -> Result<Summary, String> {
    let mut s = Summary {
        complete: current.complete && !current.cancelled,
        visited: current.visited,
        skipped: current.skipped,
        files: current.baseline.files.len(),
        logical_bytes: logical(&current.baseline.files)?,
        comparison: Comparison::Unavailable,
        added: None,
        changed: None,
        not_seen: None,
        logical_delta_bytes: None,
        changes: vec![],
        changes_truncated: false,
    };
    if !s.complete {
        return Ok(s);
    }
    let Some(previous) = previous else {
        s.comparison = Comparison::First;
        return Ok(s);
    };
    if previous.root != current.baseline.root
        || previous.nodes != current.baseline.nodes
        || previous.rules != current.baseline.rules
    {
        s.comparison = Comparison::Reset;
        return Ok(s);
    }
    s.comparison = Comparison::Compared;
    let (mut added, mut changed, mut missing) = (0, 0, 0);
    let mut push = |path: &String, kind| {
        if s.changes.len() < MAX_CHANGES {
            s.changes.push(Change {
                path: path.clone(),
                kind,
            });
        } else {
            s.changes_truncated = true;
        }
    };
    for (path, f) in &current.baseline.files {
        match previous.files.get(path) {
            None => {
                added += 1;
                push(path, ChangeKind::Added);
            }
            Some(old) if old != f => {
                changed += 1;
                push(path, ChangeKind::Changed);
            }
            _ => {}
        }
    }
    for path in previous
        .files
        .keys()
        .filter(|p| !current.baseline.files.contains_key(*p))
    {
        missing += 1;
        push(path, ChangeKind::NotSeen);
    }
    s.added = Some(added);
    s.changed = Some(changed);
    s.not_seen = Some(missing);
    s.logical_delta_bytes = Some(s.logical_bytes as i64 - logical(&previous.files)? as i64);
    Ok(s)
}
pub(crate) fn reconcile(db: &rusqlite::Connection) -> Result<(), String> {
    journal::reconcile(db, &[])
}
#[cfg(test)]
mod tests;
