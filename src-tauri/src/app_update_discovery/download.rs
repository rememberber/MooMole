//! A verified delivery is still not installation authority.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Seek, Write},
    os::fd::{AsRawFd, FromRawFd},
    os::unix::fs::MetadataExt,
};
pub(super) const MAX_BYTES: u64 = 256 * 1024 * 1024;
fn decode<const N: usize>(s: &str) -> Result<[u8; N], String> {
    if s.len() > 128 {
        return Err("更新签名编码不受支持。".into());
    }
    STANDARD
        .decode(s)
        .map_err(|_| "更新签名编码无法核对。")?
        .try_into()
        .map_err(|_| "更新签名长度无法核对。".into())
}
pub(super) fn key(s: &str) -> Result<VerifyingKey, String> {
    let k = VerifyingKey::from_bytes(&decode(s)?).map_err(|_| "应用更新公钥无效。")?;
    if k.is_weak() {
        return Err("应用更新公钥不受支持。".into());
    }
    Ok(k)
}
pub(super) fn signature(s: &str) -> Result<Signature, String> {
    Ok(Signature::from_bytes(&decode(s)?))
}
pub(super) fn extension(raw: &str) -> Result<&'static str, String> {
    let u = network::url(raw)?;
    let p = u.path().to_ascii_lowercase();
    if p.ends_with(".zip") {
        Ok("zip")
    } else if p.ends_with(".dmg") {
        Ok("dmg")
    } else {
        Err("应用内下载首批支持 ZIP 或 DMG 完整包。".into())
    }
}
fn verify(bytes: &[u8], release: &Release, public_key: &str) -> Result<String, String> {
    if release.bytes != Some(bytes.len() as u64)
        || bytes.is_empty()
        || bytes.len() as u64 > MAX_BYTES
    {
        return Err("下载大小与来源声明不一致，未保存安装包。".into());
    }
    key(public_key)?
        .verify_strict(
            bytes,
            &signature(release.signature.as_deref().ok_or("更新缺少完整包签名。")?)?,
        )
        .map_err(|_| "下载包签名不匹配，未保存；请使用应用自身更新器核对。")?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Delivery {
    probe_id: String,
    candidate_id: String,
    request_id: String,
    path: String,
    bytes: u64,
    sha256: String,
    verification: &'static str,
}
pub(super) async fn fetch(
    release: &Release,
    job: Arc<Job>,
    output: &mut File,
) -> Result<(), String> {
    let mut u = network::url(&release.url)?;
    let expected = release
        .bytes
        .filter(|n| *n > 0 && *n <= MAX_BYTES)
        .ok_or("下载大小未知或超过 256 MiB，请使用原更新器。")?;
    for hop in 0..=4 {
        check(&job)?;
        let addresses = network::resolve(&u)?;
        check(&job)?;
        let client = reqwest::Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(0)
            .resolve_to_addrs(u.domain().unwrap(), &addresses)
            .user_agent("MooMole/0.2 verified-application-download")
            .build()
            .map_err(|_| "无法创建下载连接。")?;
        let next = {
            let action = async {
                let mut response = client
                    .get(u.clone())
                    .header("Accept-Encoding", "identity")
                    .send()
                    .await
                    .map_err(|_| "更新下载连接失败或超时。")?;
                if response
                    .remote_addr()
                    .is_some_and(|a| !network::public(a.ip()))
                {
                    return Err("下载连接地址无法核对。".into());
                }
                if response.status().is_redirection() {
                    if hop == 4 {
                        return Err("更新下载重定向超过上限。".into());
                    }
                    let location = response
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|v| v.to_str().ok())
                        .ok_or("下载重定向位置缺失。")?;
                    let next = u.join(location).map_err(|_| "下载重定向地址无效。")?;
                    return network::url(next.as_str()).map(Some);
                }
                if response.status() != reqwest::StatusCode::OK
                    || response.content_length().is_some_and(|n| n != expected)
                    || response
                        .headers()
                        .get(reqwest::header::CONTENT_ENCODING)
                        .is_some_and(|v| v != "identity")
                {
                    return Err("下载响应、编码或声明大小无法核对。".into());
                }
                let mut total = 0u64;
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(|_| "下载内容没有完整读取。")?
                {
                    check(&job)?;
                    total += chunk.len() as u64;
                    if total > expected {
                        return Err("下载内容超过声明大小，已停止。".into());
                    }
                    output
                        .write_all(&chunk)
                        .map_err(|_| "下载暂存文件写入失败。")?;
                }
                if total != expected {
                    return Err("下载未完成，未保存安装包。".into());
                }
                Ok(None)
            };
            tokio::pin!(action);
            let mut tick = tokio::time::interval(Duration::from_millis(50));
            loop {
                tokio::select! {r=&mut action=>break r?,_=tick.tick()=>check(&job)?}
            }
        };
        match next {
            Some(next) => u = next,
            None => return Ok(()),
        }
    }
    Err("更新下载未完成。".into())
}
unsafe extern "C" {
    fn fcopyfile(from: i32, to: i32, state: *mut libc::c_void, flags: u32) -> i32;
}
struct Partial<'a> {
    parent: &'a File,
    name: CString,
    identity: (u64, u64),
    published: bool,
}
impl Drop for Partial<'_> {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        let mut m = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                m.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0
        {
            let m = unsafe { m.assume_init() };
            if (m.st_dev as u64, m.st_ino) == self.identity {
                unsafe {
                    libc::unlinkat(self.parent.as_raw_fd(), self.name.as_ptr(), 0);
                }
            }
        }
    }
}
pub(super) fn save(
    input: &mut File,
    release: &Release,
    public_key: &str,
    directory: &str,
    settings: &db::Settings,
    job: &Job,
    validate: impl Fn() -> Result<(), String>,
) -> Result<(String, String), String> {
    check(job)?;
    input.rewind().map_err(|e| e.to_string())?;
    let mut bytes = vec![];
    input
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let hash = verify(&bytes, release, public_key)?;
    drop(bytes);
    let held = crate::personal_files::movement::directory(directory, settings)?;
    let id = uuid::Uuid::new_v4();
    let final_name = format!("MooMole-update-{id}.{}", extension(&release.url)?);
    let final_c = CString::new(final_name.clone()).unwrap();
    let name = CString::new(format!(".moomole-download-{id}.partial")).unwrap();
    let fd = unsafe {
        libc::openat(
            held.target().as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err("无法在所选目录创建新的下载文件。".into());
    }
    let mut out = unsafe { File::from_raw_fd(fd) };
    let m = out.metadata().map_err(|e| e.to_string())?;
    let mut partial = Partial {
        parent: held.target(),
        name,
        identity: (m.dev(), m.ino()),
        published: false,
    };
    check(job)?;
    input.rewind().map_err(|e| e.to_string())?;
    // Copy the already marked download, including its system quarantine metadata.
    if unsafe { fcopyfile(input.as_raw_fd(), out.as_raw_fd(), std::ptr::null_mut(), 15) } != 0 {
        return Err("下载内容或隔离属性复制失败。".into());
    }
    out.rewind().map_err(|e| e.to_string())?;
    let mut bytes = vec![];
    (&mut out)
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if verify(&bytes, release, public_key)? != hash {
        return Err("下载保存读回不一致。".into());
    }
    drop(bytes);
    out.sync_all().map_err(|e| e.to_string())?;
    validate()?;
    check(job)?;
    let current = crate::personal_files::movement::directory(directory, settings)?;
    if current.witness.nodes != held.witness.nodes {
        return Err("下载目录已变化，未发布。".into());
    }
    let path = Path::new(directory).join(&final_name);
    let mut named = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            held.target().as_raw_fd(),
            partial.name.as_ptr(),
            named.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err("下载暂存位置已变化，未发布。".into());
    }
    let named = unsafe { named.assume_init() };
    if (named.st_dev as u64, named.st_ino) != partial.identity || named.st_nlink != 1 {
        return Err("下载暂存文件身份已变化，未发布。".into());
    }
    if unsafe {
        libc::renameatx_np(
            held.target().as_raw_fd(),
            partial.name.as_ptr(),
            held.target().as_raw_fd(),
            final_c.as_ptr(),
            libc::RENAME_EXCL,
        )
    } != 0
    {
        return Err("下载保存位置冲突或发布失败，未覆盖已有文件。".into());
    }
    partial.published = true;
    // Publication is the commit point. Never remove or retry a published file.
    let result = (|| {
        held.target().sync_all().map_err(|e| e.to_string())?;
        let end = crate::observation::AnchoredPath::open(&path)?;
        let m = end.target().metadata().map_err(|e| e.to_string())?;
        let before = out.metadata().map_err(|e| e.to_string())?;
        if m.dev() != before.dev() || m.ino() != before.ino() {
            return Err("保存后的文件身份无法核对。".into());
        }
        crate::update_quarantine::present(&path)?;
        Ok(())
    })();
    result.map_err(|e: String| {
        format!(
            "安装包可能已保存到 {}，请核对；不会自动重试。{e}",
            path.display()
        )
    })?;
    Ok((path.to_str().ok_or("下载路径无法编码。")?.into(), hash))
}
pub(super) fn delivery(
    probe_id: &str,
    release: &Release,
    request_id: String,
    path: String,
    sha256: String,
) -> Delivery {
    Delivery {
        probe_id: probe_id.into(),
        candidate_id: release.id.clone(),
        request_id,
        path,
        bytes: release.bytes.unwrap(),
        sha256,
        verification: "installedKeyEd25519",
    }
}
#[cfg(test)]
mod tests;
