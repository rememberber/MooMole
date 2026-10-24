//! Read-only preference observations for two pinned Mole tasks. No mutation authority.
use crate::{engine, observation::AnchoredPath};
use serde::Serialize;
use std::{
    ffi::CString,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::Path,
    time::Instant,
};

const IDS: [&str; 2] = ["prevent_network_dsstore", "legacy_overrides_audit"];
const MAX_RULES: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Value {
    Enabled,
    Disabled,
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    domain: &'static str,
    key: &'static str,
    label: &'static str,
    value: Value,
    managed: bool,
    original_evidence: Vec<originals::Evidence>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    id: &'static str,
    code: &'static str,
    task_protection: &'static str,
    findings: Vec<Finding>,
    paths: Vec<paths::Observation>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    protocol: u32,
    observation_id: String,
    revision: &'static str,
    checked_at: i64,
    rule_source: &'static str,
    items: Vec<Item>,
}

enum Config {
    Absent,
    Present(String),
    Unknown,
}
fn config(path: &Path) -> Config {
    // Confirm missing ancestors physically instead of interpreting every open
    // error as an empty file. Never follow links or open FIFOs/devices.
    let mut nearest = path;
    loop {
        match std::fs::symlink_metadata(nearest) {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let Some(parent) = nearest.parent() else {
                    return Config::Unknown;
                };
                nearest = parent;
            }
            Err(_) => return Config::Unknown,
        }
    }
    let Ok(held) = AnchoredPath::open(nearest) else {
        return Config::Unknown;
    };
    if !held.local_materialized() {
        return Config::Unknown;
    }
    if nearest != path {
        return if held.target().metadata().is_ok_and(|m| m.is_dir()) && held.validate().is_ok() {
            Config::Absent
        } else {
            Config::Unknown
        };
    }
    if !held
        .target()
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= MAX_RULES)
    {
        return Config::Unknown;
    }
    let mut bytes = vec![];
    let Some(name) = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| CString::new(name).ok())
    else {
        return Config::Unknown;
    };
    // AnchoredPath deliberately holds O_EVTONLY. Open a separate read-only fd
    // relative to the held parent, bind its identity before reading any bytes.
    let fd = unsafe {
        libc::openat(
            held.parent().as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Config::Unknown;
    }
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    let same = || {
        file.metadata().is_ok_and(|m| {
            let node = held.witness.nodes.last().unwrap();
            m.is_file()
                && m.dev() == node.dev
                && m.ino() == node.ino
                && m.len() == held.witness.size
                && m.nlink() == 1
                && m.mtime() == held.witness.mtime
                && m.mtime_nsec() == held.witness.mtime_ns
                && m.ctime() == held.witness.ctime
                && m.ctime_nsec() == held.witness.ctime_ns
        })
    };
    if !same()
        || (&file).take(MAX_RULES + 1).read_to_end(&mut bytes).is_err()
        || bytes.len() as u64 > MAX_RULES
        || !same()
        || held.validate().is_err()
    {
        return Config::Unknown;
    }
    match String::from_utf8(bytes) {
        Ok(text) if !text.contains('\0') && (text.is_empty() || text.ends_with('\n')) => {
            Config::Present(text)
        }
        _ => Config::Unknown,
    }
}

fn rules(home: &Path) -> (&'static str, Option<Vec<String>>) {
    let primary = home.join(".config/mole/whitelist_optimize");
    let legacy = home.join(".config/mole/whitelist_checks");
    let (source, text) = match config(&primary) {
        Config::Present(text) => ("current", text),
        Config::Unknown => return ("unknown", None),
        Config::Absent => match config(&legacy) {
            Config::Present(text) => ("legacy", text),
            Config::Unknown => return ("unknown", None),
            // The pinned DEFAULT_OPTIMIZE_WHITELIST_PATTERNS is empty.
            Config::Absent => return ("defaults", Some(vec![])),
        },
    };
    let mut patterns = vec![];
    for line in text.lines() {
        let line = line.trim_matches(|c: char| c.is_ascii_whitespace());
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if patterns.len() >= 256 {
            return ("unknown", None);
        }
        patterns.push(line.into());
    }
    // Only exact action IDs are evaluated. Path/glob protection is deliberately
    // not ported or declared clear. Do not call load_whitelist: it may migrate.
    (source, Some(patterns))
}

