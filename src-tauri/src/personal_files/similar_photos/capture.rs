//! Capture hints from the same bounded, identity-checked bytes as the visual feature.
//! No filename/mtime inference and no claim of a camera's authoritative burst ID.
use core_foundation_sys::{
    base::{CFGetTypeID, CFRelease},
    data::CFDataCreate,
    dictionary::{CFDictionaryGetTypeID, CFDictionaryGetValue},
    string::{kCFStringEncodingUTF8, CFStringGetCString, CFStringGetLength, CFStringGetTypeID},
};
use std::{ffi::c_void, ptr};
type Ref = *const c_void;
#[link(name = "ImageIO", kind = "framework")]
extern "C" {
    fn CGImageSourceCreateWithData(data: Ref, options: Ref) -> Ref;
    fn CGImageSourceCopyPropertiesAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    static kCGImagePropertyExifDictionary: Ref;
    static kCGImagePropertyTIFFDictionary: Ref;
    static kCGImagePropertyExifDateTimeOriginal: Ref;
    static kCGImagePropertyExifSubsecTimeOriginal: Ref;
    static kCGImagePropertyExifOffsetTimeOriginal: Ref;
    static kCGImagePropertyExifBodySerialNumber: Ref;
    static kCGImagePropertyTIFFMake: Ref;
    static kCGImagePropertyTIFFModel: Ref;
}
struct Owned(Ref);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}
fn owned(p: Ref) -> Option<Owned> {
    if p.is_null() {
        None
    } else {
        Some(Owned(p))
    }
}
unsafe fn value(dict: Ref, key: Ref) -> Ref {
    if dict.is_null() || CFGetTypeID(dict) != CFDictionaryGetTypeID() {
        return ptr::null();
    }
    CFDictionaryGetValue(dict.cast(), key)
}
unsafe fn string(raw: Ref) -> Option<String> {
    if raw.is_null() || CFGetTypeID(raw) != CFStringGetTypeID() {
        return None;
    }
    let len = CFStringGetLength(raw.cast());
    if !(1..=128).contains(&len) {
        return None;
    }
    let mut out = [0u8; 513];
    if CFStringGetCString(
        raw.cast(),
        out.as_mut_ptr().cast(),
        out.len() as isize,
        kCFStringEncodingUTF8,
    ) == 0
    {
        return None;
    }
    let end = out.iter().position(|b| *b == 0)?;
    let s = std::str::from_utf8(&out[..end]).ok()?;
    if s.trim().is_empty() || s.chars().any(char::is_control) {
        return None;
    }
    Some(s.into())
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Metadata {
    pub date: String,
    pub nanos: u64,
    pub label: String,
    pub camera: (String, String, Option<String>),
    pub offset: Option<String>,
}
fn parse(
    time: &str,
    subsecond: Option<&str>,
    offset: Option<&str>,
    make: String,
    model: String,
    serial: Option<String>,
) -> Option<Metadata> {
    let b = time.as_bytes();
    if b.len() != 19
        || !b.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b':',
            10 => *b == b' ',
            13 | 16 => *b == b':',
            _ => b.is_ascii_digit(),
        })
    {
        return None;
    }
    let n = |a, b| time[a..b].parse::<u32>().ok();
    let (year, month, day, hour, minute, second) = (
        n(0, 4)?,
        n(5, 7)?,
        n(8, 10)?,
        n(11, 13)?,
        n(14, 16)?,
        n(17, 19)?,
    );
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return None,
    };
    if !(1970..=9999).contains(&year)
        || day == 0
        || day > days
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let fractional = if let Some(s) = subsecond {
        if s.is_empty() || s.len() > 9 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        format!("{s}000000000")[..9].parse::<u64>().ok()?
    } else {
        0
    };
    if let Some(s) = offset {
        let b = s.as_bytes();
        if b.len() != 6
            || ![b'+', b'-'].contains(&b[0])
            || b[3] != b':'
            || ![1, 2, 4, 5].iter().all(|i| b[*i].is_ascii_digit())
        {
            return None;
        }
        let h = s[1..3].parse::<u32>().ok()?;
        let m = s[4..6].parse::<u32>().ok()?;
        if h > 14 || m > 59 || h == 14 && m != 0 {
            return None;
        }
    }
    Some(Metadata {
        date: time[..10].into(),
        nanos: u64::from((hour * 60 + minute) * 60 + second) * 1_000_000_000 + fractional,
        label: format!(
            "{time}{}{}",
            if let Some(s) = subsecond {
                format!(".{s}")
            } else {
                String::new()
            },
            offset.unwrap_or("")
        ),
        camera: (make, model, serial),
        offset: offset.map(str::to_owned),
    })
}
impl Metadata {
    pub(super) fn follows(&self, anchor: &Self) -> bool {
        self.date == anchor.date
            && self.camera == anchor.camera
            && self.offset == anchor.offset
            && self.nanos >= anchor.nanos
            && self.nanos - anchor.nanos <= 2_000_000_000
    }
}
pub(super) fn read(bytes: &[u8]) -> Option<Metadata> {
    if bytes.len() > 32 * 1024 * 1024 {
        return None;
    }
    unsafe {
        let data = owned(CFDataCreate(ptr::null(), bytes.as_ptr(), bytes.len() as isize).cast())?;
        let source = owned(CGImageSourceCreateWithData(data.0, ptr::null()))?;
        let props = owned(CGImageSourceCopyPropertiesAtIndex(source.0, 0, ptr::null()))?;
        let exif = value(props.0, kCGImagePropertyExifDictionary);
        let tiff = value(props.0, kCGImagePropertyTIFFDictionary);
        let time = string(value(exif, kCGImagePropertyExifDateTimeOriginal))?;
        // A present but malformed hint cannot silently fall back to weaker evidence.
        let optional = |key| {
            let raw = value(exif, key);
            if raw.is_null() {
                Some(None)
            } else {
                string(raw).map(Some)
            }
        };
        let subsecond = optional(kCGImagePropertyExifSubsecTimeOriginal)?;
        let offset = optional(kCGImagePropertyExifOffsetTimeOriginal)?;
        parse(
            &time,
            subsecond.as_deref(),
            offset.as_deref(),
            string(value(tiff, kCGImagePropertyTIFFMake))?,
            string(value(tiff, kCGImagePropertyTIFFModel))?,
            optional(kCGImagePropertyExifBodySerialNumber)?,
        )
    }
}
#[cfg(test)]
pub(super) fn fixture_image(png: &[u8], time: &str, subsecond: &str, model: &str) -> Vec<u8> {
    use core_foundation_sys::{
        data::{CFDataCreateMutable, CFDataGetBytePtr, CFDataGetLength},
        dictionary::{
            kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks, CFDictionaryCreate,
        },
        string::CFStringCreateWithBytes,
    };
    #[link(name = "ImageIO", kind = "framework")]
    extern "C" {
        fn CGImageDestinationCreateWithData(
            data: Ref,
            kind: Ref,
            count: usize,
            options: Ref,
        ) -> Ref;
        fn CGImageDestinationAddImageFromSource(
            dest: Ref,
            source: Ref,
            index: usize,
            properties: Ref,
        );
        fn CGImageDestinationFinalize(dest: Ref) -> bool;
    }
    unsafe fn text(s: &str) -> Owned {
        owned(
            CFStringCreateWithBytes(
                ptr::null(),
                s.as_ptr(),
                s.len() as isize,
                kCFStringEncodingUTF8,
                0,
            )
            .cast(),
        )
        .unwrap()
    }
    unsafe fn dict(keys: &[Ref], values: &[Ref]) -> Owned {
        owned(
            CFDictionaryCreate(
                ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                keys.len() as isize,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            )
            .cast(),
        )
        .unwrap()
    }
    unsafe {
        let data =
            owned(CFDataCreate(ptr::null(), png.as_ptr(), png.len() as isize).cast()).unwrap();
        let source = owned(CGImageSourceCreateWithData(data.0, ptr::null())).unwrap();
        let (time, sub, offset, make, model) = (
            text(time),
            text(subsecond),
            text("+08:00"),
            text("MooMole owned camera"),
            text(model),
        );
        let exif = dict(
            &[
                kCGImagePropertyExifDateTimeOriginal,
                kCGImagePropertyExifSubsecTimeOriginal,
                kCGImagePropertyExifOffsetTimeOriginal,
            ],
            &[time.0, sub.0, offset.0],
        );
        let tiff = dict(
            &[kCGImagePropertyTIFFMake, kCGImagePropertyTIFFModel],
            &[make.0, model.0],
        );
        let props = dict(
            &[
                kCGImagePropertyExifDictionary,
                kCGImagePropertyTIFFDictionary,
            ],
            &[exif.0, tiff.0],
        );
        let out = owned(CFDataCreateMutable(ptr::null(), 0).cast()).unwrap();
        let kind = text("public.jpeg");
        let destination = owned(CGImageDestinationCreateWithData(
            out.0,
            kind.0,
            1,
            ptr::null(),
        ))
        .unwrap();
        CGImageDestinationAddImageFromSource(destination.0, source.0, 0, props.0);
        assert!(CGImageDestinationFinalize(destination.0));
        std::slice::from_raw_parts(
            CFDataGetBytePtr(out.0.cast()),
            CFDataGetLength(out.0.cast()) as usize,
        )
        .to_vec()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamps_are_strict_and_do_not_invent_offsets_or_precision() {
        let parsed = |t, s, o| parse(t, s, o, "Owned camera".into(), "Owned model".into(), None);
        let v = parsed("2024:02:29 12:34:56", Some("12"), Some("+08:00")).unwrap();
        assert_eq!(v.nanos, 45_296_120_000_000);
        assert_eq!(v.label, "2024:02:29 12:34:56.12+08:00");
        assert!(parsed("2023:02:29 12:34:56", None, None).is_none());
        assert!(parsed("2024:02:29 24:00:00", None, None).is_none());
        assert!(parsed("2024:02:29 12:34:56", Some("bad"), None).is_none());
        assert!(parsed("2024:02:29 12:34:56", None, Some("+14:01")).is_none());
        assert!(parsed("2024:02:29 12:34:56", None, None)
            .unwrap()
            .offset
            .is_none());
        assert!(read(b"not an image").is_none());
        let mut later = v.clone();
        later.nanos += 2_000_000_000;
        assert!(later.follows(&v));
        later.nanos += 1;
        assert!(!later.follows(&v));
        later.nanos = v.nanos;
        later.offset = None;
        assert!(!later.follows(&v));
        later.offset = v.offset.clone();
        later.camera.2 = Some("different body".into());
        assert!(!later.follows(&v));
    }
}
