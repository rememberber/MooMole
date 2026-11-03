use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Status {
    NotRegistered,
    Enabled,
    RequiresApproval,
    NotFound,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct System {
    pub bundle_id: String,
    pub bundle_path: String,
    pub status: Status,
}
impl System {
    pub(super) fn validate(&self, bundle: &str) -> Result<(), String> {
        let p = std::path::Path::new(&self.bundle_path);
        if self.bundle_id != bundle
            || self.bundle_id.len() > 255
            || !p.is_absolute()
            || self.bundle_path.len() > 4096
            || self.bundle_path.chars().any(char::is_control)
            || p.extension().is_none_or(|v| v != "app")
            || p.components().any(|c| {
                !matches!(
                    c,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                )
            })
        {
            return Err("loginInvalidResponse".into());
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Response {
    system: Option<System>,
    accepted: Option<bool>,
    error: Option<String>,
    native_code: Option<i64>,
}
pub(super) struct Reply {
    pub system: System,
    pub accepted: bool,
    pub error: Option<String>,
    pub native_code: Option<i64>,
}
#[derive(Clone, Copy)]
pub(super) enum Action {
    Read,
    Enable,
    Disable,
}
#[cfg(target_os = "macos")]
pub(super) fn call(bundle: &str, action: Action) -> Result<Reply, String> {
    use std::ffi::{CStr, CString};
    unsafe extern "C" {
        fn moomole_main_app_login(
            operation: i32,
            expected: *const libc::c_char,
        ) -> *mut libc::c_char;
    }
    let id = CString::new(bundle).map_err(|_| "loginInvalidRequest")?;
    let raw = unsafe {
        moomole_main_app_login(
            match action {
                Action::Read => 0,
                Action::Enable => 1,
                Action::Disable => 2,
            },
            id.as_ptr(),
        )
    };
    if raw.is_null() {
        return Err("loginNativeUnavailable".into());
    }
    let parsed = unsafe { serde_json::from_slice::<Response>(CStr::from_ptr(raw).to_bytes()) };
    unsafe { libc::free(raw.cast()) };
    let r = parsed.map_err(|_| "loginInvalidResponse")?;
    let system = r.system.ok_or_else(|| {
        match r.error.as_deref() {
            Some("requiresPackagedApp") => "loginRequiresPackagedApp",
            Some("unsupportedSystem") => "loginUnsupportedSystem",
            _ => "loginNativeUnavailable",
        }
        .to_string()
    })?;
    system.validate(bundle)?;
    if r.error.as_ref().is_some_and(|v| v != "requestRejected") {
        return Err("loginInvalidResponse".into());
    }
    Ok(Reply {
        system,
        accepted: r.accepted.ok_or("loginInvalidResponse")?,
        error: r.error.map(|_| "loginRequestRejected".into()),
        native_code: r.native_code,
    })
}
#[cfg(not(target_os = "macos"))]
pub(super) fn call(_: &str, _: Action) -> Result<Reply, String> {
    Err("loginUnsupportedSystem".into())
}
