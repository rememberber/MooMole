//! Exact software content roots, independent of cache classification.
//! No mailbox/player databases, messages or recursive root removal.
use super::*;
use crate::{db, engine::Job, observation::AnchoredPath};
use std::{os::unix::fs::MetadataExt, path::PathBuf, process::Command, sync::Arc};
pub(crate) mod browser_profiles;
mod chromium;
mod playlist;
mod telegram;
pub(crate) const MODULE: &str = "softwareContent";
pub(super) const REVISION: &str = "MooMole.software-content.v1";
const MINIMUM: u64 = 5 * 1024 * 1024;
const AGE: i64 = 31 * 86400;
fn old_enough(item: &Item, now: i64) -> bool {
    now.saturating_sub(
        item.witness
            .mtime
            .saturating_mul(1000)
            .saturating_add(item.witness.mtime_ns / 1_000_000),
    ) >= AGE * 1000
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Source {
    MailDownloads,
    MailSandboxDownloads,
    ChromeDownloads,
    VlcPlaylist,
    TelegramExport,
}
impl Source {
    fn recorded_files(self) -> bool {
        matches!(
            self,
            Self::ChromeDownloads | Self::VlcPlaylist | Self::TelegramExport
        )
    }
    pub(super) fn path(self, home: &Path) -> PathBuf {
        if self.recorded_files() {
            return home.to_path_buf();
        }
        home.join(match self {
            Self::MailDownloads => "Library/Mail Downloads",
            Self::MailSandboxDownloads => {
                "Library/Containers/com.apple.mail/Data/Library/Mail Downloads"
            }
            Self::ChromeDownloads | Self::VlcPlaylist | Self::TelegramExport => unreachable!(),
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Binding {
    pub source: Source,
    pub home: String,
    pub directory: Witness,
    pub allocated_at_scan: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<Witness>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_profile: Option<browser_profiles::Selection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playlist: Option<Witness>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telegram_export: Option<Witness>,
}
impl Binding {
    pub(super) fn root(&self) -> PathBuf {
        self.source.path(Path::new(&self.home))
    }
    pub(super) fn allows(&self, path: &Path) -> bool {
        let root = self.root();
        if self.source == Source::TelegramExport {
            return path == root
                || self
                    .telegram_export
                    .as_ref()
                    .and_then(|w| w.nodes.last())
                    .is_some_and(|n| telegram::attachment_allowed(path, &n.path));
        }
        if self.source.recorded_files() {
            return path.starts_with(&root)
                && ordinary(path)
                && (path == root || self.source != Source::VlcPlaylist || playlist::media(path));
        }
        path == root
            || path.strip_prefix(&root).is_ok_and(|r| {
                !r.as_os_str().is_empty()
                    && r.components().all(|c| match c {
                        std::path::Component::Normal(n) => n.to_str().is_some_and(|n| {
                            !sensitive(n) && !n.ends_with(".emlx") && !n.ends_with(".mbox")
                        }),
                        _ => false,
                    })
            })
    }
    pub(super) fn validate_record(&self, root: &str, items: &[Item]) -> Result<(), String> {
        if !ordinary(Path::new(&self.home))
            || Path::new(root) != self.root()
            || self.directory.version != crate::observation::VERSION
            || self
                .directory
                .nodes
                .last()
                .is_none_or(|n| n.path != Path::new(root))
            || (!self.source.recorded_files() && self.allocated_at_scan < MINIMUM)
            || (self.source == Source::VlcPlaylist
                && self.playlist.as_ref().is_none_or(|w| {
                    w.version != crate::observation::VERSION
                        || w.nodes.last().is_none_or(|n| {
                            !playlist::manifest_allowed(&n.path, Path::new(&self.home))
                        })
                }))
            || (self.source != Source::VlcPlaylist && self.playlist.is_some())
            || (self.source != Source::TelegramExport && self.telegram_export.is_some())
            || (self.source == Source::TelegramExport
                && self.telegram_export.as_ref().is_none_or(|w| {
                    w.version != crate::observation::VERSION
                        || w.nodes.last().is_none_or(|n| {
                            !telegram::manifest_allowed(&n.path, Path::new(&self.home))
                        })
                }))
            || (self.source != Source::ChromeDownloads
                && (self.history.is_some() || self.browser_profile.is_some()))
            || (self.source == Source::ChromeDownloads
                && self.history.as_ref().is_none_or(|w| {
                    w.version != crate::observation::VERSION
                        || w.nodes.last().is_none_or(|n| {
                            self.browser_profile
                                .clone()
                                .unwrap_or_default()
                                .history(Path::new(&self.home))
                                .ok()
                                .as_ref()
                                != Some(&n.path)
                        })
                }))
            || items
                .iter()
                .any(|i| !self.allows(Path::new(&i.path)) || i.path == root)
        {
            return Err("软件内容来源、目录或范围无法核对。".into());
        }
        Ok(())
    }
    pub(super) fn validate(&self, item: &Item, home: &Path, now: i64) -> Result<(), String> {
        if Path::new(&self.home) != home
            || !self.allows(Path::new(&item.path))
            || (!self.source.recorded_files() && !old_enough(item, now))
        {
            return Err("附件不属于已确认目录或未超过保留期限，本项保留。".into());
        }
        let held = AnchoredPath::open(&self.root())?;
        let m = held.target().metadata().map_err(|e| e.to_string())?;
        if held.witness.nodes != self.directory.nodes
            || !m.is_dir()
            || m.uid() != unsafe { libc::geteuid() }
            || !held.local_materialized()
        {
            return Err("附件目录或祖先身份已变化，本项保留。".into());
        }
        if self.source == Source::ChromeDownloads {
            let witness = self
                .history
                .as_ref()
                .ok_or("浏览器下载来源缺失，本项保留。")?;
            let path = &witness.nodes.last().ok_or("浏览器配置路径缺失。")?.path;
            let parent = AnchoredPath::open(path.parent().ok_or("浏览器配置路径缺失。")?)?;
            if parent.witness.nodes != witness.nodes[..witness.nodes.len() - 1]
                || !parent.local_materialized()
            {
                return Err("浏览器配置目录身份已变化，请重新扫描；本项保留。".into());
            }
        }
        if self.source == Source::VlcPlaylist {
            let witness = self
                .playlist
                .as_ref()
                .ok_or("播放列表来源缺失，本项保留。")?;
            let path = &witness.nodes.last().ok_or("播放列表路径缺失。")?.path;
            let manifest = AnchoredPath::open(path)?;
            if manifest.witness != *witness || !manifest.local_materialized() {
                return Err("播放列表已变化，请重新扫描；本项保留。".into());
            }
        }
        if self.source == Source::TelegramExport {
            let witness = self
                .telegram_export
                .as_ref()
                .ok_or("聊天导出来源缺失，本项保留。")?;
            let path = &witness.nodes.last().ok_or("聊天导出路径缺失。")?.path;
            let manifest = AnchoredPath::open(path)?;
            if manifest.witness != *witness || !manifest.local_materialized() {
                return Err("聊天导出清单已变化，请重新扫描；本项保留。".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Info {
    pub source: Source,
    pub minimum_age_days: u32,
    pub minimum_directory_bytes: u64,
    pub minimum_size_met: bool,
    pub execution_supported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_profile: Option<browser_profiles::Selection>,
}
pub(super) fn collect_telegram(
    home: &Path,
    manifest: &Path,
    rules: &[String],
    job: Arc<Job>,
) -> Result<Scan, String> {
    telegram::collect(home, manifest, rules, job)
}
pub(super) fn collect_browser(
    home: &Path,
    profile: browser_profiles::Selection,
    rules: &[String],
    job: Arc<Job>,
) -> Result<Scan, String> {
    chromium::collect_profile(home, profile, rules, job)
}
pub(super) fn collect_playlist(
    home: &Path,
    manifest: &Path,
    rules: &[String],
    job: Arc<Job>,
) -> Result<Scan, String> {
    playlist::collect(home, manifest, rules, job)
}
pub(super) fn collect(
    home: &Path,
    source: Source,
    rules: &[String],
    job: Arc<Job>,
) -> Result<Scan, String> {
    if source == Source::ChromeDownloads {
        return chromium::collect(home, source, rules, job);
    }
    if source == Source::VlcPlaylist {
        return Err("请先选择 VLC 导出的 M3U8 播放列表。".into());
    }
    if source == Source::TelegramExport {
        return Err("请先选择 Telegram Desktop 导出的 result.json。".into());
    }
    let path = source.path(home);
    let held = AnchoredPath::open(&path)
        .map_err(|_| "附件目录不存在或无法访问；未检查的范围保持未知，不会请求提升权限。")?;
    let binding = Binding {
        source,
        home: home.to_str().ok_or("账户目录无法编码。")?.into(),
        directory: held.witness,
        allocated_at_scan: 0,
        history: None,
        browser_profile: None,
        playlist: None,
        telegram_export: None,
    };
    let mut scan = super::scan::collect_scoped(&path, rules, job, Some(binding))?;
    let enough = scan.allocated_bytes >= MINIMUM;
    let now = db::now();
    scan.items.retain(|i| enough && old_enough(i, now));
    scan.report.items = scan.items.iter().map(row).collect();
    if let Some(b) = &mut scan.software_content {
        b.allocated_at_scan = scan.allocated_bytes;
    }
    scan.report.software_content = Some(Info {
        source,
        minimum_age_days: 31,
        minimum_directory_bytes: MINIMUM,
        minimum_size_met: enough,
        execution_supported: source == Source::MailDownloads,
        playlist_path: None,
        export_path: None,
        browser_profile: None,
    });
    Ok(scan)
}
// Same fixed process predicate as pinned Mole _mail_process_state. Never parses
// pgrep prose, runs a shell, sends signals to Mail or treats probe errors as idle.
pub(super) fn execution_gate(source: Source, job: &Job) -> Result<(), String> {
    if source.recorded_files() {
        return Ok(());
    }
    let mut command = Command::new("/usr/bin/pgrep");
    command
        .args(["-x", "Mail"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    let attempt = crate::maintenance::quicklook::supervise(
        (),
        command,
        job,
        Duration::from_secs(2),
        Duration::from_millis(200),
        |_| {},
    );
    if attempt.exit_code == Some(1) && attempt.stop.is_none() {
        Ok(())
    } else {
        Err(
            "Mail 正在运行、状态未知或检查已停止，附件保持原样；请先正常退出 Mail 后重新检查。"
                .into(),
        )
    }
}
#[cfg(test)]
mod tests;
