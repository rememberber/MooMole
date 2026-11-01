use super::*;
use core_foundation_sys::{
    base::{CFGetTypeID, CFRelease, CFTypeRef},
    date::{CFDateGetAbsoluteTime, CFDateGetTypeID},
    string::{CFStringGetCString, CFStringGetTypeID},
    url::CFURLCreateFromFileSystemRepresentation,
};
use objc2_app_kit::NSRunningApplication;
use objc2_foundation::NSString;
use std::{ffi::c_void, os::unix::ffi::OsStrExt, ptr};
type Ref = *const c_void;
#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn MDItemCreateWithURL(a: Ref, url: Ref) -> Ref;
    fn MDItemCopyAttribute(item: Ref, name: Ref) -> Ref;
    static kMDItemLastUsedDate: Ref;
    static kMDItemPath: Ref;
}
struct Owned(Ref);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0 as CFTypeRef) }
        }
    }
}
fn date(path: &str) -> Date {
    let raw = Path::new(path).as_os_str().as_bytes();
    let url = Owned(unsafe {
        CFURLCreateFromFileSystemRepresentation(ptr::null(), raw.as_ptr(), raw.len() as isize, 1)
    } as Ref);
    if url.0.is_null() {
        return Date::Unknown("systemUnavailable");
    }
    let item = Owned(unsafe { MDItemCreateWithURL(ptr::null(), url.0) });
    if item.0.is_null() {
        return Date::Unknown("notIndexed");
    }
    let location = Owned(unsafe { MDItemCopyAttribute(item.0, kMDItemPath) });
    if location.0.is_null() || unsafe { CFGetTypeID(location.0) != CFStringGetTypeID() } {
        return Date::Unknown("metadataPathUnknown");
    }
    let mut buffer = [0i8; 4097];
    if unsafe {
        CFStringGetCString(
            location.0.cast(),
            buffer.as_mut_ptr(),
            buffer.len() as isize,
            0x08000100,
        )
    } == 0
    {
        return Date::Unknown("metadataPathUnknown");
    }
    if unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) }.to_bytes() != path.as_bytes() {
        return Date::Unknown("metadataPathUnknown");
    }
    let value = Owned(unsafe { MDItemCopyAttribute(item.0, kMDItemLastUsedDate) });
    if value.0.is_null() {
        return Date::Missing;
    }
    if unsafe { CFGetTypeID(value.0) != CFDateGetTypeID() } {
        return Date::Unknown("invalidDate");
    }
    Date::Value(unsafe { CFDateGetAbsoluteTime(value.0.cast()) })
}
fn running(path: &str, id: &str) -> Option<bool> {
    objc2::rc::autoreleasepool(|_| {
        let apps =
            NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(id));
        if apps.count() > 64 {
            return None;
        }
        let mut found = false;
        for app in apps.iter() {
            if app.isTerminated() {
                continue;
            }
            let location = app.bundleURL()?.path()?.to_string();
            if location == path {
                found = true;
            }
        }
        Some(found)
    })
}
pub(super) fn read(path: &str, id: &str) -> (Date, Option<bool>) {
    (date(path), running(path, id))
}
