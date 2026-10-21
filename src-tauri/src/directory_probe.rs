//! Shared no-follow, descriptor-relative metadata and directory access.
use std::{
    ffi::CStr,
    fs::File,
    mem::MaybeUninit,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
};
pub(crate) struct Stream(pub(crate) *mut libc::DIR);
impl Drop for Stream {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}
pub(crate) fn stat_at(parent: &File, name: &CStr) -> Result<libc::stat, String> {
    let mut stat = MaybeUninit::uninit();
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err("子项无法读取。".into());
    }
    Ok(unsafe { stat.assume_init() })
}
pub(crate) fn local(file: &File) -> bool {
    use std::os::macos::fs::MetadataExt as _;
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    (unsafe {
        libc::fstatfs(file.as_raw_fd(), stat.as_mut_ptr()) == 0
            && stat.assume_init().f_flags & libc::MNT_LOCAL as u32 != 0
    }) && file
        .metadata()
        .is_ok_and(|m| m.st_flags() & 0x40000000 == 0)
}
pub(crate) fn open_child(parent: &File, name: &CStr, before: &libc::stat) -> Result<File, String> {
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY
                | libc::O_DIRECTORY
                | libc::O_NOFOLLOW
                | libc::O_NONBLOCK
                | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err("目录不可读或已变化。".into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    if !file
        .metadata()
        .is_ok_and(|m| m.dev() == before.st_dev as u64 && m.ino() == before.st_ino && m.is_dir())
    {
        return Err("目录身份已变化。".into());
    }
    Ok(file)
}
