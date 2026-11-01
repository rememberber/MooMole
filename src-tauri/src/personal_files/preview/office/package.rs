use super::*;
use std::{collections::HashSet, io::Read};
pub(super) struct Package<'a> {
    zip: zip::ZipArchive<Cursor<&'a [u8]>>,
    entries: HashMap<String, usize>,
    read: usize,
    start: Instant,
}
pub(super) struct Relationship {
    kind: String,
    target: String,
    external: bool,
}
impl Relationship {
    pub fn kind(&self, kind: &str) -> bool {
        self.kind
            == format!("http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}")
            || self.kind
                == format!("http://purl.oclc.org/ooxml/officeDocument/relationships/{kind}")
    }
    pub fn target(&self, source: &str, kind: &str) -> Result<String, String> {
        if self.external || !self.kind(kind) {
            return Err(FAIL.into());
        }
        target(source, &self.target)
    }
}
pub(super) fn unique<'a>(
    rels: &'a HashMap<String, Relationship>,
    kind: &str,
) -> Result<&'a Relationship, String> {
    let found = rels.values().filter(|r| r.kind(kind)).collect::<Vec<_>>();
    if found.len() != 1 {
        return Err(FAIL.into());
    }
    Ok(found[0])
}
fn safe(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 4096
        && !name.contains(['\\', ':', '?', '#', '%'])
        && !name.chars().any(char::is_control)
        && name.split('/').count() <= 64
        && name
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != ".." && p.len() <= 255)
}
fn target(source: &str, raw: &str) -> Result<String, String> {
    if raw.is_empty() || raw.len() > 4096 || raw.starts_with("//") {
        return Err(FAIL.into());
    }
    let mut decoded = Vec::new();
    let b = raw.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let s = b.get(i + 1..i + 3).ok_or(FAIL)?;
            let hex = std::str::from_utf8(s).map_err(|_| FAIL)?;
            let v = u8::from_str_radix(hex, 16).map_err(|_| FAIL)?;
            if [b'/', b'\\'].contains(&v) {
                return Err(FAIL.into());
            }
            decoded.push(v);
            i += 3;
        } else {
            decoded.push(b[i]);
            i += 1;
        }
    }
    let decoded = std::str::from_utf8(&decoded).map_err(|_| FAIL)?;
    let mut parts = if decoded.starts_with('/') {
        vec![]
    } else {
        source
            .rsplit_once('/')
            .map(|(p, _)| p.split('/').collect::<Vec<_>>())
            .unwrap_or_default()
    };
    for p in decoded.trim_start_matches('/').split('/') {
        match p {
            "." => {}
            ".." => {
                parts.pop().ok_or(FAIL)?;
            }
            "" => return Err(FAIL.into()),
            _ => parts.push(p),
        }
    }
    let name = parts.join("/");
    if !safe(&name) {
        return Err(FAIL.into());
    }
    Ok(name)
}
impl<'a> Package<'a> {
    pub fn open(bytes: &'a [u8], start: Instant) -> Result<Self, String> {
        if bytes.len() < 22
            || bytes.len() > IMAGE_LIMIT as usize
            || !bytes.starts_with(b"PK\x03\x04")
        {
            return Err(FAIL.into());
        }
        // Reject every possible ZIP64 header before zip can fall back and allocate
        // from an embedded record. Office preview currently accepts ordinary ZIP.
        for offset in (0..bytes.len()).step_by(65536) {
            budget(start)?;
            let end = (offset + 65536).min(bytes.len());
            if bytes[offset.saturating_sub(3)..end]
                .windows(4)
                .any(|b| b == b"PK\x06\x06")
            {
                return Err(FAIL.into());
            }
        }
        let end = (bytes.len().saturating_sub(65557)..bytes.len() - 21)
            .rev()
            .find(|&p| {
                &bytes[p..p + 4] == b"PK\x05\x06"
                    && p + 22
                        + u16::from_le_bytes(bytes[p + 20..p + 22].try_into().unwrap()) as usize
                        == bytes.len()
            })
            .ok_or(FAIL)?;
        let h = &bytes[end..];
        let u16at = |p| u16::from_le_bytes(h[p..p + 2].try_into().unwrap());
        let u32at = |p| u32::from_le_bytes(h[p..p + 4].try_into().unwrap()) as usize;
        let count = u16at(10) as usize;
        if u16at(4) != 0
            || u16at(6) != 0
            || u16at(8) as usize != count
            || count == 0
            || count > 4096
            || u32at(16).checked_add(u32at(12)) != Some(end)
            || u32at(12) < count * 46
        {
            return Err(FAIL.into());
        }
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| FAIL)?;
        if zip.len() != count || zip.offset() != 0 {
            return Err(FAIL.into());
        }
        let mut entries = HashMap::new();
        let mut names = HashSet::new();
        let mut total = 0u64;
        for index in 0..zip.len() {
            budget(start)?;
            let file = zip.by_index(index).map_err(|_| FAIL)?;
            let raw = std::str::from_utf8(file.name_raw()).map_err(|_| FAIL)?;
            let name = raw.trim_end_matches('/');
            if !safe(name)
                || !names.insert(name.to_ascii_lowercase())
                || file.is_symlink()
                || file.unix_mode().is_some_and(|m| {
                    ![0, libc::S_IFREG as u32, libc::S_IFDIR as u32]
                        .contains(&(m & libc::S_IFMT as u32))
                })
                || !matches!(
                    file.compression(),
                    zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
                )
            {
                return Err(FAIL.into());
            }
            total = total.checked_add(file.size()).ok_or(FAIL)?;
            if total > 256 * 1024 * 1024 {
                return Err(FAIL.into());
            }
            if !file.is_dir() {
                entries.insert(name.to_ascii_lowercase(), index);
            }
        }
        Ok(Self {
            zip,
            entries,
            read: 0,
            start,
        })
    }
    pub fn xml(&mut self, name: &str) -> Result<xml::Node, String> {
        budget(self.start)?;
        let index = self.entries.get(&name.to_ascii_lowercase()).ok_or(FAIL)?;
        let mut file = self.zip.by_index(*index).map_err(|_| FAIL)?;
        if file.size() > PART_LIMIT as u64 || self.read + file.size() as usize > READ_LIMIT {
            return Err(FAIL.into());
        }
        let expected = file.size();
        let mut bytes = vec![];
        file.by_ref()
            .take(PART_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| FAIL)?;
        if bytes.len() > PART_LIMIT || bytes.len() as u64 != expected {
            return Err(FAIL.into());
        }
        self.read += bytes.len();
        xml::parse(&bytes, self.start)
    }
    pub fn rels(&mut self, source: &str) -> Result<HashMap<String, Relationship>, String> {
        let path = if source.is_empty() {
            "_rels/.rels".into()
        } else {
            let (parent, name) = source.rsplit_once('/').unwrap_or(("", source));
            format!(
                "{}{}_rels/{name}.rels",
                parent,
                if parent.is_empty() { "" } else { "/" }
            )
        };
        let root = self.xml(&path)?;
        root.require("rel", "Relationships")?;
        let mut rels = HashMap::new();
        for n in root.children {
            n.require("rel", "Relationship")?;
            let id = n
                .attr("", "Id")
                .filter(|s| !s.is_empty())
                .ok_or(FAIL)?
                .to_string();
            let mode = n.attr("", "TargetMode");
            if ![None, Some("External"), Some("Internal")].contains(&mode) {
                return Err(FAIL.into());
            }
            let rel = Relationship {
                kind: n.attr("", "Type").ok_or(FAIL)?.into(),
                target: n.attr("", "Target").ok_or(FAIL)?.into(),
                external: mode == Some("External"),
            };
            if rels.insert(id, rel).is_some() {
                return Err(FAIL.into());
            }
        }
        Ok(rels)
    }
}

