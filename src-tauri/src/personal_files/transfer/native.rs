use super::*;
use sha2::{Digest, Sha256};
use std::{
    ffi::{c_void, CString},
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::fd::{AsRawFd, FromRawFd},
};
extern "C" {
    fn copyfile_state_alloc() -> *mut c_void;
    fn copyfile_state_free(state: *mut c_void) -> i32;
    fn copyfile_state_set(state: *mut c_void, flag: u32, value: *const c_void) -> i32;
    fn fcopyfile(source: i32, destination: i32, state: *mut c_void, flags: u32) -> i32;
}
struct Cancel<'a> {
    job: &'a Job,
    began: Instant,
}
unsafe extern "C" fn progress(
    _: i32,
    stage: i32,
    _: *mut c_void,
    _: *const i8,
    _: *const i8,
    context: *mut c_void,
) -> i32 {
    let control = unsafe { &*(context as *const Cancel<'_>) };
    if stage == 3 || stopped(control.job, control.began).is_err() {
        2
    } else {
        0
    }
}
struct CopyState(*mut c_void);
impl Drop for CopyState {
    fn drop(&mut self) {
        unsafe {
            copyfile_state_free(self.0);
        }
    }
}
struct Temporary {
    parent: File,
    name: CString,
    file: File,
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0
        {
            let s = unsafe { stat.assume_init() };
            if self
                .file
                .metadata()
                .is_ok_and(|m| m.dev() == s.st_dev as u64 && m.ino() == s.st_ino)
            {
                unsafe {
                    libc::unlinkat(self.parent.as_raw_fd(), self.name.as_ptr(), 0);
                }
            }
        }
    }
}
pub(super) struct Copied {
    pub receipt: Receipt,
    pub witness: Witness,
    pub sha256: String,
    pub metadata_sha256: String,
    size: u64,
}
fn open(held: &AnchoredPath) -> Result<File, String> {
    let node = held.witness.nodes.last().ok_or("缺少文件身份。")?;
    let name = CString::new(node.path.file_name().unwrap().as_encoded_bytes())
        .map_err(|e| e.to_string())?;
    let fd = unsafe {
        libc::openat(
            held.parent().as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err("文件不可读取，原件保留。".into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let m = file.metadata().map_err(|e| e.to_string())?;
    if !m.is_file() || m.nlink() != 1 || (m.dev(), m.ino()) != (node.dev, node.ino) {
        return Err("文件读取身份变化。".into());
    }
    held.validate()?;
    Ok(file)
}
fn content(file: &mut File, size: u64, job: &Job, began: Instant) -> Result<String, String> {
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 128 * 1024];
    loop {
        stopped(job, began)?;
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > size {
            return Err("文件内容在校验期间变大。".into());
        }
        digest.update(&buffer[..n]);
    }
    if count != size {
        return Err("文件未完整校验，原件保留。".into());
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn metadata(file: &File, job: &Job, began: Instant) -> Result<String, String> {
    stopped(job, began)?;
    let m = file.metadata().map_err(|e| e.to_string())?;
    if crate::permissions::has_acl(file).map_err(|_| "访问控制列表无法核对。")? {
        return Err("存在无法核对的访问控制列表，原件保留。".into());
    }
    let length = unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0, 0) };
    if !(0..=65536).contains(&length) {
        return Err("扩展属性列表超过核验范围。".into());
    }
    let mut names = vec![0u8; length as usize];
    if length > 0
        && unsafe { libc::flistxattr(file.as_raw_fd(), names.as_mut_ptr().cast(), names.len(), 0) }
            != length
    {
        return Err("扩展属性列表在读取期间变化。".into());
    }
    let mut attributes = vec![];
    let mut total = 0usize;
    for name in names.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        stopped(job, began)?;
        let name = CString::new(name).map_err(|e| e.to_string())?;
        let size = unsafe {
            libc::fgetxattr(
                file.as_raw_fd(),
                name.as_ptr(),
                std::ptr::null_mut(),
                0,
                0,
                0,
            )
        };
        if size < 0 || size as usize > 16 * 1024 * 1024 || total + size as usize > 16 * 1024 * 1024
        {
            return Err("扩展属性或资源分支超出本轮十六 MiB 核验范围，原件保留。".into());
        }
        total += size as usize;
        let mut bytes = vec![0u8; size as usize];
        if unsafe {
            libc::fgetxattr(
                file.as_raw_fd(),
                name.as_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                0,
                0,
            )
        } != size
        {
            return Err("扩展属性或资源分支变化，原件保留。".into());
        }
        // libquarantine may reserialize the agent display field during Apple's
        // copy operation. Do not write/remove security attributes to undo that.
        // Require unchanged flags and event ID. Apple's copy API assigns its
        // own timestamp/agent; do not rewrite either field. Other xattrs
        // remain byte-exact, including resource forks and provenance.
        let canonical = if name.to_bytes() == b"com.apple.quarantine" {
            quarantine(&bytes)?
        } else {
            bytes
        };
        attributes.push((
            name.to_bytes().to_vec(),
            canonical.len(),
            format!("{:x}", Sha256::digest(canonical)),
        ));
    }
    attributes.sort_by(|a, b| a.0.cmp(&b.0));
    let body = serde_json::to_vec(&(
        m.uid(),
        m.gid(),
        m.mode(),
        m.mtime(),
        m.mtime_nsec(),
        attributes,
    ))
    .map_err(|e| e.to_string())?;
    Ok(format!("{:x}", Sha256::digest(body)))
}
fn quarantine(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "隔离标记格式不支持，原件保留。")?;
    let fields = text.split(';').collect::<Vec<_>>();
    if fields.len() != 4 || text.chars().any(char::is_control) {
        return Err("隔离标记结构未知，原件保留。".into());
    }
    let flags = u64::from_str_radix(fields[0], 16).map_err(|_| "隔离保护标记无法核对。")?;
    let _timestamp = u64::from_str_radix(fields[1], 16).map_err(|_| "隔离标记时间无法核对。")?;
    Ok(format!("{flags:x};{}", fields[3]).into_bytes())
}
pub(super) fn verify_copy(copy: &Copied, job: &Job, began: Instant) -> Result<(), String> {
    stopped(job, began)?;
    let held = AnchoredPath::open(Path::new(&copy.receipt.destination))?;
    if held.witness != copy.witness || !held.local_materialized() {
        return Err("目标副本身份或内容已变化，原件保留。".into());
    }
    let mut file = open(&held)?;
    if content(&mut file, copy.size, job, began)? != copy.sha256
        || metadata(&file, job, began)? != copy.metadata_sha256
    {
        return Err("目标副本或元数据校验变化，原件保留。".into());
    }
    held.validate()
}
pub(super) fn copy(
    item: &Item,
    movement: &movement::Movement,
    target: &str,
    temporary: &Path,
    job: &Job,
    began: Instant,
    gate: impl Fn() -> Result<(), String>,
) -> Result<Copied, String> {
    gate().map_err(|e| format!("复制前复核：{e}"))?;
    let source = AnchoredPath::open(Path::new(&item.path))?;
    if source.witness != item.witness || !source.local_materialized() {
        return Err("复制源身份变化。".into());
    }
    let directory = AnchoredPath::open(Path::new(&movement.directory))?;
    if directory.witness.nodes != movement.witness.nodes
        || temporary.parent() != Some(Path::new(&movement.directory))
        || Path::new(target).parent() != Some(Path::new(&movement.directory))
    {
        return Err("目标目录范围变化。".into());
    }
    let mut input = open(&source)?;
    let source_metadata = metadata(&input, job, began)?;
    source.validate()?;
    let parent = directory.target().try_clone().map_err(|e| e.to_string())?;
    let name = CString::new(temporary.file_name().unwrap().as_encoded_bytes())
        .map_err(|e| e.to_string())?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err("临时副本无法排他创建，原件保留。".into());
    }
    let mut temporary = Temporary {
        parent,
        name,
        file: unsafe { File::from_raw_fd(fd) },
    };
    let state = CopyState(unsafe { copyfile_state_alloc() });
    if state.0.is_null() {
        return Err("系统复制状态不可用。".into());
    }
    let context = Cancel { job, began };
    if unsafe { copyfile_state_set(state.0, 6, progress as *const c_void) } != 0
        || unsafe { copyfile_state_set(state.0, 7, (&context as *const Cancel<'_>).cast()) } != 0
    {
        return Err("系统复制取消回调不可用。".into());
    }
    // COPYFILE_ALL copies data, stat, ACL and xattrs (including resource forks).
    // Never MOVE, UNLINK, recursive traversal, or a copy/delete fallback.
    source
        .validate()
        .map_err(|e| format!("调用系统复制前复核：{e}"))?;
    if unsafe { fcopyfile(input.as_raw_fd(), temporary.file.as_raw_fd(), state.0, 15) } != 0 {
        return Err("系统复制未完整完成；原件未处理，临时位置请按记录核对。".into());
    }
    // Complete resource-fork descriptor cleanup before taking final witnesses.
    drop(state);
    stopped(job, began)?;
    source
        .validate()
        .map_err(|e| format!("复制后源文件复核：{e}"))?;
    let sha256 = content(&mut input, item.size, job, began)?;
    if content(&mut temporary.file, item.size, job, began)? != sha256
        || metadata(&temporary.file, job, began)? != source_metadata
    {
        return Err("副本的数据、权限、时间或扩展属性不一致；原件保留。".into());
    }
    temporary.file.sync_all().map_err(|e| e.to_string())?;
    if unsafe { libc::fcntl(temporary.file.as_raw_fd(), libc::F_FULLFSYNC) } != 0 {
        return Err("目标副本落盘状态无法核对，原件保留。".into());
    }
    gate().map_err(|e| format!("发布前复核：{e}"))?;
    source
        .validate()
        .map_err(|e| format!("发布前源文件复核：{e}"))?;
    let actual = temporary.file.metadata().map_err(|e| e.to_string())?;
    let mut named = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            temporary.parent.as_raw_fd(),
            temporary.name.as_ptr(),
            named.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err("临时副本名称变化。".into());
    }
    let named = unsafe { named.assume_init() };
    if actual.nlink() != 1 || (actual.dev(), actual.ino()) != (named.st_dev as u64, named.st_ino) {
        return Err("临时副本身份变化。".into());
    }
    let to = CString::new(Path::new(target).file_name().unwrap().as_encoded_bytes())
        .map_err(|e| e.to_string())?;
    if unsafe {
        libc::renameatx_np(
            temporary.parent.as_raw_fd(),
            temporary.name.as_ptr(),
            temporary.parent.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_EXCL,
        )
    } != 0
    {
        return Err("目标名称已占用或发布失败，未覆盖；原件保留。".into());
    }
    temporary
        .parent
        .sync_all()
        .map_err(|_| "目标副本发布后的落盘状态未知，请核对。")?;
    let destination = AnchoredPath::open(Path::new(target))?;
    let node = destination.witness.nodes.last().unwrap();
    if (node.dev, node.ino) != (actual.dev(), actual.ino()) || !destination.local_materialized() {
        return Err("发布后的目标身份未知，原件保留。".into());
    }
    let copied = Copied {
        receipt: Receipt {
            destination: target.into(),
            device: node.dev,
            inode: node.ino,
        },
        witness: destination.witness,
        sha256,
        metadata_sha256: source_metadata,
        size: item.size,
    };
    verify_copy(&copied, job, began)?;
    Ok(copied)
}
