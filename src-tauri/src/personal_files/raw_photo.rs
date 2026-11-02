//! RAW images decoded in a supervised process from an anonymous, bounded copy.
//! Only an inherited descriptor crosses the process boundary, never a user path.
use super::*;
use crate::{
    db,
    engine::{Job, TaskKind},
    observation::AnchoredPath,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    sync::{atomic::Ordering, Arc},
};
pub(crate) mod commands;
pub(crate) mod worker;
pub(super) const FILE_LIMIT: u64 = 256 * 1024 * 1024;
pub(super) const FD: i32 = 198;
pub(super) fn supported(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        [
            "dng", "cr2", "cr3", "crw", "nef", "nrw", "arw", "sr2", "srf", "raf", "rw2", "orf",
            "pef", "srw", "3fr", "fff", "iiq", "rwl", "raw", "erf", "mrw", "mos", "mef", "kdc",
            "dcr",
        ]
        .contains(&s.to_ascii_lowercase().as_str())
    })
}
fn check(job: &Job, began: Instant) -> Result<(), String> {
    if job.cancelled.load(Ordering::SeqCst) || began.elapsed() >= Duration::from_secs(120) {
        Err("RAW 读取已停止，原文件保留。".into())
    } else {
        Ok(())
    }
}
pub(super) fn content(
    item: &Item,
    rules: &[String],
    job: &Job,
    began: Instant,
    mut copy: Option<&mut File>,
    count: &mut u64,
) -> Result<String, String> {
    check(job, began)?;
    let limit = if supported(Path::new(&item.path)) {
        FILE_LIMIT
    } else {
        32 * 1024 * 1024
    };
    if item.size == 0 || item.size > limit || protected(Path::new(&item.path), rules)? {
        return Err("RAW 或照片已保护、未落地或超过读取范围，原件保留。".into());
    }
    let h = AnchoredPath::open(Path::new(&item.path))?;
    if h.witness != item.witness || !h.local_materialized() {
        return Err("RAW 或照片身份已变化，请重新扫描。".into());
    }
    let mut input = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(&item.path)
        .map_err(|e| e.to_string())?;
    let m = input.metadata().map_err(|e| e.to_string())?;
    let node = h.witness.nodes.last().ok_or("照片身份缺失。")?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || m.len() != item.size
        || (m.dev(), m.ino()) != (node.dev, node.ino)
    {
        return Err("照片不是当前账户的普通单链接文件。".into());
    }
    if let Some(out) = copy.as_ref() {
        let mut v = std::mem::MaybeUninit::<libc::statfs>::zeroed();
        if unsafe { libc::fstatfs(out.as_raw_fd(), v.as_mut_ptr()) } != 0 {
            return Err("无法核对 RAW 临时副本空间。".into());
        }
        let v = unsafe { v.assume_init() };
        if (v.f_bavail as u64).saturating_mul(v.f_bsize as u64)
            < item.size.saturating_add(64 * 1024 * 1024)
        {
            return Err("RAW 临时副本空间不足，原件保留。".into());
        }
    }
    let mut hash = Sha256::new();
    let mut b = [0; 128 * 1024];
    let mut read = 0u64;
    loop {
        check(job, began)?;
        let n = input.read(&mut b).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        read += n as u64;
        *count = count.saturating_add(n as u64);
        if read > item.size {
            return Err("照片读取期间变大，请重新扫描。".into());
        }
        hash.update(&b[..n]);
        if let Some(out) = copy.as_mut() {
            out.write_all(&b[..n]).map_err(|e| e.to_string())?;
        }
    }
    h.validate()?;
    if read != item.size || AnchoredPath::open(Path::new(&item.path))?.witness != item.witness {
        return Err("照片读取期间已变化，请重新扫描。".into());
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub(super) fn png_bytes(png: &str) -> Result<Vec<u8>, String> {
    use objc2::AnyThread;
    use objc2_foundation::{NSData, NSDataBase64DecodingOptions, NSString};
    if png.len() > 5_592_432 {
        return Err("RAW 预览输出超限。".into());
    }
    let data = png
        .strip_prefix("data:image/png;base64,")
        .ok_or("RAW 预览格式无法核对。")?;
    NSData::initWithBase64EncodedString_options(
        NSData::alloc(),
        &NSString::from_str(data),
        NSDataBase64DecodingOptions::empty(),
    )
    .map(|d| d.to_vec())
    .ok_or("RAW 预览格式无法核对。".into())
}
fn executable() -> Result<std::path::PathBuf, String> {
    #[cfg(test)]
    if let Some(p) = std::env::var_os("MOOMOLE_RAW_WORKER_PATH") {
        return Ok(p.into());
    }
    std::env::current_exe().map_err(|e| e.to_string())
}
pub(super) fn analyze(
    item: &Item,
    rules: &[String],
    job: Arc<Job>,
    began: Instant,
    max: u32,
    count: &mut u64,
) -> Result<worker::Output, String> {
    if !supported(Path::new(&item.path)) || ![384, 1024].contains(&max) {
        return Err("RAW 预览请求格式无法核对。".into());
    }
    let mut snapshot = tempfile::tempfile().map_err(|e| e.to_string())?;
    let hash = content(item, rules, &job, began, Some(&mut snapshot), count)?;
    snapshot.rewind().map_err(|e| e.to_string())?;
    let m = snapshot.metadata().map_err(|e| e.to_string())?;
    if m.nlink() != 0 {
        return Err("RAW 临时副本未匿名，未交给解码进程。".into());
    }
    let high = unsafe { libc::fcntl(snapshot.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 200) };
    if high < 0 {
        return Err("无法准备 RAW 解码句柄。".into());
    }
    let inherited = unsafe { File::from_raw_fd(high) };
    let request = worker::Input {
        id: uuid::Uuid::new_v4().to_string(),
        sha256: hash.clone(),
        bytes: item.size,
        device: m.dev().to_string(),
        inode: m.ino().to_string(),
        max,
        extension: Path::new(&item.path)
            .extension()
            .and_then(|s| s.to_str())
            .ok_or("RAW 格式提示缺失。")?
            .to_ascii_lowercase(),
    };
    let mut command = tokio::process::Command::new(executable()?);
    command
        .arg("--moomole-raw-worker-v1")
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .current_dir("/");
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(high, FD) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    check(&job, began)?;
    let remaining = Duration::from_secs(120)
        .saturating_sub(began.elapsed())
        .min(Duration::from_secs(15));
    let bytes = tauri::async_runtime::block_on(crate::process::output_with_input(
        command,
        job.clone(),
        serde_json::to_vec(&request).map_err(|e| e.to_string())?,
        remaining,
        6 * 1024 * 1024,
    ))?;
    drop(inherited);
    let output: worker::Output =
        serde_json::from_slice(&bytes).map_err(|_| "RAW 解码结果无法核对。")?;
    output.validate(&request)?;
    check(&job, began)?;
    let png = media::Raster::decode(&png_bytes(&output.png)?, max)?;
    if png.width > max as u64 || png.height > max as u64 || png.frames != 1 {
        return Err("RAW 预览像素无法核对。".into());
    }
    if AnchoredPath::open(Path::new(&item.path))?.witness != item.witness
        || protected(Path::new(&item.path), rules)?
    {
        return Err("RAW 预览期间文件或保护范围变化。".into());
    }
    Ok(output)
}
#[cfg(test)]
mod tests;
