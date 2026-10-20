//! Bounded, read-only observations of a held target and its immediate parent.
//! No access result grants mutation authority, evaluates TCC globally, probes
//! Trash, or promises that permissions remain unchanged after this observation.
use crate::observation::AnchoredPath;
use serde::Serialize;
use std::{
    ffi::c_void,
    fs::{File, Metadata},
    mem::MaybeUninit,
    os::{fd::AsRawFd, macos::fs::MetadataExt as _, unix::fs::MetadataExt as _},
    ptr::NonNull,
    time::{Duration, Instant},
};

pub const VERSION: u64 = 1;
const LOCK_FLAGS: u32 = libc::UF_IMMUTABLE | libc::SF_IMMUTABLE | libc::UF_APPEND | libc::SF_APPEND;

// macOS SDK sys/acl.h: opaque pointers, enum values and return convention.
// libc's Rust bindings do not expose these Darwin ACL functions.
extern "C" {
    fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut c_void;
    fn acl_get_entry(
        acl: *mut c_void,
        entry_id: libc::c_int,
        entry: *mut *mut c_void,
    ) -> libc::c_int;
    fn acl_free(acl: *mut c_void) -> libc::c_int;
}

struct Acl(NonNull<c_void>);
impl Drop for Acl {
    fn drop(&mut self) {
        // SAFETY: this pointer is owned by a successful acl_get_fd_np call.
        unsafe {
            acl_free(self.0.as_ptr());
        }
    }
}

