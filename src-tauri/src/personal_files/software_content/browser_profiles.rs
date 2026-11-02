//! Fixed local browser roots. Profile discovery never reads preferences,
//! account names, cookies, credentials or database contents.
use super::*;
use std::{ffi::CStr, os::fd::AsRawFd};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Browser {
    Chrome,
    Chromium,
    Edge,
    Brave,
    Firefox,
    Safari,
}
impl Browser {
    pub(super) fn root(self, home: &Path) -> PathBuf {
        if self == Self::Safari {
            return home.join("Library/Safari");
        }
        home.join("Library/Application Support").join(match self {
            Self::Chrome => "Google/Chrome",
            Self::Chromium => "Chromium",
            Self::Edge => "Microsoft Edge",
            Self::Brave => "BraveSoftware/Brave-Browser",
            Self::Firefox => "Firefox/Profiles",
            Self::Safari => unreachable!(),
        })
    }
    pub(super) fn database_name(self) -> &'static str {
        if self == Self::Firefox {
            "places.sqlite"
        } else {
            "History"
        }
    }
    fn accepts_profile(self, name: &str) -> bool {
        if self == Self::Firefox {
            !name.is_empty()
                && name.len() <= 128
                && !name.starts_with('.')
                && !name.ends_with('.')
                && name.trim() == name
                && !name
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
        } else {
            profile_name(name)
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Selection {
    pub browser: Browser,
    pub profile: String,
}
impl Default for Selection {
    fn default() -> Self {
        Self {
            browser: Browser::Chrome,
            profile: "Default".into(),
        }
    }
}
pub(super) fn profile_name(name: &str) -> bool {
    name == "Default"
        || name.strip_prefix("Profile ").is_some_and(|n| {
            !n.is_empty()
                && n.len() <= 10
                && n.bytes().all(|b| b.is_ascii_digit())
                && (n == "0" || !n.starts_with('0'))
        })
}
impl Selection {
    pub(in crate::personal_files) fn history(&self, home: &Path) -> Result<PathBuf, String> {
        if self.browser == Browser::Safari && ordinary(home) {
            return match self.profile.as_str() {
                "Standard" => Ok(home.join("Library/Safari/Downloads.plist")),
                "Sandbox" => Ok(home.join(
                    "Library/Containers/com.apple.Safari/Data/Library/Safari/Downloads.plist",
                )),
                _ => Err("Safari 仅支持标准或沙盒下载列表，不接受任意配置路径。".into()),
            };
        }
        if !self.browser.accepts_profile(&self.profile) || !ordinary(home) {
            return Err("浏览器或配置目录不属于已支持的固定范围。".into());
        }
        Ok(self
            .browser
            .root(home)
            .join(&self.profile)
            .join(self.browser.database_name()))
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Profile {
    pub selection: Selection,
    pub history_path: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Profiles {
    pub browser: Browser,
    pub profiles: Vec<Profile>,
    pub complete: bool,
    pub skipped: u32,
}
struct Stream(*mut libc::DIR);
impl Drop for Stream {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}
pub(in crate::personal_files) fn discover(
    home: &Path,
    browser: Browser,
    rules: &[String],
) -> Result<Profiles, String> {
    if !ordinary(home) {
        return Err("账户目录范围无法核对。".into());
    }
    if browser == Browser::Safari {
        return super::safari::discover(home, rules);
    }
    let root = browser.root(home);
    if protected(&root, rules)? {
        return Err("浏览器配置目录与保护范围重叠，未读取。".into());
    }
    let mut result = Profiles {
        browser,
        profiles: vec![],
        complete: true,
        skipped: 0,
    };
    match std::fs::symlink_metadata(&root) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(_) => return Err("无法确认浏览器配置目录，未检查的范围保持未知。".into()),
        Ok(_) => {}
    }
    let held = AnchoredPath::open(&root)?;
    let meta = held.target().metadata().map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || !held.local_materialized() {
        return Err("浏览器配置目录不是当前账户的普通本地目录。".into());
    }
    let fd = unsafe {
        libc::openat(
            held.target().as_raw_fd(),
            c".".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err("无法列出浏览器配置目录。".into());
    }
    let ptr = unsafe { libc::fdopendir(fd) };
    if ptr.is_null() {
        unsafe {
            libc::close(fd);
        }
        return Err("无法列出浏览器配置目录。".into());
    }
    let stream = Stream(ptr);
    let began = Instant::now();
    for count in 0..=2048 {
        if count == 2048 || began.elapsed() >= Duration::from_secs(2) || result.profiles.len() >= 64
        {
            result.complete = false;
            break;
        }
        unsafe {
            *libc::__error() = 0;
        }
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            if unsafe { *libc::__error() } != 0 {
                result.complete = false;
            }
            break;
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        let Ok(name) = name.to_str() else {
            result.complete = false;
            result.skipped += 1;
            continue;
        };
        if !browser.accepts_profile(name) {
            continue;
        }
        let selection = Selection {
            browser,
            profile: name.into(),
        };
        let history = selection.history(home)?;
        let inspected = (|| -> Result<(), String> {
            if protected(&history, rules)? {
                return Err("protected".into());
            }
            let file = AnchoredPath::open(&history)?;
            let m = file.target().metadata().map_err(|e| e.to_string())?;
            let parent = file
                .witness
                .nodes
                .iter()
                .rev()
                .nth(1)
                .ok_or("unsupported")?;
            if !m.is_file()
                || m.uid() != unsafe { libc::geteuid() }
                || m.dev() != meta.dev()
                || !file.local_materialized()
                || m.len() > 128 * 1024 * 1024
                || parent.uid != unsafe { libc::geteuid() }
                || parent.dev != meta.dev()
                || !file.witness.nodes.starts_with(&held.witness.nodes)
            {
                return Err("unsupported".into());
            }
            file.validate()
        })();
        if inspected.is_err() {
            result.complete = false;
            result.skipped += 1;
            continue;
        }
        result.profiles.push(Profile {
            selection,
            history_path: history.to_str().ok_or("配置路径无法编码。")?.into(),
        });
    }
    held.validate()?;
    result.profiles.sort_by(|a, b| {
        let key = |s: &Selection| {
            s.profile
                .strip_prefix("Profile ")
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(0)
        };
        key(&a.selection)
            .cmp(&key(&b.selection))
            .then(a.selection.profile.cmp(&b.selection.profile))
    });
    Ok(result)
}

#[cfg(test)]
mod tests;
