//! Bounded container checks, not a codec validator. Require self-contained data
//! references and refuse reference movies, compressed metadata and DRM entries.
use super::*;
const META: u64 = 8 * 1024 * 1024;
fn header(bytes: &[u8], remaining: u64) -> Result<([u8; 4], u64, usize), String> {
    if bytes.len() < 8 {
        return Err("影音容器头不完整。".into());
    }
    let mut size = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as u64;
    let mut head = 8;
    if size == 1 {
        if bytes.len() < 16 {
            return Err("影音扩展容器头不完整。".into());
        }
        size = u64::from_be_bytes(bytes[8..16].try_into().unwrap());
        head = 16;
    } else if size == 0 {
        size = remaining;
    }
    if size < head as u64 || size > remaining {
        return Err("影音容器长度无法核对。".into());
    }
    Ok((bytes[4..8].try_into().unwrap(), size, head))
}
pub(super) fn boxes(
    bytes: &[u8],
    depth: usize,
    count: &mut usize,
    refs: &mut usize,
) -> Result<(), String> {
    if depth > 12 {
        return Err("影音元数据层级超出本轮范围。".into());
    }
    let mut offset = 0;
    while offset < bytes.len() {
        *count += 1;
        if *count > 4096 {
            return Err("影音元数据条目超出本轮范围。".into());
        }
        let (kind, size, head) = header(&bytes[offset..], (bytes.len() - offset) as u64)?;
        let end = offset + size as usize;
        let body = &bytes[offset + head..end];
        match &kind {
            b"rmra" | b"rdrf" | b"cmov" | b"pssh" => {
                return Err("外部引用、压缩元数据或受保护影音不在本轮播放范围。".into())
            }
            b"trak" => {
                let mut track_refs = 0;
                boxes(body, depth + 1, count, &mut track_refs)?;
                if track_refs != 1 {
                    return Err("影音轨道的数据来源无法核对。".into());
                }
                *refs += track_refs;
            }
            b"mdia" | b"minf" | b"dinf" | b"stbl" => boxes(body, depth + 1, count, refs)?,
            b"dref" => {
                if body.len() < 8 || body[..4] != [0, 0, 0, 0] {
                    return Err("影音数据引用结构未知。".into());
                }
                let entries = u32::from_be_bytes(body[4..8].try_into().unwrap()) as usize;
                if entries == 0 || entries > 64 || body.len() != 8 + 12 * entries {
                    return Err("影音包含外部或未知数据引用。".into());
                }
                for entry in body[8..].chunks_exact(12) {
                    if entry[..4] != 12u32.to_be_bytes()
                        || &entry[4..8] != b"url "
                        || entry[8..12] != [0, 0, 0, 1]
                    {
                        return Err("影音包含外部或未知数据引用。".into());
                    }
                }
                *refs += 1;
            }
            b"stsd" => {
                if body.len() < 8 {
                    return Err("影音编码目录不完整。".into());
                }
                let entries = u32::from_be_bytes(body[4..8].try_into().unwrap());
                if entries > 64 {
                    return Err("影音编码目录超出范围。".into());
                }
                let mut pos = 8;
                for _ in 0..entries {
                    let (kind, size, _) = header(&body[pos..], (body.len() - pos) as u64)?;
                    if matches!(&kind, b"encv" | b"enca" | b"drms" | b"drmi") {
                        return Err("加密或 DRM 影音不在本轮播放范围。".into());
                    }
                    pos += size as usize;
                }
                if pos != body.len() {
                    return Err("影音编码目录长度未知。".into());
                }
            }
            _ => {}
        }
        offset = end;
    }
    Ok(())
}
pub(in crate::personal_files) fn self_contained(file: &File, len: u64) -> Result<(), String> {
    let (mut offset, mut count, mut moov, mut media) = (0, 0, 0, 0);
    while offset < len {
        count += 1;
        if count > 2048 {
            return Err("影音顶层容器超出本轮范围。".into());
        }
        let prefix = read_at(file, offset, (len - offset).min(16))?;
        let (kind, size, head) = header(&prefix, len - offset)?;
        match &kind {
            b"moov" => {
                if size > META {
                    return Err("影音元数据超过八 MiB 核验范围。".into());
                }
                let bytes = read_at(file, offset + head as u64, size - head as u64)?;
                let mut refs = 0;
                let mut boxes_count = 0;
                boxes(&bytes, 0, &mut boxes_count, &mut refs)?;
                if refs == 0 {
                    return Err("影音缺少可核对的本地数据引用。".into());
                }
                moov += 1;
            }
            b"mdat" => media += 1,
            b"rmra" | b"rdrf" | b"cmov" | b"pssh" => {
                return Err("外部引用或受保护影音不在本轮播放范围。".into())
            }
            _ => {}
        }
        offset = offset.checked_add(size).ok_or("影音长度溢出。")?;
    }
    if moov != 1 || media == 0 {
        return Err("不是本轮支持的自包含 MP4/MOV 文件。".into());
    }
    Ok(())
}
