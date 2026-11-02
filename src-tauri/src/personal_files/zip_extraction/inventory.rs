use super::*;
use std::collections::BTreeMap;
// Validate EOCD allocation bounds before zip allocates its central directory.
// Supports ordinary ZIP and bounded ZIP64, refuses SFX, trailing/multi-disk data.
fn preflight(f: &mut File, job: &Job, start: Instant) -> Result<usize, String> {
    // zip's parser may fall back to an earlier EOCD if the last candidate is
    // invalid. Bound every possible ZIP64 allocation, including embedded or
    // misleading candidates, before calling that parser. Normal 32-bit EOCDs
    // are intrinsically capped at 65535 entries.
    f.rewind().map_err(|e| e.to_string())?;
    let mut overlap = Vec::new();
    let mut buffer = [0; 128 * 1024];
    loop {
        check(job, start)?;
        let n = f.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        overlap.extend_from_slice(&buffer[..n]);
        for p in 0..overlap.len().saturating_sub(55) {
            if overlap[p..].starts_with(b"PK\x06\x06") {
                let h = &overlap[p..p + 56];
                let at = |i| u64::from_le_bytes(h[i..i + 8].try_into().unwrap());
                if !(44..=65536).contains(&at(4)) || at(24) > 10000 || at(32) > 10000 {
                    return Err("ZIP64 含无法核对的目录容量，未解压。".into());
                }
            }
        }
        let keep = overlap.len().min(55);
        overlap.drain(..overlap.len() - keep);
    }
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    let tail = len.min(65557);
    f.seek(SeekFrom::Start(len - tail))
        .map_err(|e| e.to_string())?;
    let mut b = vec![0; tail as usize];
    f.read_exact(&mut b).map_err(|e| e.to_string())?;
    let fail = || "ZIP 中央目录格式或容量无法核对。".to_string();
    let p = (0..b.len().saturating_sub(21))
        .rev()
        .find(|&i| {
            b[i..].starts_with(b"PK\x05\x06")
                && i + 22 + u16::from_le_bytes([b[i + 20], b[i + 21]]) as usize == b.len()
        })
        .ok_or_else(fail)?;
    let h = &b[p..];
    let u16at = |p| u16::from_le_bytes([h[p], h[p + 1]]);
    let u32at = |p| u32::from_le_bytes(h[p..p + 4].try_into().unwrap());
    if u16at(4) != 0 || u16at(6) != 0 || u16at(8) != u16at(10) {
        return Err(fail());
    }
    let end = len - tail + p as u64;
    let (count, size, offset, cd_end) =
        if u16at(10) == u16::MAX || u32at(12) == u32::MAX || u32at(16) == u32::MAX {
            if end < 20 {
                return Err(fail());
            }
            f.seek(SeekFrom::Start(end - 20))
                .map_err(|e| e.to_string())?;
            let mut l = [0; 20];
            f.read_exact(&mut l).map_err(|e| e.to_string())?;
            if &l[..4] != b"PK\x06\x07" || l[4..8] != [0; 4] || l[16..20] != [1, 0, 0, 0] {
                return Err(fail());
            }
            let pos = u64::from_le_bytes(l[8..16].try_into().unwrap());
            if pos.checked_add(56).is_none_or(|n| n > end - 20) {
                return Err(fail());
            }
            f.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
            let mut z = [0; 56];
            f.read_exact(&mut z).map_err(|e| e.to_string())?;
            let u = |i| u64::from_le_bytes(z[i..i + 8].try_into().unwrap());
            if &z[..4] != b"PK\x06\x06"
                || z[16..24] != [0; 8]
                || !(44..=65536).contains(&u(4))
                || pos.checked_add(u(4) + 12) != Some(end - 20)
                || u(24) != u(32)
            {
                return Err(fail());
            }
            (u(32), u(40), u(48), pos)
        } else {
            (u16at(10) as u64, u32at(12) as u64, u32at(16) as u64, end)
        };
    if count > 10000
        || size > 16 * 1024 * 1024
        || offset.checked_add(size) != Some(cd_end)
        || size < count * 46
    {
        return Err(fail());
    }
    f.rewind().map_err(|e| e.to_string())?;
    Ok(count as usize)
}
pub(super) fn read(f: &mut File, job: &Job, start: Instant) -> Result<Vec<Entry>, String> {
    read_password(f, job, start, None).map(|(entries, _)| entries)
}
pub(super) fn read_password(
    f: &mut File,
    job: &Job,
    start: Instant,
    password: Option<&password::Password>,
) -> Result<(Vec<Entry>, bool), String> {
    let count = preflight(f, job, start)?;
    check(job, start)?;
    let mut zip = zip::ZipArchive::new(f).map_err(|_| "ZIP 结构无法读取。")?;
    if zip.len() != count || zip.offset() != 0 {
        return Err("ZIP 条目重复或包含未支持的前置数据。".into());
    }
    let mut nodes = BTreeMap::<String, Entry>::new();
    let mut folded = HashMap::<String, String>::new();
    let mut listed = HashSet::new();
    let mut bytes = 0u64;
    let mut names = 0usize;
    let mut encrypted = false;
    for index in 0..zip.len() {
        check(job, start)?;
        let mut entry = password::open(&mut zip, index, password)?;
        encrypted |= entry.encrypted();
        let raw =
            std::str::from_utf8(entry.name_raw()).map_err(|_| "ZIP 名称必须是可核对的 UTF-8。")?;
        let directory = entry.is_dir();
        let name = if directory {
            raw.strip_suffix('/').ok_or("ZIP 目录名称无法核对。")?
        } else {
            raw
        };
        if name.is_empty()
            || name.len() > 4096
            || name.split('/').count() > 64
            || name.chars().any(char::is_control)
            || name.contains(['\\', ':'])
            || name.split('/').any(|s| {
                s.is_empty() || s == "." || s == ".." || s.len() > 255 || s.ends_with(['.', ' '])
            })
            || !listed.insert(name.to_string())
        {
            return Err("ZIP 含越界、歧义、重复或无法核对的名称，整份未解压。".into());
        }
        if entry.unix_mode().is_some_and(|m| {
            let t = m & libc::S_IFMT as u32;
            t != 0
                && t != if directory {
                    libc::S_IFDIR as u32
                } else {
                    libc::S_IFREG as u32
                }
        }) || entry.is_symlink()
            || !matches!(
                entry.compression(),
                zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
            )
            || directory && entry.size() != 0
        {
            return Err("ZIP 含链接、特殊文件或未支持的压缩条目，整份未解压。".into());
        }
        // macOS ditto may write UTF-8 raw names without setting the UTF-8 bit.
        // Bind the validated raw UTF-8 bytes, never a guessed CP437 decoding or
        // an optional Unicode-name hint. Content is read by exact ZIP index.
        let name = name.to_string();
        names += name.len();
        bytes = bytes
            .checked_add(entry.size())
            .ok_or("ZIP 大小超过上限。")?;
        if bytes > MAX_BYTES || names > 4 * 1024 * 1024 {
            return Err("ZIP 解压内容超过 8 GiB 或名称容量上限。".into());
        }
        let parts = name.split('/').collect::<Vec<_>>();
        for i in 1..parts.len() {
            insert(
                &mut nodes,
                &mut folded,
                Entry {
                    name: parts[..i].join("/"),
                    directory: true,
                    size: 0,
                    sha256: String::new(),
                    index: None,
                    offset: None,
                },
            )?;
        }
        let size = entry.size();
        let sha256 = archive::digest(&mut entry, job, start, size)?;
        insert(
            &mut nodes,
            &mut folded,
            Entry {
                name,
                directory,
                size,
                sha256,
                index: Some(index),
                offset: None,
            },
        )?;
        if nodes.len() + 1 > 10000 {
            return Err("ZIP 解压后的文件和目录超过 10000 项。".into());
        }
    }
    Ok((nodes.into_values().collect(), encrypted))
}
pub(super) fn insert(
    nodes: &mut BTreeMap<String, Entry>,
    folded: &mut HashMap<String, String>,
    entry: Entry,
) -> Result<(), String> {
    let key = entry.name.to_lowercase();
    if folded.get(&key).is_some_and(|n| n != &entry.name)
        || nodes
            .get(&entry.name)
            .is_some_and(|n| !n.directory || !entry.directory)
    {
        return Err("ZIP 名称发生大小写或文件/目录冲突，未解压。".into());
    }
    folded.insert(key, entry.name.clone());
    if entry.index.is_some() || entry.offset.is_some() || !nodes.contains_key(&entry.name) {
        nodes.insert(entry.name.clone(), entry);
    }
    Ok(())
}
