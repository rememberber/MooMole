//! In-memory system raster/PDF decoding. No external paths, URLs or renderers.
use core_foundation_sys::{
    base::{CFGetTypeID, CFRelease},
    data::{CFDataCreate, CFDataCreateMutable, CFDataGetBytePtr, CFDataGetLength},
    dictionary::{
        kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks, CFDictionaryCreate,
        CFDictionaryGetValue,
    },
    number::{
        kCFBooleanFalse, kCFBooleanTrue, kCFNumberSInt64Type, CFNumberCreate, CFNumberGetTypeID,
        CFNumberGetValue,
    },
    string::{kCFStringEncodingUTF8, CFStringCreateWithBytes},
};
use std::{ffi::c_void, ptr};
type Ref = *const c_void;
#[repr(C)]
#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Size {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Rect {
    origin: Point,
    size: Size,
}
#[link(name = "ImageIO", kind = "framework")]
extern "C" {
    fn CGImageSourceCreateWithData(data: Ref, options: Ref) -> Ref;
    fn CGImageSourceGetCount(source: Ref) -> usize;
    fn CGImageSourceCopyPropertiesAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    fn CGImageSourceCreateThumbnailAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    fn CGImageDestinationCreateWithData(data: Ref, kind: Ref, count: usize, options: Ref) -> Ref;
    fn CGImageDestinationAddImage(dest: Ref, image: Ref, properties: Ref);
    fn CGImageDestinationFinalize(dest: Ref) -> bool;
    static kCGImageSourceShouldCache: Ref;
    static kCGImageSourceCreateThumbnailFromImageAlways: Ref;
    static kCGImageSourceCreateThumbnailWithTransform: Ref;
    static kCGImageSourceThumbnailMaxPixelSize: Ref;
    static kCGImagePropertyPixelWidth: Ref;
    static kCGImagePropertyPixelHeight: Ref;
}
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGImageGetWidth(image: Ref) -> usize;
    fn CGImageGetHeight(image: Ref) -> usize;
    fn CGImageRelease(image: Ref);
    fn CGDataProviderCreateWithCFData(data: Ref) -> Ref;
    fn CGDataProviderRelease(provider: Ref);
    fn CGPDFDocumentCreateWithProvider(provider: Ref) -> Ref;
    fn CGPDFDocumentRelease(doc: Ref);
    fn CGPDFDocumentGetNumberOfPages(doc: Ref) -> usize;
    fn CGPDFDocumentIsEncrypted(doc: Ref) -> bool;
    fn CGPDFDocumentGetPage(doc: Ref, page: usize) -> Ref;
    fn CGPDFPageGetBoxRect(page: Ref, box_kind: i32) -> Rect;
    fn CGPDFPageGetRotationAngle(page: Ref) -> i32;
    fn CGColorSpaceCreateDeviceRGB() -> Ref;
    fn CGColorSpaceRelease(space: Ref);
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits: usize,
        stride: usize,
        space: Ref,
        info: u32,
    ) -> Ref;
    fn CGContextRelease(context: Ref);
    fn CGContextSetRGBFillColor(context: Ref, r: f64, g: f64, b: f64, a: f64);
    fn CGContextFillRect(context: Ref, rect: Rect);
    fn CGContextScaleCTM(context: Ref, sx: f64, sy: f64);
    fn CGContextTranslateCTM(context: Ref, x: f64, y: f64);
    fn CGContextRotateCTM(context: Ref, angle: f64);
    fn CGContextDrawPDFPage(context: Ref, page: Ref);
    fn CGBitmapContextCreateImage(context: Ref) -> Ref;
}
struct Owned(Ref, unsafe extern "C" fn(Ref));
impl Owned {
    fn new(raw: Ref, release: unsafe extern "C" fn(Ref)) -> Result<Self, String> {
        if raw.is_null() {
            Err("系统无法解码此文件，原文件保持不变。".into())
        } else {
            Ok(Self(raw, release))
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { (self.1)(self.0) };
    }
}
unsafe fn cf(raw: Ref) -> Result<Owned, String> {
    Owned::new(raw, CFRelease)
}
unsafe fn data(bytes: &[u8]) -> Result<Owned, String> {
    cf(CFDataCreate(ptr::null(), bytes.as_ptr(), bytes.len() as isize).cast())
}
unsafe fn dictionary(keys: &[Ref], values: &[Ref]) -> Result<Owned, String> {
    cf(CFDictionaryCreate(
        ptr::null(),
        keys.as_ptr(),
        values.as_ptr(),
        keys.len() as isize,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks,
    )
    .cast())
}
unsafe fn number(dict: Ref, key: Ref) -> Result<u64, String> {
    let value = CFDictionaryGetValue(dict.cast(), key);
    let mut n = 0i64;
    if value.is_null()
        || CFGetTypeID(value) != CFNumberGetTypeID()
        || !CFNumberGetValue(
            value.cast(),
            kCFNumberSInt64Type,
            (&mut n as *mut i64).cast(),
        )
        || n <= 0
    {
        return Err("图片像素信息无法核对。".into());
    }
    Ok(n as u64)
}
pub(super) struct Raster {
    pub width: u64,
    pub height: u64,
    pub frames: usize,
    image: Owned,
}
impl Raster {
    pub(super) fn decode(bytes: &[u8], max: u32) -> Result<Self, String> {
        unsafe {
            let data = data(bytes)?;
            let options = dictionary(&[kCGImageSourceShouldCache], &[kCFBooleanFalse.cast()])?;
            let source = cf(CGImageSourceCreateWithData(data.0, options.0))?;
            let props = cf(CGImageSourceCopyPropertiesAtIndex(source.0, 0, options.0))?;
            let width = number(props.0, kCGImagePropertyPixelWidth)?;
            let height = number(props.0, kCGImagePropertyPixelHeight)?;
            if width > 60_000
                || height > 60_000
                || width.checked_mul(height).is_none_or(|n| n > 80_000_000)
            {
                return Err("图片像素超过本次预览范围，请在系统预览中查看。".into());
            }
            let n = i64::from(max.clamp(8, 1024));
            let limit =
                cf(
                    CFNumberCreate(ptr::null(), kCFNumberSInt64Type, (&n as *const i64).cast())
                        .cast(),
                )?;
            let options = dictionary(
                &[
                    kCGImageSourceCreateThumbnailFromImageAlways,
                    kCGImageSourceCreateThumbnailWithTransform,
                    kCGImageSourceThumbnailMaxPixelSize,
                ],
                &[kCFBooleanTrue.cast(), kCFBooleanTrue.cast(), limit.0],
            )?;
            let image = Owned::new(
                CGImageSourceCreateThumbnailAtIndex(source.0, 0, options.0),
                CGImageRelease,
            )?;
            if CGImageGetWidth(image.0) > max as usize || CGImageGetHeight(image.0) > max as usize {
                return Err("系统缩略图大小无法核对。".into());
            }
            Ok(Self {
                width,
                height,
                frames: CGImageSourceGetCount(source.0),
                image,
            })
        }
    }
    pub(super) fn png(&self) -> Result<String, String> {
        png(self.image.0)
    }
}
fn png(image: Ref) -> Result<String, String> {
    unsafe {
        let bytes = cf(CFDataCreateMutable(ptr::null(), 0).cast())?;
        let kind = cf(CFStringCreateWithBytes(
            ptr::null(),
            b"public.png".as_ptr(),
            10,
            kCFStringEncodingUTF8,
            0,
        )
        .cast())?;
        let dest = cf(CGImageDestinationCreateWithData(
            bytes.0,
            kind.0,
            1,
            ptr::null(),
        ))?;
        CGImageDestinationAddImage(dest.0, image, ptr::null());
        if !CGImageDestinationFinalize(dest.0) {
            return Err("缩略图编码未完成。".into());
        }
        let len = CFDataGetLength(bytes.0.cast());
        if !(1..=4 * 1024 * 1024).contains(&len) {
            return Err("缩略图超过本次显示范围。".into());
        }
        use objc2_foundation::{NSData, NSDataBase64EncodingOptions};
        let bytes = std::slice::from_raw_parts(CFDataGetBytePtr(bytes.0.cast()), len as usize);
        let data = NSData::with_bytes(bytes);
        Ok(format!(
            "data:image/png;base64,{}",
            data.base64EncodedStringWithOptions(NSDataBase64EncodingOptions::empty())
        ))
    }
}
pub(super) fn pdf(bytes: &[u8], page: u32) -> Result<(String, u32), String> {
    unsafe {
        let data = data(bytes)?;
        let provider = Owned::new(
            CGDataProviderCreateWithCFData(data.0),
            CGDataProviderRelease,
        )?;
        let doc = Owned::new(
            CGPDFDocumentCreateWithProvider(provider.0),
            CGPDFDocumentRelease,
        )?;
        let pages = CGPDFDocumentGetNumberOfPages(doc.0);
        if CGPDFDocumentIsEncrypted(doc.0)
            || pages == 0
            || pages > 10_000
            || page == 0
            || page as usize > pages
        {
            return Err("PDF 已加密、页码无效或超出本次预览范围，请在系统预览中查看。".into());
        }
        let page = CGPDFDocumentGetPage(doc.0, page as usize);
        if page.is_null() {
            return Err("PDF 页面无法读取。".into());
        }
        let rect = CGPDFPageGetBoxRect(page, 0);
        let (w, h) = (rect.size.width, rect.size.height);
        if !w.is_finite()
            || !h.is_finite()
            || !(1.0..=14400.0).contains(&w)
            || !(1.0..=14400.0).contains(&h)
            || !rect.origin.x.is_finite()
            || !rect.origin.y.is_finite()
        {
            return Err("PDF 页面尺寸超出本次预览范围。".into());
        }
        let angle = CGPDFPageGetRotationAngle(page).rem_euclid(360);
        let (dw, dh) = if angle == 90 || angle == 270 {
            (h, w)
        } else {
            (w, h)
        };
        let scale = (1024.0 / dw.max(dh)).min(1.0);
        let (pw, ph) = ((dw * scale).ceil() as usize, (dh * scale).ceil() as usize);
        let space = Owned::new(CGColorSpaceCreateDeviceRGB(), CGColorSpaceRelease)?;
        let mut pixels = vec![0u8; pw * ph * 4];
        let context = Owned::new(
            CGBitmapContextCreate(pixels.as_mut_ptr().cast(), pw, ph, 8, pw * 4, space.0, 1),
            CGContextRelease,
        )?;
        CGContextSetRGBFillColor(context.0, 1., 1., 1., 1.);
        CGContextFillRect(
            context.0,
            Rect {
                origin: Point { x: 0., y: 0. },
                size: Size {
                    width: pw as f64,
                    height: ph as f64,
                },
            },
        );
        CGContextScaleCTM(context.0, scale, scale);
        match angle {
            90 => {
                CGContextTranslateCTM(context.0, 0., w);
                CGContextRotateCTM(context.0, -std::f64::consts::FRAC_PI_2);
            }
            180 => {
                CGContextTranslateCTM(context.0, w, h);
                CGContextRotateCTM(context.0, std::f64::consts::PI);
            }
            270 => {
                CGContextTranslateCTM(context.0, h, 0.);
                CGContextRotateCTM(context.0, std::f64::consts::FRAC_PI_2);
            }
            _ => {}
        }
        CGContextTranslateCTM(context.0, -rect.origin.x, -rect.origin.y);
        CGContextDrawPDFPage(context.0, page);
        let image = Owned::new(CGBitmapContextCreateImage(context.0), CGImageRelease)?;
        Ok((png(image.0)?, pages as u32))
    }
}

#[cfg(test)]
pub(super) fn fixture_png(width: usize, height: usize) -> Vec<u8> {
    unsafe {
        let mut pixels = vec![255u8; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let at = (y * width + x) * 4;
                pixels[at] = (x * 255 / width) as u8;
                pixels[at + 1] = (y * 255 / height) as u8;
                pixels[at + 2] = 80;
            }
        }
        let space = Owned::new(CGColorSpaceCreateDeviceRGB(), CGColorSpaceRelease).unwrap();
        let context = Owned::new(
            CGBitmapContextCreate(
                pixels.as_mut_ptr().cast(),
                width,
                height,
                8,
                width * 4,
                space.0,
                1,
            ),
            CGContextRelease,
        )
        .unwrap();
        let image = Owned::new(CGBitmapContextCreateImage(context.0), CGImageRelease).unwrap();
        let url = png(image.0).unwrap();
        use objc2::AnyThread;
        use objc2_foundation::{NSData, NSDataBase64DecodingOptions, NSString};
        NSData::initWithBase64EncodedString_options(
            NSData::alloc(),
            &NSString::from_str(url.strip_prefix("data:image/png;base64,").unwrap()),
            NSDataBase64DecodingOptions::empty(),
        )
        .unwrap()
        .to_vec()
    }
}
