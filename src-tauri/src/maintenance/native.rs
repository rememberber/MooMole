//! Six allowlisted CFPreferences reads; no synchronize/set/delete or user prompts.
use super::{originals, Value};
use core_foundation_sys::{
    base::{CFGetTypeID, CFRelease, CFTypeRef},
    number::{
        kCFNumberSInt64Type, CFBooleanGetTypeID, CFBooleanGetValue, CFNumberGetTypeID,
        CFNumberGetValue, CFNumberIsFloatType,
    },
    preferences::{
        kCFPreferencesAnyApplication, CFPreferencesAppValueIsForced, CFPreferencesCopyAppValue,
    },
    string::{
        kCFStringEncodingUTF8, CFStringCreateWithCString, CFStringGetCString, CFStringGetLength,
        CFStringGetTypeID,
    },
};
use std::{
    ffi::{CStr, CString},
    ptr,
};

struct Owned(CFTypeRef);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}
fn text(value: &str) -> Option<Owned> {
    let value = CString::new(value).ok()?;
    let raw =
        unsafe { CFStringCreateWithCString(ptr::null(), value.as_ptr(), kCFStringEncodingUTF8) };
    (!raw.is_null()).then(|| Owned(raw.cast()))
}

fn string_value(value: &str, legacy: bool) -> Value {
    match value {
        "1" => Value::Enabled,
        "0" => Value::Disabled,
        _ if legacy
            && (value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("yes")) =>
        {
            Value::Enabled
        }
        _ if legacy
            && (value.eq_ignore_ascii_case("false") || value.eq_ignore_ascii_case("no")) =>
        {
            Value::Disabled
        }
        _ => Value::Unknown,
    }
}
// Only fixed preference keys reach this function. Return neither raw strings nor
// arbitrary plist data to the webview. Missing and unreadable remain indistinct.
pub(super) fn read(domain: &str, key: &str) -> (Value, bool, Vec<originals::Evidence>) {
    if !matches!(
        (domain, key),
        (
            "com.apple.desktopservices",
            "DSDontWriteNetworkStores" | "DSDontWriteUSBStores"
        ) | ("-g", "NSAppSleepDisabled")
            | (
                "com.apple.frameworks.diskimages",
                "skip-verify" | "skip-verify-locked" | "skip-verify-remote"
            )
    ) {
        return (Value::Unknown, false, originals::unavailable("notChecked"));
    }
    let Some(key_ref) = text(key) else {
        return (Value::Unknown, false, originals::unavailable("notChecked"));
    };
    let app = if domain == "-g" { None } else { text(domain) };
    if domain != "-g" && app.is_none() {
        return (Value::Unknown, false, originals::unavailable("notChecked"));
    }
    unsafe {
        let application = app
            .as_ref()
            .map_or(kCFPreferencesAnyApplication, |v| v.0.cast());
        let managed = CFPreferencesAppValueIsForced(key_ref.0.cast(), application) != 0;
        let effective = || {
            let raw = CFPreferencesCopyAppValue(key_ref.0.cast(), application);
            if raw.is_null() {
                return Value::Unknown;
            }
            let value = Owned(raw);
            decode(value.0, domain != "com.apple.desktopservices")
        };
        let decoded = effective();
        let evidence = originals::capture(application, key_ref.0.cast());
        let after = effective();
        if managed != (CFPreferencesAppValueIsForced(key_ref.0.cast(), application) != 0) {
            return (Value::Unknown, true, originals::unavailable("changed"));
        }
        if decoded != after {
            return (Value::Unknown, managed, originals::unavailable("changed"));
        }
        (decoded, managed, evidence)
    }
}

unsafe fn decode(raw: CFTypeRef, legacy: bool) -> Value {
    let kind = CFGetTypeID(raw);
    if kind == CFBooleanGetTypeID() {
        return if CFBooleanGetValue(raw.cast()) {
            Value::Enabled
        } else {
            Value::Disabled
        };
    }
    if kind == CFNumberGetTypeID() && CFNumberIsFloatType(raw.cast()) == 0 {
        let mut number: i64 = -1;
        if CFNumberGetValue(
            raw.cast(),
            kCFNumberSInt64Type,
            (&mut number as *mut i64).cast(),
        ) {
            return match number {
                1 => Value::Enabled,
                0 => Value::Disabled,
                _ => Value::Unknown,
            };
        }
    }
    if kind == CFStringGetTypeID() && CFStringGetLength(raw.cast()) <= 16 {
        let mut buffer = [0i8; 65];
        if CFStringGetCString(
            raw.cast(),
            buffer.as_mut_ptr(),
            buffer.len() as isize,
            kCFStringEncodingUTF8,
        ) != 0
        {
            if let Ok(value) = CStr::from_ptr(buffer.as_ptr()).to_str() {
                if value.is_ascii() && value.len() as isize == CFStringGetLength(raw.cast()) {
                    return string_value(value, legacy);
                }
            }
        }
    }
    Value::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supported_text_forms_are_narrow_and_do_not_infer_false() {
        for raw in ["1", "YES", "true", "TrUe"] {
            assert_eq!(string_value(raw, true), Value::Enabled)
        }
        for raw in ["0", "NO", "false"] {
            assert_eq!(string_value(raw, true), Value::Disabled)
        }
        for raw in ["", "2", "TRUE ", "unknown", "no\0"] {
            assert_eq!(string_value(raw, true), Value::Unknown)
        }
        assert_eq!(string_value("YES", false), Value::Unknown);
    }
    #[test]
    fn native_type_decoding_handles_bool_integer_and_float_without_writing_preferences() {
        use core_foundation_sys::number::{
            kCFBooleanFalse, kCFBooleanTrue, kCFNumberFloat64Type, CFNumberCreate,
        };
        unsafe {
            assert_eq!(decode(kCFBooleanTrue.cast(), false), Value::Enabled);
            assert_eq!(decode(kCFBooleanFalse.cast(), false), Value::Disabled);
            for n in [0i64, 1, 2, -1] {
                let raw = Owned(
                    CFNumberCreate(ptr::null(), kCFNumberSInt64Type, (&n as *const i64).cast())
                        .cast(),
                );
                assert_eq!(
                    decode(raw.0, true),
                    match n {
                        0 => Value::Disabled,
                        1 => Value::Enabled,
                        _ => Value::Unknown,
                    }
                );
            }
            let n = 1.5f64;
            let raw = Owned(
                CFNumberCreate(ptr::null(), kCFNumberFloat64Type, (&n as *const f64).cast()).cast(),
            );
            assert_eq!(decode(raw.0, true), Value::Unknown);
        }
    }

    #[test]
    fn native_strings_do_not_accept_embedded_nul_or_unexpected_forms() {
        use core_foundation_sys::string::CFStringCreateWithCharacters;
        for (input, legacy, expected) in [
            ("1", false, Value::Enabled),
            ("YES", true, Value::Enabled),
            ("YES", false, Value::Unknown),
            ("YES\0NO", true, Value::Unknown),
            ("true\n", true, Value::Unknown),
            ("开启", true, Value::Unknown),
            ("a string over sixteen characters", true, Value::Unknown),
        ] {
            let units: Vec<u16> = input.encode_utf16().collect();
            unsafe {
                let raw = Owned(
                    CFStringCreateWithCharacters(ptr::null(), units.as_ptr(), units.len() as isize)
                        .cast(),
                );
                assert_eq!(decode(raw.0, legacy), expected);
            }
        }
    }
}