pub(super) struct Types {
    overrides: HashMap<String, String>,
    defaults: HashMap<String, String>,
}
impl Types {
    pub fn parse(root: &xml::Node, start: Instant) -> Result<Self, String> {
        root.require("ct", "Types")?;
        let mut types = Self {
            overrides: HashMap::new(),
            defaults: HashMap::new(),
        };
        for n in &root.children {
            budget(start)?;
            let value = n
                .attr("", "ContentType")
                .filter(|v| !v.is_empty())
                .ok_or(FAIL)?
                .to_string();
            let (map, key) = if n.is("ct", "Override") {
                let raw = n
                    .attr("", "PartName")
                    .filter(|v| v.starts_with('/') && !v.starts_with("//"))
                    .ok_or(FAIL)?;
                (&mut types.overrides, target("", raw)?.to_ascii_lowercase())
            } else if n.is("ct", "Default") {
                let raw = n
                    .attr("", "Extension")
                    .filter(|v| {
                        !v.is_empty()
                            && v.len() <= 64
                            && v.bytes().all(|b| b.is_ascii_alphanumeric())
                    })
                    .ok_or(FAIL)?;
                (&mut types.defaults, raw.to_ascii_lowercase())
            } else {
                return Err(FAIL.into());
            };
            if map.insert(key, value).is_some() {
                return Err(FAIL.into());
            }
        }
        Ok(types)
    }
    pub fn require_type(&self, name: &str, expected: &str) -> Result<(), String> {
        let name = name.to_ascii_lowercase();
        let value = self.overrides.get(&name).or_else(|| {
            name.rsplit_once('.')
                .and_then(|(_, ext)| self.defaults.get(ext))
        });
        if value.map(String::as_str) != Some(expected) {
            return Err(FAIL.into());
        }
        Ok(())
    }
}
