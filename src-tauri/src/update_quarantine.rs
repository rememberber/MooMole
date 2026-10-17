//! Documented CFURL quarantine properties, rather than a private xattr format.
#[cfg(target_os = "macos")]
mod native {
    use core_foundation_sys::dictionary::{
        kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks,
    };
    use std::{
        ffi::{c_char, c_void, CString},
        os::unix::ffi::OsStrExt,
        path::Path,
        ptr,
    };
    type Ref = *const c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFURLQuarantinePropertiesKey: Ref;
        fn CFURLCreateFromFileSystemRepresentation(a: Ref, b: *const u8, n: isize, d: u8) -> Ref;
        fn CFStringCreateWithCString(a: Ref, b: *const c_char, encoding: u32) -> Ref;
        fn CFURLCreateWithString(a: Ref, b: Ref, base: Ref) -> Ref;
        fn CFDictionaryCreate(
            a: Ref,
            keys: *const Ref,
            values: *const Ref,
            n: isize,
            k: Ref,
            v: Ref,
        ) -> Ref;
        fn CFURLSetResourcePropertyForKey(url: Ref, key: Ref, value: Ref, error: *mut Ref) -> u8;
        fn CFURLCopyResourcePropertyForKey(
            url: Ref,
            key: Ref,
            value: *mut Ref,
            error: *mut Ref,
        ) -> u8;
        fn CFDictionaryGetValue(dict: Ref, key: Ref) -> Ref;
        fn CFDictionaryGetTypeID() -> usize;
        fn CFGetTypeID(value: Ref) -> usize;
        fn CFEqual(a: Ref, b: Ref) -> u8;
        fn CFRelease(value: Ref);
    }
    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        static kLSQuarantineTypeKey: Ref;
        static kLSQuarantineTypeOtherDownload: Ref;
        static kLSQuarantineAgentNameKey: Ref;
        static kLSQuarantineDataURLKey: Ref;
        static kLSQuarantineOriginURLKey: Ref;
    }
    struct Owned(Ref);
    impl Owned {
        fn new(value: Ref) -> Result<Self, String> {
            if value.is_null() {
                Err("无法创建系统下载标记。".into())
            } else {
                Ok(Self(value))
            }
        }
        fn text(value: &str) -> Result<Self, String> {
            let value = CString::new(value).map_err(|_| "下载元数据无效。")?;
            Self::new(unsafe { CFStringCreateWithCString(ptr::null(), value.as_ptr(), 0x08000100) })
        }
        fn file(path: &Path) -> Result<Self, String> {
            let path = path.as_os_str().as_bytes();
            Self::new(unsafe {
                CFURLCreateFromFileSystemRepresentation(
                    ptr::null(),
                    path.as_ptr(),
                    path.len() as isize,
                    0,
                )
            })
        }
        fn url(value: &str) -> Result<Self, String> {
            let value = Self::text(value)?;
            Self::new(unsafe { CFURLCreateWithString(ptr::null(), value.0, ptr::null()) })
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) }
        }
    }
    pub fn present(path: &Path) -> Result<(), String> {
        let url = Owned::file(path)?;
        let mut property = ptr::null();
        if unsafe {
            CFURLCopyResourcePropertyForKey(
                url.0,
                kCFURLQuarantinePropertiesKey,
                &mut property,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err("无法检查下载隔离标记。".into());
        }
        let property = Owned::new(property)?;
        let agent = Owned::text("MooMole")?;
        if unsafe { CFGetTypeID(property.0) != CFDictionaryGetTypeID() } {
            return Err("下载隔离标记缺失。".into());
        }
        let name = unsafe { CFDictionaryGetValue(property.0, kLSQuarantineAgentNameKey) };
        if name.is_null() || unsafe { CFEqual(name, agent.0) } == 0 {
            return Err("下载隔离标记已变化，请重新下载。".into());
        }
        Ok(())
    }
    pub fn mark(path: &Path, source: &str) -> Result<(), String> {
        let url = Owned::file(path)?;
        let agent = Owned::text("MooMole")?;
        let source = Owned::url(source)?;
        let origin = Owned::url(crate::updates::RELEASES)?;
        let properties = unsafe {
            let keys = [
                kLSQuarantineTypeKey,
                kLSQuarantineAgentNameKey,
                kLSQuarantineDataURLKey,
                kLSQuarantineOriginURLKey,
            ];
            let values = [kLSQuarantineTypeOtherDownload, agent.0, source.0, origin.0];
            Owned::new(CFDictionaryCreate(
                ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                4,
                (&raw const kCFTypeDictionaryKeyCallBacks).cast(),
                (&raw const kCFTypeDictionaryValueCallBacks).cast(),
            ))?
        };
        if unsafe {
            CFURLSetResourcePropertyForKey(
                url.0,
                kCFURLQuarantinePropertiesKey,
                properties.0,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err("无法保留下载隔离标记，已停止保存。".into());
        }
        present(path)
    }
}
#[cfg(target_os = "macos")]
pub use native::{mark, present};
#[cfg(not(target_os = "macos"))]
pub fn mark(_: &std::path::Path, _: &str) -> Result<(), String> {
    Ok(())
}
#[cfg(not(target_os = "macos"))]
pub fn present(_: &std::path::Path) -> Result<(), String> {
    Ok(())
}
