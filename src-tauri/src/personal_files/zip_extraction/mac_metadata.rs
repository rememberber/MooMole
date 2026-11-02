//! Bounded AppleDouble interpretation. Never ask copyfile to unpack untrusted
//! security attributes, ACLs, aliases or ownership into a live filesystem.
use super::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    os::unix::ffi::OsStrExt,
};
const ITEM_LIMIT: usize = 32 * 1024 * 1024 + 65536;
const TOTAL_LIMIT: u64 = 256 * 1024 * 1024;
const INVALID: &str =
    "Mac 元数据不完整、存在歧义或包含未支持属性；未恢复。可另选仅解压正文保留原始元数据文件。";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Summary {
    pub items: usize,
    pub attributes: usize,
    pub executable: usize,
    pub finder_hidden: usize,
    pub skipped_quarantine: usize,
}
#[derive(Serialize)]
pub(super) struct Properties {
    pub mode: u32,
    pub modified: i64,
    pub attributes: BTreeMap<String, Vec<u8>>,
}
pub(super) struct Metadata {
    pub summary: Summary,
    pub bytes: u64,
    pub entries: BTreeMap<String, Properties>,
}
fn u16be(b: &[u8], p: usize) -> Result<usize, String> {
    Ok(u16::from_be_bytes(
        b.get(p..p + 2)
            .ok_or(INVALID)?
            .try_into()
            .map_err(|_| INVALID)?,
    ) as usize)
}
fn u32be(b: &[u8], p: usize) -> Result<usize, String> {
    Ok(u32::from_be_bytes(
        b.get(p..p + 4)
            .ok_or(INVALID)?
            .try_into()
            .map_err(|_| INVALID)?,
    ) as usize)
}
fn allowed(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 127
        && !key.chars().any(char::is_control)
        && !key.starts_with("system.")
        && !key.starts_with("security.")
        && (!key.starts_with("com.apple.")
            || matches!(
                key,
                "com.apple.metadata:_kMDItemUserTags"
                    | "com.apple.metadata:kMDItemFinderComment"
                    | "com.apple.TextEncoding"
            ))
}
fn parse(b: &[u8], directory: bool) -> Result<(BTreeMap<String, Vec<u8>>, bool, bool), String> {
    if b.len() > ITEM_LIMIT
        || u32be(b, 0)? != 0x00051607
        || u32be(b, 4)? != 0x00020000
        || u16be(b, 24)? != 2
        || u32be(b, 26)? != 9
        || u32be(b, 30)? != 50
        || u32be(b, 38)? != 2
    {
        return Err(INVALID.into());
    }
    let fork = u32be(b, 42)?;
    let length = u32be(b, 46)?;
    if fork < 82
        || fork > b.len()
        || fork.checked_add(length) != Some(b.len())
        || u32be(b, 34)? != fork - 50
    {
        return Err(INVALID.into());
    }
    let finder = b.get(50..82).ok_or(INVALID)?;
    let flags = u16be(finder, 8)?;
    if flags & 0x8000 != 0 || (!directory && &finder[..4] == b"alis") {
        return Err(INVALID.into());
    }
    let mut attributes = BTreeMap::new();
    if finder.iter().any(|b| *b != 0) {
        attributes.insert("com.apple.FinderInfo".into(), finder.to_vec());
    }
    if length > 0 {
        if directory {
            return Err(INVALID.into());
        }
        attributes.insert("com.apple.ResourceFork".into(), b[fork..].to_vec());
    }
    let mut data_size = 32 + length;
    if data_size > 32 * 1024 * 1024 {
        return Err(INVALID.into());
    }
    let mut skipped = false;
    if fork > 82 {
        if fork < 120
            || u32be(b, 84)? != 0x41545452
            || u32be(b, 92)? != fork
            || b.get(104..118).ok_or(INVALID)?.iter().any(|b| *b != 0)
        {
            return Err(INVALID.into());
        }
        let start = u32be(b, 96)?;
        if !(120..=65536).contains(&start) || start > fork || u32be(b, 100)? != fork - start {
            return Err(INVALID.into());
        }
        let count = u16be(b, 118)?;
        if count > 1024 {
            return Err(INVALID.into());
        }
        let mut at = 120;
        let mut seen = BTreeSet::new();
        let mut ranges = Vec::new();
        for _ in 0..count {
            let offset = u32be(b, at)?;
            let size = u32be(b, at + 4)?;
            let n = *b.get(at + 10).ok_or(INVALID)? as usize;
            let end = at.checked_add((11 + n + 3) & !3).ok_or(INVALID)?;
            if u16be(b, at + 8)? != 0 || n < 2 || end > start {
                return Err(INVALID.into());
            }
            let raw = b.get(at + 11..at + 11 + n).ok_or(INVALID)?;
            if raw.last() != Some(&0) || raw[..n - 1].contains(&0) {
                return Err(INVALID.into());
            }
            let key = std::str::from_utf8(&raw[..n - 1]).map_err(|_| INVALID)?;
            if !seen.insert(key.to_string())
                || offset < start
                || offset.checked_add(size).is_none_or(|e| e > fork)
            {
                return Err(INVALID.into());
            }
            data_size += size;
            if data_size > 32 * 1024 * 1024 {
                return Err(INVALID.into());
            }
            ranges.push((offset, offset + size));
            if key == "com.apple.quarantine" {
                skipped = true;
            } else {
                if !allowed(key) {
                    return Err(INVALID.into());
                }
                attributes.insert(key.to_string(), b[offset..offset + size].to_vec());
            }
            at = end;
        }
        ranges.sort();
        if ranges.windows(2).any(|p| p[0].1 > p[1].0) {
            return Err(INVALID.into());
        }
    }
    Ok((attributes, flags & 0x4000 != 0, skipped))
}
fn modified(extra: &[u8]) -> Result<i64, String> {
    let mut at = 0;
    let mut result = None;
    while at < extra.len() {
        let h = extra.get(at..at + 4).ok_or(INVALID)?;
        let kind = u16::from_le_bytes([h[0], h[1]]);
        let size = u16::from_le_bytes([h[2], h[3]]) as usize;
        let data = extra.get(at + 4..at + 4 + size).ok_or(INVALID)?;
        let time = match kind {
            0x5455 if data.first().is_some_and(|b| b & 1 != 0) => {
                Some(data.get(1..5).ok_or(INVALID)?)
            }
            0x5855 if data.len() >= 8 => Some(&data[4..8]),
            _ => None,
        };
        if let Some(t) = time {
            let t = u32::from_le_bytes(t.try_into().map_err(|_| INVALID)?) as i64;
            if result.is_some_and(|r| r != t) {
                return Err(INVALID.into());
            }
            result = Some(t);
        }
        at += 4 + size;
    }
    result.ok_or_else(|| "ZIP 未提供明确的 Unix 修改时间，不能完整恢复文件属性。".into())
}
fn pair(name: &str) -> Option<String> {
    let raw = name.strip_prefix("__MACOSX/").unwrap_or(name);
    let (parent, leaf) = raw.rsplit_once('/').unwrap_or(("", raw));
    let leaf = leaf.strip_prefix("._")?;
    if leaf.is_empty() {
        return None;
    }
    Some(if parent.is_empty() {
        leaf.into()
    } else {
        format!("{parent}/{leaf}")
    })
}
impl Metadata {
    pub(super) fn hash_into(&self, hash: &mut Sha256) {
        hash.update(b"MooMole Mac metadata v1\0");
        for (name, p) in &self.entries {
            hash.update((name.len() as u64).to_le_bytes());
            hash.update(name.as_bytes());
            hash.update(p.mode.to_le_bytes());
            hash.update(p.modified.to_le_bytes());
            hash.update((p.attributes.len() as u64).to_le_bytes());
            for (key, value) in &p.attributes {
                hash.update((key.len() as u64).to_le_bytes());
                hash.update(key.as_bytes());
                hash.update((value.len() as u64).to_le_bytes());
                hash.update(value);
            }
        }
    }
    pub(super) fn read(
        input: &mut File,
        entries: &mut Vec<Entry>,
        job: &Job,
        start: Instant,
        password: Option<&password::Password>,
    ) -> Result<Self, String> {
        input.rewind().map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(input.try_clone().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let mut consumed = BTreeSet::new();
        let mut paired = BTreeMap::new();
        let mut bytes = 0;
        let mut summary = Summary {
            items: 0,
            attributes: 0,
            executable: 0,
            finder_hidden: 0,
            skipped_quarantine: 0,
        };
        for e in entries.iter().filter(|e| !e.directory) {
            check(job, start)?;
            let Some(target) = pair(&e.name) else {
                continue;
            };
            let original = entries.iter().find(|o| o.name == target).ok_or(INVALID)?;
            if target == e.name || pair(&target).is_some() || e.size > ITEM_LIMIT as u64 {
                return Err(INVALID.into());
            }
            bytes += e.size;
            if bytes > TOTAL_LIMIT {
                return Err("Mac 元数据总量超过 256 MiB，未恢复。".into());
            }
            let mut raw = password::open(&mut zip, e.index.ok_or(INVALID)?, password)?;
            let mut b = Vec::with_capacity(e.size as usize);
            let mut buf = [0; 128 * 1024];
            loop {
                check(job, start)?;
                let n = raw.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                if b.len() + n > e.size as usize {
                    return Err(INVALID.into());
                }
                b.extend_from_slice(&buf[..n]);
            }
            if b.len() != e.size as usize || format!("{:x}", Sha256::digest(&b)) != e.sha256 {
                return Err(INVALID.into());
            }
            let (attributes, hidden, skipped) = parse(&b, original.directory)?;
            summary.attributes += attributes.len();
            summary.finder_hidden += usize::from(hidden);
            summary.skipped_quarantine += usize::from(skipped);
            if paired.insert(target, attributes).is_some() {
                return Err(INVALID.into());
            }
            consumed.insert(e.name.clone());
        }
        // Remove only parsed, paired sidecars and now-empty container directories.
        entries.retain(|e| !consumed.contains(&e.name));
        let mut removable = BTreeSet::new();
        for e in entries
            .iter()
            .rev()
            .filter(|e| e.directory && (e.name == "__MACOSX" || e.name.starts_with("__MACOSX/")))
        {
            let prefix = e.name.clone() + "/";
            if consumed.iter().any(|n| n.starts_with(&prefix))
                && !entries
                    .iter()
                    .any(|c| c.name.starts_with(&prefix) && !removable.contains(&c.name))
            {
                removable.insert(e.name.clone());
            }
        }
        entries.retain(|e| !removable.contains(&e.name));
        let mut properties = BTreeMap::new();
        for e in entries.iter() {
            check(job, start)?;
            if e.name == "__MACOSX" || e.name.starts_with("__MACOSX/") {
                return Err(INVALID.into());
            }
            let raw = password::open(
                &mut zip,
                e.index.ok_or("ZIP 缺少明确的目录属性，无法完整恢复。")?,
                password,
            )?;
            let mode = raw.unix_mode().ok_or(INVALID)? & 0o7777;
            if mode & 0o7000 != 0 || mode & 0o400 == 0 || (e.directory && mode & 0o100 == 0) {
                return Err("ZIP 含特殊或不可读权限，未恢复属性。".into());
            }
            let modified = modified(raw.extra_data().unwrap_or_default())?;
            summary.executable += usize::from(!e.directory && mode & 0o111 != 0);
            properties.insert(
                e.name.clone(),
                Properties {
                    mode,
                    modified,
                    attributes: paired.remove(&e.name).unwrap_or_default(),
                },
            );
        }
        if !paired.is_empty() {
            return Err(INVALID.into());
        }
        summary.items = properties.len();
        Ok(Self {
            summary,
            bytes,
            entries: properties,
        })
    }
}
pub(super) fn fresh_quarantine() -> Result<Vec<u8>, String> {
    extern "C" {
        fn moomole_archive_quarantine(path: *const libc::c_char) -> libc::c_int;
    }
    let dir = tempfile::Builder::new()
        .prefix("moomole-archive-isolation-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let f = tempfile::NamedTempFile::new_in(dir.path()).map_err(|e| e.to_string())?;
    let path = CString::new(f.path().as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    if unsafe { moomole_archive_quarantine(path.as_ptr()) } != 1 {
        return Err("无法创建解压内容的系统隔离标记，未恢复。".into());
    }
    quarantine(f.as_file())?
        .filter(|q| !q.is_empty())
        .ok_or("系统隔离标记未能读回核对，未恢复。".into())
}
fn attributes(file: &File) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let fd = file.as_raw_fd();
    let len = unsafe { libc::flistxattr(fd, std::ptr::null_mut(), 0, 0) };
    if !(0..=65536).contains(&len) {
        return Err(INVALID.into());
    }
    let mut names = vec![0; len as usize];
    if unsafe { libc::flistxattr(fd, names.as_mut_ptr().cast(), names.len(), 0) } != len {
        return Err(INVALID.into());
    }
    let mut out = BTreeMap::new();
    let mut total = 0;
    for key in names.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        if key == b"com.apple.quarantine" {
            continue;
        }
        let name = CString::new(key).map_err(|_| INVALID)?;
        let size = unsafe { libc::fgetxattr(fd, name.as_ptr(), std::ptr::null_mut(), 0, 0, 0) };
        if size < 0 || size as usize > ITEM_LIMIT - total {
            return Err(INVALID.into());
        }
        total += size as usize;
        let mut value = vec![0; size as usize];
        if unsafe {
            libc::fgetxattr(
                fd,
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
                0,
                0,
            )
        } != size
        {
            return Err(INVALID.into());
        }
        out.insert(
            std::str::from_utf8(key).map_err(|_| INVALID)?.to_string(),
            value,
        );
    }
    Ok(out)
}
impl Properties {
    pub(super) fn apply(&self, file: &File) -> Result<(), String> {
        if crate::permissions::has_acl(file).map_err(|_| INVALID)? || !attributes(file)?.is_empty()
        {
            return Err("解压临时内容存在未核对的属性或 ACL，未恢复。".into());
        }
        for (key, value) in &self.attributes {
            let key = CString::new(key.as_bytes()).map_err(|_| INVALID)?;
            if unsafe {
                libc::fsetxattr(
                    file.as_raw_fd(),
                    key.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    0,
                    libc::XATTR_CREATE,
                )
            } != 0
            {
                return Err("Mac 属性写入失败，未发布解压结果。".into());
            }
        }
        let times = [
            libc::timespec {
                tv_sec: 0,
                tv_nsec: libc::UTIME_OMIT,
            },
            libc::timespec {
                tv_sec: self.modified,
                tv_nsec: 0,
            },
        ];
        if unsafe { libc::fchmod(file.as_raw_fd(), self.mode as libc::mode_t) } != 0
            || unsafe { libc::futimens(file.as_raw_fd(), times.as_ptr()) } != 0
        {
            return Err("解压权限或修改时间未恢复，未发布。".into());
        }
        file.sync_all().map_err(|e| e.to_string())?;
        if file.metadata().map_err(|e| e.to_string())?.is_file()
            && unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } != 0
        {
            return Err("Mac 属性同步未完成，未发布。".into());
        }
        Ok(())
    }
    pub(super) fn verify(&self, file: &File) -> Result<(), String> {
        let m = file.metadata().map_err(|e| e.to_string())?;
        if m.mode() & 0o7777 != self.mode
            || m.mtime() != self.modified
            || m.mtime_nsec() != 0
            || crate::permissions::has_acl(file).map_err(|_| INVALID)?
            || attributes(file)? != self.attributes
        {
            return Err("Mac 属性、权限或修改时间读回不符，未确认成功。".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
