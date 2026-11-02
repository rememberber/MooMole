//! Homebrew Cask operations remain separate from native whole-App Trash.
//! Retained Mole source + live manager metadata are required; no arbitrary
//! package token/path, zap, force, dependency bypass or automatic dependency removal.
use crate::{
    app_inspection::identity,
    db,
    engine::Job,
    observation::AnchoredPath,
    uninstall_plan::{Context, Source},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
pub(crate) mod commands;
pub(crate) mod journal;
mod receipt;
#[cfg(test)]
mod tests;
const CAP: usize = 2 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Draft {
    id: String,
    task_id: String,
    index: usize,
    path: String,
    bundle_id: String,
    token: String,
    name: String,
    installed_version: String,
    manager_path: String,
    metadata_sha256: String,
    created_at: i64,
    expires_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Record {
    id: String,
    request_id: String,
    draft: Draft,
    state: String,
    phase: String,
    created_at: i64,
    finished_at: Option<i64>,
    reason: String,
    reviewed_at: Option<i64>,
}
#[derive(Clone)]
struct Tool {
    path: PathBuf,
    witness: crate::observation::Witness,
    sha256: String,
}
impl Tool {
    fn capture(path: &Path) -> Result<Self, String> {
        if !["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].contains(&path.to_str().unwrap_or(""))
        {
            return Err("仅支持标准位置的 Homebrew，不执行任意命令路径。".into());
        }
        let resolved = path
            .canonicalize()
            .map_err(|_| "未发现标准 Homebrew 命令。")?;
        if ![
            "/opt/homebrew/bin/brew",
            "/usr/local/bin/brew",
            "/usr/local/Homebrew/bin/brew",
        ]
        .contains(&resolved.to_str().unwrap_or(""))
        {
            return Err("Homebrew 标准命令指向未支持的位置，保留。".into());
        }
        let held = AnchoredPath::open(&resolved)?;
        let m = held.target().metadata().map_err(|e| e.to_string())?;
        if !m.is_file()
            || m.nlink() != 1
            || !held.local_materialized()
            || m.len() > 1024 * 1024
            || m.mode() & 0o111 == 0
            || m.mode() & 0o022 != 0
            || ![0, unsafe { libc::geteuid() }].contains(&m.uid())
        {
            return Err("Homebrew 命令身份或权限无法核对。".into());
        }
        let f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(&resolved)
            .map_err(|e| e.to_string())?;
        let opened = f.metadata().map_err(|e| e.to_string())?;
        if (opened.dev(), opened.ino()) != (m.dev(), m.ino()) {
            return Err("Homebrew 命令已变化。".into());
        }
        let mut bytes = vec![];
        f.take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        held.validate()?;
        if bytes.len() as u64 != m.len() {
            return Err("Homebrew 命令读取期间变化。".into());
        }
        Ok(Self {
            path: resolved,
            witness: held.witness,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        })
    }
    fn detect() -> Result<Self, String> {
        let path = if cfg!(target_arch = "aarch64") {
            Path::new("/opt/homebrew/bin/brew")
        } else {
            Path::new("/usr/local/bin/brew")
        };
        Self::capture(path)
    }
    fn validate(&self) -> Result<(), String> {
        let current = Self::detect()?;
        if current.path != self.path
            || current.witness != self.witness
            || current.sha256 != self.sha256
        {
            return Err("Homebrew 命令身份已变化，请重新检查。".into());
        }
        Ok(())
    }
    fn command(&self, home: &Path) -> tokio::process::Command {
        let mut c = tokio::process::Command::new(&self.path);
        c.env_clear()
            .env("HOME", home)
            .env("HOMEBREW_CACHE", home.join("Library/Caches/Homebrew"))
            .env("HOMEBREW_LOGS", home.join("Library/Logs/Homebrew"))
            .env("HOMEBREW_USER_CONFIG_HOME", home.join(".homebrew"))
            .env(
                "PATH",
                "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin",
            )
            .env("HOMEBREW_NO_AUTO_UPDATE", "1")
            .env("HOMEBREW_NO_AUTOREMOVE", "1")
            .env("HOMEBREW_NO_ANALYTICS", "1")
            .env("HOMEBREW_NO_ENV_HINTS", "1")
            .env("HOMEBREW_NO_INSTALL_CLEANUP", "1")
            .env("HOMEBREW_NO_INSTALL_UPGRADE", "1")
            .current_dir("/");
        c
    }
    async fn installed(&self, home: &Path, job: Arc<Job>) -> Result<Catalog, String> {
        self.validate()?;
        let mut c = self.command(home);
        c.args(["info", "--json=v2", "--cask", "--installed"]);
        let bytes =
            crate::process::output_bounded(c, job, Duration::from_secs(30), CAP as u64).await?;
        self.validate()?;
        packages(&bytes)
    }
    fn uninstall_command(
        &self,
        home: &Path,
        token: &str,
    ) -> Result<tokio::process::Command, String> {
        if !valid_token(token) {
            return Err("包标识无法核对。".into());
        }
        let mut c = self.command(home);
        c.args(["uninstall", "--cask", token]);
        Ok(c)
    }
}
use std::os::unix::fs::OpenOptionsExt;
struct Catalog {
    packages: Vec<Package>,
    tokens: std::collections::HashSet<String>,
}
#[derive(Clone, Debug)]
struct Package {
    token: String,
    version: String,
    path: String,
    sha256: String,
}
fn text(v: &serde_json::Value, max: usize) -> Result<&str, String> {
    v.as_str()
        .filter(|s| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control))
        .ok_or("Homebrew 字段不受支持，保留原状态。".into())
}
fn valid_token(s: &str) -> bool {
    s.len() <= 256
        && [1, 3].contains(&s.split('/').count())
        && s.split('/').all(|p| {
            !p.is_empty()
                && p.len() <= 96
                && p.as_bytes()[0].is_ascii_alphanumeric()
                && p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.+@".contains(&b))
        })
}
fn packages(bytes: &[u8]) -> Result<Catalog, String> {
    if bytes.len() > CAP {
        return Err("Homebrew 清单超过读取上限。".into());
    }
    let v: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "Homebrew 没有返回可核对的 JSON 清单。")?;
    let rows = v
        .get("casks")
        .and_then(|v| v.as_array())
        .filter(|v| v.len() <= 512)
        .ok_or("Homebrew Cask 清单结构或数量不受支持。")?;
    let mut out = vec![];
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        let raw_token = text(&row["full_token"], 256)?;
        if !valid_token(raw_token)
            || !seen.insert(raw_token.to_owned())
            || row["installed"].as_str().is_none_or(|s| s.is_empty())
        {
            return Err("Homebrew 已安装包清单不完整或标识重复，保留原状态。".into());
        }

        // Keep unsupported casks outside this first executable route. The full
        // manager snapshot is still bounded; never fall back to deleting App paths.
        let parsed = (|| -> Result<Package, String> {
            let token = text(&row["full_token"], 256)?;
            if !valid_token(token) || row["pinned"].as_bool() != Some(false) {
                return Err("包被固定或标识未知。".into());
            }
            let version = text(&row["installed"], 128)?;
            let artifacts = row["artifacts"]
                .as_array()
                .filter(|a| !a.is_empty() && a.len() <= 16)
                .ok_or("包内容未知。")?;
            let mut app = None;
            for a in artifacts {
                let d = a.as_object().ok_or("包结构未知。")?;
                if d.len() == 1 && d.contains_key("zap") {
                    continue;
                }
                if d.len() != 2
                    || !d.contains_key("app")
                    || !d.contains_key("target")
                    || app.is_some()
                {
                    return Err("此包包含脚本、多个应用或其他安装内容，需使用原管理工具。".into());
                }
                let names = a["app"]
                    .as_array()
                    .filter(|v| v.len() == 1)
                    .ok_or("应用声明未知。")?;
                let name = text(&names[0], 255)?;
                let path = text(&a["target"], 4096)?;
                if !name.ends_with(".app")
                    || name.contains(['/', '\\'])
                    || Path::new(path).file_name().and_then(|v| v.to_str()) != Some(name)
                    || !Path::new(path).is_absolute()
                    || path.split('/').any(|s| s == "." || s == "..")
                {
                    return Err("包应用路径未知。".into());
                }
                app = Some(path.to_string());
            }
            Ok(Package {
                token: token.into(),
                version: version.into(),
                path: app.ok_or("没有唯一应用目标。")?,
                sha256: format!(
                    "{:x}",
                    Sha256::digest(serde_json::to_vec(row).map_err(|e| e.to_string())?)
                ),
            })
        })();
        if let Ok(p) = parsed {
            out.push(p)
        }
    }
    Ok(Catalog {
        packages: out,
        tokens: seen,
    })
}
#[derive(Clone)]
struct Plan {
    draft: Draft,
    source: Source,
    snapshot: identity::Snapshot,
    tool: Tool,
    context: Context,
    issued: Instant,
    receipt: receipt::Stamp,
    package_sha256: String,
}
#[derive(Default)]
struct Store {
    plan: Option<Plan>,
}
#[derive(Clone, Default)]
pub(crate) struct Controller(Arc<Mutex<Store>>);
fn check(job: &Job) -> Result<(), String> {
    if job.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
        Err("Homebrew 操作已请求停止。".into())
    } else {
        Ok(())
    }
}
fn guards(source: &Source, ctx: &Context) -> Result<(), String> {
    let (path, id, name, kind) = source.control_target();
    if kind != "Homebrew"
        || unsafe { libc::geteuid() } == 0
        || unsafe { libc::getuid() } != unsafe { libc::geteuid() }
        || (Path::new(path).parent() != Some(ctx.home.join("Applications").as_path())
            && Path::new(path).parent() != Some(Path::new("/Applications")))
    {
        return Err(
            "请选择 Applications 中经当前清单识别的 Homebrew 应用，并使用普通账户。".into(),
        );
    }
    if !crate::app_protection::check(
        path,
        Some(id),
        ctx.current_app.as_deref(),
        &ctx.current_id,
        &ctx.protected_paths,
    )?
    .reasons
    .is_empty()
    {
        return Err("应用受保护或属于当前客户端，保留。".into());
    }
    let policy = crate::app_uninstall_rules::load(&ctx.engine_root)?;
    if crate::app_uninstall_rules::observe(Some(&policy), Some(id), "matching").code != "notMatched"
        || crate::app_uninstall_rules::observe_path_with_identity(
            Some(&policy),
            &ctx.home,
            Path::new(path),
            "present",
            &mut Default::default(),
        )
        .code
            != "notMatched"
        || crate::app_uninstall_rules::observe_official(
            Some(&policy),
            Some(id),
            "matching",
            Some(name),
            path,
        )
        .code()
            != "notMatched"
    {
        return Err("Mole 保护要求保留、使用官方卸载器或状态未知。".into());
    }
    Ok(())
}
fn simple_app(path: &Path) -> Result<(), String> {
    if identity::update_managed(path)? {
        return Err("带后台或特权组件的应用需通过原管理工具卸载。".into());
    }
    for relative in [
        "Contents/_MASReceipt",
        "Contents/Library/LoginItems",
        "Contents/Library/LaunchServices",
        "Contents/Library/LaunchAgents",
        "Contents/Library/LaunchDaemons",
        "Contents/Library/SystemExtensions",
        "Contents/Library/PrivilegedHelperTools",
    ] {
        match std::fs::symlink_metadata(path.join(relative)) {
            Ok(_) => return Err("含商店收据、后台或特权组件的应用需通过原管理工具卸载。".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("应用后台或特权组件状态无法核对。".into()),
        }
    }
    Ok(())
}
impl Plan {
    async fn prepare(source: Source, ctx: Context, job: Arc<Job>) -> Result<Self, String> {
        guards(&source, &ctx)?;
        check(&job)?;
        let (path, id, name, _) = source.control_target();
        let snapshot = identity::capture(Path::new(path), id)?;
        simple_app(Path::new(path))?;
        if !matches!(
            snapshot.background,
            crate::app_background::Marker::Absent | crate::app_background::Marker::Disabled
        ) {
            return Err("后台专用或标记未知的应用需使用原管理工具。".into());
        }
        let tool = Tool::detect()?;
        let packages = tool.installed(&ctx.home, job.clone()).await?;
        let matches = packages
            .packages
            .into_iter()
            .filter(|p| p.path == path)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err("未找到唯一、已安装且仅包含此 App 的 Homebrew Cask；请使用原管理工具，未改走普通卸载。".into());
        }
        let p = &matches[0];
        let receipt = receipt::capture(&tool, p, &ctx)?;
        let now = db::now();
        let (task, index) = source.address();
        let result = Self {
            draft: Draft {
                id: uuid::Uuid::new_v4().to_string(),
                task_id: task,
                index,
                path: path.into(),
                bundle_id: id.into(),
                token: p.token.clone(),
                name: name.into(),
                installed_version: p.version.clone(),
                manager_path: tool.path.to_str().ok_or("Homebrew 路径无法编码。")?.into(),
                metadata_sha256: format!(
                    "{:x}",
                    Sha256::digest(
                        serde_json::to_vec(&(
                            receipt.digest(),
                            &snapshot,
                            &source,
                            crate::engine::REVISION
                        ))
                        .map_err(|e| e.to_string())?
                    )
                ),
                created_at: now,
                expires_at: now + 120_000,
            },
            source,
            snapshot,
            tool,
            context: ctx,
            issued: Instant::now(),
            receipt,
            package_sha256: p.sha256.clone(),
        };
        result.validate(&result.context, &job)?;
        Ok(result)
    }
    fn validate(&self, ctx: &Context, job: &Job) -> Result<(), String> {
        check(job)?;
        let now = db::now();
        if self.issued.elapsed() >= Duration::from_secs(120)
            || now < self.draft.created_at
            || now >= self.draft.expires_at
            || self.context.home != ctx.home
            || self.context.current_id != ctx.current_id
            || self.context.current_app != ctx.current_app
            || self.context.protected_paths != ctx.protected_paths
        {
            return Err("Homebrew 确认已过期或保护设置变化，请重新检查。".into());
        }
        guards(&self.source, ctx)?;
        simple_app(Path::new(&self.draft.path))?;
        self.tool.validate()?;
        if identity::capture(Path::new(&self.draft.path), &self.draft.bundle_id)? != self.snapshot {
            return Err("应用完整包已变化，请重新读取。".into());
        }
        let node = self.snapshot.root.nodes.last().ok_or("应用身份缺失。")?;
        if node.uid != unsafe { libc::geteuid() } {
            return Err("仅支持当前账户拥有的受管理 App。".into());
        }
        let held = AnchoredPath::open(Path::new(&self.draft.path))?;
        let access = crate::permissions::observe(&held, &crate::permissions::Budget::new())?;
        if access.code != crate::permissions::Code::NoRestrictionObserved {
            return Err(access.refusal_reason());
        }
        let package = Package {
            token: self.draft.token.clone(),
            version: self.draft.installed_version.clone(),
            path: self.draft.path.clone(),
            sha256: self.package_sha256.clone(),
        };
        if receipt::capture(&self.tool, &package, ctx)? != self.receipt {
            return Err("Homebrew 安装收据已变化，请重新检查。".into());
        }
        crate::uninstall_runtime::require_stopped(Path::new(&self.draft.path), (node.dev, node.ino))
    }
}
