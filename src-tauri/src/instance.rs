//! Cooperative ownership of one application's data directory. The kernel lock
//! precedes SQLite open/migration/recovery and lives as long as the connection.
//! This is not a lock on cleanup targets or protection against non-cooperating
//! processes. Never unlink a lock file: that would create two lock namespaces.
use std::{
    fs::{File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
};

pub const LOCK_NAME: &str = ".moomole-instance.lock";

#[derive(Debug)]
pub enum Error {
    Busy,
    Unavailable(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => f.write_str("另一个 MooMole 实例正在使用本机数据。请切换到已打开的应用；如需重启，请先正常退出它。"),
            Self::Unavailable(reason) => write!(f, "无法确认本机数据的独占使用权：{reason}"),
        }
    }
}

#[derive(Debug)]
pub struct Lease {
    // Closing the fd releases flock, including on process termination. No PID,
    // timestamp, heartbeat or file removal is used to guess whether it is free.
    _file: File,
}

impl Drop for Lease {
    fn drop(&mut self) {
        // flock's open-file description can briefly be shared by a concurrent
        // fork before CLOEXEC closes the child's copy. Explicitly unlock when
        // the final owning Database drops; never leave release to that child.
        unsafe {
            libc::flock(self._file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn unavailable(error: impl std::fmt::Display) -> Error {
    Error::Unavailable(error.to_string())
}

pub fn acquire(directory: &Path) -> Result<Lease, Error> {
    let path = directory.join(LOCK_NAME);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(&path)
        .map_err(unavailable)?;
    let metadata = file.metadata().map_err(unavailable)?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(unavailable(
            "占用文件不是当前用户的私有普通文件，请检查应用数据目录。",
        ));
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = std::io::Error::last_os_error();
        return Err(if error.kind() == std::io::ErrorKind::WouldBlock {
            Error::Busy
        } else {
            unavailable(error)
        });
    }
    // An accidentally replaced lock name must not be accepted as the namespace
    // we own. This check does not defend later hostile directory replacement.
    let named = std::fs::symlink_metadata(&path).map_err(unavailable)?;
    let held = file.metadata().map_err(unavailable)?;
    if !named.is_file()
        || named.dev() != held.dev()
        || named.ino() != held.ino()
        || held.nlink() != 1
        || held.uid() != unsafe { libc::geteuid() }
        || held.mode() & 0o077 != 0
    {
        return Err(unavailable(
            "占用文件在打开期间发生变化，请重新启动后检查。",
        ));
    }
    Ok(Lease { _file: file })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        os::unix::fs::{symlink, PermissionsExt},
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    #[test]
    fn persistent_private_file_is_reused_and_contents_are_not_ownership_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let owner = acquire(directory.path()).unwrap();
        let path = directory.path().join(LOCK_NAME);
        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_ne!(
            unsafe { libc::fcntl(owner._file.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        std::fs::write(&path, "not a live PID, far future timestamp, stale marker").unwrap();
        assert!(matches!(acquire(directory.path()), Err(Error::Busy)));
        let alias = directory.path().join("directory-alias");
        symlink(directory.path(), &alias).unwrap();
        assert!(matches!(acquire(&alias), Err(Error::Busy)));
        drop(owner);
        let next = acquire(directory.path()).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), metadata.ino());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "not a live PID, far future timestamp, stale marker"
        );
        drop(next);
        assert!(
            path.exists(),
            "closing ownership must NEVER unlink the lock file"
        );
    }

    #[test]
    fn releasing_owner_unlocks_even_while_a_pre_exec_descriptor_copy_exists() {
        let directory = tempfile::tempdir().unwrap();
        let owner = acquire(directory.path()).unwrap();
        // dup shares the open-file description just like the brief fork/exec
        // window. CLOEXEC alone cannot release a copy that has not exec'd yet.
        let inherited = owner._file.try_clone().unwrap();
        drop(owner);
        let next = acquire(directory.path()).unwrap();
        drop(inherited);
        assert!(
            matches!(acquire(directory.path()), Err(Error::Busy)),
            "closing an old duplicate must not release the new owner's lock"
        );
        drop(next);
    }

    #[test]
    fn unsafe_links_permissions_and_special_files_are_refused_without_rewriting_them() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(LOCK_NAME);
        let target = directory.path().join("unchanged");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&target)
            .unwrap()
            .write_all(b"keep fixture")
            .unwrap();
        symlink(&target, &path).unwrap();
        assert!(matches!(
            acquire(directory.path()),
            Err(Error::Unavailable(_))
        ));
        std::fs::remove_file(&path).unwrap();
        std::fs::hard_link(&target, &path).unwrap();
        assert!(matches!(
            acquire(directory.path()),
            Err(Error::Unavailable(_))
        ));
        std::fs::remove_file(&path).unwrap();
        std::fs::copy(&target, &path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            acquire(directory.path()),
            Err(Error::Unavailable(_))
        ));
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o644);
        std::fs::remove_file(&path).unwrap();
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(matches!(
            acquire(directory.path()),
            Err(Error::Unavailable(_))
        ));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep fixture");
    }

    struct OwnedChild(Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn child(directory: &Path) -> OwnedChild {
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "instance::tests::child_owner", "--nocapture"])
            .env("MOOMOLE_INSTANCE_CHILD_FIXTURE", directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut child = OwnedChild(child);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !directory.join("ready").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "fixture child exited before owning data"
            );
            assert!(
                Instant::now() < deadline,
                "fixture child did not become ready"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        child
    }

    #[test]
    fn child_owner() {
        let Some(directory) = std::env::var_os("MOOMOLE_INSTANCE_CHILD_FIXTURE") else {
            return;
        };
        let directory = Path::new(&directory);
        let db = crate::db::open_owned(&directory.join("app.sqlite")).unwrap();
        crate::db::start(&db, "live-child", "clean", None).unwrap();
        std::fs::write(directory.join("ready"), "owned").unwrap();
        // A parent crash closes this pipe too, so this fixture cannot linger.
        let mut byte = [0];
        let _ = std::io::stdin().read(&mut byte);
        drop(db);
    }

    #[test]
    fn independent_processes_refuse_live_owners_and_recover_only_after_exit_or_kill() {
        for killed in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let mut owner = child(directory.path());
            let path = directory.path().join("app.sqlite");
            assert!(matches!(acquire(directory.path()), Err(Error::Busy)));
            assert!(matches!(
                crate::db::open_owned(&path),
                Err(crate::db::OpenError::Instance(Error::Busy))
            ));
            let read = rusqlite::Connection::open_with_flags(
                &path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            assert_eq!(crate::db::history(&read).unwrap()[0].state, "running");
            if killed {
                owner.0.kill().unwrap();
            } else {
                owner.0.stdin.take().unwrap().write_all(b"x").unwrap();
            }
            let status = owner.0.wait().unwrap();
            assert_eq!(status.success(), !killed);
            let recovered = crate::db::open_owned(&path).unwrap();
            assert_eq!(
                crate::db::history(&recovered).unwrap()[0].state,
                "interrupted"
            );
            assert!(directory.path().join(LOCK_NAME).exists());
        }
    }

    #[test]
    fn exec_children_do_not_inherit_directory_ownership() {
        let directory = tempfile::tempdir().unwrap();
        let owner = acquire(directory.path()).unwrap();
        let mut child = OwnedChild(
            Command::new("/bin/cat")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        assert!(child.0.try_wait().unwrap().is_none());
        drop(owner);
        let next = acquire(directory.path())
            .expect("an unrelated exec child must not retain the owner fd");
        assert!(child.0.try_wait().unwrap().is_none());
        drop(next);
    }
}
