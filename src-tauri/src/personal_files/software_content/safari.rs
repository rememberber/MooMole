//! Safari's fixed Downloads.plist lists only. No browsing DB, bookmarks,
//! URL output, security-scoped bookmark resolution or source writes.
use super::*;
use browser_profiles::{Browser, Profile, Profiles, Selection};
use objc2::{rc::autoreleasepool, runtime::AnyObject};
use objc2_foundation::{
    NSArray, NSData, NSDate, NSDictionary, NSNumber, NSPropertyListReadOptions,
    NSPropertyListSerialization, NSString,
};
use std::sync::atomic::Ordering;
const LIMIT: u64 = 8 * 1024 * 1024;
fn get(d: &NSDictionary, key: &str) -> Option<objc2::rc::Retained<AnyObject>> {
    d.objectForKey(&NSString::from_str(key))
}
fn integer(d: &NSDictionary, key: &str) -> Option<u64> {
    use core_foundation_sys::{
        base::CFGetTypeID,
        number::{CFNumberGetTypeID, CFNumberIsFloatType},
    };
    let v = get(d, key)?;
    let n = v.downcast_ref::<NSNumber>()?;
    let p = &*v as *const AnyObject as *const std::ffi::c_void;
    if unsafe { CFGetTypeID(p) != CFNumberGetTypeID() || CFNumberIsFloatType(p.cast()) != 0 } {
        return None;
    }
    n.stringValue()
        .to_string()
        .parse::<u64>()
        .ok()
        .filter(|n| *n <= 9_007_199_254_740_991)
}
struct Candidate {
    path: String,
    bytes: u64,
}
// An explicit finish date plus equal integral counters and no error/resume
// markers are required. This is a source claim, never proof of content identity.
fn candidate(d: &NSDictionary, now: i64) -> Result<Option<Candidate>, ()> {
    if [
        "DownloadEntryErrorCodeDictionaryKey",
        "DownloadEntryErrorDomainDictionaryKey",
        "DownloadEntryError",
        "DownloadEntryResumeInformation",
        "DownloadEntryResumeData",
        "DownloadEntryPaused",
    ]
    .iter()
    .any(|k| get(d, k).is_some())
    {
        return Ok(None);
    }
    let total = integer(d, "DownloadEntryProgressTotalToLoad").ok_or(())?;
    let received = integer(d, "DownloadEntryProgressBytesSoFar").ok_or(())?;
    if received != total {
        return Ok(None);
    }
    let date = get(d, "DownloadEntryDateFinished").ok_or(())?;
    let finished = date
        .downcast_ref::<NSDate>()
        .ok_or(())?
        .timeIntervalSince1970()
        * 1000.0;
    if !finished.is_finite() || finished <= 0.0 || finished > now as f64 + 300_000.0 {
        return Err(());
    }
    let value = get(d, "DownloadEntryPath").ok_or(())?;
    let text = value.downcast_ref::<NSString>().ok_or(())?;
    if text.length() > 4096 {
        return Err(());
    }
    let path = text.to_string();
    if path.len() > 4096
        || path.chars().any(char::is_control)
        || !ordinary(Path::new(&path))
        || path.to_ascii_lowercase().ends_with(".download")
    {
        return Err(());
    }
    Ok(Some(Candidate { path, bytes: total }))
}
fn decode(
    bytes: &[u8],
    job: &Job,
    start: Instant,
) -> Result<(Vec<Candidate>, usize, usize, bool), String> {
    autoreleasepool(|_| {
        let plist = unsafe {
            NSPropertyListSerialization::propertyListWithData_options_format_error(
                &NSData::with_bytes(bytes),
                NSPropertyListReadOptions::Immutable,
                std::ptr::null_mut(),
            )
        }
        .map_err(|_| "Safari 下载列表格式无法识别，原记录保留。")?;
        let d = plist
            .downcast_ref::<NSDictionary>()
            .ok_or("Safari 下载列表不是已支持的字典结构。")?;
        let list = get(d, "DownloadHistory")
            .ok_or("Safari 下载列表缺少 DownloadHistory，未推断下载文件。")?;
        let rows = list
            .downcast_ref::<NSArray>()
            .ok_or("Safari 下载条目不是数组。")?;
        let mut candidates: Vec<Candidate> = vec![];
        let mut paths = HashMap::<String, usize>::new();
        let mut visited = 0;
        let mut skipped = 0;
        let mut complete = rows.count() <= 2000;
        let now = db::now();
        for row in rows.iter().take(2000) {
            if job.cancelled.load(Ordering::SeqCst) || start.elapsed() >= Duration::from_secs(10) {
                complete = false;
                break;
            }
            visited += 1;
            if let Some(d) = row.downcast_ref::<NSDictionary>() {
                if let Some(v) = get(d, "DownloadEntryPath") {
                    if let Some(p) = v.downcast_ref::<NSString>().filter(|p| p.length() <= 4096) {
                        *paths.entry(p.to_string()).or_default() += 1;
                    }
                }
            }
            let item = row
                .downcast_ref::<NSDictionary>()
                .ok_or(())
                .and_then(|d| candidate(d, now));
            match item {
                Ok(Some(v)) => candidates.push(v),
                Ok(None) => {
                    skipped += 1;
                }
                Err(()) => {
                    skipped += 1;
                    complete = false;
                }
            }
        }
        candidates.retain(|c| {
            if paths.get(&c.path) == Some(&1) {
                true
            } else {
                skipped += 1;
                complete = false;
                false
            }
        });
        Ok((candidates, visited, skipped, complete))
    })
}
pub(super) fn discover(home: &Path, rules: &[String]) -> Result<Profiles, String> {
    let mut result = Profiles {
        browser: Browser::Safari,
        profiles: vec![],
        complete: true,
        skipped: 0,
    };
    for profile in ["Standard", "Sandbox"] {
        let selection = Selection {
            browser: Browser::Safari,
            profile: profile.into(),
        };
        let history = selection.history(home)?;
        let observed = (|| -> Result<bool, String> {
            if protected(&history, rules)? {
                return Err("protected".into());
            }
            match std::fs::symlink_metadata(&history) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(_) => return Err("Safari 下载列表访问受限或状态未知。".into()),
                Ok(_) => {}
            }
            let h = AnchoredPath::open(&history)?;
            let m = h.target().metadata().map_err(|e| e.to_string())?;
            if !m.is_file()
                || m.uid() != unsafe { libc::geteuid() }
                || m.nlink() != 1
                || m.len() > LIMIT
                || !h.local_materialized()
            {
                return Err("unsupported".into());
            }
            h.validate()?;
            Ok(true)
        })();
        match observed {
            Ok(true) => result.profiles.push(Profile {
                selection,
                history_path: history.to_str().ok_or("Safari 路径无法编码。")?.into(),
            }),
            Ok(false) => {}
            Err(_) => {
                result.complete = false;
                result.skipped += 1;
            }
        }
    }
    Ok(result)
}
pub(super) fn collect(
    home: &Path,
    profile: Selection,
    rules: &[String],
    job: Arc<Job>,
) -> Result<Scan, String> {
    let start = Instant::now();
    if profile.browser != Browser::Safari || !ordinary(home) {
        return Err("Safari 来源范围无法核对。".into());
    }
    let root = AnchoredPath::open(home)?;
    let meta = root.target().metadata().map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || !root.local_materialized() {
        return Err("账户目录不属于当前用户或本地状态未知。".into());
    }
    let path = profile.history(home)?;
    if protected(&path, rules)? {
        return Err("Safari 下载列表与保护范围重叠，未读取。".into());
    }
    // Reject aliases and unsupported source objects before reading any bytes;
    // discovery is optional and cannot supply a later read authorization.
    let before = AnchoredPath::open(&path)?;
    let source_meta = before.target().metadata().map_err(|e| e.to_string())?;
    if !source_meta.is_file()
        || source_meta.uid() != unsafe { libc::geteuid() }
        || source_meta.nlink() != 1
        || source_meta.dev() != meta.dev()
        || source_meta.len() > LIMIT
        || !before.local_materialized()
    {
        return Err("Safari 下载列表身份不受支持。".into());
    }
    let read = chromium::read(&path, LIMIT, &job, start)
        .map_err(|_| "Safari 下载列表无法读取、已变化或访问受限；未扫描其他位置。")?;
    let m = read.held.target().metadata().map_err(|e| e.to_string())?;
    if read.held.witness != before.witness || m.nlink() != 1 || m.dev() != meta.dev() {
        return Err("Safari 下载列表身份不受支持。".into());
    }
    let (candidates, visited, mut skipped, mut complete) = decode(&read.bytes, &job, start)?;
    let mut seen = HashSet::new();
    let mut items = vec![];
    let mut serialized = 0;
    for c in candidates {
        if job.cancelled.load(Ordering::SeqCst) || start.elapsed() >= Duration::from_secs(10) {
            complete = false;
            break;
        }
        let p = Path::new(&c.path);
        if !p.starts_with(home) || p == home || protected(p, rules)? {
            skipped += 1;
            continue;
        }
        let item = (|| -> Result<Item, String> {
            let h = AnchoredPath::open(p)?;
            let m = h.target().metadata().map_err(|e| e.to_string())?;
            if !m.is_file()
                || m.uid() != unsafe { libc::geteuid() }
                || m.nlink() != 1
                || m.dev() != meta.dev()
                || m.len() != c.bytes
                || !h.local_materialized()
                || !seen.insert((m.dev(), m.ino()))
            {
                return Err("下载文件不符，保留。".into());
            }
            h.validate()?;
            Ok(Item {
                id: uuid::Uuid::new_v4().to_string(),
                path: c.path.clone(),
                size: m.len(),
                modified_at: m.mtime().saturating_mul(1000),
                witness: h.witness,
            })
        })();
        match item {
            Ok(item) => {
                serialized += serde_json::to_vec(&item).map_err(|e| e.to_string())?.len();
                if serialized > 7 * 1024 * 1024 {
                    complete = false;
                    break;
                }
                items.push(item);
            }
            Err(_) => {
                skipped += 1;
                complete = false;
            }
        }
    }
    root.validate()?;
    read.held.validate()?;
    let now = db::now();
    let binding = Binding {
        source: Source::ChromeDownloads,
        home: home.to_str().ok_or("账户路径无法编码。")?.into(),
        directory: root.witness,
        allocated_at_scan: 0,
        history: Some(read.held.witness),
        browser_profile: Some(profile.clone()),
        playlist: None,
        telegram_export: None,
    };
    let report = Report {
        id: uuid::Uuid::new_v4().to_string(),
        root: home.to_str().unwrap().into(),
        created_at: now,
        expires_at: now + TTL,
        complete,
        cancelled: job.cancelled.load(Ordering::SeqCst),
        skipped,
        visited,
        items: items.iter().map(row).collect(),
        live_photos: None,
        ai_focus: None,
        duplicates: None,
        similar_photos: None,
        software_content: Some(Info {
            source: Source::ChromeDownloads,
            minimum_age_days: 0,
            minimum_directory_bytes: 0,
            minimum_size_met: true,
            execution_supported: true,
            playlist_path: None,
            export_path: None,
            browser_profile: Some(profile),
        }),
    };
    Ok(Scan {
        id: report.id.clone(),
        root: report.root.clone(),
        created_at: now,
        expires_at: now + TTL,
        items,
        report,
        software_content: Some(binding),
        allocated_bytes: 0,
    })
}
#[cfg(test)]
mod tests;
