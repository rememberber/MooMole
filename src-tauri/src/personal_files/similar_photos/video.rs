//! Visual video candidates from three ordered sample frames. No audio, subtitle
//! or full-sequence equality claim; final mutation binds both full file hashes.
use super::*;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
};
pub(crate) mod worker;
pub(super) const METHOD: &str = "videoFramesVisionV2";
pub(in crate::personal_files) const REVISION: &str = "MooMole.similar-videos.v1";
pub(super) const FILE_LIMIT: u64 = 512 * 1024 * 1024;
const INPUT_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Frame {
    pub at_ms: u64,
    pub thumbnail: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Video {
    pub id: String,
    pub duration_ms: u64,
    pub frames: Vec<Frame>,
}
pub(super) fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| ["mp4", "m4v", "mov"].contains(&s.to_ascii_lowercase().as_str()))
}
pub(in crate::personal_files) fn content(
    item: &Item,
    rules: &[String],
    job: &Job,
    began: Instant,
    mut copy: Option<&mut File>,
    read_count: &mut u64,
) -> Result<String, String> {
    if item.size == 0
        || item.size > FILE_LIMIT
        || !supported(Path::new(&item.path))
        || protected(Path::new(&item.path), rules)?
    {
        return Err("视频未落地、已保护或超过本轮读取范围，原文件保留。".into());
    }
    let held = AnchoredPath::open(Path::new(&item.path))?;
    if held.witness != item.witness || !held.local_materialized() {
        return Err("视频身份或本地状态已变化，请重新扫描。".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(&item.path)
        .map_err(|e| e.to_string())?;
    let m = file.metadata().map_err(|e| e.to_string())?;
    let node = held.witness.nodes.last().ok_or("缺少视频身份。")?;
    if !m.is_file()
        || m.nlink() != 1
        || m.uid() != unsafe { libc::geteuid() }
        || (m.dev(), m.ino()) != (node.dev, node.ino)
    {
        return Err("视频不是当前账户的普通单链接本地文件。".into());
    }
    if let Some(dest) = copy.as_ref() {
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if unsafe { libc::fstatvfs(dest.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
            return Err("无法核对视频临时副本的可用空间。".into());
        }
        let stat = unsafe { stat.assume_init() };
        if (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64)
            < item.size.saturating_add(64 * 1024 * 1024)
        {
            return Err("视频临时副本空间不足，原件保持不变。".into());
        }
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];
    let mut read = 0u64;
    loop {
        if job.cancelled.load(Ordering::SeqCst) || began.elapsed() >= Duration::from_secs(120) {
            return Err("视频内容核对已停止，未确认的文件保留。".into());
        }
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        read += n as u64;
        *read_count = read_count.saturating_add(n as u64);
        if read > item.size {
            return Err("视频在读取期间变大，请重新扫描。".into());
        }
        hash.update(&buffer[..n]);
        if let Some(dest) = copy.as_mut() {
            dest.write_all(&buffer[..n])
                .map_err(|_| "视频临时副本未写完，原件保持不变。")?;
        }
    }
    held.validate()?;
    if read != item.size || AnchoredPath::open(Path::new(&item.path))?.witness != item.witness {
        return Err("视频在读取期间已变化，请重新扫描。".into());
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn png_bytes(png: &str) -> Result<Vec<u8>, String> {
    use objc2::AnyThread;
    use objc2_foundation::{NSData, NSDataBase64DecodingOptions, NSString};
    if png.len() > 800_000 {
        return Err("视频采样输出超过本轮范围。".into());
    }
    let text = png
        .strip_prefix("data:image/png;base64,")
        .ok_or("视频采样格式无法核对。")?;
    NSData::initWithBase64EncodedString_options(
        NSData::alloc(),
        &NSString::from_str(text),
        NSDataBase64DecodingOptions::empty(),
    )
    .map(|d| d.to_vec())
    .ok_or("视频采样格式无法核对。".into())
}
fn analyze(
    item: &Item,
    rules: &[String],
    job: Arc<Job>,
    began: Instant,
    read_count: &mut u64,
) -> Result<(Image, Video, Vec<vision::Feature>), String> {
    let scratch = tempfile::Builder::new()
        .prefix("MooMole-video-snapshot-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let path = scratch
        .path()
        .canonicalize()
        .map_err(|e| e.to_string())?
        .join("video.mp4");
    let mut file = OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let digest = content(item, rules, &job, began, Some(&mut file), read_count)?;
    file.flush().map_err(|e| e.to_string())?;
    preview::stream::containers::self_contained(&file, item.size)?;
    let mut command = tokio::process::Command::new(worker_executable()?);
    command
        .arg("--moomole-video-worker-v1")
        .current_dir(scratch.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    let remaining = Duration::from_secs(120)
        .saturating_sub(began.elapsed())
        .min(Duration::from_secs(15));
    if remaining.is_zero() || job.cancelled.load(Ordering::SeqCst) {
        return Err("视频内容核对已停止，未确认的文件保留。".into());
    }
    let bytes = tauri::async_runtime::block_on(crate::process::output_with_input(
        command,
        job.clone(),
        serde_json::to_vec(&worker::Input {
            path: path.to_str().ok_or("视频临时路径无法编码。")?.into(),
        })
        .map_err(|e| e.to_string())?,
        remaining,
        2 * 1024 * 1024,
    ))?;
    let output: worker::Output =
        serde_json::from_slice(&bytes).map_err(|_| "视频采样返回格式无法核对。")?;
    output.validate()?;
    if job.cancelled.load(Ordering::SeqCst) {
        return Err("视频内容核对已停止，未确认的文件保留。".into());
    }
    let mut features = vec![];
    let mut frames = vec![];
    for frame in &output.frames {
        if began.elapsed() >= Duration::from_secs(120) || job.cancelled.load(Ordering::SeqCst) {
            return Err("视频内容核对已停止，未确认的文件保留。".into());
        }
        let bytes = png_bytes(&frame.thumbnail)?;
        let raster = media::Raster::decode(&bytes, 160)?;
        if (raster.width, raster.height) != (output.width, output.height) {
            return Err("视频采样帧与返回尺寸不匹配。".into());
        }
        features.push(vision::Feature::new(&bytes)?);
        frames.push(Frame {
            at_ms: frame.at_ms,
            thumbnail: raster.png()?,
        });
    }
    if AnchoredPath::open(Path::new(&item.path))?.witness != item.witness
        || protected(Path::new(&item.path), rules)?
    {
        return Err("视频身份或本地状态已变化，请重新扫描。".into());
    }
    let image = Image {
        id: item.id.clone(),
        width: output.width,
        height: output.height,
        thumbnail: frames[1].thumbnail.clone(),
        captured_at: None,
        raw_type: None,
        digest,
        capture: None,
        parent: Path::new(&item.path)
            .parent()
            .ok_or("视频缺少父目录。")?
            .into(),
    };
    Ok((
        image,
        Video {
            id: item.id.clone(),
            duration_ms: output.duration_ms,
            frames,
        },
        features,
    ))
}
fn worker_executable() -> Result<PathBuf, String> {
    #[cfg(test)]
    if let Some(path) = std::env::var_os("MOOMOLE_VIDEO_WORKER_PATH") {
        return Ok(PathBuf::from(path));
    }
    std::env::current_exe().map_err(|_| "无法定位本地视频采样程序。".into())
}
fn geometry(a: &Image, b: &Image) -> bool {
    let x = (a.width * b.height) as f64;
    let y = (b.width * a.height) as f64;
    (x - y).abs() <= x.max(y) * 0.02
}
fn duration(a: u64, b: u64) -> bool {
    a.abs_diff(b) <= (a.min(b) / 100).clamp(100, 1000)
}
pub(in crate::personal_files) fn collect(
    mut scan: Scan,
    job: Arc<Job>,
    rules: &[String],
) -> Result<Scan, String> {
    let began = Instant::now();
    let mut videos = vec![];
    let mut bytes_read = 0u64;
    let mut reserved_read = 0u64;
    let mut thumbs = 0usize;
    let mut attempted = 0;
    scan.items.sort_by(|a, b| a.path.cmp(&b.path));
    for item in &scan.items {
        if !supported(Path::new(&item.path)) {
            continue;
        }
        if item.size > FILE_LIMIT {
            scan.report.skipped += 1;
            scan.report.complete = false;
            continue;
        }
        if began.elapsed() >= Duration::from_secs(120)
            || job.cancelled.load(Ordering::SeqCst)
            || attempted >= 40
            || reserved_read.saturating_add(item.size) > INPUT_LIMIT
        {
            scan.report.complete = false;
            break;
        }
        attempted += 1;
        reserved_read = reserved_read.saturating_add(item.size);
        match objc2::rc::autoreleasepool(|_| {
            analyze(item, rules, job.clone(), began, &mut bytes_read)
        }) {
            Ok(value) => {
                let size = value
                    .1
                    .frames
                    .iter()
                    .map(|f| f.thumbnail.len())
                    .sum::<usize>()
                    + value.0.thumbnail.len();
                if thumbs + size > 6 * 1024 * 1024 {
                    scan.report.complete = false;
                    break;
                }
                thumbs += size;
                videos.push(value);
            }
            Err(_) => {
                scan.report.skipped += 1;
                scan.report.complete = false;
            }
        }
    }
    let checked_files = videos.len();
    let mut used = HashSet::new();
    let mut groups = vec![];
    for i in 0..videos.len() {
        if used.contains(&i) {
            continue;
        }
        let mut members = vec![i];
        for j in i + 1..videos.len() {
            if began.elapsed() >= Duration::from_secs(120) || job.cancelled.load(Ordering::SeqCst) {
                scan.report.complete = false;
                break;
            }
            if used.contains(&j)
                || !duration(videos[i].1.duration_ms, videos[j].1.duration_ms)
                || !geometry(&videos[i].0, &videos[j].0)
            {
                continue;
            }
            let distances = videos[i]
                .2
                .iter()
                .zip(&videos[j].2)
                .map(|(a, b)| a.distance(b))
                .collect::<Result<Vec<_>, _>>();
            match distances {
                Ok(d) if d.len() == 3 && d.iter().all(|d| *d <= 0.12) => members.push(j),
                Err(_) => scan.report.complete = false,
                _ => {}
            }
        }
        if job.cancelled.load(Ordering::SeqCst) {
            scan.report.complete = false;
            break;
        }
        if members.len() > 1 {
            groups.push(Group {
                id: uuid::Uuid::new_v4().to_string(),
                item_ids: members.iter().map(|n| videos[*n].0.id.clone()).collect(),
                capture_span_ms: None,
            });
            used.extend(members);
        }
    }
    let ids: HashSet<_> = groups
        .iter()
        .flat_map(|g| g.item_ids.iter().cloned())
        .collect();
    scan.items.retain(|i| ids.contains(&i.id));
    scan.report.items = scan.items.iter().map(row).collect();
    scan.report.cancelled = job.cancelled.load(Ordering::SeqCst);
    let (images, details): (Vec<_>, Vec<_>) = videos
        .into_iter()
        .filter(|v| ids.contains(&v.0.id))
        .map(|(image, video, _)| (image, video))
        .unzip();
    scan.report.similar_photos = Some(Report {
        groups,
        images,
        checked_files,
        bytes_read,
        method: METHOD,
        sequence: None,
        videos: Some(details),
    });
    Ok(scan)
}
#[cfg(test)]
mod tests;
