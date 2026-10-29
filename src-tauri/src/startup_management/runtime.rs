//! Optional deprecated SM query, data only. No terminal output is interpreted.
use super::*;
use objc2::{
    msg_send,
    rc::{autoreleasepool, Retained},
    runtime::AnyClass,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString};
use std::{ffi::c_void, sync::OnceLock};
type CopyJob = unsafe extern "C" fn(*const c_void, *const c_void) -> *mut NSDictionary;
struct Api {
    copy: CopyJob,
    domain: usize,
}
fn framework() -> Result<usize, String> {
    static HANDLE: OnceLock<usize> = OnceLock::new();
    let handle = *HANDLE.get_or_init(|| unsafe {
        libc::dlopen(
            c"/System/Library/Frameworks/ServiceManagement.framework/ServiceManagement".as_ptr(),
            libc::RTLD_LAZY | libc::RTLD_LOCAL,
        ) as usize
    });
    if handle == 0 {
        Err("系统服务管理框架不可用。".into())
    } else {
        Ok(handle)
    }
}
fn api() -> Result<&'static Api, String> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        // Retain one known system framework handle for process lifetime.
        let handle = framework().ok()? as *mut c_void;
        let function = libc::dlsym(handle, c"SMJobCopyDictionary".as_ptr());
        let domain = libc::dlsym(handle, c"kSMDomainUserLaunchd".as_ptr());
        if function.is_null() || domain.is_null() {
            return None;
        }
        let domain = *(domain as *const *const c_void);
        if domain.is_null() {
            return None;
        }
        Some(Api {
            copy: std::mem::transmute::<*mut c_void, CopyJob>(function),
            domain: domain as usize,
        })
    })
    .as_ref()
    .ok_or_else(|| "当前系统不再提供兼容的后台状态接口，请在系统登录项设置中管理。".into())
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Service {
    program: String,
    arguments: Vec<String>,
    pid: Option<i32>,
}
pub(super) fn query(label: &str) -> Result<Option<Service>, String> {
    let api = api()?;
    autoreleasepool(|_| {
        let s = NSString::from_str(label);
        let pointer =
            unsafe { (api.copy)(api.domain as *const c_void, (&*s as *const NSString).cast()) };
        let Some(dict) = (unsafe { Retained::from_raw(pointer) }) else {
            return Ok(None);
        };
        let observed = plist::text(&*plist::get(&dict, "Label").ok_or("后台状态缺少标签。")?)?;
        if observed != label {
            return Err("后台标签无法核对。".into());
        }
        let program = plist::text(&*plist::get(&dict, "Program").ok_or("后台状态缺少程序位置。")?)?;
        let arguments =
            plist::args(&*plist::get(&dict, "ProgramArguments").ok_or("后台参数结构不受支持。")?)?;
        let pid = plist::get(&dict, "PID")
            .and_then(|v| v.downcast_ref::<NSNumber>().map(|n| n.intValue()))
            .filter(|n| *n > 0);
        Ok(Some(Service {
            program,
            arguments,
            pid,
        }))
    })
}
pub(super) fn matches(service: &Option<Service>, definition: &Definition) -> Result<(), String> {
    if service
        .as_ref()
        .is_some_and(|s| s.program != definition.program || s.arguments != definition.arguments)
    {
        Err("同标签的后台程序或参数与配置不同，保持原样。".into())
    } else {
        Ok(())
    }
}
pub(super) fn request(
    action: Action,
    label: &str,
    path: &Path,
    job: &Job,
) -> Result<crate::maintenance::quicklook::Attempt<()>, String> {
    let tool = AnchoredPath::open(Path::new("/bin/launchctl"))?;
    let mut fs = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    if unsafe { libc::fstatfs(tool.target().as_raw_fd(), fs.as_mut_ptr()) } != 0
        || unsafe { fs.assume_init() }.f_flags & libc::MNT_RDONLY as u32 == 0
    {
        return Err("系统后台管理工具身份无法核对。".into());
    }
    let mut command = std::process::Command::new("/bin/launchctl");
    let domain = format!("gui/{}", unsafe { libc::geteuid() });
    match action {
        Action::Pause => {
            command.args(["bootout", &format!("{domain}/{label}")]);
        }
        Action::Resume => {
            command.args(["bootstrap", &domain]).arg(path);
        }
    }
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    tool.validate()?;
    Ok(crate::maintenance::quicklook::supervise(
        (),
        command,
        job,
        Duration::from_secs(15),
        Duration::from_secs(1),
        |_| {},
    ))
}
pub(super) fn open_settings() -> Result<(), String> {
    let _ = framework()?;
    let class = AnyClass::get(c"SMAppService").ok_or("系统登录项设置入口不可用。")?;
    let supported: bool =
        unsafe { msg_send![class,respondsToSelector:objc2::sel!(openSystemSettingsLoginItems)] };
    if !supported {
        return Err("系统登录项设置入口不可用。".into());
    }
    unsafe {
        let _: () = msg_send![class, openSystemSettingsLoginItems];
    }
    Ok(())
}
