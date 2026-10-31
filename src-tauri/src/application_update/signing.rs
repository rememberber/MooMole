//! Signature integrity and publisher continuity, not Gatekeeper assessment.
use core_foundation_sys::{
    base::{CFGetTypeID, CFRelease, CFTypeRef},
    dictionary::{CFDictionaryGetValue, CFDictionaryRef},
    number::{kCFNumberSInt32Type, CFNumberGetTypeID, CFNumberGetValue},
    string::{kCFStringEncodingUTF8, CFStringGetCString, CFStringGetTypeID, CFStringRef},
    url::CFURLCreateFromFileSystemRepresentation,
};
use std::{ffi::c_void, os::unix::ffi::OsStrExt, path::Path};

#[link(name = "Security", kind = "framework")]
extern "C" {
    fn SecStaticCodeCreateWithPath(path: CFTypeRef, flags: u32, code: *mut CFTypeRef) -> i32;
    fn SecStaticCodeCheckValidity(code: CFTypeRef, flags: u32, requirement: CFTypeRef) -> i32;
    fn SecCodeCopySigningInformation(code: CFTypeRef, flags: u32, info: *mut CFTypeRef) -> i32;
    fn SecCodeCopyDesignatedRequirement(
        code: CFTypeRef,
        flags: u32,
        requirement: *mut CFTypeRef,
    ) -> i32;
    static kSecCodeInfoTeamIdentifier: CFStringRef;
    static kSecCodeInfoFlags: CFStringRef;
}
struct Owned(CFTypeRef);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.0);
        }
    }
}
fn owned(value: CFTypeRef) -> Result<Owned, String> {
    if value.is_null() {
        Err("应用签名信息缺失。".into())
    } else {
        Ok(Owned(value))
    }
}
fn status(value: i32) -> Result<(), String> {
    if value == 0 {
        Ok(())
    } else {
        Err(format!("应用签名核对未通过（{value}），应用保留。"))
    }
}
fn code(path: &Path) -> Result<Owned, String> {
    let bytes = path.as_os_str().as_bytes();
    let url = owned(
        unsafe {
            CFURLCreateFromFileSystemRepresentation(
                std::ptr::null(),
                bytes.as_ptr(),
                bytes.len() as isize,
                1,
            )
        }
        .cast(),
    )?;
    let mut out = std::ptr::null();
    status(unsafe { SecStaticCodeCreateWithPath(url.0, 0, &mut out) })?;
    let result = owned(out)?;
    // All architectures + nested code + strict validation. No skip-resource or
    // skip-executable flags, no resigning, no quarantine edits.
    status(unsafe { SecStaticCodeCheckValidity(result.0, 1 | 8 | 16, std::ptr::null()) })?;
    Ok(result)
}
fn info(code: &Owned) -> Result<(Option<String>, bool), String> {
    let mut out = std::ptr::null();
    status(unsafe { SecCodeCopySigningInformation(code.0, 2, &mut out) })?;
    let dictionary = owned(out)?;
    let get = |key| unsafe {
        CFDictionaryGetValue(dictionary.0 as CFDictionaryRef, key as *const c_void)
    };
    let flags = get(unsafe { kSecCodeInfoFlags });
    let mut value = 0i32;
    if flags.is_null()
        || unsafe { CFGetTypeID(flags) } != unsafe { CFNumberGetTypeID() }
        || !unsafe {
            CFNumberGetValue(
                flags.cast(),
                kCFNumberSInt32Type,
                (&mut value as *mut i32).cast(),
            )
        }
    {
        return Err("应用签名类型无法核对。".into());
    }
    let team = get(unsafe { kSecCodeInfoTeamIdentifier });
    let team = if team.is_null() {
        None
    } else {
        if unsafe { CFGetTypeID(team) } != unsafe { CFStringGetTypeID() } {
            return Err("应用发布者无法核对。".into());
        }
        let mut bytes = [0i8; 256];
        if unsafe {
            CFStringGetCString(
                team.cast(),
                bytes.as_mut_ptr(),
                bytes.len() as isize,
                kCFStringEncodingUTF8,
            )
        } == 0
        {
            return Err("应用发布者标识无效。".into());
        }
        Some(
            unsafe { std::ffi::CStr::from_ptr(bytes.as_ptr()) }
                .to_str()
                .map_err(|_| "应用发布者标识无效。")?
                .to_owned(),
        )
    };
    Ok((team, value & 2 != 0))
}
pub(super) fn continuity(old: &Path, new: &Path) -> Result<String, String> {
    let old = code(old)?;
    let new = code(new)?;
    let (team, adhoc) = info(&old)?;
    let (next, next_adhoc) = info(&new)?;
    if adhoc && next_adhoc && team.is_none() && next.is_none() {
        return Ok("adHoc".into());
    }
    if adhoc || next_adhoc || team.is_none() || team != next {
        return Err("新版与已安装应用的签名发布者不一致或无法确认。".into());
    }
    let mut raw = std::ptr::null();
    status(unsafe { SecCodeCopyDesignatedRequirement(old.0, 0, &mut raw) })?;
    let requirement = owned(raw)?;
    status(unsafe { SecStaticCodeCheckValidity(new.0, 1 | 8 | 16, requirement.0) })?;
    Ok(team.unwrap())
}
