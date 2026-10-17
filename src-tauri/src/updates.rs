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

// The backend owns this guard, including when the settings panel disappears.
// Failure is published before releasing the check admission flag.
struct Checking<'a> {
    checker: &'a Checker,
    finished: bool,
}
impl Checking<'_> {
    fn finish(&mut self, error: Option<String>) -> Result<(), String> {
        let mut s = self
            .checker
            .session
            .lock()
            .map_err(|_| "无法保存更新状态。")?;
        s.checking = false;
        s.check_error = error;
        if s.check_error.is_some() {
            s.check = None;
            s.checked_at = None;
            s.ready = None;
        }
        self.finished = true;
        Ok(())
    }
}
impl Drop for Checking<'_> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish(Some("更新检查未完成，请重新检查。".into()));
        }
    }
}

#[derive(Default)]
pub struct Checker {
    pub installation: Arc<crate::update_install::Gate>,
    auto_download: AtomicBool,
    busy: AtomicBool,
    maintaining: AtomicBool,
    session: Mutex<Session>,
}

#[derive(Default)]
struct Session {
    checking: bool,
    check_error: Option<String>,
    check: Option<Check>,
    checked_at: Option<Instant>,
    download: Download,
    control: Option<Arc<Control>>,
    ready: Option<Ready>,
    closing: bool,
    install_error: Option<String>,
    recovering: bool,
    recovery_error: Option<String>,
    preference_error: Option<String>,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub state: &'static str,
    restored: bool,
    file_name: Option<String>,
    transferred: u64,
    total: u64,
    error: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    checking: bool,
    check_error: Option<String>,
    auto_download: bool,
    preference_error: Option<String>,
    recovering: bool,
    recovery_error: Option<String>,
    installing: bool,
    install_error: Option<String>,
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
    /// Local-only upkeep; no network access and no new cache directories.
    pub fn schedule_maintenance(self: &Arc<Self>, path: std::path::PathBuf) {
        let weak = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            loop {
                let Some(owner) = weak.upgrade() else { break };
                if owner.session.lock().map(|s| s.closing).unwrap_or(true) {
                    break;
                }
                let path = path.clone();
                let result = tokio::task::spawn_blocking(move || owner.maintain(&path)).await;
                if !matches!(result, Ok(Ok(()))) {
                    eprintln!("MooMole: 更新缓存维护未完成，后续将重试。");
                }
                tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
            }
        });
    }
    fn maintain(&self, path: &std::path::Path) -> Result<(), String> {
        // The guard lives in the blocking worker, never an abortable awaiter.
        if self.busy.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let _busy = Reset(&self.busy);
        let protected = {
            let s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
            if s.closing || s.control.is_some() || self.installation.active() {
                return Ok(());
            }
            self.maintaining.store(true, Ordering::SeqCst);
            s.ready.as_ref().map(|r| r.cache_name().to_owned())
        };
        let _maintenance = Reset(&self.maintaining);
        Cache::cleanup_existing(path, protected.as_deref(), || {
            self.session.lock().map(|s| s.closing).unwrap_or(true)
        })?;
        Ok(())
    }
    pub fn snapshot(&self) -> Result<Snapshot, String> {
        let s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
        let mut download = s.download.clone();
        if download.state.is_empty() {
            download.state = "idle";
        }
        Ok(Snapshot {
            checking: s.checking,
            check_error: s.check_error.clone(),
            auto_download: self.auto_download.load(Ordering::SeqCst),
            preference_error: s.preference_error.clone(),
            recovering: s.recovering,
            recovery_error: s.recovery_error.clone(),
            installing: self.installation.active(),
            install_error: s.install_error.clone(),
            check: s.check.clone(),
            download,
        })
    }
    pub fn set_preference(&self, preference: Result<bool, String>) -> Result<(), String> {
        let mut s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
        let enabled = match preference {
            Ok(enabled) => {
                s.preference_error = None;
                enabled
            }
            Err(_) => {
                s.preference_error =
                    Some("自动下载设置无法读取，本次不会自动下载；可重新设置。".into());
                false
            }
        };
        self.auto_download.store(enabled, Ordering::SeqCst);
        Ok(())
    }
    pub fn after_check(
        self: &Arc<Self>,
        preference: Result<bool, String>,
        path: std::path::PathBuf,
    ) -> Result<Snapshot, String> {
        self.set_preference(preference)?;
        if !self.auto_download.load(Ordering::SeqCst) {
            return self.snapshot();
        }
        let (asset, checked_at) = {
            let s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
            if s.closing
                || s.control.is_some()
                || s.ready.is_some()
                || !matches!(s.download.state, "" | "idle")
            {
                drop(s);
                return self.snapshot();
            }
            let Some(asset) = s.check.as_ref().and_then(|c| c.asset.clone()) else {
                drop(s);
                return self.snapshot();
            };
            (asset, s.checked_at)
        };
        // Only the post-check decision may use the admission owned by check().
        // Keeping it through this decision prevents another check/maintenance
        // from replacing the selection before automatic download admission.
        let attempt = self.start_inner(path, true);
        if let Err(error) = attempt {
            let mut s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
            if !s.closing
                && s.control.is_none()
                && s.ready.is_none()
                && s.checked_at == checked_at
                && s.check.as_ref().and_then(|c| c.asset.as_ref()) == Some(&asset)
                && matches!(s.download.state, "" | "idle")
            {
                s.download = Download {
                    state: "failed",
                    file_name: Some(asset.file_name),
                    total: asset.size,
                    error: Some(error),
                    ..Download::default()
                };
            }
        }
        self.snapshot()
    }
    pub fn start(self: &Arc<Self>, path: std::path::PathBuf) -> Result<Snapshot, String> {
        self.start_inner(path, false)
    }
    fn start_inner(
        self: &Arc<Self>,
        path: std::path::PathBuf,
        after_check: bool,
    ) -> Result<Snapshot, String> {
        let mut s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
        if s.closing
            || (self.busy.load(Ordering::SeqCst) && !(after_check && s.checking))
            || s.control.is_some()
        {
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
        s.install_error = None;
        s.recovery_error = None;
        s.download = Download {
            state: "downloading",
            restored: false,
            file_name: Some(asset.file_name.clone()),
            transferred: 0,
            total: asset.size,
            error: None,
        };
        s.control = Some(control.clone());
        let owner = self.clone();
        tauri::async_runtime::spawn(async move {
            let cache = path
                .parent()
                .ok_or_else(|| "无法定位应用缓存。".to_string())
                .and_then(|root| {
                    std::fs::create_dir_all(root).map_err(|_| "无法创建应用缓存。".to_string())
                })
                .and_then(|_| Cache::open(&path));
            let result = match cache {
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
            if !s.recovering {
                s.download.state = "cancelling";
            }
        }
        Ok(())
    }
    pub fn shutdown(&self) {
        let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        s.closing = true;
        if let Some(control) = &s.control {
            control.cancel();
            if !s.recovering {
                s.download.state = "cancelling";
            }
        }
    }
    pub fn active(&self) -> bool {
        self.maintaining.load(Ordering::SeqCst)
            || self
                .session
                .lock()
                .map(|s| s.control.is_some())
                .unwrap_or(true)
    }
    pub async fn ready_path(self: &Arc<Self>) -> Result<std::path::PathBuf, String> {
        self.with_ready(|path| async { Ok(path) }).await
    }
    async fn with_ready<T, F, Fut>(self: &Arc<Self>, use_file: F) -> Result<T, String>
    where
        F: FnOnce(std::path::PathBuf) -> Fut,
        Fut: std::future::Future<Output = Result<T, String>>,
    {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err("已有更新操作正在进行，请稍候。".into());
        }
        let _reset = Reset(&self.busy);
        let ready = {
            let s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
            if s.closing || s.control.is_some() {
                return Err("应用正在退出或下载尚未结束。".into());
            }
            s.ready.clone().ok_or("没有已验证的更新包。")?
        };
        let result = tokio::task::spawn_blocking(move || ready.verified_path())
            .await
            .map_err(|_| "更新文件复核未完成。")?;
        if result.is_err() {
            let mut s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
            s.ready = None;
            s.download.state = "failed";
            s.download.error = Some("更新文件已变化，请重新下载。".into());
        }
        let path = result?;
        if self
            .session
            .lock()
            .map_err(|_| "无法读取更新状态。")?
            .closing
        {
            return Err("应用正在退出，已停止安装准备。".into());
        }
        use_file(path).await
    }
    pub async fn install<P, PF, O, OF, E>(
        self: &Arc<Self>,
        prepare: P,
        open: O,
        exit: E,
    ) -> Result<(), String>
    where
        P: FnOnce() -> PF,
        PF: std::future::Future<Output = Result<(), String>>,
        O: FnOnce(std::path::PathBuf) -> OF,
        OF: std::future::Future<Output = Result<(), String>>,
        E: FnOnce(),
    {
        let installation = match self.installation.begin() {
            Ok(installation) => installation,
            Err(error) => {
                // A rejected duplicate must not overwrite the running attempt.
                if !self.installation.active() {
                    if let Ok(mut s) = self.session.lock() {
                        s.install_error = Some(error.clone());
                    }
                }
                return Err(error);
            }
        };
        let result = async {
            {
                let mut s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
                s.install_error = None;
                if s.closing || s.control.is_some() || s.ready.is_none() {
                    return Err("没有可安装的已验证更新包。".into());
                }
            }
            prepare().await?;
            self.with_ready(|path| async move {
                open(path).await?;
                installation.commit();
                self.shutdown();
                exit();
                Ok(())
            })
            .await
        }
        .await;
        if let Err(error) = &result {
            if let Ok(mut s) = self.session.lock() {
                s.install_error = Some(error.clone());
            }
        }
        result
    }
    pub async fn check(
        &self,
        cache: Option<std::path::PathBuf>,
        after: impl FnOnce() -> Result<(), String>,
    ) -> Result<Snapshot, String> {
        self.check_using(self.check_source(cache), after).await
    }
    async fn check_using(
        &self,
        source: impl std::future::Future<Output = Result<Check, String>>,
        after: impl FnOnce() -> Result<(), String>,
    ) -> Result<Snapshot, String> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err("已有更新操作正在进行，请稍候。".into());
        }
        let _reset = Reset(&self.busy);
        {
            let mut s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
            if s.closing || s.control.is_some() {
                return Err("请等待当前下载结束，或先取消下载。".into());
            }
            s.checking = true;
            s.check_error = None;
            s.install_error = None;
            s.recovery_error = None;
            s.check = None;
            s.checked_at = None;
            s.ready = None;
            s.download = Download::default();
        }
        let mut checking = Checking {
            checker: self,
            finished: false,
        };
        let result = async {
            source.await?;
            after()
        }
        .await;
        checking.finish(result.as_ref().err().cloned())?;
        result?;
        self.snapshot()
    }
    async fn check_source(&self, cache: Option<std::path::PathBuf>) -> Result<Check, String> {
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
        self.accept_check(result?, cache).await
    }
    async fn accept_check(
        &self,
        result: Check,
        cache: Option<std::path::PathBuf>,
    ) -> Result<Check, String> {
        {
            let mut s = self.session.lock().map_err(|_| "无法保存更新检查。")?;
            if s.closing {
                return Err("应用正在退出，更新检查已停止。".into());
            }
            s.check = Some(result.clone());
            s.checked_at = Some(Instant::now());
        }
        if let (Some(asset), Some(path)) = (result.asset.clone(), cache) {
            self.restore_selected(asset, path).await?;
        }
        Ok(result)
    }
    async fn restore_selected(&self, asset: Asset, path: std::path::PathBuf) -> Result<(), String> {
        let control = Arc::new(Control::default());
        {
            let mut s = self.session.lock().map_err(|_| "无法读取更新状态。")?;
            if s.closing {
                return Err("应用正在退出，本地复核已停止。".into());
            }
            s.recovering = true;
            s.control = Some(control.clone());
        }
        let ctl = control.clone();
        let result =
            tokio::task::spawn_blocking(move || Cache::recover_existing(&path, &asset, &ctl))
                .await
                .unwrap_or_else(|_| Err("本地更新包未能复用，请重新下载。".into()));
        let mut s = self.session.lock().map_err(|_| "无法保存更新状态。")?;
        s.recovering = false;
        s.control = None;
        if s.closing || control.cancelled() {
            return Err("本地更新包复核已停止。".into());
        }
        match result {
            Ok(Some(ready)) => {
                s.download = Download {
                    state: "ready",
                    restored: true,
                    file_name: Some(ready.asset.file_name.clone()),
                    transferred: ready.asset.size,
                    total: ready.asset.size,
                    error: None,
                };
                s.ready = Some(ready);
            }
            Ok(None) => (),
            Err(error) => s.recovery_error = Some(error),
        }
        Ok(())
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

    fn selection(asset: &Asset, current: &str, arch: &str) -> Check {
        let mut value = release("0.2.0", "arm64");
        value["assets"][0] = serde_json::to_value(asset).unwrap();
        parse(manifest(vec![value]), current, arch).unwrap()
    }
    #[tokio::test]
    async fn pending_check_and_failure_are_observable_and_duplicates_preserve_the_owner() {
        let checker = Arc::new(Checker::default());
        let (started, observing) = tokio::sync::oneshot::channel();
        let (release, waiting) = tokio::sync::oneshot::channel::<()>();
        let owner = checker.clone();
        let task = tokio::spawn(async move {
            owner
                .check_using(
                    async {
                        started.send(()).unwrap();
                        waiting.await.unwrap();
                        Err("连接失败 fixture".into())
                    },
                    || Ok(()),
                )
                .await
        });
        observing.await.unwrap();
        let pending = checker.snapshot().unwrap();
        assert!(pending.checking);
        assert!(pending.check_error.is_none());
        assert!(pending.check.is_none());
        assert!(checker
            .check(None, || panic!("duplicate must not complete"))
            .await
            .is_err());
        let temp = tempfile::tempdir().unwrap();
        assert!(checker.start(temp.path().join("unused")).is_err());
        assert!(!temp.path().join("unused").exists());
        assert!(checker.snapshot().unwrap().checking);
        release.send(()).unwrap();
        assert!(task.await.unwrap().is_err());
        let failed = checker.snapshot().unwrap();
        assert!(!failed.checking);
        assert_eq!(failed.check_error.as_deref(), Some("连接失败 fixture"));
        assert!(failed.check.is_none());
        assert!(!checker.active());
        // Looking at the state again neither clears the failure nor restarts work.
        assert_eq!(checker.snapshot().unwrap().check_error, failed.check_error);
        let next = parse(manifest(vec![]), "0.1.0", "arm64").unwrap();
        let snapshot = checker
            .check_using(checker.accept_check(next, None), || Ok(()))
            .await
            .unwrap();
        assert!(!snapshot.checking);
        assert!(snapshot.check_error.is_none());
        assert_eq!(snapshot.check.unwrap().status, Status::Unpublished);
    }

    #[tokio::test]
    async fn post_check_failure_withdraws_selection_and_the_next_check_can_retry() {
        let checker = Checker::default();
        let selected = parse(manifest(vec![release("0.2.0", "arm64")]), "0.1.0", "arm64").unwrap();
        assert!(checker
            .check_using(checker.accept_check(selected, None), || {
                assert!(checker.snapshot().unwrap().checking);
                Err("本机偏好状态无法确认 fixture".into())
            })
            .await
            .is_err());
        let failed = checker.snapshot().unwrap();
        assert!(!failed.checking);
        assert!(failed.check.is_none());
        assert!(failed.check_error.unwrap().contains("偏好"));
        assert!(checker.session.lock().unwrap().checked_at.is_none());
        assert!(!checker.busy.load(Ordering::SeqCst));
        // Maintenance/reveal admission alone is never reported as a check.
        checker.busy.store(true, Ordering::SeqCst);
        assert!(!checker.snapshot().unwrap().checking);
        checker.busy.store(false, Ordering::SeqCst);
    }

    #[tokio::test]
    async fn abandoning_a_pending_source_releases_admission_and_retains_an_interruption() {
        let checker = Arc::new(Checker::default());
        let (started, observing) = tokio::sync::oneshot::channel();
        let owner = checker.clone();
        let task = tokio::spawn(async move {
            owner
                .check_using(
                    async {
                        started.send(()).unwrap();
                        std::future::pending::<Result<Check, String>>().await
                    },
                    || Ok(()),
                )
                .await
        });
        observing.await.unwrap();
        task.abort();
        assert!(matches!(task.await, Err(error) if error.is_cancelled()));
        let snapshot = checker.snapshot().unwrap();
        assert!(!snapshot.checking);
        assert!(snapshot.check_error.unwrap().contains("未完成"));
        assert!(!checker.active());
        let next = parse(manifest(vec![]), "0.1.0", "arm64").unwrap();
        assert!(checker
            .check_using(checker.accept_check(next, None), || Ok(()))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn automatic_admission_remains_inside_the_observable_check_ownership() {
        let (temp, ready) = crate::update_download::tests::installation_fixture().await;
        let checker = Arc::new(Checker::default());
        let blocked = temp.path().join("blocked-cache");
        std::fs::write(&blocked, b"preserve").unwrap();
        let selected = selection(&ready.asset, "0.1.0", "arm64");
        let snapshot = checker
            .check_using(checker.accept_check(selected, None), || {
                assert!(checker.busy.load(Ordering::SeqCst));
                assert!(checker.snapshot().unwrap().checking);
                assert!(checker.start(blocked.clone()).is_err());
                let admitted = checker.after_check(Ok(true), blocked.clone())?;
                assert!(admitted.checking);
                assert!(matches!(admitted.download.state, "downloading" | "failed"));
                Ok(())
            })
            .await
            .unwrap();
        assert!(!snapshot.checking);
        assert!(snapshot.check_error.is_none());
        assert!(!checker.busy.load(Ordering::SeqCst));
        tokio::time::timeout(Duration::from_secs(2), async {
            while checker.active() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(checker.snapshot().unwrap().download.state, "failed");
        assert_eq!(std::fs::read(blocked).unwrap(), b"preserve");
    }

    #[tokio::test]
    async fn fresh_selection_restores_in_new_session_and_protects_the_verified_package() {
        let (_temp, ready) = crate::update_download::tests::installation_fixture().await;
        let asset = ready.asset.clone();
        let path = ready.verified_path().unwrap();
        let cache = path.parent().unwrap().to_owned();
        drop(ready);
        let checker = Arc::new(Checker::default());
        assert!(checker.session.lock().unwrap().ready.is_none());
        let result = checker
            .accept_check(selection(&asset, "0.1.0", "arm64"), Some(cache.clone()))
            .await
            .unwrap();
        assert_eq!(result.status, Status::Available);
        let snapshot = checker.snapshot().unwrap();
        assert_eq!(snapshot.download.state, "ready");
        assert!(snapshot.download.restored);
        assert!(!snapshot.recovering);
        assert!(snapshot.recovery_error.is_none());
        assert!(!checker.active());
        assert_eq!(checker.ready_path().await.unwrap(), path);
        assert!(checker.start(cache.clone()).is_err());
        std::fs::File::open(&path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - Duration::from_secs(30 * 86400))
            .unwrap();
        checker.maintain(&cache).unwrap();
        assert!(path.is_file());
    }
    #[tokio::test]
    async fn only_a_fresh_matching_upgrade_can_restore_and_local_failure_keeps_download_selection()
    {
        let (temp, ready) = crate::update_download::tests::installation_fixture().await;
        let asset = ready.asset.clone();
        let path = ready.verified_path().unwrap();
        let cache = path.parent().unwrap().to_owned();
        drop(ready);
        let invalid_cache = temp.path().join("not-a-directory");
        std::fs::write(&invalid_cache, b"unchanged").unwrap();
        for selected in [
            selection(&asset, "0.2.0", "arm64"),
            selection(&asset, "0.1.0", "x64"),
            parse(manifest(vec![]), "0.1.0", "arm64").unwrap(),
        ] {
            let checker = Checker::default();
            checker
                .accept_check(selected, Some(invalid_cache.clone()))
                .await
                .unwrap();
            let snapshot = checker.snapshot().unwrap();
            assert_eq!(snapshot.download.state, "idle");
            assert!(snapshot.recovery_error.is_none());
            assert!(checker.session.lock().unwrap().ready.is_none());
        }
        std::fs::write(&path, b"tampered installer fixture").unwrap();
        let checker = Checker::default();
        checker
            .accept_check(selection(&asset, "0.1.0", "arm64"), Some(cache.clone()))
            .await
            .unwrap();
        let snapshot = checker.snapshot().unwrap();
        assert_eq!(snapshot.check.unwrap().status, Status::Available);
        assert_eq!(snapshot.download.state, "idle");
        assert!(snapshot.recovery_error.is_some());
        assert!(!snapshot.recovering);
        assert!(checker
            .session
            .lock()
            .unwrap()
            .check
            .as_ref()
            .unwrap()
            .asset
            .is_some());
        assert!(checker.session.lock().unwrap().checked_at.is_some());
        let closing = Checker::default();
        closing.shutdown();
        assert!(closing
            .accept_check(selection(&asset, "0.1.0", "arm64"), Some(cache))
            .await
            .is_err());
        assert!(path.is_file());
        assert_eq!(std::fs::read(&invalid_cache).unwrap(), b"unchanged");
    }

    #[tokio::test]
    async fn automatic_download_is_opt_in_and_skips_unavailable_or_already_restored_assets() {
        let (temp, ready) = crate::update_download::tests::installation_fixture().await;
        let asset = ready.asset.clone();
        let path = ready.verified_path().unwrap();
        let missing = temp.path().join("not-created/pending-updates");
        let checker = Arc::new(Checker::default());
        checker
            .accept_check(selection(&asset, "0.1.0", "arm64"), None)
            .await
            .unwrap();
        assert!(!checker.snapshot().unwrap().auto_download);
        assert_eq!(
            checker
                .after_check(Ok(false), missing.clone())
                .unwrap()
                .download
                .state,
            "idle"
        );
        assert!(!missing.parent().unwrap().exists());
        for result in [
            selection(&asset, "0.2.0", "arm64"),
            selection(&asset, "0.1.0", "x64"),
            parse(manifest(vec![]), "0.1.0", "arm64").unwrap(),
        ] {
            let other = Arc::new(Checker::default());
            other.accept_check(result, None).await.unwrap();
            assert_eq!(
                other
                    .after_check(Ok(true), missing.clone())
                    .unwrap()
                    .download
                    .state,
                "idle"
            );
        }
        let restored = Arc::new(Checker::default());
        restored
            .accept_check(
                selection(&asset, "0.1.0", "arm64"),
                Some(path.parent().unwrap().to_owned()),
            )
            .await
            .unwrap();
        assert!(
            restored
                .after_check(Ok(true), missing.clone())
                .unwrap()
                .download
                .restored
        );
        assert!(!missing.parent().unwrap().exists());
        assert_eq!(restored.ready_path().await.unwrap(), path);
    }
    #[tokio::test]
    async fn automatic_download_attempt_uses_existing_worker_and_preserves_failure_without_retry_loop(
    ) {
        let (temp, ready) = crate::update_download::tests::installation_fixture().await;
        let checker = Arc::new(Checker::default());
        checker
            .accept_check(selection(&ready.asset, "0.1.0", "arm64"), None)
            .await
            .unwrap();
        let blocked = temp.path().join("blocked");
        std::fs::write(&blocked, b"preserve").unwrap();
        let snapshot = checker.after_check(Ok(true), blocked.clone()).unwrap();
        assert!(matches!(snapshot.download.state, "downloading" | "failed"));
        tokio::time::timeout(Duration::from_secs(2), async {
            while checker.active() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let snapshot = checker.snapshot().unwrap();
        assert_eq!(snapshot.download.state, "failed");
        assert_eq!(snapshot.download.transferred, 0);
        assert!(snapshot.download.error.unwrap().contains("缓存"));
        assert_eq!(snapshot.check.unwrap().status, Status::Available);
        let unused = temp.path().join("no-retry/pending-updates");
        checker.after_check(Ok(true), unused.clone()).unwrap();
        assert!(!unused.parent().unwrap().exists());
        assert_eq!(std::fs::read(&blocked).unwrap(), b"preserve");
        assert!(!checker.session.lock().unwrap().closing);
    }
    #[tokio::test]
    async fn unreadable_preference_expiry_and_toggle_off_never_start_or_cancel_existing_work() {
        let (temp, ready) = crate::update_download::tests::installation_fixture().await;
        let checker = Arc::new(Checker::default());
        checker
            .accept_check(selection(&ready.asset, "0.1.0", "arm64"), None)
            .await
            .unwrap();
        let missing = temp.path().join("not-created/pending-updates");
        let snapshot = checker
            .after_check(Err("fixture read failure".into()), missing.clone())
            .unwrap();
        assert!(!snapshot.auto_download);
        assert!(snapshot.preference_error.unwrap().contains("无法读取"));
        assert_eq!(snapshot.download.state, "idle");
        assert!(!missing.parent().unwrap().exists());
        let control = Arc::new(Control::default());
        checker.session.lock().unwrap().control = Some(control.clone());
        checker.after_check(Ok(false), missing.clone()).unwrap();
        checker.after_check(Ok(true), missing).unwrap();
        assert!(!control.cancelled());
        assert!(Arc::ptr_eq(
            checker.session.lock().unwrap().control.as_ref().unwrap(),
            &control
        ));
        {
            let mut s = checker.session.lock().unwrap();
            s.control = None;
            s.checked_at = Some(Instant::now() - Duration::from_secs(16 * 60));
        }
        let expired = checker
            .after_check(Ok(true), temp.path().join("expired"))
            .unwrap();
        assert!(expired.download.error.unwrap().contains("过期"));
        assert!(!temp.path().join("expired").exists());
    }

    async fn installer() -> (tempfile::TempDir, Arc<Checker>, std::path::PathBuf) {
        let (temp, ready) = crate::update_download::tests::installation_fixture().await;
        let path = ready.verified_path().unwrap();
        let checker = Arc::new(Checker::default());
        {
            let mut s = checker.session.lock().unwrap();
            s.download.state = "ready";
            s.ready = Some(ready);
        }
        (temp, checker, path)
    }
    #[tokio::test]
    async fn installation_opens_only_after_data_and_file_checks_then_closes_admission_before_exit()
    {
        let (_temp, checker, path) = installer().await;
        let order = Mutex::new(Vec::new());
        checker
            .install(
                || async {
                    order.lock().unwrap().push("saved");
                    Ok(())
                },
                |actual| {
                    assert_eq!(actual, path);
                    async {
                        assert!(checker.snapshot().unwrap().installing);
                        assert!(checker.installation.enter().is_err());
                        assert!(checker.ready_path().await.is_err());
                        assert!(checker.check(None, || Ok(())).await.is_err());
                        order.lock().unwrap().push("open");
                        Ok(())
                    }
                },
                || {
                    assert!(checker.session.lock().unwrap().closing);
                    assert!(checker.installation.enter().is_err());
                    order.lock().unwrap().push("exit");
                },
            )
            .await
            .unwrap();
        assert_eq!(*order.lock().unwrap(), vec!["saved", "open", "exit"]);
        assert!(path.is_file());
        assert!(checker.snapshot().unwrap().installing);
        assert!(checker.installation.enter().is_err());
    }
    #[tokio::test]
    async fn installation_refuses_active_work_missing_package_and_failed_save_without_opening_or_exit(
    ) {
        let (_temp, checker, _path) = installer().await;
        let held = checker.installation.enter().unwrap();
        assert!(checker
            .install(
                || async { panic!("prepare while busy") },
                |_| async { panic!("open") },
                || panic!("exit")
            )
            .await
            .is_err());
        assert!(checker
            .snapshot()
            .unwrap()
            .install_error
            .unwrap()
            .contains("检查"));
        drop(held);
        assert!(checker
            .install(
                || async { Err("save fixture failed".into()) },
                |_| async { panic!("open") },
                || panic!("exit")
            )
            .await
            .is_err());
        assert!(checker.session.lock().unwrap().ready.is_some());
        assert!(!checker.session.lock().unwrap().closing);
        assert_eq!(
            checker.snapshot().unwrap().install_error.as_deref(),
            Some("save fixture failed")
        );
        assert!(checker.installation.enter().is_ok());
        let empty = Arc::new(Checker::default());
        assert!(empty
            .install(
                || async { panic!("prepare without a package") },
                |_| async { panic!("open") },
                || panic!("exit")
            )
            .await
            .is_err());
    }
    #[tokio::test]
    async fn installation_survives_observer_loss_rejects_duplicates_and_retries_open_failure() {
        let (_temp, checker, path) = installer().await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let worker = checker.clone();
        let signal = entered.clone();
        let wait = release.clone();
        let (completed, outcome) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let result = worker
                .install(
                    || async { Ok(()) },
                    |_| async {
                        signal.notify_one();
                        wait.notified().await;
                        Err("open fixture failed".into())
                    },
                    || panic!("exit on failure"),
                )
                .await;
            let _ = completed.send(result);
        });
        entered.notified().await;
        drop(task); // Losing the UI observer does not abort the owned backend task.
        assert!(checker.snapshot().unwrap().installing);
        assert!(checker.installation.enter().is_err());
        assert!(checker
            .install(
                || async { panic!("duplicate prepare") },
                |_| async { panic!("open") },
                || panic!("exit")
            )
            .await
            .is_err());
        release.notify_one();
        assert!(outcome.await.unwrap().is_err());
        assert!(!checker.snapshot().unwrap().installing);
        assert_eq!(
            checker.snapshot().unwrap().install_error.as_deref(),
            Some("open fixture failed")
        );
        assert!(!checker.session.lock().unwrap().closing);
        assert_eq!(checker.ready_path().await.unwrap(), path);
        let exited = AtomicBool::new(false);
        checker
            .install(
                || async { Ok(()) },
                |_| async { Ok(()) },
                || {
                    exited.store(true, Ordering::SeqCst);
                },
            )
            .await
            .unwrap();
        assert!(exited.load(Ordering::SeqCst));
        assert!(checker.snapshot().unwrap().install_error.is_none());
    }
    #[tokio::test]
    async fn installation_invalidates_tampered_file_and_respects_shutdown_before_open() {
        let (_temp, checker, path) = installer().await;
        std::fs::write(path, b"tampered installer fixture").unwrap();
        assert!(checker
            .install(
                || async { Ok(()) },
                |_| async { panic!("open a changed file") },
                || panic!("exit")
            )
            .await
            .is_err());
        assert!(checker.session.lock().unwrap().ready.is_none());
        assert_eq!(checker.snapshot().unwrap().download.state, "failed");
        assert!(checker.installation.enter().is_ok());
        let (_temp, checker, _) = installer().await;
        assert!(checker
            .install(
                || async {
                    checker.shutdown();
                    Ok(())
                },
                |_| async { panic!("open after shutdown") },
                || panic!("exit")
            )
            .await
            .is_err());
    }

    #[test]
    fn maintenance_skips_active_operations_and_shutdown_and_releases_guards_on_error() {
        use std::os::unix::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cache");
        let checker = Checker::default();
        let _cache = Cache::open(&path).unwrap();
        let name = format!(".moomole-{}.download", uuid::Uuid::new_v4());
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path.join(&name))
            .unwrap();
        file.set_modified(std::time::SystemTime::now() - Duration::from_secs(2 * 86400))
            .unwrap();
        checker.busy.store(true, Ordering::SeqCst);
        checker.maintain(&path).unwrap();
        assert!(path.join(&name).exists());
        assert!(checker.busy.load(Ordering::SeqCst));
        checker.busy.store(false, Ordering::SeqCst);
        checker.session.lock().unwrap().control = Some(Arc::new(Control::default()));
        checker.maintain(&path).unwrap();
        assert!(path.join(&name).exists());
        assert!(!checker.busy.load(Ordering::SeqCst));
        checker.session.lock().unwrap().control = None;
        let bad = temp.path().join("file");
        std::fs::write(&bad, b"not a cache").unwrap();
        assert!(checker.maintain(&bad).is_err());
        assert!(!checker.busy.load(Ordering::SeqCst));
        assert!(!checker.active());
        checker.maintain(&path).unwrap();
        assert!(!path.join(&name).exists());
        assert!(!checker.active());
        std::fs::write(path.join("keep"), b"unknown").unwrap();
        checker.shutdown();
        checker.maintain(&bad).unwrap();
        assert!(path.join("keep").exists());
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
        assert!(checker.check(None, || Ok(())).await.is_err());
        checker.cancel().unwrap();
        assert!(control.cancelled());
        assert_eq!(checker.snapshot().unwrap().download.state, "cancelling");
        checker.shutdown();
        checker.session.lock().unwrap().control = None;
        assert!(!checker.active());
        assert!(checker.start(temp.path().join("cache")).is_err());
        assert!(checker.check(None, || Ok(())).await.is_err());
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
        assert!(checker.check(None, || Ok(())).await.is_err());
        assert!(checker.busy.load(Ordering::SeqCst));
    }
}
