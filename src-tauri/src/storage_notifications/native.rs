use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Permission {
    pub authorization: String,
    pub alerts: bool,
    pub center: bool,
}
impl Permission {
    pub(super) fn allowed(&self) -> bool {
        ["authorized", "provisional"].contains(&self.authorization.as_str())
            && (self.alerts || self.center)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Response {
    authorization: Option<String>,
    alerts: Option<bool>,
    center: Option<bool>,
    submitted: Option<bool>,
    error: Option<String>,
}
#[derive(Clone, Copy)]
pub(super) enum Action {
    Read,
    Request,
    Send,
}
#[cfg(target_os = "macos")]
fn raw(
    action: Action,
    bundle: &str,
    identifier: &str,
    title: &str,
    body: &str,
) -> Result<Response, String> {
    use std::ffi::{CStr, CString};
    unsafe extern "C" {
        fn moomole_storage_notification(
            operation: *const libc::c_char,
            bundle: *const libc::c_char,
            identifier: *const libc::c_char,
            title: *const libc::c_char,
            body: *const libc::c_char,
        ) -> *mut libc::c_char;
    }
    let args = [
        match action {
            Action::Read => "read",
            Action::Request => "request",
            Action::Send => "send",
        },
        bundle,
        identifier,
        title,
        body,
    ]
    .map(CString::new);
    let args = args
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "invalidRequest")?;
    let p = unsafe {
        moomole_storage_notification(
            args[0].as_ptr(),
            args[1].as_ptr(),
            args[2].as_ptr(),
            args[3].as_ptr(),
            args[4].as_ptr(),
        )
    };
    if p.is_null() {
        return Err("nativeUnavailable".into());
    }
    let value = unsafe { serde_json::from_slice::<Response>(CStr::from_ptr(p).to_bytes()) };
    unsafe { libc::free(p.cast()) };
    let r = value.map_err(|_| "nativeUnavailable")?;
    if let Some(e) = r.error.as_ref() {
        return Err(e.clone());
    }
    Ok(r)
}
#[cfg(not(target_os = "macos"))]
fn raw(_: Action, _: &str, _: &str, _: &str, _: &str) -> Result<Response, String> {
    Err("requiresPackagedApp".into())
}
pub(super) fn permission(bundle: &str, request: bool) -> Result<Permission, String> {
    let r = raw(
        if request {
            Action::Request
        } else {
            Action::Read
        },
        bundle,
        "",
        "",
        "",
    )?;
    let p = Permission {
        authorization: r.authorization.ok_or("nativeUnavailable")?,
        alerts: r.alerts.ok_or("nativeUnavailable")?,
        center: r.center.ok_or("nativeUnavailable")?,
    };
    if ![
        "notDetermined",
        "denied",
        "authorized",
        "provisional",
        "unknown",
    ]
    .contains(&p.authorization.as_str())
    {
        return Err("nativeUnavailable".into());
    }
    Ok(p)
}
pub(super) fn send(bundle: &str, id: &str, title: &str, body: &str) -> Result<(), String> {
    let r = raw(Action::Send, bundle, id, title, body)?;
    if r.submitted == Some(true) {
        Ok(())
    } else {
        Err("submissionUnknown".into())
    }
}
