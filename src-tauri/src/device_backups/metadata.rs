use super::*;
use objc2::{
    rc::{autoreleasepool, Retained},
    runtime::AnyObject,
};
use objc2_foundation::{
    NSData, NSDictionary, NSNumber, NSPropertyListReadOptions, NSPropertyListSerialization,
    NSString,
};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::File,
    io::Read,
    os::fd::{AsRawFd, FromRawFd},
};
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Metadata {
    pub device: String,
    pub product: Option<String>,
    pub encrypted: Option<bool>,
    pub finished: bool,
    hashes: Vec<String>,
}
fn text(d: &NSDictionary, key: &str) -> Option<String> {
    d.objectForKey(&NSString::from_str(key))
        .and_then(|o| o.downcast_ref::<NSString>().map(ToString::to_string))
        .filter(|s| !s.is_empty() && s.len() <= 200 && !s.chars().any(char::is_control))
}
fn bytes(held: &AnchoredPath, name: &str) -> Result<Vec<u8>, String> {
    let key = CString::new(name).unwrap();
    let before = crate::directory_probe::stat_at(held.target(), &key)?;
    if before.st_mode & libc::S_IFMT != libc::S_IFREG
        || before.st_nlink != 1
        || before.st_uid != unsafe { libc::geteuid() }
        || before.st_size <= 0
        || before.st_size > 16 * 1024 * 1024
        || before.st_flags & 0x40000000 != 0
    {
        return Err("备份状态清单缺失、过大、共享或尚未落地。".into());
    }
    let fd = unsafe {
        libc::openat(
            held.target().as_raw_fd(),
            key.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err("备份状态清单不可读。".into());
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let m = file.metadata().map_err(|e| e.to_string())?;
    if m.dev() != before.st_dev as u64 || m.ino() != before.st_ino {
        return Err("备份状态清单已变化。".into());
    }
    let mut out = vec![];
    (&mut file)
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    let after = file.metadata().map_err(|e| e.to_string())?;
    let current = crate::directory_probe::stat_at(held.target(), &key)?;
    if out.len() != m.len() as usize
        || out.len() > 16 * 1024 * 1024
        || after.mtime() != m.mtime()
        || after.mtime_nsec() != m.mtime_nsec()
        || after.ctime() != m.ctime()
        || after.ctime_nsec() != m.ctime_nsec()
        || current.st_ino != before.st_ino
        || current.st_dev != before.st_dev
    {
        return Err("读取时备份状态清单变化。".into());
    }
    Ok(out)
}
fn parse(bytes: &[u8]) -> Result<Retained<NSDictionary>, String> {
    let value: Retained<AnyObject> = unsafe {
        NSPropertyListSerialization::propertyListWithData_options_format_error(
            &NSData::with_bytes(bytes),
            NSPropertyListReadOptions::Immutable,
            std::ptr::null_mut(),
        )
    }
    .map_err(|_| "备份状态格式无法识别。")?;
    value
        .downcast::<NSDictionary>()
        .map_err(|_| "备份状态不是字典。".into())
}
pub(super) fn read(held: &AnchoredPath) -> Result<Metadata, String> {
    autoreleasepool(|_| {
        let info = bytes(held, "Info.plist")?;
        let status = bytes(held, "Status.plist")?;
        let manifest = bytes(held, "Manifest.plist")?;
        let i = parse(&info)?;
        let s = parse(&status)?;
        let m = parse(&manifest)?;
        let database =
            crate::directory_probe::stat_at(held.target(), &CString::new("Manifest.db").unwrap())?;
        if database.st_mode & libc::S_IFMT != libc::S_IFREG
            || database.st_nlink != 1
            || database.st_size <= 0
            || database.st_flags & 0x40000000 != 0
        {
            return Err("本轮仅支持具有普通 Manifest.db 的备份；旧版或无法核对的格式保留。".into());
        }
        let device = text(&i, "Device Name").ok_or("缺少可识别的设备名称，本轮保留。")?;
        let product = text(&i, "Product Type");
        let encrypted = m
            .objectForKey(&NSString::from_str("IsEncrypted"))
            .and_then(|v| v.downcast_ref::<NSNumber>().map(|n| n.boolValue()));
        held.validate()?;
        Ok(Metadata {
            device,
            product,
            encrypted,
            finished: text(&s, "SnapshotState").as_deref() == Some("finished"),
            hashes: [info, status, manifest]
                .iter()
                .map(|v| format!("{:x}", Sha256::digest(v)))
                .collect(),
        })
    })
}
