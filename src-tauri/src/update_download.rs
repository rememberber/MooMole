//! Only receives assets selected by the fixed update feed, never IPC URLs/paths.
use crate::updates::Asset;
use reqwest::{redirect::Policy, Client, StatusCode, Url};
use sha2::{Digest, Sha512};
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{io::AsyncWriteExt, sync::Notify};

#[path = "update_cache.rs"]
mod maintenance;

#[derive(Default)]
pub struct Control {
    cancelled: AtomicBool,
    wake: Notify,
}
impl Control {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.wake.notify_one();
    }
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
    pub async fn wait(&self) {
        while !self.cancelled() {
            self.wake.notified().await;
        }
    }
}

pub fn allowed_redirect(url: &Url, initial: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.fragment().is_none()
        && ((url.host_str() == Some("github.com")
            && url.path() == initial.path()
            && url.query().is_none())
            || (url.host_str() == Some("release-assets.githubusercontent.com")
                && url.path().starts_with("/github-production-release-asset/")))
}

fn client(asset: &Asset) -> Result<Client, String> {
    let initial = Url::parse(&asset.url).map_err(|_| "更新地址无效。")?;
    Client::builder()
        .https_only(true)
        .redirect(Policy::custom(move |attempt| {
            if attempt.previous().len() <= 3 && allowed_redirect(attempt.url(), &initial) {
                attempt.follow()
            } else {
                attempt.error("Update redirect refused")
            }
        }))
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(30 * 60))
        .user_agent(concat!("MooMole/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "无法初始化下载连接。".into())
}

pub struct Cache {
    directory: File,
    path: PathBuf,
}
fn open_directory(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| "更新缓存目录不可用。".into())
}
impl Cache {
    pub fn open(path: &Path) -> Result<Arc<Self>, String> {
        match std::fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err("无法创建更新缓存目录。".into()),
        }
        Self::existing(path)
    }
    fn existing(path: &Path) -> Result<Arc<Self>, String> {
        let directory = open_directory(path)?;
        let meta = directory.metadata().map_err(|_| "无法读取更新缓存目录。")?;
        if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
            return Err("更新缓存目录权限不符合要求。".into());
        }
        Ok(Arc::new(Self {
            directory,
            path: path.to_owned(),
        }))
    }
    fn attached(&self) -> Result<(), String> {
        let current = open_directory(&self.path)?
            .metadata()
            .map_err(|_| "缓存目录已变化。")?;
        let original = self.directory.metadata().map_err(|_| "缓存目录不可用。")?;
        if (current.dev(), current.ino()) != (original.dev(), original.ino()) {
            return Err("更新缓存目录已变化，请重新检查更新。".into());
        }
        Ok(())
    }
    fn open_file(&self, name: &str, create: bool) -> Result<File, String> {
        let name = CString::new(name).map_err(|_| "缓存文件名称无效。")?;
        let flags = libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | libc::O_CLOEXEC
            | if create {
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL
            } else {
                libc::O_RDONLY
            };
        let fd = unsafe { libc::openat(self.directory.as_raw_fd(), name.as_ptr(), flags, 0o600) };
        if fd < 0 {
            return Err("无法创建或读取更新缓存文件。".into());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    fn unlink(&self, name: &str) -> Result<(), String> {
        let name = CString::new(name).map_err(|_| "缓存文件名称无效。")?;
        if unsafe { libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err("临时更新文件清理失败，请检查更新缓存目录。".into());
        }
        Ok(())
    }
    fn budget(&self, size: u64) -> Result<(), String> {
        self.attached()?;
        let mut fs = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if unsafe { libc::fstatvfs(self.directory.as_raw_fd(), fs.as_mut_ptr()) } != 0 {
            return Err("无法确认下载所需空间。".into());
        }
        let fs = unsafe { fs.assume_init() };
        if (fs.f_bavail as u64).saturating_mul(fs.f_frsize as u64) < size + 16 * 1024 * 1024 {
            return Err("下载可用空间不足，请释放空间后重试。".into());
        }
        let mut count = 0;
        let mut bytes = 0u64;
        for entry in std::fs::read_dir(&self.path)
            .map_err(|_| "无法检查更新缓存。")?
            .take(32)
        {
            let entry = entry.map_err(|_| "无法检查更新缓存。")?;
            let meta = std::fs::symlink_metadata(entry.path()).map_err(|_| "无法检查更新缓存。")?;
            count += 1;
            bytes = bytes.saturating_add(meta.len());
        }
        if count >= 32 || bytes.saturating_add(size) > 2 * 1024 * 1024 * 1024 {
            return Err("更新缓存达到容量限制，可前往发布页手动下载。".into());
        }
        Ok(())
    }
}

struct Pending {
    cache: Arc<Cache>,
    name: String,
    armed: bool,
}
impl Pending {
    fn cleanup(&mut self) -> Result<(), String> {
        if self.armed {
            self.cache.unlink(&self.name)?;
            self.armed = false;
        }
        Ok(())
    }
}
impl Drop for Pending {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[derive(Clone)]
pub struct Ready {
    cache: Arc<Cache>,
    name: String,
    pub asset: Asset,
    identity: (u64, u64),
}
impl Ready {
    pub fn cache_name(&self) -> &str {
        &self.name
    }
    pub fn discard(&self) -> Result<(), String> {
        self.cache.unlink(&self.name)
    }
    pub fn verified_path(&self) -> Result<PathBuf, String> {
        self.cache.attached()?;
        let mut file = self.cache.open_file(&self.name, false)?;
        verify_file(&mut file, &self.asset, self.identity, None)?;
        crate::update_quarantine::present(&self.cache.path.join(&self.name))?;
        Ok(self.cache.path.join(&self.name))
    }
}

fn verify_file(
    file: &mut File,
    asset: &Asset,
    identity: (u64, u64),
    control: Option<&Control>,
) -> Result<(), String> {
    let before = file.metadata().map_err(|_| "无法读取更新文件。")?;
    if !before.is_file()
        || before.nlink() != 1
        || before.uid() != unsafe { libc::geteuid() }
        || before.mode() & 0o777 != 0o600
        || before.len() != asset.size
        || (before.dev(), before.ino()) != identity
    {
        return Err("更新文件已变化或权限无效，请重新下载。".into());
    }
    let mut hash = Sha512::new();
    let mut bytes = 0u64;
    let mut buffer = [0; 128 * 1024];
    loop {
        if control.is_some_and(Control::cancelled) {
            return Err("下载已取消。".into());
        }
        let n = file.read(&mut buffer).map_err(|_| "更新文件读取失败。")?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > asset.size {
            return Err("更新文件大小已变化。".into());
        }
        hash.update(&buffer[..n]);
    }
    let after = file.metadata().map_err(|_| "无法复核更新文件。")?;
    if bytes != asset.size
        || format!("{:x}", hash.finalize()) != asset.sha512
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err("更新文件校验失败，请重新下载。".into());
    }
    Ok(())
}

pub async fn download(
    asset: Asset,
    cache: Arc<Cache>,
    control: Arc<Control>,
    progress: impl Fn(u64, bool) + Send + Sync,
) -> Result<Ready, String> {
    let http = client(&asset)?;
    transfer(&http, &asset.url, asset.clone(), cache, control, progress).await
}

async fn transfer(
    http: &Client,
    url: &str,
    asset: Asset,
    cache: Arc<Cache>,
    control: Arc<Control>,
    progress: impl Fn(u64, bool) + Send + Sync,
) -> Result<Ready, String> {
    // No transfer file exists yet; this session exclusively owns downloads.
    let maintenance = cache.clone();
    let stop = control.clone();
    let _ = tokio::task::spawn_blocking(move || {
        maintenance.cleanup(None, std::time::SystemTime::now(), || stop.cancelled())
    })
    .await;
    if control.cancelled() {
        return Err("下载已取消。".into());
    }
    cache.budget(asset.size)?;
    let suffix = uuid::Uuid::new_v4();
    let mut pending = Pending {
        cache: cache.clone(),
        name: format!(".moomole-{suffix}.download"),
        armed: false,
    };
    let file = cache.open_file(&pending.name, true)?;
    pending.armed = true;
    let identity = {
        let m = file.metadata().map_err(|_| "无法检查临时文件。")?;
        (m.dev(), m.ino())
    };
    let mut file = tokio::fs::File::from_std(file);
    let work = async {
        let mut response = http
            .get(url)
            .header(reqwest::header::ACCEPT, "application/octet-stream")
            .send()
            .await
            .map_err(|_| "更新下载连接失败，请重试。")?;
        if response.status() != StatusCode::OK {
            return Err("更新下载失败，服务器未返回完整安装包。".to_string());
        }
        if response.content_length().is_some_and(|n| n != asset.size) {
            return Err("更新包声明大小不匹配。".into());
        }
        let mut size = 0u64;
        let mut hash = Sha512::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "更新下载中断，请重试。")?
        {
            if size.saturating_add(chunk.len() as u64) > asset.size {
                return Err("更新包超过预期大小。".into());
            }
            file.write_all(&chunk)
                .await
                .map_err(|_| "更新包写入失败，请检查可用空间。")?;
            size += chunk.len() as u64;
            hash.update(&chunk);
            progress(size, false);
        }
        if size != asset.size || format!("{:x}", hash.finalize()) != asset.sha512 {
            return Err("更新包大小或 SHA-512 校验失败。".into());
        }
        progress(size, true);
        file.sync_all().await.map_err(|_| "更新包保存失败。")?;
        Ok(())
    };
    let outcome = tokio::select! {biased; _=control.wait()=>Err("下载已取消。".into()), r=tokio::time::timeout(Duration::from_secs(30*60),work)=>r.unwrap_or_else(|_|Err("更新下载超时，请重试。".into()))};
    drop(file);
    if let Err(error) = outcome {
        pending.cleanup()?;
        return Err(error);
    }
    let c = cache.clone();
    let a = asset.clone();
    let name = pending.name.clone();
    let ctl = control.clone();
    let verification = tokio::task::spawn_blocking(move || {
        let mut file = c.open_file(&name, false)?;
        verify_file(&mut file, &a, identity, Some(&ctl))?;
        c.attached()?;
        crate::update_quarantine::mark(&c.path.join(&name), &a.url)?;
        file.sync_all()
            .map_err(|_| "无法保存下载隔离标记。".to_string())
    })
    .await
    .map_err(|_| "更新包复核未完成。".to_string())
    .and_then(|r| r);
    if let Err(error) = verification {
        pending.cleanup()?;
        return Err(error);
    }
    if control.cancelled() {
        pending.cleanup()?;
        return Err("下载已取消。".into());
    }
    if let Err(error) = cache.attached() {
        pending.cleanup()?;
        return Err(error);
    }
    let name = format!("{}-{suffix}.dmg", asset.file_name.trim_end_matches(".dmg"));
    let source = CString::new(pending.name.clone()).unwrap();
    let destination = CString::new(name.clone()).unwrap();
    if unsafe {
        libc::linkat(
            cache.directory.as_raw_fd(),
            source.as_ptr(),
            cache.directory.as_raw_fd(),
            destination.as_ptr(),
            0,
        )
    } != 0
    {
        pending.cleanup()?;
        return Err("更新包发布到缓存失败，未覆盖已有文件。".into());
    }
    if let Err(error) = pending.cleanup() {
        let _ = cache.unlink(&name);
        return Err(error);
    }
    let ready = Ready {
        cache,
        name,
        asset,
        identity,
    };
    if control.cancelled() {
        ready.cache.unlink(&ready.name)?;
        return Err("下载已取消。".into());
    }
    Ok(ready)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        os::unix::fs::{symlink, PermissionsExt},
        sync::Mutex,
    };
    fn asset(body: &[u8]) -> Asset {
        serde_json::from_value(serde_json::json!({"platform":"darwin","architecture":"arm64","packageType":"dmg","fileName":"MooMole-0.2.0-mac-arm64.dmg","url":"https://github.com/rememberber/MooMole/releases/download/v0.2.0/MooMole-0.2.0-mac-arm64.dmg","size":body.len(),"sha512":format!("{:x}",Sha512::digest(body))})).unwrap()
    }
    fn serve(body: Vec<u8>, header: String, stall: bool) -> (String, std::thread::JoinHandle<()>) {
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let url = format!("http://{}", socket.local_addr().unwrap());
        let task = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while std::time::Instant::now() < deadline {
                if let Ok((mut stream, _)) = socket.accept() {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(Duration::from_secs(10)))
                        .unwrap();
                    let mut req = Vec::new();
                    let mut buffer = [0; 4096];
                    while !req.ends_with(b"\r\n\r\n") && req.len() < 8192 {
                        match stream.read(&mut buffer) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => req.extend_from_slice(&buffer[..n]),
                        }
                    }
                    let _ = stream.write_all(header.as_bytes());
                    if stall {
                        std::thread::sleep(Duration::from_millis(300));
                    } else {
                        let _ = stream.write_all(&body);
                    }
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        (url, task)
    }
    fn http() -> Client {
        Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap()
    }
    async fn receive(
        body: Vec<u8>,
        a: Asset,
        cache: Arc<Cache>,
        control: Arc<Control>,
        header: Option<String>,
    ) -> Result<Ready, String> {
        let header = header.unwrap_or_else(|| {
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
        });
        let (url, task) = serve(body, header, false);
        let result = transfer(&http(), &url, a, cache, control, |_, _| {}).await;
        task.join().unwrap();
        result
    }
    pub(crate) async fn installation_fixture() -> (tempfile::TempDir, Ready) {
        let temp = tempfile::tempdir().unwrap();
        let cache = Cache::open(&temp.path().join("cache")).unwrap();
        let body = b"isolated installer fixture".to_vec();
        let ready = receive(
            body.clone(),
            asset(&body),
            cache,
            Arc::new(Control::default()),
            None,
        )
        .await
        .unwrap();
        (temp, ready)
    }
    #[tokio::test]
    async fn download_roundtrip_retains_private_permissions_quarantine_and_rechecks_changed_file() {
        let temp = tempfile::tempdir().unwrap();
        let cache = Cache::open(&temp.path().join("cache")).unwrap();
        let body = b"isolated downloaded file".repeat(10000);
        let a = asset(&body);
        let progress = Mutex::new(Vec::new());
        let (url, task) = serve(
            body.clone(),
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            ),
            false,
        );
        let ready = transfer(
            &http(),
            &url,
            a,
            cache.clone(),
            Arc::new(Control::default()),
            |n, v| progress.lock().unwrap().push((n, v)),
        )
        .await
        .unwrap();
        task.join().unwrap();
        let path = ready.verified_path().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), body);
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            progress.lock().unwrap().last(),
            Some(&(body.len() as u64, true))
        );
        assert_eq!(std::fs::read_dir(&cache.path).unwrap().count(), 1);
        let mut changed = body.clone();
        changed[0] ^= 1;
        std::fs::write(&path, changed).unwrap();
        assert!(ready.verified_path().is_err());
        std::fs::write(&path, &body).unwrap();
        assert!(ready.verified_path().is_ok());
        let other = temp.path().join("other");
        std::fs::rename(&path, &other).unwrap();
        symlink(&other, &path).unwrap();
        assert!(ready.verified_path().is_err());
    }
    #[tokio::test]
    async fn mismatches_truncation_excess_and_status_errors_never_publish_and_preserve_existing_files(
    ) {
        let temp = tempfile::tempdir().unwrap();
        let cache = Cache::open(&temp.path().join("cache")).unwrap();
        let body = b"expected".to_vec();
        let a = asset(&body);
        let keep = cache.path.join("keep.dmg");
        std::fs::write(&keep, b"keep").unwrap();
        for (payload, header) in [
            (b"bad hash".to_vec(), None),
            (
                b"short".to_vec(),
                Some("HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\n".into()),
            ),
            (
                body.clone(),
                Some("HTTP/1.1 206 Partial Content\r\nContent-Length: 8\r\n\r\n".into()),
            ),
            (
                vec![],
                Some("HTTP/1.1 503 Error\r\nContent-Length: 0\r\n\r\n".into()),
            ),
            (
                b"9\r\nexcessive\r\n0\r\n\r\n".to_vec(),
                Some("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".into()),
            ),
        ] {
            assert!(receive(
                payload,
                a.clone(),
                cache.clone(),
                Arc::new(Control::default()),
                header
            )
            .await
            .is_err());
            assert_eq!(std::fs::read_dir(&cache.path).unwrap().count(), 1);
            assert_eq!(std::fs::read(&keep).unwrap(), b"keep");
        }
    }
    #[tokio::test]
    async fn cancellation_interrupts_a_stalled_body_and_after_verification_leaves_no_ready_file() {
        let temp = tempfile::tempdir().unwrap();
        let cache = Cache::open(&temp.path().join("cache")).unwrap();
        let a = asset(b"body");
        let control = Arc::new(Control::default());
        let cancel = control.clone();
        let (url, server) = serve(
            vec![],
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n".into(),
            true,
        );
        let task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            cancel.cancel();
        });
        let result = transfer(&http(), &url, a.clone(), cache.clone(), control, |_, _| {}).await;
        assert!(result.is_err());
        task.await.unwrap();
        server.join().unwrap();
        assert_eq!(std::fs::read_dir(&cache.path).unwrap().count(), 0);
        let control = Arc::new(Control::default());
        let cancel = control.clone();
        let (url, server) = serve(
            b"body".to_vec(),
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n".into(),
            false,
        );
        assert!(
            transfer(&http(), &url, a, cache.clone(), control, |_, verifying| {
                if verifying {
                    cancel.cancel()
                }
            })
            .await
            .is_err()
        );
        server.join().unwrap();
        assert_eq!(std::fs::read_dir(&cache.path).unwrap().count(), 0);
    }
    #[tokio::test]
    #[ignore = "Explicit transfer of a local validation DMG; never a published update"]
    async fn local_validation_dmg_transfer() {
        let input =
            std::path::PathBuf::from(std::env::var("MOOMOLE_TEST_DMG").expect("local DMG path"));
        assert!(std::fs::metadata(&input).unwrap().len() < 64 * 1024 * 1024);
        let body = std::fs::read(&input).unwrap();
        let mut a = asset(&body);
        a.file_name = input.file_name().unwrap().to_string_lossy().into_owned();
        let temp = tempfile::tempdir().unwrap();
        let cache = Cache::open(&temp.path().join("cache")).unwrap();
        let ready = receive(
            body.clone(),
            a,
            cache.clone(),
            Arc::new(Control::default()),
            None,
        )
        .await
        .unwrap();
        let path = ready.verified_path().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), body);
        assert!(std::process::Command::new("/usr/bin/hdiutil")
            .arg("verify")
            .arg(&path)
            .output()
            .unwrap()
            .status
            .success());
        println!("Local DMG transfer verified: {} bytes; private file, quarantine properties and container checksum passed.",body.len());
    }

    #[test]
    fn cache_refuses_links_public_permissions_replacement_and_exclusive_name_collision() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("original");
        let cache = Cache::open(&original).unwrap();
        cache.open_file("unique", true).unwrap();
        assert!(cache.open_file("unique", true).is_err());
        let link = temp.path().join("link");
        symlink(&original, &link).unwrap();
        assert!(Cache::open(&link).is_err());
        std::fs::set_permissions(&original, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Cache::open(&original).is_err());
        std::fs::set_permissions(&original, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::rename(&original, temp.path().join("moved")).unwrap();
        Cache::open(&original).unwrap();
        assert!(cache.attached().is_err());
    }
    #[test]
    fn redirect_rules_only_allow_expected_github_asset_path_and_release_cdn() {
        let initial = Url::parse(&asset(b"body").url).unwrap();
        assert!(allowed_redirect(&initial, &initial));
        assert!(allowed_redirect(&Url::parse("https://release-assets.githubusercontent.com/github-production-release-asset/1/a?signature=example").unwrap(),&initial));
        for url in [
            "http://release-assets.githubusercontent.com/github-production-release-asset/1/a",
            "https://github.com.evil.invalid/a",
            "https://github.com/other/repo/releases/download/v0.2.0/a.dmg",
            "https://release-assets.githubusercontent.com/other/a",
            "https://user@release-assets.githubusercontent.com/github-production-release-asset/a",
            "https://release-assets.githubusercontent.com:444/github-production-release-asset/a",
        ] {
            assert!(!allowed_redirect(&Url::parse(url).unwrap(), &initial));
        }
    }
}
