//! Read-only, explicit-domain evidence. No missing-value proof or recovery authority.
use core_foundation_sys::{
    base::{CFGetTypeID, CFRange, CFRelease, CFTypeRef},
    number::{
        kCFNumberSInt64Type, CFBooleanGetTypeID, CFBooleanGetValue, CFNumberGetTypeID,
        CFNumberGetValue, CFNumberIsFloatType,
    },
    preferences::{
        kCFPreferencesAnyHost, kCFPreferencesCurrentHost, kCFPreferencesCurrentUser,
        CFPreferencesCopyValue,
    },
    string::{CFStringGetCharacters, CFStringGetLength, CFStringGetTypeID, CFStringRef},
};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub scope: &'static str,
    pub state: &'static str,
    pub kind: Option<&'static str>,
}
const SCOPES: [&str; 2] = ["anyHost", "thisHost"];
pub fn unavailable(state: &'static str) -> Vec<Evidence> {
    SCOPES
        .iter()
        .map(|scope| Evidence {
            scope,
            state,
            kind: None,
        })
        .collect()
}
// Raw payloads are transient, never serialized, logged, or kept as restore records.
#[derive(PartialEq)]
enum Atom {
    Boolean(bool),
    Integer(i64),
    Text(Vec<u16>),
}
impl Atom {
    fn kind(&self) -> &'static str {
        match self {
            Self::Boolean(_) => "boolean",
            Self::Integer(_) => "integer",
            Self::Text(_) => "string",
        }
    }
    unsafe fn copy(raw: CFTypeRef) -> Option<Self> {
        let kind = CFGetTypeID(raw);
        if kind == CFBooleanGetTypeID() {
            return Some(Self::Boolean(CFBooleanGetValue(raw.cast())));
        }
        if kind == CFNumberGetTypeID() && CFNumberIsFloatType(raw.cast()) == 0 {
            let mut value = 0i64;
            if CFNumberGetValue(
                raw.cast(),
                kCFNumberSInt64Type,
                (&mut value as *mut i64).cast(),
            ) {
                return Some(Self::Integer(value));
            }
        }
        if kind == CFStringGetTypeID() {
            let count = CFStringGetLength(raw.cast());
            if (0..=1024).contains(&count) {
                let mut units = vec![0; count as usize];
                CFStringGetCharacters(
                    raw.cast(),
                    CFRange {
                        location: 0,
                        length: count,
                    },
                    units.as_mut_ptr(),
                );
                if String::from_utf16(&units).is_ok() {
                    return Some(Self::Text(units));
                }
            }
        }
        None
    }
}
#[derive(PartialEq)]
enum Read {
    Unconfirmed,
    Unsupported,
    Present(Atom),
}
unsafe fn read(application: CFStringRef, key: CFStringRef, scope: usize) -> Read {
    let raw = CFPreferencesCopyValue(
        key,
        application,
        kCFPreferencesCurrentUser,
        if scope == 0 {
            kCFPreferencesAnyHost
        } else {
            kCFPreferencesCurrentHost
        },
    );
    if raw.is_null() {
        return Read::Unconfirmed;
    }
    let value = Atom::copy(raw);
    CFRelease(raw);
    value.map_or(Read::Unsupported, Read::Present)
}
fn compare(scope: &'static str, first: &Read, second: &Read) -> Evidence {
    let (state, kind) = match (first, second) {
        (Read::Unconfirmed, Read::Unconfirmed) => ("unconfirmed", None),
        (Read::Unsupported, Read::Unsupported) => ("unsupported", None),
        (Read::Present(a), Read::Present(b)) if a == b => ("observed", Some(a.kind())),
        _ => ("changed", None),
    };
    Evidence { scope, state, kind }
}
// Called only after native::read has checked the fixed six-key allowlist.
// Two whole-scope passes catch observed differences, not unobserved races or stale caches.
pub(super) unsafe fn capture(application: CFStringRef, key: CFStringRef) -> Vec<Evidence> {
    let first = [read(application, key, 0), read(application, key, 1)];
    let second = [read(application, key, 0), read(application, key, 1)];
    SCOPES
        .iter()
        .enumerate()
        .map(|(i, scope)| compare(scope, &first[i], &second[i]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_foundation_sys::{
        number::{kCFBooleanTrue, kCFNumberFloat64Type, CFNumberCreate},
        string::CFStringCreateWithCharacters,
    };
    use std::ptr;
    #[test]
    fn originals_preserve_native_types_and_all_string_units_without_serializing_payloads() {
        unsafe {
            assert!(matches!(
                Atom::copy(kCFBooleanTrue.cast()),
                Some(Atom::Boolean(true))
            ));
            for value in [i64::MIN, 0, 1, i64::MAX] {
                let raw = CFNumberCreate(
                    ptr::null(),
                    kCFNumberSInt64Type,
                    (&value as *const i64).cast(),
                );
                assert!(matches!(Atom::copy(raw.cast()), Some(Atom::Integer(n)) if n == value));
                CFRelease(raw.cast());
            }
            for value in ["", "1", "中文🚀\0尾"] {
                let units: Vec<u16> = value.encode_utf16().collect();
                let raw =
                    CFStringCreateWithCharacters(ptr::null(), units.as_ptr(), units.len() as isize);
                let atom = Atom::copy(raw.cast()).unwrap();
                assert!(matches!(&atom, Atom::Text(text) if *text == units));
                let summary = compare(
                    SCOPES[0],
                    &Read::Present(atom),
                    &Read::Present(Atom::Text(units)),
                );
                assert_eq!(
                    serde_json::to_value(summary).unwrap(),
                    serde_json::json!({"scope":"anyHost", "state":"observed", "kind":"string"})
                );
                CFRelease(raw.cast());
            }
            let value = 1.5f64;
            let raw = CFNumberCreate(
                ptr::null(),
                kCFNumberFloat64Type,
                (&value as *const f64).cast(),
            );
            assert!(Atom::copy(raw.cast()).is_none());
            CFRelease(raw.cast());
            for units in [vec![0xd800], vec![1; 1025]] {
                let raw =
                    CFStringCreateWithCharacters(ptr::null(), units.as_ptr(), units.len() as isize);
                assert!(Atom::copy(raw.cast()).is_none());
                CFRelease(raw.cast());
            }
        }
    }
    #[test]
    fn absence_is_not_proven_and_representation_changes_are_not_coalesced() {
        assert_eq!(
            compare(SCOPES[0], &Read::Unconfirmed, &Read::Unconfirmed).state,
            "unconfirmed"
        );
        assert_eq!(
            compare(SCOPES[0], &Read::Unsupported, &Read::Unsupported).state,
            "unsupported"
        );
        for (first, second) in [
            (
                Read::Present(Atom::Boolean(true)),
                Read::Present(Atom::Integer(1)),
            ),
            (
                Read::Present(Atom::Integer(0)),
                Read::Present(Atom::Boolean(false)),
            ),
            (Read::Unconfirmed, Read::Present(Atom::Boolean(false))),
            (Read::Unsupported, Read::Present(Atom::Boolean(true))),
            (Read::Present(Atom::Integer(1)), Read::Unsupported),
            (
                Read::Present(Atom::Text(vec![49])),
                Read::Present(Atom::Text(vec![49, 0])),
            ),
        ] {
            let evidence = compare(SCOPES[1], &first, &second);
            assert_eq!(evidence.state, "changed");
            assert_eq!(evidence.kind, None);
        }
    }
}
