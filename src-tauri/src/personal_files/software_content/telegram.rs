//! Telegram Desktop's JSON export is a local manifest, not a chat database.
//! Deserialize attachment fields only. Never retain names, senders or text.
use super::*;
use serde::{
    de::{DeserializeOwned, IgnoredAny, SeqAccess, Visitor},
    Deserializer,
};
use std::sync::atomic::Ordering;
const MESSAGES: usize = 20_000;
const CHATS: usize = 200;
fn bounded<'de, D: Deserializer<'de>, T: DeserializeOwned, const N: usize>(
    d: D,
) -> Result<Vec<T>, D::Error> {
    struct List<T, const N: usize>(std::marker::PhantomData<T>);
    impl<'de, T: DeserializeOwned, const N: usize> Visitor<'de> for List<T, N> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "an array with at most {N} entries")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut v = Vec::new();
            while let Some(value) = seq.next_element()? {
                if v.len() >= N {
                    return Err(serde::de::Error::custom(
                        "export array exceeds observation budget",
                    ));
                }
                v.push(value);
            }
            Ok(v)
        }
    }
    d.deserialize_seq(List::<T, N>(std::marker::PhantomData))
}
fn messages<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Message>, D::Error> {
    bounded::<D, Message, MESSAGES>(d)
}
fn chats<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Chat>, D::Error> {
    bounded::<D, Chat, CHATS>(d)
}
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    IgnoredAny::deserialize(d)?;
    Ok(true)
}
#[derive(Deserialize)]
struct Message {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
    file: Option<String>,
    photo: Option<String>,
    file_skip_reason: Option<String>,
    photo_skip_reason: Option<String>,
    #[serde(default, deserialize_with = "present")]
    rich_message: bool,
}
#[derive(Deserialize)]
struct Chat {
    id: u64,
    #[serde(rename = "type")]
    kind: String,
    #[serde(deserialize_with = "messages")]
    messages: Vec<Message>,
}
#[derive(Deserialize)]
struct Chats {
    #[serde(deserialize_with = "chats")]
    list: Vec<Chat>,
}
#[derive(Deserialize)]
struct Export {
    id: Option<u64>,
    #[serde(rename = "type")]
    kind: Option<String>,
    // Single-chat messages use the same bounded decoder as account exports.
    #[serde(default, deserialize_with = "optional_messages")]
    messages: Option<Vec<Message>>,
    chats: Option<Chats>,
    left_chats: Option<Chats>,
}
fn optional_messages<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<Message>>, D::Error> {
    messages(d).map(Some)
}
fn known(kind: &str) -> bool {
    matches!(
        kind,
        "saved_messages"
            | "replies"
            | "verification_codes"
            | "personal_chat"
            | "bot_chat"
            | "private_group"
            | "private_supergroup"
            | "public_supergroup"
            | "private_channel"
            | "public_channel"
    )
}
fn parse(bytes: &[u8]) -> Result<Vec<Message>, String> {
    let data: Export = serde_json::from_slice(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes))
        .map_err(|_| {
            "聊天导出格式未知、过深或超过本轮清单预算；请选择 JSON 格式的 result.json。"
        })?;
    if let Some(messages) = data.messages {
        if data.chats.is_some()
            || data.left_chats.is_some()
            || data.id.is_none()
            || !data.kind.as_deref().is_some_and(known)
        {
            return Err("单个聊天导出结构无法核对。".into());
        }
        return Ok(messages);
    }
    if data.id.is_some()
        || data.kind.is_some()
        || (data.chats.is_none() && data.left_chats.is_none())
    {
        return Err("未找到支持的聊天导出结构，未推断附件文件。".into());
    }
    let mut out = vec![];
    let mut seen = HashSet::new();
    for group in [data.chats, data.left_chats].into_iter().flatten() {
        for chat in group.list {
            if !known(&chat.kind)
                || !seen.insert((chat.id, chat.kind.clone()))
                || out.len() + chat.messages.len() > MESSAGES
            {
                return Err("聊天类别重复、未知或超过两万条消息的观察预算。".into());
            }
            out.extend(chat.messages);
        }
    }
    Ok(out)
}
pub(super) fn manifest_allowed(path: &Path, home: &Path) -> bool {
    path.starts_with(home) && ordinary(path) && path.file_name().is_some_and(|n| n == "result.json")
}
pub(super) fn attachment_allowed(path: &Path, manifest: &Path) -> bool {
    let Some(parent) = manifest.parent() else {
        return false;
    };
    let Some(name) = path
        .file_name()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
    else {
        return false;
    };
    let transcript = name == "result.json"
        || name == "export_results.html"
        || name
            .strip_prefix("messages")
            .and_then(|s| s.strip_suffix(".html"))
            .is_some_and(|s| s.bytes().all(|b| b.is_ascii_digit()));
    path.starts_with(parent) && path != parent && ordinary(path) && !transcript
}
fn relative(parent: &Path, text: &str) -> Option<PathBuf> {
    if text.is_empty()
        || text.len() > 4096
        || text.contains(['\\', ':'])
        || text.chars().any(char::is_control)
        || text.contains("(File ")
        || text.contains("(Photo ")
        || text
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
    {
        return None;
    }
    let path = Path::new(text);
    if !path
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return None;
    }
    Some(parent.join(path))
}
pub(super) fn collect(
    home: &Path,
    manifest: &Path,
    rules: &[String],
    job: Arc<Job>,
) -> Result<Scan, String> {
    if !ordinary(home) || !manifest_allowed(manifest, home) || protected(manifest, rules)? {
        return Err("请选择本账户普通目录中 Telegram Desktop 导出的 result.json；保护与应用资料库位置保留。".into());
    }
    let began = Instant::now();
    let root = AnchoredPath::open(home)?;
    let metadata = root.target().metadata().map_err(|e| e.to_string())?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || !root.local_materialized()
    {
        return Err("账户目录身份或本地状态无法核对。".into());
    }
    let read = chromium::read(manifest, 16 * 1024 * 1024, &job, began)?;
    let m = read.held.target().metadata().map_err(|e| e.to_string())?;
    if m.nlink() != 1 || m.dev() != metadata.dev() {
        return Err("聊天导出清单须为本账户同卷的单链接普通文件。".into());
    }
    let messages = parse(&read.bytes)?;
    let parent = manifest.parent().ok_or("聊天导出目录缺失。")?;
    let (mut complete, mut visited, mut skipped, mut serialized) = (true, 0, 0, 0usize);
    let mut items = vec![];
    let mut seen = HashSet::new();
    'messages: for message in messages {
        if job.cancelled.load(Ordering::SeqCst)
            || began.elapsed() > Duration::from_secs(10)
            || items.len() >= 2000
        {
            complete = false;
            break;
        }
        visited += 1;
        let _message_identity = message.id;
        if !matches!(message.kind.as_str(), "message" | "service") {
            skipped += 1;
            complete = false;
            continue;
        }
        if message.rich_message {
            skipped += 1;
            complete = false;
        }
        for (value, reason) in [
            (message.file, message.file_skip_reason),
            (message.photo, message.photo_skip_reason),
        ] {
            let Some(value) = value else {
                if reason.is_some() {
                    skipped += 1;
                    complete = false;
                }
                continue;
            };
            let item = (|| -> Result<Item, String> {
                if reason.is_some() {
                    return Err("附件在导出时未提供。".into());
                }
                let path = relative(parent, &value).ok_or("附件不是导出目录内的普通相对路径。")?;
                if !attachment_allowed(&path, manifest) || protected(&path, rules)? {
                    return Err("聊天清单、受保护或未支持路径保留。".into());
                }
                let held = AnchoredPath::open(&path)?;
                let m = held.target().metadata().map_err(|e| e.to_string())?;
                if !m.is_file()
                    || m.nlink() != 1
                    || m.uid() != unsafe { libc::geteuid() }
                    || m.dev() != metadata.dev()
                    || !held.local_materialized()
                    || m.len() > 9_007_199_254_740_991
                    || !seen.insert((m.dev(), m.ino()))
                {
                    return Err("附件重复、非本地或身份无法核对。".into());
                }
                Ok(Item {
                    id: uuid::Uuid::new_v4().to_string(),
                    path: path.to_str().ok_or("附件路径无法编码。")?.into(),
                    size: m.len(),
                    modified_at: m.mtime().saturating_mul(1000),
                    witness: held.witness,
                })
            })();
            match item {
                Ok(item) => {
                    let size = serde_json::to_vec(&item).map_err(|e| e.to_string())?.len();
                    if items.len() >= 2000 || serialized + size > 7 * 1024 * 1024 {
                        complete = false;
                        break 'messages;
                    }
                    serialized += size;
                    items.push(item);
                }
                Err(_) => {
                    skipped += 1;
                    complete = false;
                }
            }
        }
    }
    read.held.validate()?;
    root.validate()?;
    let now = db::now();
    let binding = Binding {
        source: Source::TelegramExport,
        home: home.to_str().ok_or("账户目录无法编码。")?.into(),
        directory: root.witness,
        allocated_at_scan: 0,
        history: None,
        browser_profile: None,
        playlist: None,
        telegram_export: Some(read.held.witness),
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
        live_photos: None,
        ai_focus: None,
        duplicates: None,
        similar_photos: None,
        software_content: Some(Info {
            browser_profile: None,
            source: Source::TelegramExport,
            minimum_age_days: 0,
            minimum_directory_bytes: 0,
            minimum_size_met: true,
            execution_supported: true,
            playlist_path: None,
            export_path: Some(manifest.to_str().ok_or("聊天导出路径无法编码。")?.into()),
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
