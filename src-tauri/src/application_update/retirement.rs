//! Explicitly retire a retained complete App identified by update/restore receipts.
//! Never removes the installed App, package-external data or unknown stage contents.
use super::*;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
pub(crate) mod commands;
pub(super) mod journal;
#[cfg(test)]
mod tests;
#[derive(Clone, Serialize)]
pub(super) struct RestoredMapping {
    pub record_id: String,
    pub fingerprint: String,
    pub installed_device: String,
    pub installed_inode: String,
    pub retained_device: String,
    pub retained_inode: String,
    pub installed_hash: String,
    pub retained_hash: String,
    pub installed_version: String,
    pub retained_version: String,
}
#[derive(Clone, Serialize)]
struct Authority {
    update: super::Record,
    restored: Option<RestoredMapping>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Draft {
    id: String,
    source_id: String,
    name: String,
    installed: String,
    retained: String,
    stage: String,
    installed_version: String,
    retained_version: String,
    after_restore: bool,
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
    draft: Draft,
    state: String,
    phase: String,
    reason: String,
    created_at: i64,
    finished_at: Option<i64>,
    receipt: Option<crate::execution::Receipt>,
    stage_removed: bool,
    reviewed_at: Option<i64>,
}
pub(super) struct Plan {
    draft: Draft,
    authority: Authority,
    fingerprint: String,
    context: Context,
    issued: Instant,
    installed: identity::Snapshot,
    retained: identity::Snapshot,
}
fn digest(a: &Authority) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(a).map_err(|e| e.to_string())?)
    ))
}
fn source(c: &rusqlite::Connection, id: &str) -> Result<Authority, String> {
    let update = super::journal::list(c)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("原应用更新记录不存在。")?;
    if update.state != "updated"
        || update.phase != "finished"
        || update.finished_at.is_none()
        || update.recovery_receipt.is_none()
    {
        return Err("缺少可靠的应用更新收据，保留版本不能自动整理。".into());
    }
    let restored = recovery::retirement_mapping(c, id)?;
    Ok(Authority { update, restored })
}
pub(super) fn block_source(c: &rusqlite::Connection, id: &str) -> Result<(), String> {
    if !available(&journal::list(c)?, id) {
        return Err("此保留包已整理或结果待核实，不能沿用原收据恢复或再次整理。".into());
    }
    Ok(())
}
pub(super) fn available(rows: &[Record], id: &str) -> bool {
    !rows.iter().any(|r| {
        r.draft.source_id == id && ["moved", "running", "unknown"].contains(&r.state.as_str())
    })
}
fn stage(a: &Authority, job: &Job) -> Result<AnchoredPath, String> {
    cancelled(job)?;
    let path = Path::new(&a.update.backup);
    let p = path.parent().ok_or("保留目录无效。")?;
    if p.parent() != Path::new(&a.update.draft.path).parent()
        || p.file_name()
            .is_none_or(|n| n != format!(".moomole-app-update-{}", a.update.id).as_str())
        || path.file_name() != Path::new(&a.update.draft.path).file_name()
    {
        return Err("保留包位置不对应原更新。".into());
    }
    let held = AnchoredPath::open(p)?;
    let receipt = a.update.recovery_receipt.as_ref().unwrap();
    if held.witness.nodes
        != receipt.retained.nodes[..receipt
            .retained
            .nodes
            .len()
            .checked_sub(1)
            .ok_or("保留收据无效。")?]
        || !held.local_materialized()
        || native::names(held.target())? != vec![native::name(path)?]
    {
        return Err("专用保留目录已变化或包含其他内容，整份保留。".into());
    }
    held.validate()?;
    Ok(held)
}
fn gate(a: &Authority, ctx: &Context, job: &Job) -> Result<(), String> {
    cancelled(job)?;
    let d = &a.update.draft;
    crate::uninstall_plan::guards(
        &Source::from_row(
            &d.task_id,
            d.index,
            &serde_json::json!({"path":d.path,"bundle_id":d.bundle_id,"name":d.name,"source":"App"}),
        )?,
        ctx,
    )?;
    if unsafe { libc::geteuid() } == 0 {
        return Err("请使用普通账户整理保留版本。".into());
    }
    for path in [&d.path, &a.update.backup] {
        if !crate::app_protection::check(
            path,
            Some(&d.bundle_id),
            ctx.current_app.as_deref(),
            &ctx.current_id,
            &ctx.protected_paths,
        )?
        .reasons
        .is_empty()
        {
            return Err("应用或保留版本受保护，未处理。".into());
        }
        managed(Path::new(path))?;
        stopped(Path::new(path))?;
        let snapshot = identity::capture(Path::new(path), &d.bundle_id)?;
        if !matches!(
            snapshot.background,
            crate::app_background::Marker::Absent | crate::app_background::Marker::Disabled
        ) {
            return Err("后台或特权应用的保留版本请使用原管理方式。".into());
        }
    }
    stage(a, job)?;
    Ok(())
}
fn evidence(
    a: &Authority,
    job: &Job,
) -> Result<(identity::Snapshot, identity::Snapshot, native::Tree), String> {
    let r = &a.update;
    let saved = r.recovery_receipt.as_ref().ok_or("缺少保留版本收据。")?;
    let current = identity::capture(Path::new(&r.draft.path), &r.draft.bundle_id)?;
    let kept = identity::capture(Path::new(&r.backup), &r.draft.bundle_id)?;
    let tree_current = native::inspect(Path::new(&r.draft.path), job, true)?;
    let tree_kept = native::inspect(Path::new(&r.backup), job, true)?;
    if let Some(mapping) = &a.restored {
        for (actual, witness, device, inode, content, expected) in [
            (
                &tree_current,
                &current.root,
                &mapping.installed_device,
                &mapping.installed_inode,
                &mapping.installed_hash,
                &saved.retained_tree.content,
            ),
            (
                &tree_kept,
                &kept.root,
                &mapping.retained_device,
                &mapping.retained_inode,
                &mapping.retained_hash,
                &saved.installed_tree.content,
            ),
        ] {
            let n = witness.nodes.last().ok_or("应用收据身份无效。")?;
            if n.dev.to_string() != *device
                || n.ino.to_string() != *inode
                || actual.content != *content
                || content != expected
            {
                return Err("恢复后的应用包不再对应可靠收据，保留版本保持。".into());
            }
        }
        if current.app_version.as_deref() != Some(mapping.installed_version.as_str())
            || kept.app_version.as_deref() != Some(mapping.retained_version.as_str())
        {
            return Err("恢复后的版本对应关系已变化。".into());
        }
    } else if current.root != saved.installed
        || kept.root != saved.retained
        || tree_current != saved.installed_tree
        || tree_kept != saved.retained_tree
        || current.app_version.as_deref() != Some(r.draft.new_version.as_str())
        || kept.app_version.as_deref() != Some(r.draft.old_version.as_str())
    {
        return Err("当前或保留应用包已变化，不能重新认领为原备份。".into());
    }
    let publisher = if a.restored.is_some() {
        signing::continuity(Path::new(&r.draft.path), Path::new(&r.backup))?
    } else {
        signing::continuity(Path::new(&r.backup), Path::new(&r.draft.path))?
    };
    if publisher != r.draft.publisher {
        return Err("应用签名与原更新收据不一致。".into());
    }
    Ok((current, kept, tree_kept))
}
impl Plan {
    fn prepare(a: Authority, ctx: Context, job: &Job) -> Result<Self, String> {
        gate(&a, &ctx, job)?;
        let (installed, retained, tree) = evidence(&a, job)?;
        let now = db::now();
        let draft = Draft {
            id: uuid::Uuid::new_v4().to_string(),
            source_id: a.update.id.clone(),
            name: a.update.draft.name.clone(),
            installed: a.update.draft.path.clone(),
            retained: a.update.backup.clone(),
            stage: Path::new(&a.update.backup)
                .parent()
                .unwrap()
                .to_str()
                .ok_or("保留目录无法编码。")?
                .into(),
            installed_version: installed.app_version.clone().ok_or("当前版本缺失。")?,
            retained_version: retained.app_version.clone().ok_or("保留版本缺失。")?,
            after_restore: a.restored.is_some(),
            publisher: a.update.draft.publisher.clone(),
            bytes: tree.bytes,
            created_at: now,
            expires_at: now + 120000,
        };
        Ok(Self {
            fingerprint: digest(&a)?,
            authority: a,
            context: ctx,
            issued: Instant::now(),
            draft,
            installed,
            retained,
        })
    }
    fn check(&self, c: &rusqlite::Connection, ctx: &Context, job: &Job) -> Result<(), String> {
        let now = db::now();
        if self.issued.elapsed() >= Duration::from_secs(120)
            || now < self.draft.created_at
            || now >= self.draft.expires_at
            || !context_equal(ctx, &self.context)
        {
            return Err("保留版本确认已过期或保护设置变化，请重新检查。".into());
        }
        if digest(&source(c, &self.draft.source_id)?)? != self.fingerprint {
            return Err("更新或恢复记录已变化，请重新检查保留版本。".into());
        }
        gate(&self.authority, ctx, job)?;
        let (a, b, _) = evidence(&self.authority, job)?;
        if a != self.installed || b != self.retained {
            return Err("确认期间应用身份发生变化，原包保留。".into());
        }
        let now = db::now();
        if self.issued.elapsed() >= Duration::from_secs(120)
            || now < self.draft.created_at
            || now >= self.draft.expires_at
        {
            return Err("保留版本确认已过期，请重新检查。".into());
        }
        cancelled(job)
    }
}
fn remove_empty_stage(plan: &Plan) -> bool {
    let old = &plan.retained.root.nodes;
    let expected = &old[..old.len() - 1];
    let result = (|| {
        let held = AnchoredPath::open(Path::new(&plan.draft.stage))?;
        if held.witness.nodes != expected || !native::names(held.target())?.is_empty() {
            return Err("保留目录已变化。".into());
        }
        held.validate()?;
        native::syscall(unsafe {
            libc::unlinkat(
                held.parent().as_raw_fd(),
                native::name(Path::new(&plan.draft.stage))?.as_ptr(),
                libc::AT_REMOVEDIR,
            )
        })
    })();
    result.is_ok()
}
fn run(
    plan: Plan,
    request_id: String,
    job: &Job,
    c: &Mutex<db::Database>,
    ctx: impl Fn() -> Result<Context, String>,
    checkpoint: impl Fn(&Record),
) -> Result<Record, String> {
    let mut r = Record {
        id: plan.draft.id.clone(),
        request_id,
        draft: plan.draft.clone(),
        state: "running".into(),
        phase: "checking".into(),
        reason: "正在复查当前和保留的应用包。".into(),
        created_at: db::now(),
        finished_at: None,
        receipt: None,
        stage_removed: false,
        reviewed_at: None,
    };
    journal::begin(&mut *c.lock().map_err(|e| e.to_string())?, &r)?;
    let result = (|| {
        // Context reads persisted protection settings through this same mutex.
        // Release that read before acquiring the journal guard for validation.
        let current = ctx()?;
        plan.check(&*c.lock().map_err(|e| e.to_string())?, &current, job)?;
        let prepared = crate::uninstall_target::PreparedBundle::prepare_retained_copy(
            Path::new(&plan.draft.retained),
            &plan.retained,
            plan.draft.created_at,
            plan.draft.expires_at,
        )?;
        r.phase = "intent".into();
        r.reason = "已记录仅处理保留版本的独立意图。".into();
        journal::save(&*c.lock().map_err(|e| e.to_string())?, &r)?;
        prepared.move_to_trash_checked(|| {
            let current = ctx()?;
            plan.check(&*c.lock().map_err(|e| e.to_string())?, &current, job)
        })
    })();
    match result {
        Err(e) => {
            r.state = if job.cancelled.load(Ordering::SeqCst) {
                "cancelled"
            } else {
                "retained"
            }
            .into();
            r.reason = e;
        }
        Ok(attempt) => {
            r.reason = attempt.reason.into();
            match attempt.outcome {
                crate::uninstall_target::Outcome::Moved(receipt) => {
                    r.state = "moved".into();
                    r.receipt = Some(receipt.actual);
                }
                crate::uninstall_target::Outcome::Uncertain(receipt) => {
                    r.state = "unknown".into();
                    r.receipt = receipt.map(|v| v.actual);
                }
            }
            checkpoint(&r);
            let verified = Job::new(TaskKind::AppControl);
            if identity::capture(
                Path::new(&plan.draft.installed),
                &plan.authority.update.draft.bundle_id,
            )
            .map_or(true, |v| v != plan.installed)
            {
                r.state = "unknown".into();
                r.reason = "处理后当前安装包无法再次核对，请核查两处实际位置。".into();
            }
            let expected = plan
                .authority
                .restored
                .as_ref()
                .map(|m| m.installed_hash.as_str())
                .unwrap_or(
                    &plan
                        .authority
                        .update
                        .recovery_receipt
                        .as_ref()
                        .unwrap()
                        .installed_tree
                        .content,
                );
            if native::inspect(Path::new(&plan.draft.installed), &verified, true)
                .map_or(true, |v| v.content != expected)
            {
                r.state = "unknown".into();
                r.reason = "处理后当前安装包内容无法核对，请核查实际结果。".into();
            }
            if r.state == "moved" && !job.cancelled.load(Ordering::SeqCst) {
                r.stage_removed = remove_empty_stage(&plan);
            }
            if r.state == "moved" {
                r.reason = if r.stage_removed {
                    "保留版本已移入废纸篓，空的专用目录已移除；当前安装包和包外数据保留。"
                } else {
                    "保留版本已移入废纸篓，专用目录保留供核对；当前安装包和包外数据保留。"
                }
                .into();
            }
        }
    }
    r.phase = "finished".into();
    r.finished_at = Some(db::now().max(r.created_at));
    journal::save(&*c.lock().map_err(|e| e.to_string())?, &r)
        .map_err(|_| "保留版本整理结果未可靠保存，请刷新并核查实际文件；不会自动重试。")?;
    Ok(r)
}

pub(crate) fn reconcile_startup(c: &rusqlite::Connection) -> Result<(), String> {
    journal::reconcile(c, &[]).map(|_| ())
}
