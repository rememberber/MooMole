use crate::update_download::{Cache, Control, Ready};
use reqwest::{redirect::Policy, Client, StatusCode};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub const PRODUCT: &str = "io.github.rememberber.moomole";
pub const RELEASES: &str = "https://github.com/rememberber/MooMole/releases";
const FEED: &str =
    "https://raw.githubusercontent.com/rememberber/MooMole/main/update-manifest.json";
const MAX_BYTES: usize = 512 * 1024;
const MAX_RELEASES: usize = 64;
const MAX_PACKAGE: u64 = 1024 * 1024 * 1024;

struct Reset<'a>(&'a AtomicBool);
impl Drop for Reset<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[derive(Default)]
pub struct Checker {
    busy: AtomicBool,
    session: Mutex<Session>,
}

#[derive(Default)]
struct Session {
    check: Option<Check>,
    checked_at: Option<Instant>,
    download: Download,
    control: Option<Arc<Control>>,
    ready: Option<Ready>,
    closing: bool,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub state: &'static str,
    file_name: Option<String>,
    transferred: u64,
    total: u64,
    error: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    check: Option<Check>,
    download: Download,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    product_id: String,
    releases: Vec<Release>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Release {
    version: String,
    notes: String,
    release_url: String,
    assets: Vec<Asset>,
}

#[derive(Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Asset {
    platform: String,
    architecture: String,
    package_type: String,
    pub(crate) file_name: String,
    pub(crate) url: String,
    pub(crate) size: u64,
    pub(crate) sha512: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Unpublished,
    Current,
    Available,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub status: Status,
    current_version: String,
    latest_version: Option<String>,
    channel: &'static str,
    architecture: String,
    notes: String,
    notes_truncated: bool,
    pub asset: Option<Asset>,
}

fn version(raw: &str) -> Result<Version, String> {
    if raw.len() > 128 || raw.is_empty() {
        return Err("更新版本号无效。".into());
    }
    Version::parse(raw).map_err(|_| "更新版本号无效。".into())
}

fn initial(current: &Version, arch: &str) -> Check {
    Check {
        status: Status::Unpublished,
        current_version: current.to_string(),
        latest_version: None,
        channel: if current.pre.is_empty() {
            "stable"
        } else {
            "preview"
        },
        architecture: arch.into(),
        notes: String::new(),
        notes_truncated: false,
        asset: None,
    }
}

fn select(raw: &[u8], current: &str, arch: &str) -> Result<Check, String> {
    if raw.is_empty() || raw.len() > MAX_BYTES {
        return Err("更新清单过大或为空。".into());
    }
    if !matches!(arch, "arm64" | "x64") {
        return Err("当前架构尚未支持应用更新，请查看发布页。".into());
    }
    let current = version(current)?;
    let manifest: Manifest = serde_json::from_slice(raw).map_err(|_| "更新清单格式无效。")?;
    if manifest.schema_version != 1 || manifest.product_id != PRODUCT {
        return Err("更新清单不属于本产品或版本不受支持。".into());
    }
    if manifest.releases.len() > MAX_RELEASES {
        return Err("更新清单版本数量超出限制。".into());
    }
    let mut versions = HashSet::new();
    let mut releases = Vec::new();
    for release in manifest.releases {
        let parsed = version(&release.version)?;
        // SemVer build metadata never creates a newer release.
        let precedence = format!(
            "{}.{}.{}-{}",
            parsed.major, parsed.minor, parsed.patch, parsed.pre
        );
        if !versions.insert(precedence) {
            return Err("更新清单存在重复版本。".into());
        }
        if release.notes.len() > 16 * 1024
            || release
                .notes
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err("更新说明过长或包含无效字符。".into());
        }
        let page = format!("{RELEASES}/tag/v{}", release.version);
        if release.release_url != page || release.assets.len() > 2 {
            return Err("更新发布页或安装包清单无效。".into());
        }
        let mut architectures = HashSet::new();
        for asset in &release.assets {
            let name = format!("MooMole-{}-mac-{}.dmg", release.version, asset.architecture);
            if asset.platform != "darwin"
                || !matches!(asset.architecture.as_str(), "arm64" | "x64")
                || !architectures.insert(&asset.architecture)
                || asset.package_type != "dmg"
                || asset.file_name != name
                || asset.url != format!("{RELEASES}/download/v{}/{name}", release.version)
                || asset.size == 0
                || asset.size > MAX_PACKAGE
                || asset.sha512.len() != 128
                || !asset
                    .sha512
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err("更新安装包的来源、架构、名称或校验信息无效。".into());
            }
        }
        releases.push((parsed, release));
    }
    releases.retain(|(v, _)| !current.pre.is_empty() || v.pre.is_empty());
    releases.sort_by(|(a, _), (b, _)| b.cmp_precedence(a));
    let mut result = initial(&current, arch);
    let Some((latest, release)) = releases.first() else {
        return Ok(result);
    };
    result.latest_version = Some(latest.to_string());
    if !latest.cmp_precedence(&current).is_gt() {
        result.status = Status::Current;
        return Ok(result);
    }
    result.status = Status::Available;
    result.asset = release
        .assets
        .iter()
        .find(|asset| asset.architecture == arch)
        .cloned();
    // Keep the newest sections first and show only eligible upgrades.
    for (v, release) in &releases {
        if !v.cmp_precedence(&current).is_gt() {
            continue;
        }
        let section = format!("{}\n{}\n\n", release.version, release.notes);
        if result.notes.len() + section.len() > 32 * 1024 {
            result.notes_truncated = true;
            break;
        }
        result.notes.push_str(&section);
    }
    Ok(result)
}

