//! Test-only CFPreferences experiment. No App IPC or production domain selector.
//! Every writer is sealed to a newly minted UUID domain and three fixture keys.
use core_foundation_sys::{
    array::{CFArrayContainsValue, CFArrayGetCount},
    base::{CFGetTypeID, CFRange, CFRelease, CFRetain, CFTypeRef},
    number::{
        kCFBooleanFalse, kCFBooleanTrue, kCFNumberSInt64Type, CFBooleanGetTypeID,
        CFBooleanGetValue, CFNumberCreate, CFNumberGetTypeID, CFNumberGetValue,
        CFNumberIsFloatType,
    },
    preferences::{
        kCFPreferencesAnyHost, kCFPreferencesCurrentHost, kCFPreferencesCurrentUser,
        CFPreferencesAppValueIsForced, CFPreferencesCopyAppValue, CFPreferencesCopyKeyList,
        CFPreferencesCopyValue, CFPreferencesSetValue, CFPreferencesSynchronize,
    },
    string::{
        kCFStringEncodingUTF8, CFStringCreateWithCString, CFStringCreateWithCharacters,
        CFStringGetCharacters, CFStringGetLength, CFStringGetTypeID, CFStringRef,
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    ptr,
};

type Result<T> = std::result::Result<T, String>;
const PREFIX: &str = "io.github.rememberber.moomole.preference-lab.";
const LIMIT: u64 = 32 * 1024;
struct Owned(CFTypeRef);
impl Owned {
    fn new(raw: CFTypeRef) -> Result<Self> {
        if raw.is_null() {
            Err("CF allocation/read failed".into())
        } else {
            Ok(Self(raw))
        }
    }
    fn text(value: &str) -> Result<Self> {
        let value = CString::new(value).map_err(|_| "invalid fixture identifier")?;
        Self::new(
            unsafe {
                CFStringCreateWithCString(ptr::null(), value.as_ptr(), kCFStringEncodingUTF8)
            }
            .cast(),
        )
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum Atom {
    Boolean(bool),
    Integer(i64),
    Text(Vec<u16>),
}
impl Atom {
    fn text(value: &str) -> Self {
        Self::Text(value.encode_utf16().collect())
    }
    fn valid(&self) -> bool {
        match self {
            Self::Text(units) => units.len() <= 1024 && String::from_utf16(units).is_ok(),
            _ => true,
        }
    }
    fn native(&self) -> Result<Owned> {
        if !self.valid() {
            return Err("unsupported original value".into());
        }
        unsafe {
            match self {
                Self::Boolean(value) => Owned::new(CFRetain(if *value {
                    kCFBooleanTrue.cast()
                } else {
                    kCFBooleanFalse.cast()
                })),
                Self::Integer(value) => Owned::new(
                    CFNumberCreate(
                        ptr::null(),
                        kCFNumberSInt64Type,
                        (value as *const i64).cast(),
                    )
                    .cast(),
                ),
                Self::Text(value) => Owned::new(
                    CFStringCreateWithCharacters(ptr::null(), value.as_ptr(), value.len() as isize)
                        .cast(),
                ),
            }
        }
    }
    fn could_coalesce(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Boolean(boolean), Self::Integer(integer))
            | (Self::Integer(integer), Self::Boolean(boolean)) => *integer == i64::from(*boolean),
            _ => false,
        }
    }
    unsafe fn copy(raw: CFTypeRef) -> Result<Self> {
        let kind = CFGetTypeID(raw);
        if kind == CFBooleanGetTypeID() {
            return Ok(Self::Boolean(CFBooleanGetValue(raw.cast())));
        }
        if kind == CFNumberGetTypeID() && CFNumberIsFloatType(raw.cast()) == 0 {
            let mut value: i64 = 0;
            if CFNumberGetValue(
                raw.cast(),
                kCFNumberSInt64Type,
                (&mut value as *mut i64).cast(),
            ) {
                return Ok(Self::Integer(value));
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
                let value = Self::Text(units);
                if value.valid() {
                    return Ok(value);
                }
            }
        }
        Err("original type/value unsupported; do not mutate".into())
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
enum Scope {
    AnyHost,
    ThisHost,
}
impl Scope {
    unsafe fn native(self) -> CFStringRef {
        match self {
            Self::AnyHost => kCFPreferencesAnyHost,
            Self::ThisHost => kCFPreferencesCurrentHost,
        }
    }
}
#[derive(Clone, Copy)]
enum Key {
    Marker,
    Probe,
    Other,
}
impl Key {
    fn text(self) -> &'static str {
        match self {
            Self::Marker => "fixture-owner",
            Self::Probe => "fixture-probe",
            Self::Other => "fixture-unrelated",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    protocol: u32,
    domain: String,
    owner: String,
    scope: Scope,
    key: String,
    original: Option<Atom>,
    expected: Atom,
}
impl Snapshot {
    fn validate(&self, domain: &str, owner: &str) -> Result<()> {
        if self.protocol != 1
            || self.domain != domain
            || self.owner != owner
            || self.key != Key::Probe.text()
            || !self.domain.strip_prefix(PREFIX).is_some_and(|id| {
                uuid::Uuid::parse_str(id).is_ok_and(|id| id.get_version_num() == 4)
            })
            || !self.expected.valid()
            || self.original.as_ref().is_some_and(|v| !v.valid())
        {
            return Err("journal identity/schema/value mismatch".into());
        }
        Ok(())
    }
}
struct Ticket {
    path: PathBuf,
    digest: Vec<u8>,
}
fn persist(path: &Path, snapshot: &Snapshot) -> Result<Ticket> {
    let bytes = serde_json::to_vec(snapshot).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("journal too large".into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    File::open(path.parent().ok_or("missing journal parent")?)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(Ticket {
        path: path.into(),
        digest: Sha256::digest(&bytes).to_vec(),
    })
}
fn reload(ticket: &Ticket, domain: &str, owner: &str) -> Result<Snapshot> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&ticket.path)
        .map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o777 != 0o600
        || meta.len() > LIMIT
    {
        return Err("journal file not private/regular/bounded".into());
    }
    let mut bytes = vec![];
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT || Sha256::digest(&bytes).as_slice() != ticket.digest {
        return Err("journal changed".into());
    }
    let snapshot: Snapshot = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    snapshot.validate(domain, owner)?;
    Ok(snapshot)
}

struct Lab {
    domain: String,
    owner: String,
    journals: tempfile::TempDir,
}
impl Lab {
    // No domain or key comes from a caller, fixture file, environment, or IPC.
    fn new() -> Result<Self> {
        let lab = Self {
            domain: format!("{PREFIX}{}", uuid::Uuid::new_v4()),
            owner: uuid::Uuid::new_v4().to_string(),
            journals: tempfile::tempdir().map_err(|e| e.to_string())?,
        };
        for scope in [Scope::AnyHost, Scope::ThisHost] {
            lab.write(scope, Key::Marker, Some(&Atom::text(&lab.owner)))?;
            lab.write(
                scope,
                Key::Other,
                Some(&Atom::text("keep fixture unrelated data")),
            )?;
        }
        Ok(lab)
    }
    fn write(&self, scope: Scope, key: Key, value: Option<&Atom>) -> Result<()> {
        let domain = Owned::text(&self.domain)?;
        let key = Owned::text(key.text())?;
        let value = value.map(Atom::native).transpose()?;
        unsafe {
            CFPreferencesSetValue(
                key.0.cast(),
                value.as_ref().map_or(ptr::null(), |v| v.0),
                domain.0.cast(),
                kCFPreferencesCurrentUser,
                scope.native(),
            );
            if CFPreferencesSynchronize(domain.0.cast(), kCFPreferencesCurrentUser, scope.native())
                == 0
            {
                return Err("fixture synchronization failed; outcome unknown".into());
            }
        }
        Ok(())
    }
    fn raw(&self, scope: Scope, key: Key) -> Result<Option<Atom>> {
        let domain = Owned::text(&self.domain)?;
        let key = Owned::text(key.text())?;
        unsafe {
            let raw = CFPreferencesCopyValue(
                key.0.cast(),
                domain.0.cast(),
                kCFPreferencesCurrentUser,
                scope.native(),
            );
            if raw.is_null() {
                return Ok(None);
            }
            let raw = Owned::new(raw)?;
            Atom::copy(raw.0).map(Some)
        }
    }
    fn observe(&self, scope: Scope) -> Result<Option<Atom>> {
        let domain = Owned::text(&self.domain)?;
        let key = Owned::text(Key::Probe.text())?;
        unsafe {
            if CFPreferencesSynchronize(domain.0.cast(), kCFPreferencesCurrentUser, scope.native())
                == 0
                || CFPreferencesAppValueIsForced(key.0.cast(), domain.0.cast()) != 0
            {
                return Err("fixture read/management state uncertain".into());
            }
        }
        if self.raw(scope, Key::Marker)? != Some(Atom::text(&self.owner)) {
            return Err("fixture ownership marker missing/changed".into());
        }
        let keys = Owned::new(
            unsafe {
                CFPreferencesCopyKeyList(domain.0.cast(), kCFPreferencesCurrentUser, scope.native())
            }
            .cast(),
        )?;
        let count = unsafe { CFArrayGetCount(keys.0.cast()) };
        if !(1..=8).contains(&count) {
            return Err("fixture key list unavailable/unbounded".into());
        }
        let listed = unsafe {
            CFArrayContainsValue(
                keys.0.cast(),
                CFRange {
                    location: 0,
                    length: count,
                },
                key.0,
            )
        } != 0;
        let first = self.raw(scope, Key::Probe)?;
        if listed != first.is_some()
            || first != self.raw(scope, Key::Probe)?
            || self.raw(scope, Key::Marker)? != Some(Atom::text(&self.owner))
        {
            return Err("fixture scope/key changed while reading".into());
        }
        Ok(first)
    }
    fn prepare(&self, scope: Scope, expected: Atom) -> Result<Ticket> {
        let original = self.observe(scope)?;
        let snapshot = Snapshot {
            protocol: 1,
            domain: self.domain.clone(),
            owner: self.owner.clone(),
            scope,
            key: Key::Probe.text().into(),
            original,
            expected,
        };
        snapshot.validate(&self.domain, &self.owner)?;
        persist(
            &self
                .journals
                .path()
                .join(format!("{}.json", uuid::Uuid::new_v4())),
            &snapshot,
        )
    }
    fn apply(&self, ticket: &Ticket) -> Result<()> {
        self.apply_with(ticket, || Ok(()))
    }
    fn apply_with(&self, ticket: &Ticket, after_check: impl FnOnce() -> Result<()>) -> Result<()> {
        let snapshot = reload(ticket, &self.domain, &self.owner)?;
        if self.observe(snapshot.scope)? != snapshot.original {
            return Err("fixture changed before apply; preserve current value".into());
        }
        if snapshot
            .original
            .as_ref()
            .is_some_and(|old| old == &snapshot.expected || old.could_coalesce(&snapshot.expected))
        {
            return Err(
                "fixture equivalent/no-change or boolean-integer representation conflict; no write"
                    .into(),
            );
        }
        after_check()?;
        self.write(snapshot.scope, Key::Probe, Some(&snapshot.expected))?;
        if self.observe(snapshot.scope)? != Some(snapshot.expected) {
            return Err("fixture write-back not confirmed".into());
        }
        Ok(())
    }
    fn restore(&self, ticket: &Ticket) -> Result<()> {
        let snapshot = reload(ticket, &self.domain, &self.owner)?;
        if self.observe(snapshot.scope)? != Some(snapshot.expected) {
            return Err("fixture changed before restore; preserve current value".into());
        }
        self.write(snapshot.scope, Key::Probe, snapshot.original.as_ref())?;
        if self.observe(snapshot.scope)? != snapshot.original {
            return Err("fixture restoration not confirmed".into());
        }
        Ok(())
    }
    fn effective(&self) -> Result<Option<Atom>> {
        let domain = Owned::text(&self.domain)?;
        let key = Owned::text(Key::Probe.text())?;
        let raw = unsafe { CFPreferencesCopyAppValue(key.0.cast(), domain.0.cast()) };
        if raw.is_null() {
            return Ok(None);
        }
        let raw = Owned::new(raw)?;
        unsafe { Atom::copy(raw.0).map(Some) }
    }
    fn clear(&self) -> Result<()> {
        for scope in [Scope::AnyHost, Scope::ThisHost] {
            if self.raw(scope, Key::Marker)? != Some(Atom::text(&self.owner)) {
                return Err("fixture cleanup owner unknown; preserve domain".into());
            }
            for key in [Key::Probe, Key::Other, Key::Marker] {
                self.write(scope, key, None)?;
                if self.raw(scope, key)?.is_some() {
                    return Err("fixture key cleanup not confirmed".into());
                }
            }
        }
        Ok(())
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        // Only this generated domain and fixture keys, never a whole domain or
        // Apple keys. Unknown ownership keeps the fixture for diagnosis.
        for scope in [Scope::AnyHost, Scope::ThisHost] {
            if self.raw(scope, Key::Marker).ok() != Some(Some(Atom::text(&self.owner))) {
                continue;
            }
            for key in [Key::Probe, Key::Other, Key::Marker] {
                let _ = self.write(scope, key, None);
            }
        }
    }
}

#[test]
fn typed_atoms_preserve_bool_integer_text_and_reject_unsupported_values() {
    for atom in [
        Atom::Boolean(true),
        Atom::Boolean(false),
        Atom::Integer(1),
        Atom::Integer(i64::MIN),
        Atom::text(""),
        Atom::text("1"),
        Atom::text("中文🚀\0尾"),
    ] {
        let raw = atom.native().unwrap();
        assert_eq!(unsafe { Atom::copy(raw.0).unwrap() }, atom);
    }
    assert!(Atom::Text(vec![0xd800]).native().is_err());
    assert!(Atom::Text(vec![1; 1025]).native().is_err());
    assert!(Atom::Boolean(true).could_coalesce(&Atom::Integer(1)));
    assert!(Atom::Integer(0).could_coalesce(&Atom::Boolean(false)));
    assert!(!Atom::text("1").could_coalesce(&Atom::Boolean(true)));
    unsafe {
        use core_foundation_sys::number::kCFNumberFloat64Type;
        let value = 1.5f64;
        let raw = Owned::new(
            CFNumberCreate(
                ptr::null(),
                kCFNumberFloat64Type,
                (&value as *const f64).cast(),
            )
            .cast(),
        )
        .unwrap();
        assert!(Atom::copy(raw.0).is_err());
        assert_eq!(
            Atom::copy(kCFBooleanTrue.cast()).unwrap(),
            Atom::Boolean(true)
        );
        assert_eq!(
            Atom::copy(kCFBooleanFalse.cast()).unwrap(),
            Atom::Boolean(false)
        );
    }
}

fn fixture_snapshot() -> Snapshot {
    Snapshot {
        protocol: 1,
        domain: format!("{PREFIX}{}", uuid::Uuid::new_v4()),
        owner: uuid::Uuid::new_v4().to_string(),
        scope: Scope::AnyHost,
        key: Key::Probe.text().into(),
        original: None,
        expected: Atom::Boolean(true),
    }
}
#[test]
fn journal_is_private_exclusive_bounded_and_bound_to_exact_fixture() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("intent.json");
    let snapshot = fixture_snapshot();
    let ticket = persist(&path, &snapshot).unwrap();
    assert_eq!(
        reload(&ticket, &snapshot.domain, &snapshot.owner)
            .unwrap()
            .original,
        None
    );
    assert!(persist(&path, &snapshot).is_err());
    assert!(reload(&ticket, "com.apple.desktopservices", &snapshot.owner).is_err());
    assert!(reload(&ticket, &snapshot.domain, "other-owner").is_err());
    std::fs::write(&path, b"{}").unwrap();
    assert!(reload(&ticket, &snapshot.domain, &snapshot.owner).is_err());
}

#[test]
fn journal_schema_and_unsupported_originals_cannot_authorize_a_change() {
    let snapshot = fixture_snapshot();
    for field in ["protocol", "key", "domain", "original"] {
        let mut value = serde_json::to_value(&snapshot).unwrap();
        value[field] = match field {
            "protocol" => serde_json::json!(2),
            "key" => serde_json::json!("DSDontWriteNetworkStores"),
            "domain" => serde_json::json!("com.apple.desktopservices"),
            _ => serde_json::json!({ "Text": [55296] }),
        };
        let changed: Snapshot = serde_json::from_value(value).unwrap();
        assert!(changed.validate(&snapshot.domain, &snapshot.owner).is_err());
    }
    let mut value = serde_json::to_value(&snapshot).unwrap();
    value["scope"] = serde_json::json!("GlobalOrOtherUser");
    assert!(serde_json::from_value::<Snapshot>(value).is_err());
}

#[test]
#[ignore = "Writes/restores only newly minted UUID fixture preference domains, never production keys"]
fn live_original_types_and_missing_round_trip_in_both_explicit_scopes() {
    let lab = Lab::new().unwrap();
    for scope in [Scope::AnyHost, Scope::ThisHost] {
        for original in [
            None,
            Some(Atom::Boolean(false)),
            Some(Atom::Boolean(true)),
            Some(Atom::Integer(0)),
            Some(Atom::Integer(1)),
            Some(Atom::Integer(i64::MIN)),
            Some(Atom::text("")),
            Some(Atom::text("1")),
            Some(Atom::text("中文🚀\0尾")),
        ] {
            // Fresh fixture setup only, never a delete/rewrite workaround in apply.
            lab.write(scope, Key::Probe, None).unwrap();
            lab.write(scope, Key::Probe, original.as_ref()).unwrap();
            let desired = Atom::Boolean(!matches!(
                original,
                Some(Atom::Integer(1) | Atom::Boolean(true))
            ));
            let ticket = lab.prepare(scope, desired.clone()).unwrap();
            let stored = reload(&ticket, &lab.domain, &lab.owner).unwrap();
            assert_eq!(stored.original, original);
            lab.apply(&ticket)
                .unwrap_or_else(|error| panic!("scope={scope:?}, original={original:?}: {error}"));
            assert_eq!(lab.observe(scope).unwrap(), Some(desired));
            lab.restore(&ticket).unwrap();
            assert_eq!(lab.observe(scope).unwrap(), original);
            assert_eq!(
                lab.raw(scope, Key::Other).unwrap(),
                Some(Atom::text("keep fixture unrelated data"))
            );
        }
    }
    lab.clear().unwrap();
}
#[test]
#[ignore = "Writes/restores only newly minted UUID fixture preference domains, never production keys"]
fn live_explicit_missing_does_not_capture_inherited_effective_value() {
    let lab = Lab::new().unwrap();
    lab.write(Scope::AnyHost, Key::Probe, Some(&Atom::text("fallback")))
        .unwrap();
    assert_eq!(lab.observe(Scope::ThisHost).unwrap(), None);
    assert_eq!(lab.effective().unwrap(), Some(Atom::text("fallback")));
    let ticket = lab.prepare(Scope::ThisHost, Atom::Boolean(true)).unwrap();
    lab.apply(&ticket).unwrap();
    lab.restore(&ticket).unwrap();
    assert_eq!(lab.observe(Scope::ThisHost).unwrap(), None);
    assert_eq!(
        lab.observe(Scope::AnyHost).unwrap(),
        Some(Atom::text("fallback"))
    );
    assert_eq!(lab.effective().unwrap(), Some(Atom::text("fallback")));
    lab.clear().unwrap();
}
#[test]
#[ignore = "Writes/restores only newly minted UUID fixture preference domains, never production keys"]
fn live_concurrent_changes_and_changed_journals_are_preserved() {
    let lab = Lab::new().unwrap();
    let ticket = lab.prepare(Scope::AnyHost, Atom::Boolean(true)).unwrap();
    lab.write(
        Scope::AnyHost,
        Key::Probe,
        Some(&Atom::text("changed before apply")),
    )
    .unwrap();
    assert!(lab.apply(&ticket).is_err());
    assert_eq!(
        lab.observe(Scope::AnyHost).unwrap(),
        Some(Atom::text("changed before apply"))
    );
    let ticket = lab.prepare(Scope::AnyHost, Atom::Boolean(true)).unwrap();
    lab.apply(&ticket).unwrap();
    lab.write(Scope::AnyHost, Key::Probe, Some(&Atom::Integer(2)))
        .unwrap();
    assert!(lab.restore(&ticket).is_err());
    assert_eq!(lab.observe(Scope::AnyHost).unwrap(), Some(Atom::Integer(2)));
    let ticket = lab.prepare(Scope::AnyHost, Atom::Boolean(true)).unwrap();
    lab.apply(&ticket).unwrap();
    std::fs::write(&ticket.path, b"{}").unwrap();
    assert!(lab.restore(&ticket).is_err());
    assert_eq!(
        lab.observe(Scope::AnyHost).unwrap(),
        Some(Atom::Boolean(true))
    );
    lab.clear().unwrap();
}
#[test]
#[ignore = "Writes/restores only newly minted UUID fixture preference domains, never production keys"]
fn live_intent_can_restore_after_missing_completion_notification() {
    let lab = Lab::new().unwrap();
    let ticket = lab.prepare(Scope::AnyHost, Atom::Boolean(true)).unwrap();
    // Simulate write having happened before the observer received a result.
    lab.write(Scope::AnyHost, Key::Probe, Some(&Atom::Boolean(true)))
        .unwrap();
    lab.restore(&ticket).unwrap();
    assert_eq!(lab.observe(Scope::AnyHost).unwrap(), None);
    assert!(lab.restore(&ticket).is_err());
    lab.clear().unwrap();
}

#[test]
#[ignore = "Writes/restores only newly minted UUID fixture preference domains, never production keys"]
fn live_equivalent_integer_boolean_type_change_is_refused() {
    let lab = Lab::new().unwrap();
    for (integer, boolean) in [(0, false), (1, true)] {
        lab.write(Scope::AnyHost, Key::Probe, None).unwrap();
        lab.write(Scope::AnyHost, Key::Probe, Some(&Atom::Integer(integer)))
            .unwrap();
        let ticket = lab.prepare(Scope::AnyHost, Atom::Boolean(boolean)).unwrap();
        assert!(lab
            .apply(&ticket)
            .unwrap_err()
            .contains("representation conflict"));
        assert_eq!(
            lab.observe(Scope::AnyHost).unwrap(),
            Some(Atom::Integer(integer))
        );
    }
    lab.clear().unwrap();
}

#[test]
#[ignore = "Writes/restores only newly minted UUID fixture preference domains; reproduces the non-atomic final-check/write gap"]
fn live_final_check_write_gap_is_not_atomic() {
    let lab = Lab::new().unwrap();
    let ticket = lab.prepare(Scope::AnyHost, Atom::Boolean(true)).unwrap();
    lab.apply_with(&ticket, || {
        lab.write(
            Scope::AnyHost,
            Key::Probe,
            Some(&Atom::text("racing fixture writer")),
        )
    })
    .unwrap();
    // This proves the limitation; it is not a concurrency safety pass. A
    // matching read-back cannot prove that a later writer was not overwritten.
    assert_eq!(
        lab.observe(Scope::AnyHost).unwrap(),
        Some(Atom::Boolean(true))
    );
    lab.clear().unwrap();
}