fn item(id: &'static str, patterns: Option<&[String]>, findings: Vec<Finding>) -> Item {
    let task_protection = match patterns {
        Some(patterns) if patterns.iter().any(|pattern| pattern == id) => "matched",
        Some(_) => "notMatched",
        None => "unknown",
    };
    let code = if task_protection == "matched" || findings.iter().any(|f| f.managed) {
        "protected"
    } else if findings.iter().any(|f| {
        f.value == Value::Unknown || f.original_evidence.iter().any(|v| v.state == "changed")
    }) || patterns.is_none()
    {
        "unknown"
    } else {
        "observed"
    };
    Item {
        id,
        code,
        task_protection,
        findings,
        paths: vec![],
    }
}

pub fn inspect(home: &Path, client: &[String]) -> Result<Report, String> {
    let began = Instant::now();
    let (source, patterns) = rules(home);
    let read = |domain: &'static str, key: &'static str, label: &'static str| {
        let (value, managed, original_evidence) = if began.elapsed().as_secs() < 4 {
            native::read(domain, key)
        } else {
            (Value::Unknown, false, originals::unavailable("notChecked"))
        };
        Finding {
            domain,
            key,
            label,
            value,
            managed,
            original_evidence,
        }
    };
    let finder = vec![
        read(
            "com.apple.desktopservices",
            "DSDontWriteNetworkStores",
            "共享目录不写入 Finder 记录",
        ),
        read(
            "com.apple.desktopservices",
            "DSDontWriteUSBStores",
            "USB 卷不写入 Finder 记录",
        ),
    ];
    let overrides = vec![
        read("-g", "NSAppSleepDisabled", "全局禁用 App Nap"),
        read(
            "com.apple.frameworks.diskimages",
            "skip-verify",
            "磁盘映像跳过验证",
        ),
        read(
            "com.apple.frameworks.diskimages",
            "skip-verify-locked",
            "锁定映像跳过验证",
        ),
        read(
            "com.apple.frameworks.diskimages",
            "skip-verify-remote",
            "远程映像跳过验证",
        ),
    ];
    let mut items = vec![
        item(IDS[0], patterns.as_deref(), finder),
        item(IDS[1], patterns.as_deref(), overrides),
    ];
    let mut matching_budget = 1_000_000;
    for item in &mut items {
        item.paths = paths::locations(item.id)
            .iter()
            .map(|location| {
                paths::observe(
                    home,
                    location,
                    patterns.as_deref(),
                    client,
                    &mut matching_budget,
                    began,
                )
            })
            .collect();
        if item
            .paths
            .iter()
            .any(|path| path.client_protection == "matched" || path.mole_protection == "matched")
        {
            item.code = "protected"
        } else if item.code != "protected"
            && item.paths.iter().any(|path| path.access == "restricted")
        {
            item.code = "restricted"
        } else if item.code != "protected"
            && item.paths.iter().any(|path| {
                path.state != "present"
                    || path.client_protection == "unknown"
                    || path.mole_protection == "unknown"
                    || path.access != "noRestrictionObserved"
            })
        {
            item.code = "unknown"
        }
    }
    Ok(Report {
        protocol: 4,
        observation_id: uuid::Uuid::new_v4().to_string(),
        revision: engine::REVISION,
        checked_at: crate::db::now(),
        rule_source: source,
        items,
    })
}

mod native;
mod originals;
mod paths;
#[cfg(test)]
mod preference_sandbox;
pub(crate) mod quicklook;
pub(crate) mod refresh_journal;
pub(crate) mod review;
#[cfg(test)]
mod tests;
