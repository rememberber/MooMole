//! VLC's exported UTF-8 M3U8 is a manifest, never a script or a removal policy.
//! Local media only; no network requests, HLS segments or player DB writes.
use super::*;
use std::sync::atomic::Ordering;

pub(super) fn media(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "mp3"
                | "m4a"
                | "aac"
                | "wav"
                | "flac"
                | "aiff"
                | "aif"
                | "ogg"
                | "opus"
                | "mp4"
                | "m4v"
                | "mov"
                | "mkv"
                | "avi"
                | "webm"
                | "mpg"
                | "mpeg"
        )
    })
}
pub(super) fn manifest_allowed(path: &Path, home: &Path) -> bool {
    path.starts_with(home)
        && path != home
        && ordinary(path)
        && path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("m3u8"))
}
fn local_path(line: &str, base: &reqwest::Url) -> Option<PathBuf> {
    if line.starts_with('/') && !line.starts_with("//") {
        return Some(PathBuf::from(line));
    }
    let url = base.join(line).ok()?;
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
pub(super) fn collect(
    home: &Path,
    manifest: &Path,
    rules: &[String],
    job: Arc<Job>,
) -> Result<Scan, String> {
    if !ordinary(home) || !manifest_allowed(manifest, home) || protected(manifest, rules)? {
        return Err(
            "请选择本账户普通目录中的 M3U8 文件；播放列表不可位于保护、应用资料库或隐藏位置。"
                .into(),
        );
    }
    let started = Instant::now();
    let root = AnchoredPath::open(home)?;
    let meta = root.target().metadata().map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || !root.local_materialized() {
        return Err("账户目录不属于当前用户或本地状态未知。".into());
    }
    let read = chromium::read(manifest, 2 * 1024 * 1024, &job, started)?;
    let text = std::str::from_utf8(&read.bytes)
        .map_err(|_| "播放列表不是支持的 UTF-8 编码，未推断本地文件。")?
        .trim_start_matches('\u{feff}');
    if text.lines().find(|s| !s.trim().is_empty()).map(str::trim) != Some("#EXTM3U")
        || text.lines().any(|s| s.trim_start().starts_with("#EXT-X-"))
    {
        return Err("请选择 VLC 导出的本地 M3U8 播放列表；未知格式与 HLS 流媒体列表保留。".into());
    }
    let base = reqwest::Url::from_file_path(manifest).map_err(|_| "播放列表路径无法编码。")?;
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    let mut visited = 0;
    let mut skipped = 0;
    let mut complete = true;
    let mut size = 0usize;
    for (number, line) in text.lines().enumerate() {
        if job.cancelled.load(Ordering::SeqCst)
            || started.elapsed() > Duration::from_secs(10)
            || visited >= 2000
            || number >= 10_000
        {
            complete = false;
            break;
        }
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        visited += 1;
        let candidate = (|| -> Result<Item, String> {
            if line.len() > 4096 || line.chars().any(char::is_control) {
                return Err("播放列表条目过长或格式未知。".into());
            }
            let path = local_path(line, &base).ok_or("不是支持的本地文件地址。")?;
            if !path.starts_with(home)
                || path == home
                || !ordinary(&path)
                || !media(&path)
                || protected(&path, rules)?
            {
                return Err("不是本账户已支持的普通影音文件。".into());
            }
            let held = AnchoredPath::open(&path)?;
            let m = held.target().metadata().map_err(|e| e.to_string())?;
            if !m.is_file()
                || m.uid() != unsafe { libc::geteuid() }
                || m.dev() != meta.dev()
                || !held.local_materialized()
                || m.len() > 9_007_199_254_740_991
                || !seen.insert((m.dev(), m.ino()))
            {
                return Err("影音文件重复、非本地或状态无法核对。".into());
            }
            Ok(Item {
                id: uuid::Uuid::new_v4().to_string(),
                path: path.to_str().ok_or("影音路径无法编码。")?.into(),
                size: m.len(),
                modified_at: m.mtime().saturating_mul(1000),
                witness: held.witness,
            })
        })();
        match candidate {
            Ok(item) => {
                let n = serde_json::to_vec(&item).map_err(|e| e.to_string())?.len();
                if size + n > 7 * 1024 * 1024 {
                    complete = false;
                    break;
                }
                size += n;
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
        source: Source::VlcPlaylist,
        home: home.to_str().ok_or("账户目录无法编码。")?.into(),
        directory: root.witness,
        allocated_at_scan: 0,
        history: None,
        playlist: Some(read.held.witness),
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
        duplicates: None,
        similar_photos: None,
        software_content: Some(Info {
            source: Source::VlcPlaylist,
            minimum_age_days: 0,
            minimum_directory_bytes: 0,
            minimum_size_met: true,
            execution_supported: true,
            playlist_path: Some(manifest.to_str().ok_or("播放列表路径无法编码。")?.into()),
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
