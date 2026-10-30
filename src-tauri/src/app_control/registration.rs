//! Register one exact retained App with the public Launch Services API. Never
//! resets the database, changes explicit defaults, runs the App or kills Finder.
use super::*;
use core_foundation_sys::{
    base::{kCFAllocatorDefault, CFRelease},
    url::{CFURLCreateFromFileSystemRepresentation, CFURLRef},
};
#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn LSRegisterURL(url: CFURLRef, update: u8) -> i32;
}
fn register(path: &Path) -> Result<i32, String> {
    let bytes = path.as_os_str().as_encoded_bytes();
    let url = unsafe {
        CFURLCreateFromFileSystemRepresentation(
            kCFAllocatorDefault,
            bytes.as_ptr(),
            bytes.len() as isize,
            1,
        )
    };
    if url.is_null() {
        return Err("系统无法为所选应用建立注册地址。".into());
    }
    let status = unsafe { LSRegisterURL(url, 1) };
    unsafe { CFRelease(url.cast()) };
    Ok(status)
}
fn observed(path: &Path, id: &str) -> Result<bool, String> {
    let urls = NSWorkspace::sharedWorkspace()
        .URLsForApplicationsWithBundleIdentifier(&NSString::from_str(id));
    if urls.count() > 64 {
        return Err("同标识注册副本过多，无法完整核对。".into());
    }
    let mut exact = false;
    for url in urls.iter() {
        if !url.isFileURL() {
            return Err("系统注册列表包含无法核对的位置。".into());
        }
        let at = url
            .path()
            .map(|s| PathBuf::from(s.to_string()))
            .ok_or("系统注册副本缺少路径。")?;
        // Compare the exact physical selection, not the first/default copy.
        if at == path {
            exact = true;
        }
    }
    Ok(exact)
}
pub(super) fn perform(
    plan: &Plan,
    ctx: impl Fn() -> Result<Context, String>,
    job: &Job,
    report: Report,
) -> Result<Report, String> {
    perform_with(plan, ctx, job, report, register, observed)
}
fn perform_with(
    plan: &Plan,
    ctx: impl Fn() -> Result<Context, String>,
    job: &Job,
    mut report: Report,
    call: impl FnOnce(&Path) -> Result<i32, String>,
    mut lookup: impl FnMut(&Path, &str) -> Result<bool, String>,
) -> Result<Report, String> {
    plan.validate(&ctx()?)?;
    if job.cancelled.load(Ordering::SeqCst) {
        report.outcome = "cancelled";
        report.reason = "系统请求前已取消，未重新注册应用。".into();
        return Ok(report);
    }
    let (path, id, _, _) = plan.source.control_target();
    report.requested = true;
    let status = match call(Path::new(path)) {
        Ok(status) => status,
        Err(_) => {
            report.outcome = "unknown";
            report.reason = "注册调用未能取得明确结果，请核对所选副本；不会自动重试。".into();
            return Ok(report);
        }
    };
    report.registration_status = Some(status);
    if status != 0 {
        report.outcome = "unknown";
        report.reason = "系统未确认注册成功，请核对图标和打开方式候选；不会自动重试。".into();
        return Ok(report);
    }
    let began = Instant::now();
    loop {
        if ctx().and_then(|context| plan.validate(&context)).is_err() {
            report.outcome = "unknown";
            report.reason =
                "注册请求后应用或保护状态变化，请核对实际结果；未自动撤销或重试。".into();
            break;
        }
        match lookup(Path::new(path), id) {
            Ok(true) => {
                report.registered_path_observed = Some(true);
                report.outcome = "registered";
                report.reason="系统接受重新注册，已观察到所选安装路径。请刷新列表并检查图标或打开方式；不代表问题一定修复。".into();
                break;
            }
            Ok(false) => report.registered_path_observed = Some(false),
            Err(_) => {
                report.outcome = "unknown";
                report.reason =
                    "注册调用已返回，但系统注册位置无法核对；请检查实际状态，不会自动重试。".into();
                break;
            }
        }
        if job.cancelled.load(Ordering::SeqCst) {
            report.outcome = "cancelled";
            report.reason = "已停止后续注册核对，已发出的注册请求可能生效且不能自动撤销。".into();
            break;
        }
        if began.elapsed() >= Duration::from_secs(8) {
            report.outcome = "pending";
            report.reason =
                "系统已接受注册，但尚未观察到所选安装路径。请稍后手动检查，不会自动重试。".into();
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(report)
}
#[cfg(test)]
mod tests;
