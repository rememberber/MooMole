//! Bounded native-header classification, not a signature, full Mach-O validator
//! or a promise that dyld can launch it. Fat magic alone also occurs in Java.
use std::io::{self, Read, Seek, SeekFrom};

#[derive(Clone, Copy)]
struct Endian(bool);
impl Endian {
    fn u32(self, b: &[u8]) -> u32 {
        let b: [u8; 4] = b.try_into().unwrap();
        if self.0 {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    }
    fn u64(self, b: &[u8]) -> u64 {
        let b: [u8; 8] = b.try_into().unwrap();
        if self.0 {
            u64::from_le_bytes(b)
        } else {
            u64::from_be_bytes(b)
        }
    }
}
fn read<const N: usize>(file: &mut (impl Read + Seek), offset: u64) -> io::Result<[u8; N]> {
    file.seek(SeekFrom::Start(offset))?;
    let mut b = [0; N];
    file.read_exact(&mut b)?;
    Ok(b)
}
fn thin(
    file: &mut (impl Read + Seek),
    offset: u64,
    size: u64,
    cpu: Option<u32>,
) -> io::Result<bool> {
    let magic = read::<4>(file, offset)?;
    let (endian, header, abi64) = match magic {
        [0xce, 0xfa, 0xed, 0xfe] => (Endian(true), 28, false),
        [0xfe, 0xed, 0xfa, 0xce] => (Endian(false), 28, false),
        [0xcf, 0xfa, 0xed, 0xfe] => (Endian(true), 32, true),
        [0xfe, 0xed, 0xfa, 0xcf] => (Endian(false), 32, true),
        _ => return Ok(false),
    };
    if size < header {
        return Ok(false);
    }
    let fields = read::<24>(file, offset + 4)?;
    let actual_cpu = endian.u32(&fields[0..4]);
    let commands = endian.u32(&fields[12..16]) as u64;
    let bytes = endian.u32(&fields[16..20]) as u64;
    Ok(matches!(actual_cpu,7|12|0x01000007|0x0100000c)
        && (actual_cpu & 0x01000000!=0)==abi64 && cpu.is_none_or(|expected|expected==actual_cpu)
        && endian.u32(&fields[8..12])==2 // MH_EXECUTE
        && commands>0 && commands<=4096 && bytes>=commands*8 && bytes<=size-header)
}
pub(super) fn native_executable(file: &mut (impl Read + Seek), size: u64) -> io::Result<bool> {
    let magic = read::<4>(file, 0)?;
    let (endian, wide) = match magic {
        [0xca, 0xfe, 0xba, 0xbe] => (Endian(false), false),
        [0xbe, 0xba, 0xfe, 0xca] => (Endian(true), false),
        [0xca, 0xfe, 0xba, 0xbf] => (Endian(false), true),
        [0xbf, 0xba, 0xfe, 0xca] => (Endian(true), true),
        _ => return thin(file, 0, size, None),
    };
    let count = endian.u32(&read::<4>(file, 4)?) as usize;
    if !(1..=32).contains(&count) {
        return Ok(false);
    }
    let stride = if wide { 32 } else { 20 };
    let table_end = 8 + (count * stride) as u64;
    if table_end > size {
        return Ok(false);
    }
    let mut ranges = Vec::new();
    for index in 0..count {
        let at = 8 + (index * stride) as u64;
        let cpu = endian.u32(&read::<4>(file, at)?);
        let (offset, length) = if wide {
            let raw = read::<16>(file, at + 8)?;
            (endian.u64(&raw[..8]), endian.u64(&raw[8..]))
        } else {
            let raw = read::<8>(file, at + 8)?;
            (endian.u32(&raw[..4]) as u64, endian.u32(&raw[4..]) as u64)
        };
        let Some(end) = offset.checked_add(length) else {
            return Ok(false);
        };
        if offset < table_end
            || end > size
            || length < 28
            || ranges.iter().any(|&(a, b)| offset < b && end > a)
        {
            return Ok(false);
        }
        if !thin(file, offset, length, Some(cpu))? {
            return Ok(false);
        }
        ranges.push((offset, end));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn image(cpu: u32) -> Vec<u8> {
        let mut b = vec![0; 40];
        b[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        b[4..8].copy_from_slice(&cpu.to_le_bytes());
        b[12..16].copy_from_slice(&2u32.to_le_bytes());
        b[16..20].copy_from_slice(&1u32.to_le_bytes());
        b[20..24].copy_from_slice(&8u32.to_le_bytes());
        b
    }
    fn supported(b: Vec<u8>) -> bool {
        let size = b.len() as u64;
        native_executable(&mut Cursor::new(b), size).unwrap_or(false)
    }
    #[test]
    fn native_headers_require_cpu_executable_kind_and_bounded_command_table() {
        for cpu in [0x01000007, 0x0100000c] {
            assert!(supported(image(cpu)));
        }
        for mode in [
            "cpu",
            "dylib",
            "zeroCommands",
            "tooManyCommands",
            "tooManyBytes",
            "short",
        ] {
            let mut b = image(0x0100000c);
            match mode {
                "cpu" => b[4..8].copy_from_slice(&7u32.to_le_bytes()),
                "dylib" => b[12..16].copy_from_slice(&6u32.to_le_bytes()),
                "zeroCommands" => b[16..20].fill(0),
                "tooManyCommands" => b[16..20].copy_from_slice(&4097u32.to_le_bytes()),
                "tooManyBytes" => b[20..24].copy_from_slice(&1000u32.to_le_bytes()),
                "short" => b.truncate(4),
                _ => unreachable!(),
            }
            assert!(!supported(b), "{mode}");
        }
        assert!(!supported(b"#!/bin/sh\nexit 0".to_vec()));
        assert!(!supported(vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61]));
    }
    #[test]
    fn universal_headers_bind_every_slice_and_refuse_overlap_or_overflow() {
        for wide in [false, true] {
            let stride = if wide { 32 } else { 20 };
            let mut base = vec![0; 296];
            base[..4].copy_from_slice(if wide {
                &[0xca, 0xfe, 0xba, 0xbf]
            } else {
                &[0xca, 0xfe, 0xba, 0xbe]
            });
            base[4..8].copy_from_slice(&2u32.to_be_bytes());
            for (i, (cpu, offset)) in [(0x01000007u32, 128u64), (0x0100000c, 256)]
                .iter()
                .enumerate()
            {
                let at = 8 + i * stride;
                base[at..at + 4].copy_from_slice(&cpu.to_be_bytes());
                if wide {
                    base[at + 8..at + 16].copy_from_slice(&offset.to_be_bytes());
                    base[at + 16..at + 24].copy_from_slice(&40u64.to_be_bytes());
                } else {
                    base[at + 8..at + 12].copy_from_slice(&(*offset as u32).to_be_bytes());
                    base[at + 12..at + 16].copy_from_slice(&40u32.to_be_bytes());
                }
                base[*offset as usize..*offset as usize + 40].copy_from_slice(&image(*cpu));
            }
            assert!(supported(base.clone()));
            for mode in ["overlap", "outside", "wrongCpu", "nonNativeSlice"] {
                let mut bad = base.clone();
                let at = 8 + stride;
                match mode {
                    "overlap" | "outside" => {
                        if wide {
                            bad[at + 8..at + 16].copy_from_slice(
                                &(if mode == "overlap" { 128u64 } else { u64::MAX }).to_be_bytes(),
                            );
                        } else {
                            bad[at + 8..at + 12].copy_from_slice(
                                &(if mode == "overlap" { 128u32 } else { u32::MAX }).to_be_bytes(),
                            );
                        }
                    }
                    "wrongCpu" => bad[at..at + 4].copy_from_slice(&0x01000007u32.to_be_bytes()),
                    "nonNativeSlice" => bad[256..260].copy_from_slice(b"#!sh"),
                    _ => unreachable!(),
                }
                assert!(!supported(bad), "{wide}/{mode}");
            }
        }
    }
}