fn has_acl(file: &File) -> Result<bool, i32> {
    // ACL_TYPE_EXTENDED = 0x100; the held descriptor outlives both calls.
    let Some(pointer) = NonNull::new(unsafe { acl_get_fd_np(file.as_raw_fd(), 0x100) }) else {
        // Darwin's fd getter returns NULL/ENOENT when FILESEC_ACL is absent
        // (filesec_get_property). The caller holds and revalidates the inode;
        // other errors, including unsupported filesystems, remain unknown.
        let error = errno();
        return if error == libc::ENOENT {
            Ok(false)
        } else {
            Err(error)
        };
    };
    let acl = Acl(pointer);
    let mut entry = std::ptr::null_mut();
    // Darwin: 0 obtains an entry, -1/EINVAL means no first entry on this valid
    // newly allocated ACL. Do not use Linux's different return convention.
    let result = unsafe { acl_get_entry(acl.0.as_ptr(), 0, &mut entry) };
    match (result, entry.is_null()) {
        (0, false) => Ok(true),
        (-1, _) if errno() == libc::EINVAL => Ok(false),
        _ => Err(errno()),
    }
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn read_only(file: &File) -> Result<bool, i32> {
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: fstatfs initializes the provided struct only on success.
    if unsafe { libc::fstatfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(errno());
    }
    Ok(unsafe { stat.assume_init() }.f_flags & libc::MNT_RDONLY as u32 != 0)
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Code {
    NoRestrictionObserved,
    Restricted,
    Unknown,
    NotChecked,
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    Clear,
    Restricted,
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    kind: &'static str,
    state: State,
    reason: String,
    os_error: Option<i32>,
}
impl Finding {
    fn new(kind: &'static str, state: State, reason: &str) -> Self {
        Self {
            kind,
            state,
            reason: reason.into(),
            os_error: None,
        }
    }
    fn unknown(kind: &'static str, error: i32, reason: &str) -> Self {
        Self {
            os_error: Some(error),
            ..Self::new(kind, State::Unknown, reason)
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub version: u64,
    pub code: Code,
    pub reason: String,
    pub findings: Vec<Finding>,
}
impl Check {
    pub fn not_checked(reason: &str) -> Self {
        Self {
            version: VERSION,
            code: Code::NotChecked,
            reason: reason.into(),
            findings: vec![],
        }
    }
    pub(crate) fn refusal_reason(&self) -> String {
        let reasons = self
            .findings
            .iter()
            .filter(|finding| finding.state != State::Clear)
            .take(3)
            .map(|finding| finding.reason.as_str())
            .collect::<Vec<_>>();
        format!(
            "最终权限复查未通过：{} 请返回审阅计划重新核对权限；不会自动解除限制。",
            if reasons.is_empty() {
                self.reason.clone()
            } else {
                reasons.join(" ")
            }
        )
    }
    fn classify(findings: Vec<Finding>) -> Self {
        let (code, reason) = if findings.iter().any(|f| f.state == State::Restricted) {
            (
                Code::Restricted,
                "本次发现文件系统访问限制，请展开查看原因；不会自动提权或修改权限。",
            )
        } else if findings.is_empty() || findings.iter().any(|f| f.state == State::Unknown) {
            (
                Code::Unknown,
                "部分权限条件无法可靠确认，不能视为具备处理权限。",
            )
        } else {
            (
                Code::NoRestrictionObserved,
                "本次对目标及父目录的有限检查未发现限制；不代表已获准清理或移入废纸篓。",
            )
        };
        Self {
            version: VERSION,
            code,
            reason: reason.into(),
            findings,
        }
    }
}

pub struct Budget(Instant);
impl Budget {
    pub fn new() -> Self {
        Self(Instant::now())
    }
}

fn volume(target: Result<bool, i32>, parent: Result<bool, i32>) -> Finding {
    match (target, parent) {
        (Ok(true), _) | (_, Ok(true)) => Finding::new(
            "volume",
            State::Restricted,
            "目标或父目录所在卷为只读，不能按普通方式移动；不会更改挂载状态。",
        ),
        (Ok(false), Ok(false)) => Finding::new(
            "volume",
            State::Clear,
            "本次未发现目标及父目录所在卷被标记为只读。",
        ),
        (Err(error), _) | (_, Err(error)) => {
            Finding::unknown("volume", error, "无法读取卷状态；不能据此假定卷可写。")
        }
    }
}

fn sticky_restricted(user: u32, target: u32, parent: u32, mode: u32) -> bool {
    mode & libc::S_ISVTX as u32 != 0 && user != 0 && user != target && user != parent
}

fn changed(a: &Metadata, b: &Metadata) -> bool {
    (
        a.dev(),
        a.ino(),
        a.mode(),
        a.uid(),
        a.gid(),
        a.st_flags(),
        a.ctime(),
        a.ctime_nsec(),
    ) != (
        b.dev(),
        b.ino(),
        b.mode(),
        b.uid(),
        b.gid(),
        b.st_flags(),
        b.ctime(),
        b.ctime_nsec(),
    )
}

pub fn observe(anchor: &AnchoredPath, budget: &Budget) -> Result<Check, String> {
    observe_with(anchor, budget, || {})
}

fn observe_with(
    anchor: &AnchoredPath,
    budget: &Budget,
    hook: impl FnOnce(),
) -> Result<Check, String> {
    if budget.0.elapsed() >= Duration::from_secs(5) {
        return Ok(Check::not_checked(
            "本批权限观察的 5 秒预算已用完，请缩小选择后重新复核。",
        ));
    }
    let target = anchor.target().metadata().map_err(|e| e.to_string())?;
    let parent = anchor.parent().metadata().map_err(|e| e.to_string())?;
    let volumes = (read_only(anchor.target()), read_only(anchor.parent()));
    let mut findings = vec![volume(volumes.0, volumes.1)];
    // Checking the held parent itself avoids following another absolute path.
    // AT_EACCESS asks for the current effective identity, not file mode guesses.
    let access = unsafe {
        libc::faccessat(
            anchor.parent().as_raw_fd(),
            c".".as_ptr(),
            libc::W_OK | libc::X_OK,
            libc::AT_EACCESS,
        )
    };
    let access = if access == 0 {
        Finding::new(
            "parentAccess",
            State::Clear,
            "当前有效账户通过父目录的写入与搜索权限检查。",
        )
    } else {
        let error = errno();
        if [libc::EACCES, libc::EPERM, libc::EROFS].contains(&error) {
            Finding {
                os_error: Some(error),
                ..Finding::new(
                    "parentAccess",
                    State::Restricted,
                    "系统拒绝父目录写入或搜索访问；仅凭该错误无法区分普通权限与隐私授权限制。",
                )
            }
        } else {
            Finding::unknown(
                "parentAccess",
                error,
                "父目录访问检查未可靠完成，不能假定可以移动其中的项目。",
            )
        }
    };
    findings.push(access);
    findings.push(
        if (target.st_flags() | parent.st_flags()) & LOCK_FLAGS != 0 {
            Finding::new(
                "fileFlags",
                State::Restricted,
                "目标或父目录带有不可更改/只追加属性；不会自动解锁或修改文件属性。",
            )
        } else {
            Finding::new(
                "fileFlags",
                State::Clear,
                "本次未发现目标及父目录的不可更改/只追加属性。",
            )
        },
    );
    findings.push(
        if sticky_restricted(
            unsafe { libc::geteuid() },
            target.uid(),
            parent.uid(),
            parent.mode(),
        ) {
            Finding::new(
                "stickyDirectory",
                State::Restricted,
                "父目录有粘滞保护，当前账户既不是文件所有者，也不是目录所有者。",
            )
        } else {
            Finding::new(
                "stickyDirectory",
                State::Clear,
                "本次未发现父目录粘滞保护对当前账户的所有权限制。",
            )
        },
    );
    findings.push(match (has_acl(anchor.target()), has_acl(anchor.parent())) {
        (Ok(true), _) | (_, Ok(true)) => Finding::new(
            "extendedAcl",
            State::Unknown,
            "目标或父目录存在扩展访问控制（ACL）；未计算删除/移入权限，需单独核对。",
        ),
        (Ok(false), Ok(false)) => Finding::new(
            "extendedAcl",
            State::Clear,
            "本次未发现目标及父目录有扩展访问控制条目。",
        ),
        (Err(error), _) | (_, Err(error)) => Finding::unknown(
            "extendedAcl",
            error,
            "扩展访问控制无法读取，不能按普通权限位判断处理权限。",
        ),
    });
    findings.push(if target.dev() != parent.dev() {
        Finding::new(
            "mountBoundary",
            State::Unknown,
            "目标位于挂载边界；请单独选择卷内具体项目，不能据此判断跨卷处理权限。",
        )
    } else {
        Finding::new(
            "mountBoundary",
            State::Clear,
            "本次目标与直接父目录位于同一设备。",
        )
    });
    hook();
    let target_after = anchor.target().metadata().map_err(|e| e.to_string())?;
    let parent_after = anchor.parent().metadata().map_err(|e| e.to_string())?;
    if changed(&target, &target_after)
        || changed(&parent, &parent_after)
        || volumes != (read_only(anchor.target()), read_only(anchor.parent()))
    {
        return Err("权限观察期间目标、父目录属性或卷状态已变化，请重新复核。".into());
    }
    anchor.validate()?;
    Ok(Check::classify(findings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::CString, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};

    struct Fixture {
        _root: tempfile::TempDir,
        parent: PathBuf,
        file: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().canonicalize().unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            let parent = path.join("parent");
            std::fs::create_dir(&parent).unwrap();
            let file = parent.join("日志 空格\n[*] $(literal)");
            std::fs::write(&file, "private fixture").unwrap();
            Self {
                _root: root,
                parent,
                file,
            }
        }
        fn check(&self) -> Check {
            observe(&AnchoredPath::open(&self.file).unwrap(), &Budget::new()).unwrap()
        }
    }

    #[test]
    fn readonly_file_mode_does_not_imply_its_parent_cannot_move_it() {
        let f = Fixture::new();
        std::fs::set_permissions(&f.file, std::fs::Permissions::from_mode(0o444)).unwrap();
        let before = crate::observation::witness(&f.file).unwrap();
        let result = f.check();
        assert_eq!(result.code, Code::NoRestrictionObserved, "{result:?}");
        assert_eq!(result.findings.len(), 6);
        assert_eq!(crate::observation::witness(&f.file).unwrap(), before);
        assert_eq!(std::fs::read_to_string(&f.file).unwrap(), "private fixture");
        let directory = observe(&AnchoredPath::open(&f.parent).unwrap(), &Budget::new()).unwrap();
        assert_eq!(directory.code, Code::NoRestrictionObserved, "{directory:?}");
    }

    #[test]
    fn parent_access_checks_effective_identity_instead_of_target_write_bits() {
        let f = Fixture::new();
        std::fs::set_permissions(&f.file, std::fs::Permissions::from_mode(0o666)).unwrap();
        std::fs::set_permissions(&f.parent, std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = f.check();
        std::fs::set_permissions(&f.parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        let finding = result
            .findings
            .iter()
            .find(|v| v.kind == "parentAccess")
            .unwrap();
        if unsafe { libc::geteuid() } != 0 {
            assert_eq!(finding.state, State::Restricted, "{result:?}");
            assert_eq!(result.code, Code::Restricted);
        } else {
            assert_eq!(
                finding.state,
                State::Clear,
                "root has effective access to this fixture"
            );
        }
        assert_eq!(std::fs::read_to_string(&f.file).unwrap(), "private fixture");
    }

    struct Flags {
        path: CString,
        original: u32,
    }
    impl Flags {
        fn set(path: &std::path::Path, flag: u32) -> Self {
            let original = std::fs::symlink_metadata(path).unwrap().st_flags();
            let path = CString::new(path.to_str().unwrap()).unwrap();
            assert_eq!(unsafe { libc::chflags(path.as_ptr(), original | flag) }, 0);
            Self { path, original }
        }
    }
    impl Drop for Flags {
        fn drop(&mut self) {
            assert_eq!(
                unsafe { libc::chflags(self.path.as_ptr(), self.original) },
                0,
                "restore only this fixture's flags"
            );
        }
    }

    #[test]
    fn target_and_parent_immutable_or_append_flags_are_reported_without_unlocking() {
        for parent in [false, true] {
            for flag in [libc::UF_IMMUTABLE, libc::UF_APPEND] {
                let f = Fixture::new();
                let path = if parent { &f.parent } else { &f.file };
                let guard = Flags::set(path, flag);
                let result = f.check();
                let still_set = std::fs::symlink_metadata(path).unwrap().st_flags() & flag != 0;
                drop(guard); // Always restore our own fixture before TempDir cleanup.
                assert!(still_set, "observation must not remove flags");
                assert_eq!(result.code, Code::Restricted, "{result:?}");
                assert_eq!(
                    result
                        .findings
                        .iter()
                        .find(|v| v.kind == "fileFlags")
                        .unwrap()
                        .state,
                    State::Restricted
                );
                assert_eq!(std::fs::read_to_string(&f.file).unwrap(), "private fixture");
            }
        }
    }

    struct AclFixture(PathBuf);
    impl Drop for AclFixture {
        fn drop(&mut self) {
            assert!(
                Command::new("/bin/chmod")
                    .arg("-N")
                    .arg(&self.0)
                    .status()
                    .unwrap()
                    .success(),
                "restore only this fixture's ACL"
            );
        }
    }

    #[test]
    fn extended_acls_are_observed_but_never_treated_as_delete_authorization() {
        for parent in [false, true] {
            let f = Fixture::new();
            let path = if parent { &f.parent } else { &f.file };
            assert!(Command::new("/bin/chmod")
                .args([
                    "+a",
                    if parent {
                        "everyone deny delete_child"
                    } else {
                        "everyone deny delete"
                    }
                ])
                .arg(path)
                .status()
                .unwrap()
                .success());
            let guard = AclFixture(path.clone());
            let result = f.check();
            let anchor = AnchoredPath::open(&f.file).unwrap();
            let still_present = has_acl(if parent {
                anchor.parent()
            } else {
                anchor.target()
            });
            drop(guard);
            assert_eq!(still_present, Ok(true));
            assert_ne!(result.code, Code::NoRestrictionObserved);
            assert_eq!(
                result
                    .findings
                    .iter()
                    .find(|v| v.kind == "extendedAcl")
                    .unwrap()
                    .state,
                State::Unknown
            );
            assert_eq!(std::fs::read_to_string(&f.file).unwrap(), "private fixture");
        }
    }

    #[test]
    fn changed_paths_or_parent_permissions_discard_the_observation() {
        for parent in [false, true] {
            let f = Fixture::new();
            let anchor = AnchoredPath::open(&f.file).unwrap();
            let result = observe_with(&anchor, &Budget::new(), || {
                if parent {
                    std::fs::set_permissions(&f.parent, std::fs::Permissions::from_mode(0o500))
                        .unwrap();
                } else {
                    std::fs::rename(&f.file, f.parent.join("original")).unwrap();
                    std::fs::write(&f.file, "replacement").unwrap();
                }
            });
            std::fs::set_permissions(&f.parent, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert!(result.is_err());
        }
    }

    #[test]
    fn unknowns_limits_readonly_volumes_and_sticky_ownership_never_become_permission_grants() {
        assert_eq!(
            Check::classify(vec![volume(Ok(true), Err(libc::EIO))]).code,
            Code::Restricted
        );
        assert_eq!(
            Check::classify(vec![volume(Err(libc::EIO), Ok(false))]).code,
            Code::Unknown
        );
        assert_eq!(Check::classify(vec![]).code, Code::Unknown);
        assert!(sticky_restricted(501, 502, 503, 0o1777));
        assert!(!sticky_restricted(501, 501, 503, 0o1777));
        assert!(!sticky_restricted(501, 502, 501, 0o1777));
        assert!(!sticky_restricted(0, 502, 503, 0o1777));
        assert!(!sticky_restricted(501, 502, 503, 0o777));
        let f = Fixture::new();
        let exhausted = Budget(Instant::now() - Duration::from_secs(6));
        let check = observe(&AnchoredPath::open(&f.file).unwrap(), &exhausted).unwrap();
        assert_eq!(check.code, Code::NotChecked);
        assert!(check.findings.is_empty());
    }
}
