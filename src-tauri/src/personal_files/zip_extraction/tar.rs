//! Bounded TAR data reader; never invokes an archive's own extraction or code.
//! Supports ustar, GNU long names and POSIX local path/size extensions.
//! All other ownership, permission, time and xattr metadata stays in the source.
use super::*;
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
};
const MAX_STREAM: u64 = MAX_BYTES + 32 * 1024 * 1024;
const MAX_EXTENSION: usize = 65536;
fn bad() -> String {
    "TAR 结构、路径、类型或容量无法完整核对，整份未解压。".into()
}
pub(super) fn inflate(input: &mut File, job: &Job, start: Instant) -> Result<File, String> {
    input.rewind().map_err(|e| e.to_string())?;
    let mut gzip = flate2::bufread::GzDecoder::new(BufReader::new(input));
    let mut output = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut bytes = 0u64;
    let mut b = [0; 128 * 1024];
    loop {
        check(job, start)?;
        let n = gzip
            .read(&mut b)
            .map_err(|_| "Gzip 内容或 CRC 校验失败，未解压。")?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > MAX_STREAM {
            return Err(bad());
        }
        capacity(&output, 128 * 1024)?;
        output.write_all(&b[..n]).map_err(|e| e.to_string())?;
    }
    // A single complete member only; never silently ignore a second member,
    // a concatenated archive or arbitrary bytes after the verified CRC/footer.
    if !gzip
        .into_inner()
        .fill_buf()
        .map_err(|e| e.to_string())?
        .is_empty()
    {
        return Err("Gzip 含多个成员或尾随数据，本次未解压。".into());
    }
    output.rewind().map_err(|e| e.to_string())?;
    Ok(output)
}
fn number(raw: &[u8]) -> Result<u64, String> {
    if raw.first().is_some_and(|b| b & 0x80 != 0) {
        if raw[0] & 0x40 != 0 {
            return Err(bad());
        }
        return raw.iter().enumerate().try_fold(0u64, |n, (i, b)| {
            n.checked_mul(256)
                .and_then(|n| n.checked_add((if i == 0 { b & 0x7f } else { *b }) as u64))
                .ok_or_else(bad)
        });
    }
    let s = std::str::from_utf8(raw)
        .map_err(|_| bad())?
        .trim_matches(['\0', ' ']);
    if s.is_empty() {
        return Ok(0);
    }
    if !s.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        return Err(bad());
    }
    u64::from_str_radix(s, 8).map_err(|_| bad())
}
fn string(raw: &[u8]) -> Result<String, String> {
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    if raw[end..].iter().any(|b| *b != 0) {
        return Err(bad());
    }
    Ok(std::str::from_utf8(&raw[..end]).map_err(|_| bad())?.into())
}
#[derive(Default)]
struct Extension {
    path: Option<String>,
    size: Option<u64>,
}
fn pax(mut b: &[u8], global: bool) -> Result<Extension, String> {
    let mut out = Extension::default();
    let mut keys = HashSet::new();
    while !b.is_empty() {
        let at = b.iter().position(|v| *v == b' ').ok_or_else(bad)?;
        if at == 0 || at > 8 || !b[..at].iter().all(u8::is_ascii_digit) {
            return Err(bad());
        }
        let len = std::str::from_utf8(&b[..at])
            .map_err(|_| bad())?
            .parse::<usize>()
            .map_err(|_| bad())?;
        if len <= at + 2 || len > b.len() || b[len - 1] != b'\n' {
            return Err(bad());
        }
        let record = &b[at + 1..len - 1];
        let split = record.iter().position(|v| *v == b'=').ok_or_else(bad)?;
        let key = std::str::from_utf8(&record[..split]).map_err(|_| bad())?;
        let value = &record[split + 1..];
        if key.is_empty() || !keys.insert(key.to_string()) {
            return Err(bad());
        }
        match key {
            "path" if !global => {
                out.path = Some(std::str::from_utf8(value).map_err(|_| bad())?.into());
            }
            "size" if !global => {
                if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
                    return Err(bad());
                }
                out.size = Some(
                    std::str::from_utf8(value)
                        .map_err(|_| bad())?
                        .parse()
                        .map_err(|_| bad())?,
                );
            }
            "hdrcharset" => {
                if value != b"UTF-8" && value != b"ISO-IR 10646 2000 UTF-8" {
                    return Err(bad());
                }
            }
            "mtime"
            | "atime"
            | "ctime"
            | "uid"
            | "gid"
            | "uname"
            | "gname"
            | "charset"
            | "comment"
            | "LIBARCHIVE.creationtime"
            | "SCHILY.fflags"
            | "RHT.security.selinux" => {}
            k if k.starts_with("SCHILY.xattr.")
                || k.starts_with("LIBARCHIVE.xattr.")
                || k.starts_with("SCHILY.acl.") => {}
            _ => return Err("TAR 含尚未适配的 PAX 扩展（含链接或稀疏布局），整份未解压。".into()),
        }
        b = &b[len..];
    }
    Ok(out)
}
fn name(mut s: &str, directory: bool) -> Result<Option<String>, String> {
    if s.is_empty() || s.starts_with('/') {
        return Err(bad());
    }
    while let Some(rest) = s.strip_prefix("./") {
        s = rest
    }
    if directory {
        s = s.strip_suffix('/').unwrap_or(s);
        if s.is_empty() || s == "." {
            return Ok(None);
        }
    }
    if s.is_empty()
        || s.len() > 4096
        || s.split('/').count() > 64
        || s.chars().any(char::is_control)
        || s.contains(['\\', ':'])
        || s.split('/').any(|p| {
            p.is_empty() || p == "." || p == ".." || p.len() > 255 || p.ends_with(['.', ' '])
        })
    {
        return Err(bad());
    }
    Ok(Some(s.into()))
}
pub(super) fn read(f: &mut File, job: &Job, start: Instant) -> Result<Vec<Entry>, String> {
    f.rewind().map_err(|e| e.to_string())?;
    let length = f.metadata().map_err(|e| e.to_string())?.len();
    if length < 1024 || length % 512 != 0 || length > MAX_STREAM {
        return Err(bad());
    }
    let mut nodes = BTreeMap::new();
    let mut folded = HashMap::new();
    let mut listed = HashSet::new();
    let mut pending: Option<Extension> = None;
    let mut position = 0u64;
    let mut total = 0u64;
    let mut names = 0usize;
    let mut headers = 0usize;
    let mut metadata = 0usize;
    loop {
        check(job, start)?;
        headers += 1;
        if headers > 20002 {
            return Err(bad());
        }
        let mut h = [0u8; 512];
        f.read_exact(&mut h).map_err(|_| bad())?;
        position += 512;
        if h.iter().all(|v| *v == 0) {
            if pending.is_some() {
                return Err(bad());
            }
            let mut last = [0; 512];
            f.read_exact(&mut last).map_err(|_| bad())?;
            if last.iter().any(|b| *b != 0) {
                return Err(bad());
            }
            let mut b = [0; 128 * 1024];
            loop {
                check(job, start)?;
                let n = f.read(&mut b).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                if b[..n].iter().any(|v| *v != 0) {
                    return Err("TAR 末尾含其他归档或未知数据，整份未解压。".into());
                }
            }
            return Ok(nodes.into_values().collect());
        }
        let checksum = h
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    *b as u64
                }
            })
            .sum::<u64>();
        if checksum != number(&h[148..156])? {
            return Err("TAR 头校验失败，整份未解压。".into());
        }
        let posix = &h[257..265] == b"ustar\0\x30\x30";
        let gnu = &h[257..265] == b"ustar  \0";
        if !posix && !gnu {
            return Err("TAR 头格式未支持，整份未解压。".into());
        }
        let mut size = number(&h[124..136])?;
        let kind = h[156];
        if matches!(kind, b'x' | b'g' | b'L') {
            if pending.is_some() || size > MAX_EXTENSION as u64 {
                return Err(bad());
            }
            metadata = metadata.checked_add(size as usize).ok_or_else(bad)?;
            if metadata > 16 * 1024 * 1024 {
                return Err(bad());
            }
            let mut b = vec![0; size as usize];
            f.read_exact(&mut b).map_err(|_| bad())?;
            if kind == b'L' {
                pending = Some(Extension {
                    path: Some(string(&b)?),
                    size: None,
                })
            } else {
                let extension = pax(&b, kind == b'g')?;
                if kind == b'x' {
                    pending = Some(extension)
                }
            }
        } else {
            if !matches!(kind, 0 | b'0' | b'5') || h[157..257].iter().any(|b| *b != 0) {
                return Err("TAR 含链接、特殊文件或未支持的条目，整份未解压。".into());
            }
            let directory = kind == b'5';
            let extra = pending.take().unwrap_or_default();
            size = extra.size.unwrap_or(size);
            if size > MAX_BYTES
                || directory && size != 0
                || position.checked_add(size).is_none_or(|p| p > length)
            {
                return Err(bad());
            }
            total = total.checked_add(size).ok_or_else(bad)?;
            if total > MAX_BYTES {
                return Err(bad());
            }
            let mut raw = string(&h[..100])?;
            if posix {
                let prefix = string(&h[345..500])?;
                if !prefix.is_empty() {
                    raw = format!("{prefix}/{raw}")
                }
            }
            raw = extra.path.unwrap_or(raw);
            if let Some(name) = name(&raw, directory)? {
                if !listed.insert(name.clone()) {
                    return Err("TAR 名称重复或别名冲突，整份未解压。".into());
                }
                names = names.checked_add(name.len()).ok_or_else(bad)?;
                if names > 4 * 1024 * 1024 {
                    return Err(bad());
                }
                let parts = name.split('/').collect::<Vec<_>>();
                for i in 1..parts.len() {
                    inventory::insert(
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
                let digest = archive::digest((&mut *f).take(size), job, start, size)?;
                inventory::insert(
                    &mut nodes,
                    &mut folded,
                    Entry {
                        name,
                        directory,
                        size,
                        sha256: digest,
                        index: None,
                        offset: Some(position),
                    },
                )?;
                if nodes.len() + 1 > 10000 {
                    return Err(bad());
                }
            }
        }
        let padded = size.checked_add(511).ok_or_else(bad)? / 512 * 512;
        position = position.checked_add(padded).ok_or_else(bad)?;
        if position > length {
            return Err(bad());
        }
        f.seek(SeekFrom::Start(position))
            .map_err(|e| e.to_string())?;
    }
}
#[cfg(test)]
mod tests;
