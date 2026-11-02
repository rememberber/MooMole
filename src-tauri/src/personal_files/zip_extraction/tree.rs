use super::*;
use std::{collections::BTreeMap, ffi::CStr};
type Identity = (u64, u64, bool);
pub(super) struct Stage {
    parent: File,
    pub(super) name: CString,
    pub(super) root: File,
    owned: BTreeMap<String, Identity>,
    pub(super) published: bool,
    cleaned: bool,
}
fn open(parent: &File, name: &str, directory: bool) -> Result<File, String> {
    let name = CString::new(name).map_err(|e| e.to_string())?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY
                | libc::O_NOFOLLOW
                | libc::O_NONBLOCK
                | libc::O_CLOEXEC
                | if directory { libc::O_DIRECTORY } else { 0 },
        )
    };
    if fd < 0 {
        return Err("解压目录或文件无法以原身份打开。".into());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn identity(file: &File) -> Result<Identity, String> {
    let m = file.metadata().map_err(|e| e.to_string())?;
    Ok((m.dev(), m.ino(), m.is_dir()))
}
fn put_quarantine(file: &File, q: Option<&[u8]>) -> Result<(), String> {
    if let Some(q) = q {
        if unsafe {
            libc::fsetxattr(
                file.as_raw_fd(),
                c"com.apple.quarantine".as_ptr(),
                q.as_ptr().cast(),
                q.len(),
                0,
                0,
            )
        } != 0
        {
            return Err("解压文件的隔离属性未能保留，未发布。".into());
        }
    }
    Ok(())
}
impl Stage {
    pub(super) fn create(parent: &AnchoredPath, id: &str) -> Result<Self, String> {
        let name = CString::new(format!(".MooMole-zip-extract-{id}.partial"))
            .map_err(|e| e.to_string())?;
        let parent = parent.target().try_clone().map_err(|e| e.to_string())?;
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
            return Err("解压临时目录无法排他创建。".into());
        }
        let root = open(&parent, name.to_str().map_err(|e| e.to_string())?, true)?;
        Ok(Self {
            parent,
            name,
            root,
            owned: BTreeMap::new(),
            published: false,
            cleaned: false,
        })
    }
    pub(super) fn validate(&self) -> Result<(), String> {
        let root = open(
            &self.parent,
            self.name.to_str().map_err(|e| e.to_string())?,
            true,
        )?;
        let m = root.metadata().map_err(|e| e.to_string())?;
        if identity(&root)? != identity(&self.root)?
            || m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o7777 != 0o700
        {
            return Err("解压临时目录身份或权限变化，未继续。".into());
        }
        Ok(())
    }
    fn parent_for(&self, path: &str) -> Result<(File, String), String> {
        self.validate()?;
        let parts = path.split('/').collect::<Vec<_>>();
        let mut parent = self.root.try_clone().map_err(|e| e.to_string())?;
        for i in 0..parts.len() - 1 {
            let next = open(&parent, parts[i], true)?;
            let key = parts[..=i].join("/");
            if self.owned.get(&key) != Some(&identity(&next)?) {
                return Err("解压子目录身份变化，未继续。".into());
            }
            parent = next;
        }
        Ok((parent, parts.last().ok_or("解压名称为空。")?.to_string()))
    }
    pub(super) fn extract(
        &mut self,
        input: &mut File,
        format: Format,
        entries: &[Entry],
        q: Option<&[u8]>,
        job: &Job,
        start: Instant,
    ) -> Result<(), String> {
        input.rewind().map_err(|e| e.to_string())?;
        let mut zip = if format == Format::Zip {
            Some(
                zip::ZipArchive::new(input.try_clone().map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        put_quarantine(&self.root, q)?;
        for e in entries {
            check(job, start)?;
            let (parent, name) = self.parent_for(&e.name)?;
            let c = CString::new(name).map_err(|e| e.to_string())?;
            let mut file = if e.directory {
                if unsafe { libc::mkdirat(parent.as_raw_fd(), c.as_ptr(), 0o700) } != 0 {
                    return Err("解压目录名称发生冲突，未覆盖。".into());
                }
                open(&parent, c.to_str().map_err(|e| e.to_string())?, true)?
            } else {
                let fd = unsafe {
                    libc::openat(
                        parent.as_raw_fd(),
                        c.as_ptr(),
                        libc::O_RDWR
                            | libc::O_CREAT
                            | libc::O_EXCL
                            | libc::O_NOFOLLOW
                            | libc::O_CLOEXEC,
                        0o600,
                    )
                };
                if fd < 0 {
                    return Err("解压文件名称发生冲突，未覆盖。".into());
                }
                unsafe { File::from_raw_fd(fd) }
            };
            self.owned.insert(e.name.clone(), identity(&file)?);
            put_quarantine(&file, q)?;
            if !e.directory {
                let mut content: Box<dyn Read + '_> = if let Some(z) = zip.as_mut() {
                    Box::new(
                        z.by_index(e.index.ok_or("解压条目没有内容索引。")?)
                            .map_err(|e| e.to_string())?,
                    )
                } else {
                    input
                        .seek(SeekFrom::Start(e.offset.ok_or("解压条目没有内容位置。")?))
                        .map_err(|e| e.to_string())?;
                    Box::new((&mut *input).take(e.size))
                };
                let mut hash = Sha256::new();
                let mut bytes = 0u64;
                let mut b = [0; 128 * 1024];
                loop {
                    check(job, start)?;
                    let n = content.read(&mut b).map_err(|e| e.to_string())?;
                    if n == 0 {
                        break;
                    }
                    bytes += n as u64;
                    if bytes > e.size {
                        return Err("压缩包解压内容超过核对大小。".into());
                    }
                    file.write_all(&b[..n]).map_err(|e| e.to_string())?;
                    hash.update(&b[..n]);
                }
                if bytes != e.size || format!("{:x}", hash.finalize()) != e.sha256 {
                    return Err("压缩包解压内容校验失败，未发布。".into());
                }
                file.sync_all().map_err(|e| e.to_string())?;
                if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } != 0 {
                    return Err("解压文件同步未完成，未发布。".into());
                }
            }
            parent.sync_all().map_err(|e| e.to_string())?;
        }
        self.root.sync_all().map_err(|e| e.to_string())?;
        Ok(())
    }
    pub(super) fn verify(
        &self,
        entries: &[Entry],
        q: Option<&[u8]>,
        job: &Job,
        start: Instant,
    ) -> Result<(), String> {
        self.validate()?;
        if quarantine(&self.root)?.as_deref() != q {
            return Err("解压目录隔离属性变化，未确认成功。".into());
        }
        let mut expected = HashMap::new();
        let mut counts = HashMap::<String, usize>::new();
        counts.insert(String::new(), 0);
        for e in entries {
            expected.insert(e.name.as_str(), e);
            let p = e.name.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            *counts.entry(p.into()).or_default() += 1;
            if e.directory {
                counts.entry(e.name.clone()).or_default();
            }
        }
        for e in entries {
            check(job, start)?;
            let (parent, name) = self.parent_for(&e.name)?;
            let mut f = open(&parent, &name, e.directory)?;
            let m = f.metadata().map_err(|e| e.to_string())?;
            if self.owned.get(&e.name) != Some(&identity(&f)?)
                || m.uid() != unsafe { libc::geteuid() }
                || m.mode() & 0o7777 != if e.directory { 0o700 } else { 0o600 }
                || !e.directory && (!m.is_file() || m.nlink() != 1 || m.len() != e.size)
                || quarantine(&f)?.as_deref() != q
            {
                return Err("解压内容身份、权限或隔离属性变化，未确认成功。".into());
            }
            if !e.directory && archive::digest(&mut f, job, start, e.size)? != e.sha256 {
                return Err("解压文件读回核对失败。".into());
            }
        }
        // Every directory must contain precisely the created names. Unexpected user
        // content is neither published as verified nor removed by our cleanup.
        for (directory, count) in counts {
            check(job, start)?;
            let file = if directory.is_empty() {
                self.root.try_clone().map_err(|e| e.to_string())?
            } else {
                let (p, n) = self.parent_for(&directory)?;
                open(&p, &n, true)?
            };
            let fd = unsafe {
                libc::openat(
                    file.as_raw_fd(),
                    c".".as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err("解压目录无法完整读回。".into());
            }
            let raw = unsafe { libc::fdopendir(fd) };
            if raw.is_null() {
                unsafe { libc::close(fd) };
                return Err("解压目录枚举无法开始。".into());
            }
            struct Stream(*mut libc::DIR);
            impl Drop for Stream {
                fn drop(&mut self) {
                    unsafe { libc::closedir(self.0) };
                }
            }
            let stream = Stream(raw);
            let mut seen = 0;
            loop {
                check(job, start)?;
                unsafe {
                    *libc::__error() = 0;
                }
                let item = unsafe { libc::readdir(stream.0) };
                if item.is_null() {
                    if unsafe { *libc::__error() } != 0 {
                        return Err("解压目录未完整读回。".into());
                    }
                    break;
                }
                let n = unsafe { CStr::from_ptr((*item).d_name.as_ptr()) }
                    .to_str()
                    .map_err(|_| "解压目录出现未知名称。")?;
                if n == "." || n == ".." {
                    continue;
                }
                seen += 1;
                let key = if directory.is_empty() {
                    n.into()
                } else {
                    format!("{directory}/{n}")
                };
                if seen > count || !expected.contains_key(key.as_str()) {
                    return Err("解压目录出现未核对内容，未确认成功。".into());
                }
            }
            if seen != count {
                return Err("解压目录项目不完整。".into());
            }
            file.sync_all().map_err(|e| e.to_string())?;
        }
        self.validate()
    }
    pub(super) fn publish(&mut self, name: &str) -> Result<(), String> {
        self.validate()?;
        let name = CString::new(name).map_err(|e| e.to_string())?;
        if unsafe {
            libc::renameatx_np(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                self.parent.as_raw_fd(),
                name.as_ptr(),
                libc::RENAME_EXCL,
            )
        } != 0
        {
            return Err("解压发布未确认；不会覆盖或重试。".into());
        }
        self.name = name;
        self.published = true;
        self.parent.sync_all().map_err(|e| e.to_string())?;
        self.validate()
    }
    pub(super) fn cleanup(&mut self) -> bool {
        if self.published || self.cleaned {
            return true;
        }
        if self.validate().is_err() {
            return false;
        }
        let mut okay = true;
        for (path, expected) in self.owned.iter().rev() {
            let attempt = (|| -> Result<(), String> {
                let (p, n) = self.parent_for(path)?;
                let f = open(&p, &n, expected.2)?;
                if identity(&f)? != *expected {
                    return Err("changed".into());
                }
                let c = CString::new(n).map_err(|e| e.to_string())?;
                if unsafe {
                    libc::unlinkat(
                        p.as_raw_fd(),
                        c.as_ptr(),
                        if expected.2 { libc::AT_REMOVEDIR } else { 0 },
                    )
                } != 0
                {
                    return Err("retained".into());
                }
                Ok(())
            })();
            okay &= attempt.is_ok();
        }
        if okay && self.validate().is_ok() {
            okay = unsafe {
                libc::unlinkat(
                    self.parent.as_raw_fd(),
                    self.name.as_ptr(),
                    libc::AT_REMOVEDIR,
                )
            } == 0;
        }
        self.cleaned = okay;
        okay
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        if !self.published && !self.cleaned {
            let _ = self.cleanup();
        }
    }
}