fn client() -> Result<Client, String> {
    Client::builder()
        .https_only(true)
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("MooMole/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "无法初始化更新连接。".into())
}

async fn fetch(client: &Client, url: &str) -> Result<Option<Vec<u8>>, String> {
    let mut response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| "无法连接更新来源，请检查网络后重试。")?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if response.status() != StatusCode::OK {
        return Err(format!(
            "更新来源返回 HTTP {}，请稍后重试。",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_BYTES as u64)
    {
        return Err("更新清单过大。".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "更新清单读取中断，请重试。")?
    {
        if body.len() + chunk.len() > MAX_BYTES {
            return Err("更新清单过大。".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Some(body))
}

impl Checker {
    pub fn snapshot(&self) -> Result<Snapshot, String> {
        let s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
        let mut download = s.download.clone();
        if download.state.is_empty() {
            download.state = "idle";
        }
        Ok(Snapshot {
            check: s.check.clone(),
            download,
        })
    }
    pub fn start(self: &Arc<Self>, path: std::path::PathBuf) -> Result<Snapshot, String> {
        let mut s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
        if s.closing || self.busy.load(Ordering::SeqCst) || s.control.is_some() {
            return Err("已有更新操作正在进行，请稍候。".into());
        }
        if s.ready.is_some() {
            return Err("更新包已经下载，请在 Finder 中查看。".into());
        }
        if s.checked_at
            .is_none_or(|t| t.elapsed() > Duration::from_secs(15 * 60))
        {
            return Err("更新检查已过期，请重新检查后下载。".into());
        }
        let asset = s
            .check
            .as_ref()
            .and_then(|c| c.asset.clone())
            .ok_or("请先检查更新，确认有匹配的安装包。")?;
        let control = Arc::new(Control::default());
        s.download = Download {
            state: "downloading",
            file_name: Some(asset.file_name.clone()),
            transferred: 0,
            total: asset.size,
            error: None,
        };
        s.control = Some(control.clone());
        let owner = self.clone();
        tauri::async_runtime::spawn(async move {
            let result = match Cache::open(&path) {
                Ok(cache) => {
                    crate::update_download::download(
                        asset,
                        cache,
                        control.clone(),
                        |bytes, verifying| {
                            if let Ok(mut s) = owner.session.lock() {
                                s.download.transferred = bytes;
                                if s.download.state != "cancelling" {
                                    s.download.state = if verifying {
                                        "verifying"
                                    } else {
                                        "downloading"
                                    };
                                }
                            }
                        },
                    )
                    .await
                }
                Err(e) => Err(e),
            };
            let mut s = owner.session.lock().unwrap_or_else(|e| e.into_inner());
            match result {
                Ok(ready) if !control.cancelled() => {
                    s.ready = Some(ready);
                    s.download.state = "ready";
                }
                Ok(ready) => {
                    let outcome = ready.discard();
                    s.download.state = if outcome.is_ok() {
                        "cancelled"
                    } else {
                        "failed"
                    };
                    s.download.error = outcome.err();
                }
                Err(e) => {
                    s.download.state = if control.cancelled() && e == "下载已取消。" {
                        "cancelled"
                    } else {
                        "failed"
                    };
                    s.download.error = if s.download.state == "failed" {
                        Some(e)
                    } else {
                        None
                    };
                }
            }
            s.control = None;
        });
        drop(s);
        self.snapshot()
    }
    pub fn cancel(&self) -> Result<(), String> {
        let mut s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
        if let Some(control) = &s.control {
            control.cancel();
            s.download.state = "cancelling";
        }
        Ok(())
    }
    pub fn shutdown(&self) {
        let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        s.closing = true;
        if let Some(control) = &s.control {
            control.cancel();
            s.download.state = "cancelling";
        }
    }
    pub fn active(&self) -> bool {
        self.session
            .lock()
            .map(|s| s.control.is_some())
            .unwrap_or(true)
    }
    pub async fn ready_path(self: &Arc<Self>) -> Result<std::path::PathBuf, String> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err("已有更新操作正在进行。".into());
        }
        let _reset = Reset(&self.busy);
        let ready = self
            .session
            .lock()
            .map_err(|_| "无法读取更新状态。")?
            .ready
            .clone()
            .ok_or("没有已验证的更新包。")?;
        let result = tokio::task::spawn_blocking(move || ready.verified_path())
            .await
            .map_err(|_| "更新文件复核未完成。")?;
        if result.is_err() {
            let mut s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
            s.ready = None;
            s.download.state = "failed";
            s.download.error = Some("更新文件已变化，请重新下载。".into());
        }
        result
    }
    pub async fn check(&self) -> Result<Check, String> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err("正在检查更新，请稍候。".into());
        }
        let _reset = Reset(&self.busy);
        {
            let mut s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
            if s.closing || s.control.is_some() {
                return Err("请等待当前下载结束，或先取消下载。".into());
            }
            s.check = None;
            s.checked_at = None;
            s.ready = None;
            s.download = Download::default();
        }
        let current = env!("CARGO_PKG_VERSION");
        if std::env::consts::OS != "macos" {
            return Err("当前系统尚未支持应用更新，请查看发布页。".into());
        }
        let arch = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "x64",
            _ => return Err("当前架构尚未支持应用更新，请查看发布页。".into()),
        };
        let result = tokio::time::timeout(Duration::from_secs(15), async {
            match fetch(&client()?, FEED).await? {
                Some(raw) => select(&raw, current, arch),
                None => Ok(initial(&version(current)?, arch)),
            }
        })
        .await
        .map_err(|_| "检查更新超时，请稍后重试。")?;
        let result = result?;
        let mut s = self.session.lock().map_err(|_| "无法保存更新检查。")?;
        s.check = Some(result.clone());
        s.checked_at = Some(Instant::now());
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::{Read, Write};

    fn release(v: &str, arch: &str) -> Value {
        let name = format!("MooMole-{v}-mac-{arch}.dmg");
        json!({"version":v,"notes":format!("说明 {v}"),"releaseUrl":format!("{RELEASES}/tag/v{v}"),
          "assets":[{"platform":"darwin","architecture":arch,"packageType":"dmg","fileName":name,
            "url":format!("{RELEASES}/download/v{v}/{name}"),"size":1234,"sha512":"a".repeat(128)}]})
    }
    fn manifest(releases: Vec<Value>) -> Value {
        json!({"schemaVersion":1,"productId":PRODUCT,"releases":releases})
    }
    fn parse(value: Value, current: &str, arch: &str) -> Result<Check, String> {
        select(&serde_json::to_vec(&value).unwrap(), current, arch)
    }

    #[test]
    fn empty_seed_and_prerelease_only_are_not_claimed_current() {
        let seed = include_bytes!("../../update-manifest.json");
        assert_eq!(
            select(seed, "0.1.0", "arm64").unwrap().status,
            Status::Unpublished
        );
        let pre = manifest(vec![release("1.0.0-rc.1", "arm64")]);
        assert_eq!(
            parse(pre, "0.1.0", "arm64").unwrap().status,
            Status::Unpublished
        );
    }

    #[test]
    fn semver_order_channels_no_downgrade_and_build_metadata() {
        let data = manifest(vec![
            release("0.9.0", "arm64"),
            release("0.10.0", "arm64"),
            release("1.0.0-rc.10", "arm64"),
        ]);
        let stable = parse(data.clone(), "0.1.0", "arm64").unwrap();
        assert_eq!(stable.latest_version.as_deref(), Some("0.10.0"));
        assert_eq!(stable.channel, "stable");
        assert!(stable.notes.starts_with("0.10.0\n"));
        assert!(!stable.notes.contains("rc.10"));
        let preview = parse(data.clone(), "1.0.0-rc.2", "arm64").unwrap();
        assert_eq!(preview.latest_version.as_deref(), Some("1.0.0-rc.10"));
        assert_eq!(preview.channel, "preview");
        let current = parse(data, "2.0.0", "arm64").unwrap();
        assert_eq!(current.status, Status::Current);
        assert!(current.asset.is_none());
        let same = parse(
            manifest(vec![release("1.0.0+one", "arm64")]),
            "1.0.0+two",
            "arm64",
        )
        .unwrap();
        assert_eq!(same.status, Status::Current);
        assert_eq!(
            parse(
                manifest(vec![release("1.0.0", "arm64")]),
                "1.0.0-rc.2",
                "arm64"
            )
            .unwrap()
            .status,
            Status::Available
        );
    }

    #[test]
    fn exact_architecture_never_falls_back_to_other_or_older_packages() {
        let mut newest = release("0.3.0", "x64");
        let older = release("0.2.0", "arm64");
        let data = manifest(vec![older.clone(), newest.clone()]);
        let result = parse(data, "0.1.0", "arm64").unwrap();
        assert_eq!(result.latest_version.as_deref(), Some("0.3.0"));
        assert_eq!(result.status, Status::Available);
        assert!(result.asset.is_none());
        newest["assets"]
            .as_array_mut()
            .unwrap()
            .push(release("0.3.0", "arm64")["assets"][0].clone());
        for arch in ["arm64", "x64"] {
            let result =
                parse(manifest(vec![older.clone(), newest.clone()]), "0.1.0", arch).unwrap();
            assert_eq!(result.asset.unwrap().architecture, arch);
        }
        assert!(parse(manifest(vec![]), "0.1.0", "unknown").is_err());
    }

    #[test]
    fn invalid_identity_schema_versions_duplicates_and_budgets_fail_closed() {
        for (key, value) in [
            ("schemaVersion", json!(2)),
            ("productId", json!("next-electron")),
            ("extra", json!(true)),
        ] {
            let mut data = manifest(vec![]);
            data[key] = value;
            assert!(parse(data, "0.1.0", "arm64").is_err());
        }
        for v in ["v1.0.0", "01.0.0", "1.0", "1.0.0-01", "1.0.0\n"] {
            assert!(parse(manifest(vec![release(v, "arm64")]), "0.1.0", "arm64").is_err());
        }
        assert!(parse(
            manifest(vec![
                release("1.0.0+a", "arm64"),
                release("1.0.0+b", "arm64")
            ]),
            "0.1.0",
            "arm64"
        )
        .is_err());
        assert!(parse(
            manifest(vec![release("1.0.0", "arm64"); 65]),
            "0.1.0",
            "arm64"
        )
        .is_err());
        assert!(select(&vec![b' '; MAX_BYTES + 1], "0.1.0", "arm64").is_err());
        assert!(select(b"invalid json", "0.1.0", "arm64").is_err());
    }

    #[test]
    fn assets_reject_other_sources_traversal_wrong_package_hash_and_size() {
        for (key, value) in [
            ("platform", json!("win32")),
            ("architecture", json!("universal")),
            ("packageType", json!("zip")),
            ("fileName", json!("../MooMole.dmg")),
            (
                "url",
                json!("https://github.com/other/MooMole/releases/download/v1.0.0/x.dmg"),
            ),
            (
                "url",
                json!("http://github.com/rememberber/MooMole/releases/download/v1.0.0/x.dmg"),
            ),
            ("url", json!("https://github.com.evil.invalid/x.dmg")),
            ("url", json!("https://user:password@github.com/x.dmg")),
            ("sha512", json!("A".repeat(128))),
            ("sha512", json!("a".repeat(127))),
            ("sha512", json!("a".repeat(86) + "==")),
            ("size", json!(0)),
            ("size", json!(MAX_PACKAGE + 1)),
            ("size", json!(-1)),
        ] {
            let mut r = release("1.0.0", "arm64");
            r["assets"][0][key] = value;
            assert!(
                parse(manifest(vec![r]), "0.1.0", "arm64").is_err(),
                "accepted {key}"
            );
        }
        let mut r = release("1.0.0", "arm64");
        r["releaseUrl"] = json!("https://github.com/other/repo");
        assert!(parse(manifest(vec![r]), "0.1.0", "arm64").is_err());
        let mut r = release("1.0.0", "arm64");
        let a = r["assets"][0].clone();
        r["assets"].as_array_mut().unwrap().push(a);
        assert!(parse(manifest(vec![r]), "0.1.0", "arm64").is_err());
    }

    #[test]
    fn notes_are_plain_bounded_eligible_and_mark_truncation() {
        let mut r = release("0.2.0", "arm64");
        r["notes"] = json!("<script>alert('plain')</script>");
        let result = parse(manifest(vec![r]), "0.1.0", "arm64").unwrap();
        assert!(result.notes.contains("<script>")); // Renderer must render text, never HTML.
        let mut entries = Vec::new();
        for n in 1..4 {
            let mut r = release(&format!("0.{n}.0"), "arm64");
            r["notes"] = json!("字".repeat(5000));
            entries.push(r);
        }
        let result = parse(manifest(entries), "0.0.0", "arm64").unwrap();
        assert!(result.notes_truncated);
        assert!(result.notes.len() <= 32 * 1024);
        let mut r = release("0.2.0", "arm64");
        r["notes"] = json!("x".repeat(16 * 1024 + 1));
        assert!(parse(manifest(vec![r]), "0.1.0", "arm64").is_err());
        let mut r = release("0.2.0", "arm64");
        r["notes"] = json!("bad\u{0000}text");
        assert!(parse(manifest(vec![r]), "0.1.0", "arm64").is_err());
    }

    #[tokio::test]
    async fn download_commands_require_fresh_backend_selection_and_respect_cancel_shutdown() {
        let checker = Arc::new(Checker::default());
        let temp = tempfile::tempdir().unwrap();
        assert!(checker.start(temp.path().join("cache")).is_err());
        assert!(checker.ready_path().await.is_err());
        {
            let mut s = checker.session.lock().unwrap();
            s.check =
                Some(parse(manifest(vec![release("0.2.0", "arm64")]), "0.1.0", "arm64").unwrap());
            s.checked_at = Some(Instant::now() - Duration::from_secs(901));
        }
        assert!(checker.start(temp.path().join("cache")).is_err());
        let control = Arc::new(Control::default());
        {
            let mut s = checker.session.lock().unwrap();
            s.checked_at = Some(Instant::now());
            s.control = Some(control.clone());
            s.download.state = "downloading";
        }
        assert!(checker.start(temp.path().join("cache")).is_err());
        assert!(checker.check().await.is_err());
        checker.cancel().unwrap();
        assert!(control.cancelled());
        assert_eq!(checker.snapshot().unwrap().download.state, "cancelling");
        checker.shutdown();
        checker.session.lock().unwrap().control = None;
        assert!(!checker.active());
        assert!(checker.start(temp.path().join("cache")).is_err());
        assert!(checker.check().await.is_err());
    }

    fn serve(response: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while std::time::Instant::now() < deadline {
                if let Ok((mut socket, _)) = listener.accept() {
                    socket.set_nonblocking(false).unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    socket
                        .set_write_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = Vec::new();
                    let mut buffer = [0; 4096];
                    while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
                        match socket.read(&mut buffer) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => request.extend_from_slice(&buffer[..n]),
                        }
                    }
                    let _ = socket.write_all(&response);
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        (url, thread)
    }

    #[tokio::test]
    async fn transport_bounds_body_distinguishes_absence_and_does_not_follow_redirects() {
        let client = Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        for (response, kind) in [
            (b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_vec(),"ok"),
            (b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),"absent"),
            (b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/blocked\r\nContent-Length: 0\r\n\r\n".to_vec(),"error"),
            (b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\n\r\n".to_vec(),"error"),
            (format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",MAX_BYTES+1).into_bytes(),"error"),
            ({let mut bytes = format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n",MAX_BYTES+1).into_bytes(); bytes.extend(vec![b'x';MAX_BYTES+1]); bytes.extend_from_slice(b"\r\n0\r\n\r\n");bytes},"error"),
            (b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nx".to_vec(),"error"),
        ] {
            let (url,thread) = serve(response);
            let result = fetch(&client,&url).await;
            thread.join().unwrap();
            match kind { "ok"=>assert_eq!(result.unwrap().unwrap(),b"{}"),"absent"=>assert!(result.unwrap().is_none()),_=>assert!(result.is_err()) }
        }
        assert!(super::client()
            .unwrap()
            .get("http://127.0.0.1:1")
            .send()
            .await
            .is_err());
    }

    #[tokio::test]
    async fn duplicate_checks_are_rejected_before_network() {
        let checker = Checker::default();
        checker.busy.store(true, Ordering::SeqCst);
        assert!(checker.check().await.is_err());
        assert!(checker.busy.load(Ordering::SeqCst));
    }
}
