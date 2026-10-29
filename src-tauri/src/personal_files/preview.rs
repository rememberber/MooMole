use super::*;
use crate::{db, observation::AnchoredPath};
use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
};
const IMAGE_LIMIT: u64 = 32 * 1024 * 1024;
const TEXT_LIMIT: u64 = 64 * 1024;
#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Content {
    Image {
        data_url: String,
        width: u64,
        height: u64,
        frames: usize,
    },
    Pdf {
        data_url: String,
        page: u32,
        pages: u32,
    },
    Text {
        text: String,
        truncated: bool,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Preview {
    scan_id: String,
    item_id: String,
    path: String,
    content: Content,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Domain {
    PersonalFiles,
    DuplicateFiles,
    SimilarPhotos,
    SoftwareContent,
}
pub(super) fn read(
    item: &Item,
    rules: &[String],
    limit: u64,
    complete: bool,
) -> Result<Vec<u8>, String> {
    if protected(Path::new(&item.path), rules)? {
        return Err("文件已受保护，未读取预览内容。".into());
    }
    let held = AnchoredPath::open(Path::new(&item.path))?;
    if held.witness != item.witness || !held.local_materialized() {
        return Err("文件已变化或未下载到本机，请重新扫描。".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(&item.path)
        .map_err(|_| "无法读取所选文件，请检查访问权限。")?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    let node = held.witness.nodes.last().ok_or("缺少文件身份。")?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.uid() != unsafe { libc::geteuid() }
        || (meta.dev(), meta.ino()) != (node.dev, node.ino)
        || (complete && item.size > limit)
    {
        return Err("文件身份或大小超出本次内容预览范围，原文件保持不变。".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(if complete { limit + 1 } else { limit })
        .read_to_end(&mut bytes)
        .map_err(|_| "预览读取未完成。")?;
    if bytes.len() as u64 > limit || (complete && bytes.len() as u64 != item.size) {
        return Err("预览内容超过本次读取范围或文件大小变化。".into());
    }
    held.validate()?;
    if AnchoredPath::open(Path::new(&item.path))?.witness != item.witness {
        return Err("预览读取期间文件已变化，请重新扫描。".into());
    }
    Ok(bytes)
}
fn render(item: &Item, rules: &[String], page: u32) -> Result<Content, String> {
    let extension = Path::new(&item.path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if [
        "png", "jpg", "jpeg", "heic", "heif", "gif", "webp", "tif", "tiff", "bmp",
    ]
    .contains(&extension.as_str())
    {
        let bytes = read(item, rules, IMAGE_LIMIT, true)?;
        let raster = media::Raster::decode(&bytes, 1024)?;
        Ok(Content::Image {
            data_url: raster.png()?,
            width: raster.width,
            height: raster.height,
            frames: raster.frames,
        })
    } else if extension == "pdf" {
        let bytes = read(item, rules, IMAGE_LIMIT, true)?;
        let (data_url, pages) = media::pdf(&bytes, page)?;
        Ok(Content::Pdf {
            data_url,
            page,
            pages,
        })
    } else if [
        "txt", "md", "csv", "tsv", "log", "json", "yaml", "yml", "xml", "html", "htm", "css", "js",
        "ts", "tsx", "jsx", "py", "rs", "sh", "sql", "ini", "conf", "toml",
    ]
    .contains(&extension.as_str())
    {
        let mut bytes = read(item, rules, TEXT_LIMIT, false)?;
        // A UTF-8 codepoint may straddle the bounded prefix. Only an incomplete
        // trailing codepoint may be trimmed; invalid encodings remain refused.
        let text = match std::str::from_utf8(&bytes) {
            Ok(s) => s.to_string(),
            Err(e) if e.error_len().is_none() => {
                bytes.truncate(e.valid_up_to());
                std::str::from_utf8(&bytes)
                    .map_err(|_| "文本编码无法识别。")?
                    .to_string()
            }
            _ => return Err("当前只预览 UTF-8 文本，此文件可能是其他编码或二进制内容。".into()),
        };
        if text
            .chars()
            .any(|c| c.is_control() && !['\n', '\r', '\t'].contains(&c))
        {
            return Err("文件包含二进制控制内容，请在对应应用中查看。".into());
        }
        Ok(Content::Text {
            truncated: item.size > bytes.len() as u64,
            text: text.trim_start_matches('\u{feff}').into(),
        })
    } else {
        Err("此格式暂不支持应用内预览，可在 Finder 中查看后使用对应应用打开。".into())
    }
}
fn selected(
    state: &crate::AppState,
    domain: Domain,
    scan_id: &str,
    item_id: &str,
) -> Result<Item, String> {
    let store = match domain {
        Domain::PersonalFiles => &state.personal_files,
        Domain::DuplicateFiles => &state.duplicate_files,
        Domain::SimilarPhotos => &state.similar_photos,
        Domain::SoftwareContent => &state.software_files,
    };
    let store = store.lock().map_err(|e| e.to_string())?;
    let scan = store.scan.as_ref().ok_or("请先完成文件扫描。")?;
    let now = db::now();
    if scan.id != scan_id || now < scan.created_at || now >= scan.expires_at {
        return Err("文件扫描已失效，请重新扫描后查看内容。".into());
    }
    scan.items
        .iter()
        .find(|i| i.id == item_id)
        .cloned()
        .ok_or("此项目不属于当前扫描。".into())
}
#[tauri::command]
pub(crate) async fn preview_personal_file(
    domain: Domain,
    scan_id: String,
    item_id: String,
    page: u32,
    state: tauri::State<'_, crate::AppState>,
) -> Result<Preview, String> {
    let item = selected(&state, domain, &scan_id, &item_id)?;
    let settings = db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?;
    if !settings.acknowledged {
        return Err("请先确认首次使用说明。".into());
    }
    let slot = state
        .preview_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "上一个预览仍在处理，请稍后重试。")?;
    let copy = item.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let _slot = slot;
        objc2::rc::autoreleasepool(|_| render(&copy, &settings.protected_paths, page))
    });
    let content = tokio::time::timeout(Duration::from_secs(8), worker)
        .await
        .map_err(|_| "预览未及时返回，原读取可能仍在进行；不会自动重复读取。")?
        .map_err(|_| "预览未能完成。")??;
    let current = selected(&state, domain, &scan_id, &item_id)?;
    let settings = db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?;
    if current.witness != item.witness
        || protected(Path::new(&item.path), &settings.protected_paths)?
        || AnchoredPath::open(Path::new(&item.path))?.witness != item.witness
    {
        return Err("预览期间文件或保护范围已变化，请重新扫描。".into());
    }
    Ok(Preview {
        scan_id,
        item_id,
        path: item.path,
        content,
    })
}
#[cfg(test)]
mod tests;
