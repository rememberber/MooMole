//! Read-only snapshots for exactly the fixed support scopes. Missing paths bind
//! their nearest existing ancestor. No symlink is traversed or hydrated.
use crate::observation::{AnchoredPath, Witness};
use std::{
    ffi::{CStr, OsStr},
    fs::{self, Metadata, OpenOptions},
    io::Read,
    os::{
        fd::IntoRawFd,
        macos::fs::MetadataExt as DarwinMetadataExt,
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MAX_ENTRIES: usize = 4096;
const MAX_FILES: usize = 512;
const MAX_FILE: u64 = 512 * 1024;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const BUDGET: Duration = Duration::from_secs(2);
enum Dependency {
    Present(PathBuf, Witness),
    Missing(PathBuf, PathBuf, Witness),
    Link(PathBuf, Stamp, PathBuf),
}
struct DirectoryStream(*mut libc::DIR);
impl Drop for DirectoryStream {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}
type Stamp = (u64, u64, u32, u32, u32, u64, u64, i64, i64, i64, i64, u32);
fn stamp(m: &Metadata) -> Stamp {
    (
        m.dev(),
        m.ino(),
        m.mode(),
        m.uid(),
        m.gid(),
        m.nlink(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
        m.st_flags(),
    )
}
pub(super) struct Scan {
    deps: Vec<Dependency>,
    started: Instant,
    entries: usize,
    files: usize,
    bytes: usize,
}
impl Scan {
    pub fn new() -> Self {
        Self {
            deps: vec![],
            started: Instant::now(),
            entries: 0,
            files: 0,
            bytes: 0,
        }
    }
    pub fn entries(&self) -> usize {
        self.entries
    }
    fn checkpoint(&self) -> Result<(), &'static str> {
        if self.entries > MAX_ENTRIES
            || self.files > MAX_FILES
            || self.bytes > MAX_BYTES
            || self.started.elapsed() >= BUDGET
        {
            Err("limitReached")
        } else {
            Ok(())
        }
    }
    fn open(path: &Path) -> Result<AnchoredPath, &'static str> {
        let held = AnchoredPath::open(path).map_err(|_| "unavailable")?;
        if !held.local_materialized() {
            return Err("notLocalOrMaterialized");
        }
        Ok(held)
    }
    fn capture(&mut self, path: &Path) -> Result<bool, &'static str> {
        self.checkpoint()?;
        self.entries += 1;
        self.checkpoint()?;
        match fs::symlink_metadata(path) {
            Ok(m) if m.is_symlink() => {
                let target = fs::read_link(path).map_err(|_| "unavailable")?;
                if target.as_os_str().len() > 4096 {
                    return Err("limitReached");
                }
                if stamp(&m) != stamp(&fs::symlink_metadata(path).map_err(|_| "changed")?) {
                    return Err("changed");
                }
                self.deps
                    .push(Dependency::Link(path.into(), stamp(&m), target));
                Ok(true)
            }
            Ok(m) if m.is_dir() || m.is_file() => {
                let held = Self::open(path)?;
                if stamp(&m) != stamp(&held.target().metadata().map_err(|_| "unavailable")?) {
                    return Err("changed");
                }
                self.deps
                    .push(Dependency::Present(path.into(), held.witness));
                Ok(true)
            }
            Ok(_) => Err("unsupportedEntry"),
            Err(e) if e.raw_os_error() == Some(libc::ENOENT) => {
                let mut ancestor = path.parent().ok_or("unavailable")?;
                loop {
                    match fs::symlink_metadata(ancestor) {
                        Ok(m) if m.is_dir() => break,
                        Ok(_) => return Err("unsupportedAncestor"),
                        Err(e) if e.raw_os_error() == Some(libc::ENOENT) => {
                            ancestor = ancestor.parent().ok_or("unavailable")?
                        }
                        Err(_) => return Err("unavailable"),
                    }
                }
                let held = Self::open(ancestor)?;
                if fs::symlink_metadata(path).is_ok() {
                    return Err("changed");
                }
                self.deps.push(Dependency::Missing(
                    path.into(),
                    ancestor.into(),
                    held.witness,
                ));
                Ok(false)
            }
            Err(_) => Err("unavailable"),
        }
    }
    pub fn exists(&mut self, path: &Path) -> Result<bool, &'static str> {
        self.capture(path)
    }
    pub fn is_directory(&mut self, path: &Path) -> Result<bool, &'static str> {
        if !self.capture(path)? {
            return Ok(false);
        }
        // Do not reopen the path for type classification after capture: a
        // temporary replacement must not hide an original directory subtree.
        Ok(
            matches!(self.deps.last(),Some(Dependency::Present(_,w)) if w.nodes.last().is_some_and(|n|n.mode & libc::S_IFMT as u32==libc::S_IFDIR as u32)),
        )
    }
    pub fn directory(&mut self, path: &Path) -> Result<Vec<PathBuf>, &'static str> {
        if !self.capture(path)? {
            return Ok(vec![]);
        }
        let held = Self::open(path)?;
        if !held
            .target()
            .metadata()
            .map_err(|_| "unavailable")?
            .is_dir()
        {
            return Err("unsupportedDirectory");
        }
        // Bind the exact handle used for listing as well as the initial probe.
        // fdopendir reads that held directory, not a second path lookup.
        self.deps
            .push(Dependency::Present(path.into(), held.witness.clone()));
        let result = self.names(&held, path)?;
        held.validate().map_err(|_| "changed")?;
        Ok(result)
    }
    fn names(&mut self, held: &AnchoredPath, path: &Path) -> Result<Vec<PathBuf>, &'static str> {
        let fd = held
            .target()
            .try_clone()
            .map_err(|_| "unavailable")?
            .into_raw_fd();
        let pointer = unsafe { libc::fdopendir(fd) };
        if pointer.is_null() {
            unsafe {
                libc::close(fd);
            }
            return Err("unavailable");
        }
        let stream = DirectoryStream(pointer);
        let mut result = vec![];
        loop {
            self.checkpoint()?;
            unsafe {
                *libc::__error() = 0;
            }
            let entry = unsafe { libc::readdir(stream.0) };
            if entry.is_null() {
                if unsafe { *libc::__error() } != 0 {
                    return Err("unavailable");
                }
                break;
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            self.entries += 1;
            self.checkpoint()?;
            result.push(path.join(OsStr::from_bytes(name)));
        }
        result.sort();
        Ok(result)
    }
    pub fn read(&mut self, path: &Path) -> Result<Vec<u8>, &'static str> {
        self.checkpoint()?;
        self.files += 1;
        self.checkpoint()?;
        let held = Self::open(path)?;
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW_ANY | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| "unavailable")?;
        let before = file.metadata().map_err(|_| "unavailable")?;
        if !before.is_file()
            || before.nlink() != 1
            || before.len() > MAX_FILE
            || before.st_flags() & 0x40000000 != 0
        {
            return Err("unsupportedFile");
        }
        if stamp(&before) != stamp(&held.target().metadata().map_err(|_| "unavailable")?) {
            return Err("changed");
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "unavailable")?;
        self.bytes += bytes.len();
        self.checkpoint()?;
        if bytes.len() as u64 != before.len()
            || stamp(&before) != stamp(&file.metadata().map_err(|_| "changed")?)
        {
            return Err("changed");
        }
        held.validate().map_err(|_| "changed")?;
        self.deps
            .push(Dependency::Present(path.into(), held.witness));
        Ok(bytes)
    }
    pub fn verify(&self) -> Result<(), &'static str> {
        // Revalidation has its own bounded work budget; it does not renew the
        // outer observation's original 5-second wall/monotonic deadline.
        let started = Instant::now();
        for dependency in &self.deps {
            if started.elapsed() >= BUDGET {
                return Err("limitReached");
            }
            match dependency {
                Dependency::Present(path, witness) => {
                    if Self::open(path)?.witness != *witness {
                        return Err("changed");
                    }
                }
                Dependency::Missing(path, ancestor, witness) => {
                    if Self::open(ancestor)?.witness != *witness
                        || !fs::symlink_metadata(path)
                            .is_err_and(|e| e.raw_os_error() == Some(libc::ENOENT))
                    {
                        return Err("changed");
                    }
                }
                Dependency::Link(path, before, target) => {
                    if stamp(&fs::symlink_metadata(path).map_err(|_| "changed")?) != *before
                        || fs::read_link(path).map_err(|_| "changed")? != *target
                    {
                        return Err("changed");
                    }
                    // The leaf is never followed; its parent must remain a
                    // strict local path, not become an alias to another tree.
                    Self::open(path.parent().ok_or("changed")?)?;
                }
            }
        }
        if started.elapsed() >= BUDGET {
            Err("limitReached")
        } else {
            Ok(())
        }
    }
    pub fn finish(&self) -> Result<(), &'static str> {
        self.checkpoint()?;
        self.verify()
    }
    pub fn append(&mut self, other: Self) {
        self.deps.extend(other.deps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn held_listing_does_not_read_a_temporarily_substituted_empty_directory() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().canonicalize().unwrap();
        let path = root.join("Caskroom");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("original"), "owned fixture").unwrap();
        let held = Scan::open(&path).unwrap();
        let mut scan = Scan::new();
        fs::rename(&path, root.join("saved")).unwrap();
        fs::create_dir(&path).unwrap();
        let names = scan.names(&held, &path).unwrap();
        assert_eq!(names, vec![path.join("original")]);
        assert!(held.validate().is_err());
        fs::remove_dir(&path).unwrap();
        fs::rename(root.join("saved"), &path).unwrap();
        // Even with the original name restored, the observed names came from
        // the held original object, never from the transient empty directory.
        assert!(held.validate().is_err());
    }
}
