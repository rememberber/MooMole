//! User-selected Music/iTunes XML information export, never a live library DB.
//! Only explicit local File tracks become candidates. Export is not a backup.
use super::*;
use objc2::{rc::autoreleasepool, runtime::AnyObject};
use objc2_foundation::{
    NSData, NSDictionary, NSNumber, NSPropertyListReadOptions, NSPropertyListSerialization,
    NSString,
};
use std::{collections::HashMap, sync::atomic::Ordering};
#[cfg(test)]
mod tests;
mod xml;
const FAIL: &str = "Music 导出资料库结构、字段或容量无法核对，请重新导出 XML；原资料库和文件保留。";
pub(super) fn manifest_allowed(path: &Path, home: &Path) -> bool {
    path.starts_with(home)
        && path != home
        && ordinary(path)
        && path
            .extension()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case("xml"))
}
fn get(d: &NSDictionary, key: &str) -> Option<objc2::rc::Retained<AnyObject>> {
    d.objectForKey(&NSString::from_str(key))
}
fn text(d: &NSDictionary, key: &str) -> Option<String> {
    let v = get(d, key)?;
    let s = v.downcast_ref::<NSString>()?;
    if s.length() > 4096 {
        return None;
    }
    let s = s.to_string();
    (s.len() <= 4096 && !s.chars().any(char::is_control)).then_some(s)
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
fn false_or_absent(d: &NSDictionary, key: &str) -> bool {
    use core_foundation_sys::{base::CFGetTypeID, number::CFBooleanGetTypeID};
    let Some(v) = get(d, key) else { return true };
    let p = &*v as *const AnyObject as *const std::ffi::c_void;
    (unsafe { CFGetTypeID(p) == CFBooleanGetTypeID() })
        && v.downcast_ref::<NSNumber>().is_some_and(|n| !n.boolValue())
}
struct Candidate {
    path: PathBuf,
    bytes: u64,
}
fn local_path(d: &NSDictionary) -> Option<PathBuf> {
    let raw = text(d, "Location")?;
    let url = reqwest::Url::parse(&raw).ok()?;
    if url.scheme() != "file"
        || url.query().is_some()
        || url.fragment().is_some()
        || url
            .host_str()
            .is_some_and(|h| !h.is_empty() && h != "localhost")
    {
        return None;
    }
    url.to_file_path().ok()
}
fn candidate(id: &str, d: &NSDictionary) -> Option<Candidate> {
    let n = integer(d, "Track ID")?;
    if n == 0
        || n.to_string() != id
        || text(d, "Track Type")?.as_str() != "File"
        || !false_or_absent(d, "Protected")
        || !false_or_absent(d, "Apple Music")
    {
        return None;
    }
    Some(Candidate {
        path: local_path(d)?,
        bytes: integer(d, "Size")?,
    })
}
pub(super) fn collect(
    home: &Path,
    manifest: &Path,
    rules: &[String],
    job: Arc<Job>,
) -> Result<Scan, String> {
    if !ordinary(home) || !manifest_allowed(manifest, home) || protected(manifest, rules)? {
        return Err("请选择本账户普通目录中由 Music 导出的 XML 资料库。".into());
    }
    let started = Instant::now();
    let root = AnchoredPath::open(home)?;
    let m = root.target().metadata().map_err(|e| e.to_string())?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || !root.local_materialized() {
        return Err("账户目录不属于当前用户或本地状态未知。".into());
    }
    let read = chromium::read(manifest, 8 * 1024 * 1024, &job, started)?;
    if read
        .held
        .target()
        .metadata()
        .map_err(|e| e.to_string())?
        .nlink()
        != 1
    {
        return Err(FAIL.into());
    }
    xml::validate(&read.bytes, &job, started)?;
    let (candidates, visited, mut skipped, mut complete) =
        autoreleasepool(|_| -> Result<_, String> {
            let data = NSData::with_bytes(&read.bytes);
            let plist = unsafe {
                NSPropertyListSerialization::propertyListWithData_options_format_error(
                    &data,
                    NSPropertyListReadOptions::Immutable,
                    std::ptr::null_mut(),
                )
            }
            .map_err(|_| FAIL)?;
            let d = plist.downcast_ref::<NSDictionary>().ok_or(FAIL)?;
            if integer(d, "Major Version") != Some(1)
                || integer(d, "Minor Version") != Some(1)
                || text(d, "Application Version").is_none_or(|s| s.is_empty() || s.len() > 128)
            {
                return Err(FAIL.into());
            }
            let value = get(d, "Tracks").ok_or(FAIL)?;
            let tracks = value.downcast_ref::<NSDictionary>().ok_or(FAIL)?;
            if tracks.count() > 2000 {
                return Err(
                    "Music 导出资料库超过本轮 2000 条曲目上限，请导出较小的播放列表。".into(),
                );
            }
            let mut candidates = Vec::new();
            let mut visited = 0;
            let mut skipped = 0;
            let mut complete = true;
            let mut paths = HashMap::<PathBuf, usize>::new();
            for key in tracks.allKeys().iter() {
                if job.cancelled.load(Ordering::SeqCst)
                    || started.elapsed() > Duration::from_secs(10)
                {
                    return Err("来源记录读取已停止，未确认的范围保留。".into());
                }
                visited += 1;
                let c = (|| {
                    let v = tracks.objectForKey(&key)?;
                    let record = v.downcast_ref::<NSDictionary>()?;
                    if let Some(path) = local_path(record) {
                        *paths.entry(path).or_default() += 1;
                    }
                    let id = key.downcast_ref::<NSString>()?;
                    if id.length() > 20 {
                        return None;
                    }
                    candidate(&id.to_string(), record)
                })();
                if let Some(c) = c {
                    candidates.push(c);
                } else {
                    skipped += 1;
                    complete = false;
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
        })?;
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    let mut size = 0;
    for c in candidates {
        if job.cancelled.load(Ordering::SeqCst) || started.elapsed() > Duration::from_secs(10) {
            return Err("来源记录读取已停止，未确认的范围保留。".into());
        }
        let observed = (|| -> Result<Item, String> {
            if !c.path.starts_with(home)
                || c.path == home
                || !ordinary(&c.path)
                || !playlist::media(&c.path)
                || c.path == manifest
                || protected(&c.path, rules)?
            {
                return Err(FAIL.into());
            }
            let held = AnchoredPath::open(&c.path)?;
            let n = held.target().metadata().map_err(|e| e.to_string())?;
            if !n.is_file()
                || n.uid() != unsafe { libc::geteuid() }
                || n.nlink() != 1
                || n.dev() != m.dev()
                || n.len() != c.bytes
                || !held.local_materialized()
                || !seen.insert((n.dev(), n.ino()))
            {
                return Err(FAIL.into());
            }
            Ok(Item {
                id: uuid::Uuid::new_v4().to_string(),
                path: c.path.to_str().ok_or(FAIL)?.into(),
                size: n.len(),
                modified_at: n.mtime().saturating_mul(1000),
                witness: held.witness,
            })
        })();
        match observed {
            Ok(item) => {
                let length = serde_json::to_vec(&item).map_err(|_| FAIL)?.len();
                if size + length > 7 * 1024 * 1024 {
                    complete = false;
                    break;
                }
                size += length;
                items.push(item);
            }
            Err(_) => {
                skipped += 1;
                complete = false;
            }
        }
    }
    read.held.validate()?;
    root.validate()?;
    let now = db::now();
    let binding = Binding {
        source: Source::MusicLibrary,
        home: home.to_str().ok_or(FAIL)?.into(),
        directory: root.witness,
        allocated_at_scan: 0,
        history: None,
        browser_profile: None,
        playlist: Some(read.held.witness),
        telegram_export: None,
    };
    let report = Report {
        id: uuid::Uuid::new_v4().to_string(),
        root: binding.home.clone(),
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
            source: Source::MusicLibrary,
            minimum_age_days: 0,
            minimum_directory_bytes: 0,
            minimum_size_met: true,
            execution_supported: true,
            playlist_path: Some(manifest.to_str().ok_or(FAIL)?.into()),
            export_path: None,
            browser_profile: None,
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
