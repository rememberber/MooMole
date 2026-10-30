//! An opaque, short-lived media capability. No arbitrary path URL, generic
//! asset scope, file download endpoint or HTTP listener is exposed.
use super::*;
use crate::AppState;
use std::{
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::FileExt,
    },
    sync::{Arc, Mutex},
};
use tauri::{
    http::{header, Request, Response},
    Manager,
};
mod containers;
pub(crate) const SCHEME: &str = "moomole-media";
const CHUNK: u64 = 2 * 1024 * 1024;
const LIMIT: u64 = 16 * 1024 * 1024 * 1024;
#[derive(Default)]
pub(crate) struct Store {
    current: Mutex<Option<Arc<Lease>>>,
}
struct Prepared {
    held: AnchoredPath,
    file: File,
    mime: &'static str,
    kind: &'static str,
}
struct Lease {
    id: String,
    domain: Domain,
    scan_id: String,
    item: Item,
    source: Option<software_content::Binding>,
    prepared: Prepared,
    deadline: Instant,
    expires_at: i64,
}
impl Store {
    fn insert(&self, lease: Lease) -> Result<Arc<Lease>, String> {
        let lease = Arc::new(lease);
        *self.current.lock().map_err(|e| e.to_string())? = Some(lease.clone());
        Ok(lease)
    }
    fn get(&self, id: &str) -> Option<Arc<Lease>> {
        let mut current = self.current.lock().ok()?;
        if current
            .as_ref()
            .is_some_and(|l| Instant::now() >= l.deadline)
        {
            *current = None;
        }
        current.as_ref().filter(|l| l.id == id).cloned()
    }
    fn live(&self, id: &str) -> bool {
        self.get(id).is_some()
    }
    fn revoke(&self, id: &str) {
        if let Ok(mut current) = self.current.lock() {
            if current.as_ref().is_some_and(|l| l.id == id) {
                *current = None;
            }
        }
    }
}
pub(crate) fn start(state: AppState) {
    tauri::async_runtime::spawn(async move {
        loop {
            if let Ok(mut current) = state.media_previews.current.lock() {
                if state.shutdown.closing()
                    || current
                        .as_ref()
                        .is_some_and(|l| Instant::now() >= l.deadline)
                {
                    *current = None;
                }
            }
            if state.shutdown.closing() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}
fn read_at(file: &File, start: u64, length: u64) -> Result<Vec<u8>, String> {
    let mut data = vec![0; length as usize];
    let mut read = 0;
    while read < data.len() {
        let n = file
            .read_at(&mut data[read..], start + read as u64)
            .map_err(|_| "影音分段读取失败。")?;
        if n == 0 {
            return Err("影音文件在读取期间变短。".into());
        }
        read += n;
    }
    Ok(data)
}
fn prepare(item: &Item, rules: &[String]) -> Result<Prepared, String> {
    if protected(Path::new(&item.path), rules)? {
        return Err("文件已受保护，未读取影音内容。".into());
    }
    let held = AnchoredPath::open(Path::new(&item.path))?;
    if held.witness != item.witness
        || !held.local_materialized()
        || item.size == 0
        || item.size > LIMIT
    {
        return Err("影音身份变化、未落地或超出本轮十六 GiB 范围。".into());
    }
    let name = std::ffi::CString::new(
        Path::new(&item.path)
            .file_name()
            .ok_or("缺少文件名。")?
            .as_encoded_bytes(),
    )
    .map_err(|e| e.to_string())?;
    let fd = unsafe {
        libc::openat(
            held.parent().as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err("所选影音不可读取。".into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let m = file.metadata().map_err(|e| e.to_string())?;
    let node = held.witness.nodes.last().ok_or("缺少影音身份。")?;
    if !m.is_file()
        || m.nlink() != 1
        || m.uid() != unsafe { libc::geteuid() }
        || (m.dev(), m.ino()) != (node.dev, node.ino)
    {
        return Err("影音不是本账户单链接普通文件。".into());
    }
    let prefix = read_at(&file, 0, item.size.min(64))?;
    let ext = Path::new(&item.path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (mime, kind) = match ext.as_str() {
        "wav" if prefix.starts_with(b"RIFF") && prefix.get(8..12) == Some(b"WAVE") => {
            ("audio/wav", "audio")
        }
        "mp3"
            if prefix.starts_with(b"ID3")
                || prefix
                    .get(0..2)
                    .is_some_and(|b| b[0] == 0xff && b[1] & 0xe0 == 0xe0) =>
        {
            ("audio/mpeg", "audio")
        }
        "mp4" | "m4v" | "m4a" | "mov" => {
            containers::self_contained(&file, item.size)?;
            if ext == "m4a" {
                ("audio/mp4", "audio")
            } else if ext == "mov" {
                ("video/quicktime", "video")
            } else {
                ("video/mp4", "video")
            }
        }
        _ => {
            return Err(
                "本轮播放预览支持普通 WAV、MP3 和自包含 MP4/M4A/MOV；此格式或文件头未支持。".into(),
            )
        }
    };
    held.validate()?;
    Ok(Prepared {
        held,
        file,
        mime,
        kind,
    })
}
fn file_gate(lease: &Lease, rules: &[String]) -> Result<(), String> {
    if Instant::now() >= lease.deadline || protected(Path::new(&lease.item.path), rules)? {
        return Err("影音预览已过期或文件已受保护。".into());
    }
    lease.prepared.held.validate()?;
    let m = lease.prepared.file.metadata().map_err(|e| e.to_string())?;
    let n = lease.item.witness.nodes.last().ok_or("缺少影音身份。")?;
    if (m.dev(), m.ino(), m.nlink(), m.len()) != (n.dev, n.ino, 1, lease.item.size) {
        return Err("影音对象在预览期间变化。".into());
    }
    Ok(())
}
fn authorize(state: &AppState, lease: &Lease) -> Result<(), String> {
    if state.shutdown.closing() {
        return Err("应用正在退出。".into());
    }
    let current = selected_context(state, lease.domain, &lease.scan_id, &lease.item.id)?.0;
    if current.witness != lease.item.witness {
        return Err("影音扫描已变化。".into());
    }
    let settings = db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?;
    if !settings.acknowledged {
        return Err("请先确认首次使用说明。".into());
    }
    if let Some(source) = &lease.source {
        source.validate(&lease.item, &state.home, db::now())?;
    }
    file_gate(lease, &settings.protected_paths)
}
// Single byte ranges only. Like Tauri's native asset transport, cap a large
// range response so seeking does not allocate the entire movie in memory.
fn range(value: Option<&str>, len: u64) -> Result<(u16, u64, u64), ()> {
    if len == 0 {
        return Err(());
    }
    let Some(value) = value else {
        return if len <= CHUNK {
            Ok((200, 0, len - 1))
        } else {
            Err(())
        };
    };
    if value.len() > 80 {
        return Err(());
    }
    let value = value.strip_prefix("bytes=").ok_or(())?;
    let (start, end) = value.split_once('-').ok_or(())?;
    let number = |s: &str| {
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
            s.parse::<u64>().map_err(|_| ())
        } else {
            Err(())
        }
    };
    let (start, end) = if start.is_empty() {
        let amount = number(end)?;
        if amount == 0 {
            return Err(());
        }
        (len.saturating_sub(amount), len - 1)
    } else {
        let start = number(start)?;
        let end = if end.is_empty() {
            len - 1
        } else {
            number(end)?.min(len - 1)
        };
        (start, end)
    };
    if start >= len || end < start {
        return Err(());
    }
    Ok((206, start, end.min(start.saturating_add(CHUNK - 1))))
}
fn empty(status: u16, origin: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Access-Control-Allow-Origin", origin)
        .header(header::CACHE_CONTROL, "no-store")
        .body(vec![])
        .unwrap()
}
fn response(
    lease: &Lease,
    request: &Request<Vec<u8>>,
    origin: &str,
    mut gate: impl FnMut() -> Result<(), String>,
) -> Response<Vec<u8>> {
    if request.method() != "GET" && request.method() != "HEAD" {
        return empty(405, origin);
    }
    if gate().is_err() {
        return empty(403, origin);
    }
    let size = lease.item.size;
    let head = request.method() == "HEAD";
    let selected = if head {
        Ok((200, 0, size - 1))
    } else {
        range(
            request
                .headers()
                .get(header::RANGE)
                .and_then(|v| v.to_str().ok()),
            size,
        )
    };
    let Ok((status, start, end)) = selected else {
        return Response::builder()
            .status(416)
            .header(header::CONTENT_RANGE, format!("bytes */{size}"))
            .header("Access-Control-Allow-Origin", origin)
            .header(header::CACHE_CONTROL, "no-store")
            .body(vec![])
            .unwrap();
    };
    let body = if head {
        vec![]
    } else {
        match read_at(&lease.prepared.file, start, end - start + 1) {
            Ok(bytes) => bytes,
            Err(_) => return empty(409, origin),
        }
    };
    if gate().is_err() {
        return empty(403, origin);
    }
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, lease.prepared.mime)
        .header(header::CONTENT_LENGTH, (end - start + 1).to_string())
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header("Access-Control-Allow-Origin", origin)
        .header(
            "Access-Control-Expose-Headers",
            "content-range, content-length, accept-ranges",
        );
    if status == 206 {
        builder = builder.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
    }
    builder.body(body).unwrap()
}
fn token(request: &Request<Vec<u8>>) -> Option<&str> {
    if request.uri().query().is_some()
        || !request.body().is_empty()
        || request.uri().host() != Some("localhost")
    {
        return None;
    }
    let parts = request
        .uri()
        .path()
        .strip_prefix('/')?
        .split('/')
        .collect::<Vec<_>>();
    if parts.len() != 2
        || parts[1] != "media"
        || uuid::Uuid::parse_str(parts[0]).ok()?.to_string() != parts[0]
    {
        return None;
    }
    Some(parts[0])
}
pub(crate) fn handle(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let app = ctx.app_handle();
    let origin = if cfg!(debug_assertions) {
        app.config()
            .build
            .dev_url
            .as_ref()
            .map(|u| u.origin().ascii_serialization())
            .unwrap_or_else(|| "tauri://localhost".into())
    } else {
        "tauri://localhost".into()
    };
    if ctx.webview_label() != "main"
        || request
            .headers()
            .get(header::ORIGIN)
            .is_some_and(|o| o.to_str().ok() != Some(&origin))
    {
        responder.respond(empty(403, &origin));
        return;
    }
    let Some(state) = app.try_state::<AppState>() else {
        responder.respond(empty(403, &origin));
        return;
    };
    let state = state.inner().clone();
    let Some(lease) = token(&request).and_then(|id| state.media_previews.get(id)) else {
        responder.respond(empty(410, &origin));
        return;
    };
    tauri::async_runtime::spawn(async move {
        let Ok(Ok(slot)) = tokio::time::timeout(
            Duration::from_secs(3),
            state.media_slots.clone().acquire_owned(),
        )
        .await
        else {
            responder.respond(empty(503, &origin));
            return;
        };
        tauri::async_runtime::spawn_blocking(move || {
            let _slot = slot;
            let result = response(&lease, &request, &origin, || {
                if !state.media_previews.live(&lease.id) {
                    return Err("预览已关闭。".into());
                }
                authorize(&state, &lease)
            });
            if result.status() == 403 || result.status() == 409 {
                state.media_previews.revoke(&lease.id);
            }
            responder.respond(result);
        });
    });
}
#[tauri::command]
pub(crate) async fn prepare_media_preview(
    domain: Domain,
    scan_id: String,
    item_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Preview, String> {
    let (item, source, expires_at) = selected_context(&state, domain, &scan_id, &item_id)?;
    let settings = db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?;
    if !settings.acknowledged {
        return Err("请先确认首次使用说明。".into());
    }
    if let Some(source) = &source {
        source.validate(&item, &state.home, db::now())?;
    }
    let slot = state
        .preview_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "上一个预览仍在处理，请稍后重试。")?;
    let copy = item.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let _slot = slot;
        prepare(&copy, &settings.protected_paths)
    });
    let prepared = tokio::time::timeout(Duration::from_secs(8), worker)
        .await
        .map_err(|_| "影音检查未及时完成，原检查仍可能进行；不会重复读取。")?
        .map_err(|_| "影音检查中断。")??;
    let id = uuid::Uuid::new_v4().to_string();
    let now = db::now();
    let lease = Lease {
        id: id.clone(),
        domain,
        scan_id: scan_id.clone(),
        item: item.clone(),
        source,
        prepared,
        deadline: Instant::now()
            + Duration::from_millis(expires_at.saturating_sub(now).max(0) as u64),
        expires_at,
    };
    authorize(&state, &lease)?;
    let lease = state.media_previews.insert(lease)?;
    Ok(Preview {
        scan_id,
        item_id,
        path: item.path,
        content: Content::Media {
            url: format!("{SCHEME}://localhost/{id}/media"),
            lease_id: id,
            mime: lease.prepared.mime.into(),
            media_kind: lease.prepared.kind.into(),
            expires_at: lease.expires_at,
        },
    })
}
#[tauri::command]
pub(crate) fn release_media_preview(
    lease_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state.media_previews.revoke(&lease_id);
    Ok(())
}
#[cfg(test)]
mod tests;
