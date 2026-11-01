//! Explicit manual-package updates from a retained installed-app row. Plans
//! never authorize a shell, automatic quit, data migration, or backup deletion.
use crate::{
    app_inspection::identity,
    db,
    engine::{Job, TaskKind},
    observation::AnchoredPath,
    uninstall_plan::{Context, Source},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
pub(crate) mod commands;
mod journal;
mod native;
pub(crate) mod recovery;
mod signing;
#[cfg(test)]
mod tests;

#[derive(Default)]
pub(crate) struct Store {
    plan: Option<Plan>,
    restoration: Option<recovery::Plan>,
}
struct Plan {
    draft: Draft,
    source: Source,
    old: identity::Snapshot,
    new: identity::Snapshot,
    old_tree: native::Tree,
    new_tree: native::Tree,
    context: Context,
    started: Instant,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Draft {
    id: String,
    task_id: String,
    index: usize,
    name: String,
    path: String,
    replacement: String,
    bundle_id: String,
    old_version: String,
    new_version: String,
    publisher: String,
    bytes: u64,
    created_at: i64,
    expires_at: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Record {
    id: String,
    request_id: String,
    created_at: i64,
    finished_at: Option<i64>,
    draft: Draft,
    state: String,
    phase: String,
    reason: String,
    backup: String,
    reviewed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recovery_receipt: Option<RecoveryReceipt>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryReceipt {
    installed: crate::observation::Witness,
    retained: crate::observation::Witness,
    installed_tree: native::Tree,
    retained_tree: native::Tree,
}
impl Record {
    fn view(&self) -> Result<serde_json::Value, String> {
        let mut v = serde_json::to_value(self).map_err(|e| e.to_string())?;
        let obj = v.as_object_mut().ok_or("应用更新记录格式无效。")?;
        obj.remove("recoveryReceipt");
        obj.insert(
            "recoveryAvailable".into(),
            serde_json::json!(
                self.state == "updated"
                    && self.phase == "finished"
                    && self.finished_at.is_some()
                    && self.recovery_receipt.is_some()
            ),
        );
        Ok(v)
    }
}
fn cancelled(job: &Job) -> Result<(), String> {
    if job.cancelled.load(Ordering::SeqCst) {
        Err("已取消应用更新，原应用保留。".into())
    } else {
        Ok(())
    }
}
pub(crate) fn version(raw: &str) -> Result<Vec<u64>, String> {
    let parts = raw.split('.').collect::<Vec<_>>();
    if parts.is_empty()
        || parts.len() > 6
        || raw.len() > 128
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("本轮仅能核对纯数字应用版本，请使用应用自身更新器。".into());
    }
    let mut out = parts
        .iter()
        .map(|p| {
            p.parse::<u64>()
                .map_err(|_| "应用版本超出比较范围。".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    out.resize(6, 0);
    Ok(out)
}
fn newer(old: &identity::Snapshot, new: &identity::Snapshot) -> Result<(), String> {
    let a = version(old.app_version.as_deref().ok_or("已安装应用缺少版本。")?)?;
    let b = version(new.app_version.as_deref().ok_or("新版应用缺少版本。")?)?;
    if b > a
        || b == a
            && version(new.build.as_deref().ok_or("新版缺少构建号。")?)?
                > version(old.build.as_deref().ok_or("旧版缺少构建号。")?)?
    {
        Ok(())
    } else {
        Err("所选应用版本不高于已安装版本；这里不执行降级或重装。".into())
    }
}
fn compatible(path: &Path, snapshot: &identity::Snapshot) -> Result<(), String> {
    let exe = path.join("Contents/MacOS").join(&snapshot.executable_name);
    let held = AnchoredPath::open(&exe)?;
    let mut file = native::open(held.parent(), &native::name(&exe)?, false, false)?;
    let minimum = crate::uninstall_runtime::host_minimum(&mut file, file_len(held.target())?)
        .map_err(|_| "应用主程序格式无法读取。")?
        .ok_or("新版不包含可核对的本机架构与 macOS 版本信息。")?;
    held.validate()?;
    let mut bytes = [0u8; 128];
    let mut len = bytes.len();
    native::syscall(unsafe {
        libc::sysctlbyname(
            c"kern.osproductversion".as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    })?;
    let current = std::ffi::CStr::from_bytes_until_nul(&bytes)
        .map_err(|_| "系统版本无法核对。")?
        .to_str()
        .map_err(|_| "系统版本无法核对。")?;
    let os = version(current)?;
    if vec![
        (minimum >> 16) as u64,
        ((minimum >> 8) & 255) as u64,
        (minimum & 255) as u64,
        0,
        0,
        0,
    ] > os
    {
        return Err("新版要求更高的 macOS 版本。".into());
    }
    if let Some(min) = identity::minimum_os(path)? {
        if version(&min)? > os {
            return Err("新版要求更高的 macOS 版本。".into());
        }
    }
    Ok(())
}
fn file_len(file: &File) -> Result<u64, String> {
    file.metadata().map(|m| m.len()).map_err(|e| e.to_string())
}
fn stopped(path: &Path) -> Result<(), String> {
    let held = AnchoredPath::open(path)?;
    let m = held.target().metadata().map_err(|e| e.to_string())?;
    crate::uninstall_runtime::require_stopped(path, (m.dev(), m.ino()))?;
    held.validate()
}
fn context_equal(a: &Context, b: &Context) -> bool {
    a.home == b.home
        && a.engine_root == b.engine_root
        && a.current_app == b.current_app
        && a.current_id == b.current_id
        && a.protected_paths == b.protected_paths
}
impl Plan {
    fn prepare(
        source: Source,
        replacement: PathBuf,
        ctx: Context,
        job: &Job,
    ) -> Result<Self, String> {
        cancelled(job)?;
        crate::uninstall_plan::guards(&source, &ctx)?;
        if unsafe { libc::geteuid() } == 0 || replacement.extension().is_none_or(|x| x != "app") {
            return Err("请使用普通账户，并选择完整的新版 .app 包。".into());
        }
        let target = Path::new(source.path());
        if replacement.starts_with(target)
            || target.starts_with(&replacement)
            || replacement.to_string_lossy().len() > 4096
        {
            return Err("新版包与安装位置不能重叠。".into());
        }
        // A Store receipt or embedded privileged/background service requires its
        // manager. This is independent of Mole's ordinary source label.
        managed(target)?;
        managed(&replacement)?;
        let old = identity::capture(target, source.control_target().1)?;
        let new = identity::capture(&replacement, source.control_target().1)?;
        if !matches!(
            old.background,
            crate::app_background::Marker::Absent | crate::app_background::Marker::Disabled
        ) || !matches!(
            new.background,
            crate::app_background::Marker::Absent | crate::app_background::Marker::Disabled
        ) {
            return Err("后台专用应用请使用其更新器。".into());
        }
        newer(&old, &new)?;
        compatible(&replacement, &new)?;
        let old_tree = native::inspect(target, job, true)?;
        let new_tree = native::inspect(&replacement, job, false)?;
        let publisher = signing::continuity(target, &replacement)?;
        stopped(target)?;
        stopped(&replacement)?;
        let anchor = AnchoredPath::open(target)?;
        let permission = crate::permissions::observe(&anchor, &crate::permissions::Budget::new())?;
        if permission.code != crate::permissions::Code::NoRestrictionObserved {
            return Err(permission.refusal_reason());
        }
        if native::inspect(target, job, true)? != old_tree
            || native::inspect(&replacement, job, false)? != new_tree
        {
            return Err("应用在准备期间变化，请重新选择。".into());
        }
        let (task_id, index) = source_address(&source);
        let now = db::now();
        let draft = Draft {
            id: uuid::Uuid::new_v4().to_string(),
            task_id,
            index,
            name: source.control_target().2.into(),
            path: source.path().into(),
            replacement: replacement.to_string_lossy().into_owned(),
            bundle_id: source.control_target().1.into(),
            old_version: old.app_version.clone().unwrap(),
            new_version: new.app_version.clone().unwrap(),
            publisher,
            bytes: new_tree.bytes,
            created_at: now,
            expires_at: now + 120_000,
        };
        Ok(Self {
            draft,
            source,
            old,
            new,
            old_tree,
            new_tree,
            context: ctx,
            started: Instant::now(),
        })
    }
    fn recheck(&self, ctx: &Context, job: &Job) -> Result<(), String> {
        cancelled(job)?;
        if !context_equal(ctx, &self.context)
            || self.started.elapsed() >= Duration::from_secs(120)
            || db::now() < self.draft.created_at
            || db::now() >= self.draft.expires_at
        {
            return Err("应用更新确认已过期或保护设置变化，请重新准备。".into());
        }
        crate::uninstall_plan::guards(&self.source, ctx)?;
        let path = Path::new(&self.draft.path);
        let next = Path::new(&self.draft.replacement);
        managed(path)?;
        managed(next)?;
        if identity::capture(path, &self.draft.bundle_id)? != self.old
            || identity::capture(next, &self.draft.bundle_id)? != self.new
            || native::inspect(path, job, true)? != self.old_tree
            || native::inspect(next, job, false)? != self.new_tree
        {
            return Err("应用内容、位置或身份已变化，请重新准备。".into());
        }
        if signing::continuity(path, next)? != self.draft.publisher {
            return Err("应用签名已变化。".into());
        }
        stopped(path)?;
        stopped(next)?;
        Ok(())
    }
}
fn source_address(source: &Source) -> (String, usize) {
    source.address()
}
fn managed(path: &Path) -> Result<(), String> {
    if identity::update_managed(path)? {
        return Err("App Store 或带后台/特权组件的应用，请通过原有管理工具更新。".into());
    }
    for p in [
        "Contents/_MASReceipt",
        "Contents/Library/LoginItems",
        "Contents/Library/LaunchServices",
        "Contents/Library/LaunchAgents",
        "Contents/Library/LaunchDaemons",
        "Contents/Library/SystemExtensions",
        "Contents/Library/PrivilegedHelperTools",
    ] {
        match std::fs::symlink_metadata(path.join(p)) {
            Ok(_) => {
                return Err("App Store 或带后台/特权组件的应用，请通过原有管理工具更新。".into())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("应用管理来源无法确认。".into()),
        }
    }
    Ok(())
}
fn perform(
    plan: &Plan,
    job: &Job,
    ctx: impl Fn() -> Result<Context, String>,
    record: &mut Record,
    mut save: impl FnMut(&Record) -> Result<(), String>,
) -> Result<(), String> {
    plan.recheck(&ctx()?, job)?;
    let path = Path::new(&plan.draft.path);
    let replacement = Path::new(&plan.draft.replacement);
    let target = AnchoredPath::open(path)?;
    let source = AnchoredPath::open(replacement)?;
    let mut capacity = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    native::syscall(unsafe { libc::fstatfs(target.parent().as_raw_fd(), capacity.as_mut_ptr()) })?;
    let volume = unsafe { capacity.assume_init() };
    let kind = unsafe { std::ffi::CStr::from_ptr(volume.f_fstypename.as_ptr()) };
    if kind != c"apfs"
        || (volume.f_bavail as u64).saturating_mul(volume.f_bsize as u64)
            < plan.new_tree.bytes.saturating_add(64 * 1024 * 1024)
    {
        return Err("安装卷需为可写 APFS，且有足够空间保留新版与旧版。".into());
    }
    let stage_path = Path::new(&record.backup).parent().ok_or("缺少暂存位置。")?;
    let stage_name = native::name(stage_path)?;
    let target_name = native::name(path)?;
    native::syscall(unsafe {
        libc::mkdirat(target.parent().as_raw_fd(), stage_name.as_ptr(), 0o700)
    })?;
    let stage = native::open(target.parent(), &stage_name, true, false)?;
    let stage_identity = stage.metadata().map_err(|e| e.to_string())?;
    record.phase = "copying".into();
    record.reason = "正在复制并核对新版；原应用仍在安装位置。".into();
    save(record)?;
    native::syscall(unsafe { libc::mkdirat(stage.as_raw_fd(), target_name.as_ptr(), 0o700) })?;
    let copied = native::open(&stage, &target_name, true, false)?;
    let copied_id = copied.metadata().map_err(|e| e.to_string())?;
    let observed = native::tree(source.target(), Some(&copied), job, false)?;
    if observed != plan.new_tree
        || native::tree(&copied, None, job, true)?.content != plan.new_tree.content
    {
        return Err("新版复制核验未通过，原应用保留。".into());
    }
    source.validate()?;
    let staged = stage_path.join(path.file_name().unwrap());
    if signing::continuity(path, &staged)? != plan.draft.publisher {
        return Err("暂存应用签名不一致，原应用保留。".into());
    }
    plan.recheck(&ctx()?, job)?;
    target.validate()?;
    let permissions = crate::permissions::observe(&target, &crate::permissions::Budget::new())?;
    if permissions.code != crate::permissions::Code::NoRestrictionObserved {
        return Err(permissions.refusal_reason());
    }
    let mut capacity = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    native::syscall(unsafe { libc::fstatfs(stage.as_raw_fd(), capacity.as_mut_ptr()) })?;
    let volume = unsafe { capacity.assume_init() };
    let kind = unsafe { std::ffi::CStr::from_ptr(volume.f_fstypename.as_ptr()) };
    if kind != c"apfs" {
        return Err("本轮更新仅支持可核对的 APFS 安装卷。".into());
    }
    let stage_anchor = AnchoredPath::open(stage_path)?;
    let m = stage_anchor
        .target()
        .metadata()
        .map_err(|e| e.to_string())?;
    if m.dev() != stage_identity.dev() || m.ino() != stage_identity.ino() {
        return Err("暂存目录身份变化，原应用保留。".into());
    }
    stage_anchor.validate()?;
    cancelled(job)?;
    // Durable intent precedes the only replacement operation. A cancellation
    // after intent cannot undo an already-published whole bundle.
    record.phase = "intent".into();
    record.reason = "即将交换完整应用包；旧包将保留在记录位置。".into();
    save(record)?;
    native::exchange(&stage, &target_name, target.parent(), &target_name)?;
    let installed = AnchoredPath::open(path)?;
    let backup = AnchoredPath::open(&staged)?;
    let a = installed.target().metadata().map_err(|e| e.to_string())?;
    let b = backup.target().metadata().map_err(|e| e.to_string())?;
    let old = target.target().metadata().map_err(|e| e.to_string())?;
    // rename changes root ctime, so compare inode and portable contents. Never
    // retry, reverse an uncertain swap, or remove a mismatching old object.
    let verify = Job::new(TaskKind::AppControl);
    if (a.dev(), a.ino()) != (copied_id.dev(), copied_id.ino())
        || (b.dev(), b.ino()) != (old.dev(), old.ino())
        || native::tree(installed.target(), None, &verify, true)?.content != plan.new_tree.content
        || native::tree(backup.target(), None, &verify, true)?.content != plan.old_tree.content
        || signing::continuity(&staged, path)? != plan.draft.publisher
    {
        return Err("交换后身份或内容未完整核对，请检查安装位置与保留的旧包。".into());
    }
    let final_stage = AnchoredPath::open(stage_path)?;
    let m = final_stage.target().metadata().map_err(|e| e.to_string())?;
    if (m.dev(), m.ino()) != (stage_identity.dev(), stage_identity.ino()) {
        return Err("更新后暂存目录位置无法核对。".into());
    }
    final_stage.validate()?;
    installed.validate()?;
    backup.validate()?;
    let installed_tree = native::tree(installed.target(), None, &verify, true)?;
    let retained_tree = native::tree(backup.target(), None, &verify, true)?;
    if installed_tree.content != plan.new_tree.content
        || retained_tree.content != plan.old_tree.content
    {
        return Err("交换后完整内容变化，请核对两份应用。".into());
    }
    installed.validate()?;
    backup.validate()?;
    record.recovery_receipt = Some(RecoveryReceipt {
        installed: installed.witness.clone(),
        retained: backup.witness.clone(),
        installed_tree,
        retained_tree,
    });
    record.state = "updated".into();
    record.reason =
        "新版已放入原安装位置，旧版应用包已保留。应用数据未修改；首次打开仍由 macOS 决定。".into();
    Ok(())
}
