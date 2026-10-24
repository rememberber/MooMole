//! Parse plist data via Foundation, never plutil prose or executable scripts.
use objc2::{
    rc::{autoreleasepool, Retained},
    runtime::AnyObject,
};
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSPropertyListReadOptions, NSPropertyListSerialization, NSString,
};
use std::path::Path;

fn parse(bytes: &[u8]) -> Result<Retained<NSDictionary>, &'static str> {
    if bytes.is_empty() || bytes.len() > 512 * 1024 {
        return Err("plistUnavailable");
    }
    let object = unsafe {
        NSPropertyListSerialization::propertyListWithData_options_format_error(
            &NSData::with_bytes(bytes),
            NSPropertyListReadOptions::Immutable,
            std::ptr::null_mut(),
        )
    }
    .map_err(|_| "plistUnavailable")?;
    object
        .downcast::<NSDictionary>()
        .map_err(|_| "plistUnavailable")
}
fn value(dict: &NSDictionary, key: &str) -> Option<Retained<AnyObject>> {
    dict.objectForKey(&NSString::from_str(key))
}
fn text(object: &AnyObject) -> Result<String, &'static str> {
    let s = object
        .downcast_ref::<NSString>()
        .ok_or("plistUnsupported")?
        .to_string();
    if s.len() > 4096 || s.contains('\0') {
        return Err("plistUnsupported");
    }
    Ok(s)
}
fn strings(object: &AnyObject) -> Result<Vec<String>, &'static str> {
    if object.downcast_ref::<NSString>().is_some() {
        return Ok(vec![text(object)?]);
    }
    array_strings(object)
}
fn array_strings(object: &AnyObject) -> Result<Vec<String>, &'static str> {
    let array = object.downcast_ref::<NSArray>().ok_or("plistUnsupported")?;
    if array.count() > 128 {
        return Err("plistUnsupported");
    }
    array.iter().map(|v| text(&v)).collect()
}
pub(super) fn privileged(bytes: &[u8]) -> Result<bool, &'static str> {
    autoreleasepool(|_| {
        let dict = parse(bytes)?;
        Ok(["SMPrivilegedExecutables", "SMAuthorizedClients"]
            .iter()
            .any(|k| value(&dict, k).is_some()))
    })
}
pub(super) fn launch_reference(bytes: &[u8], id: &str, app: &Path) -> Result<bool, &'static str> {
    autoreleasepool(|_| {
        let dict = parse(bytes)?;
        let label_object = value(&dict, "Label").ok_or("plistUnsupported")?;
        let label = text(&label_object)?;
        if label.is_empty() {
            return Err("plistUnsupported");
        }
        if super::related_id(&label, id) {
            return Ok(true);
        }
        if let Some(ids) = value(&dict, "AssociatedBundleIdentifiers") {
            if strings(&ids)?.iter().any(|s| super::related_id(s, id)) {
                return Ok(true);
            }
        }
        let mut paths = vec![];
        for key in ["Program", "BundleProgram", "WorkingDirectory"] {
            if let Some(v) = value(&dict, key) {
                let raw = text(&v)?;
                if !Path::new(&raw).is_absolute() {
                    return Err("opaqueLaunchReference");
                }
                paths.push(raw);
            }
        }
        for key in ["ProgramArguments", "WatchPaths", "QueueDirectories"] {
            if let Some(v) = value(&dict, key) {
                paths.extend(array_strings(&v)?);
            }
        }
        if value(&dict, "Program").is_none() && value(&dict, "BundleProgram").is_none() {
            let args = value(&dict, "ProgramArguments").ok_or("plistUnsupported")?;
            let args = array_strings(&args)?;
            if args.first().is_none_or(|s| !Path::new(s).is_absolute()) {
                return Err("opaqueLaunchReference");
            }
        }
        for raw in paths {
            let path = Path::new(&raw);
            // Arguments may contain scripts/URLs rather than paths. A literal
            // reference remains a retention clue, never deletion ownership.
            if raw.contains(app.to_str().ok_or("entryUnavailable")?) || raw.contains(id) {
                return Ok(true);
            }
            if path.starts_with(app) {
                return Ok(true);
            }
            // A differently spelled path to the same named root is unresolved
            // here; do not silently classify an alias as unrelated.
            if path.ancestors().any(|p| p.file_name() == app.file_name()) {
                return Err("aliasedLaunchReference");
            }
        }
        Ok(false)
    })
}
