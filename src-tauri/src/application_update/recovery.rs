//! One explicitly confirmed exchange from a durable successful update receipt.
//! Application data is never rolled back. Both complete bundles are retained.
use super::*;
use sha2::{Digest, Sha256};
pub(crate) mod commands;
pub(super) mod journal;
#[cfg(test)]
pub(super) mod tests;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Draft {
    id: String,
    source_id: String,
    name: String,
    path: String,
    retained: String,
    from_version: String,
    to_version: String,
    publisher: String,
    bytes: u64,
    created_at: i64,
    expires_at: i64,
}
pub(super) struct Plan {
    draft: Draft,
    source: super::Record,
    fingerprint: String,
    context: Context,
    started: Instant,
    current: identity::Snapshot,
    older: identity::Snapshot,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Receipt {
    installed_device: String,
    installed_inode: String,
    retained_device: String,
    retained_inode: String,
    installed_hash: String,
    retained_hash: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Record {
    id: String,
    request_id: String,
    draft: Draft,
    created_at: i64,
    finished_at: Option<i64>,
    state: String,
    phase: String,
    reason: String,
    receipt: Option<Receipt>,
    reviewed_at: Option<i64>,
}
fn fingerprint(r: &super::Record) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(r).map_err(|e| e.to_string())?)
    ))
}
fn source(db: &rusqlite::Connection, id: &str) -> Result<super::Record, String> {
    super::retirement::block_source(db, id)?;
    let row = super::journal::list(db)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("原应用更新记录不存在。")?;
    if row.state != "updated"
        || row.phase != "finished"
        || row.finished_at.is_none()
        || row.recovery_receipt.is_none()
    {
        return Err("此更新缺少可靠的双端恢复收据，请保留两份应用并人工核对。".into());
    }
    if journal::list(db)?
        .iter()
        .any(|r| r.draft.source_id == id && r.state == "restored")
    {
        return Err("本次更新已经恢复过旧版，不能重复交换。".into());
    }
    Ok(row)
}
fn gate_paths(record: &super::Record, ctx: &Context, job: &Job) -> Result<(), String> {
    cancelled(job)?;
    if unsafe { libc::geteuid() } == 0 {
        return Err("请使用普通账户恢复应用。".into());
    }
    let d = &record.draft;
    let source = Source::from_row(
        &d.task_id,
        d.index,
        &serde_json::json!({"path":d.path,"bundle_id":d.bundle_id,"name":d.name,"source":"App"}),
    )?;
    crate::uninstall_plan::guards(&source, ctx)?;
    if !crate::app_protection::check(
        &record.backup,
        Some(&d.bundle_id),
        ctx.current_app.as_deref(),
        &ctx.current_id,
        &ctx.protected_paths,
    )?
    .reasons
    .is_empty()
    {
        return Err("保留的旧版位于保护范围，不能恢复。".into());
    }
    for p in [Path::new(&d.path), Path::new(&record.backup)] {
        managed(p)?;
        let snapshot = identity::capture(p, &d.bundle_id)?;
        if !matches!(
            snapshot.background,
            crate::app_background::Marker::Absent | crate::app_background::Marker::Disabled
        ) {
            return Err("后台专用应用请使用原有管理工具恢复。".into());
        }
        stopped(p)?;
        let anchor = AnchoredPath::open(p)?;
        let permission = crate::permissions::observe(&anchor, &crate::permissions::Budget::new())?;
        if permission.code != crate::permissions::Code::NoRestrictionObserved {
            return Err(permission.refusal_reason());
        }
        let mut volume = std::mem::MaybeUninit::<libc::statfs>::zeroed();
        native::syscall(unsafe {
            libc::fstatfs(anchor.parent().as_raw_fd(), volume.as_mut_ptr())
        })?;
        let v = unsafe { volume.assume_init() };
        if unsafe { std::ffi::CStr::from_ptr(v.f_fstypename.as_ptr()) } != c"apfs"
            || v.f_flags as i32 & libc::MNT_RDONLY != 0
        {
            return Err("应用恢复仅支持可写的本地 APFS 安装卷。".into());
        }
    }
    Ok(())
}
fn evidence(r: &super::Record, job: &Job) -> Result<(), String> {
    let saved = r.recovery_receipt.as_ref().ok_or("缺少应用恢复收据。")?;
    for (path, witness, tree) in [
        (&r.draft.path, &saved.installed, &saved.installed_tree),
        (&r.backup, &saved.retained, &saved.retained_tree),
    ] {
        if witness.version != crate::observation::VERSION
            || witness
                .nodes
                .last()
                .is_none_or(|n| n.path != Path::new(path))
        {
            return Err("原更新收据身份无法核对。".into());
        }
        let held = AnchoredPath::open(Path::new(path))?;
        if held.witness != *witness
            || !held.local_materialized()
            || native::tree(held.target(), None, job, true)? != *tree
        {
            return Err("应用包、保留旧版或目录身份已变化，不能沿用原更新收据。".into());
        }
        held.validate()?;
    }
    Ok(())
}
impl Plan {
    fn prepare(record: super::Record, ctx: Context, job: &Job) -> Result<Self, String> {
        gate_paths(&record, &ctx, job)?;
        evidence(&record, job)?;
        let current = identity::capture(Path::new(&record.draft.path), &record.draft.bundle_id)?;
        let older = identity::capture(Path::new(&record.backup), &record.draft.bundle_id)?;
        if current.app_version.as_deref() != Some(record.draft.new_version.as_str())
            || older.app_version.as_deref() != Some(record.draft.old_version.as_str())
        {
            return Err("当前或保留应用的版本不再对应原更新记录。".into());
        }
        compatible(Path::new(&record.backup), &older)?;
        if signing::continuity(Path::new(&record.backup), Path::new(&record.draft.path))?
            != record.draft.publisher
        {
            return Err("两份应用的签名不再对应原更新。".into());
        }
        let now = db::now();
        let draft = Draft {
            id: uuid::Uuid::new_v4().to_string(),
            source_id: record.id.clone(),
            name: record.draft.name.clone(),
            path: record.draft.path.clone(),
            retained: record.backup.clone(),
            from_version: record.draft.new_version.clone(),
            to_version: record.draft.old_version.clone(),
            publisher: record.draft.publisher.clone(),
            bytes: record
                .recovery_receipt
                .as_ref()
                .unwrap()
                .retained_tree
                .bytes,
            created_at: now,
            expires_at: now + 120_000,
        };
        Ok(Self {
            fingerprint: fingerprint(&record)?,
            source: record,
            context: ctx,
            started: Instant::now(),
            draft,
            current,
            older,
        })
    }
    fn binding(&self, db: &rusqlite::Connection) -> Result<(), String> {
        if fingerprint(&source(db, &self.draft.source_id)?)? != self.fingerprint {
            return Err("原更新记录已变化，请重新核对恢复。".into());
        }
        Ok(())
    }
    fn time(&self) -> Result<(), String> {
        let now = db::now();
        if self.started.elapsed() >= Duration::from_secs(120)
            || now < self.draft.created_at
            || now >= self.draft.expires_at
        {
            Err("旧版恢复确认已过期，请重新检查。".into())
        } else {
            Ok(())
        }
    }
    fn check(&self, ctx: &Context, job: &Job) -> Result<(), String> {
        self.time()?;
        if !context_equal(&self.context, ctx) {
            return Err("保护设置或运行环境已变化，请重新准备旧版恢复。".into());
        }
        gate_paths(&self.source, ctx, job)?;
        evidence(&self.source, job)?;
        if identity::capture(Path::new(&self.draft.path), &self.source.draft.bundle_id)?
            != self.current
            || identity::capture(
                Path::new(&self.draft.retained),
                &self.source.draft.bundle_id,
            )? != self.older
            || signing::continuity(Path::new(&self.draft.retained), Path::new(&self.draft.path))?
                != self.draft.publisher
        {
            return Err("应用身份或签名已变化，恢复保留。".into());
        }
        compatible(Path::new(&self.draft.retained), &self.older)?;
        self.time()?;
        cancelled(job)
    }
}
fn perform(
    plan: &Plan,
    job: &Job,
    ctx: impl Fn() -> Result<Context, String>,
    bind: impl Fn() -> Result<(), String>,
    record: &mut Record,
    mut save: impl FnMut(&Record) -> Result<(), String>,
) -> Result<(), String> {
    bind()?;
    plan.check(&ctx()?, job)?;
    let installed = AnchoredPath::open(Path::new(&plan.draft.path))?;
    let retained = AnchoredPath::open(Path::new(&plan.draft.retained))?;
    let a = installed.target().metadata().map_err(|e| e.to_string())?;
    let b = retained.target().metadata().map_err(|e| e.to_string())?;
    if a.dev() != b.dev() {
        return Err("两份应用不在同一安装卷，不能交换恢复。".into());
    }
    bind()?;
    gate_paths(&plan.source, &ctx()?, job)?;
    installed.validate()?;
    retained.validate()?;
    plan.time()?;
    cancelled(job)?;
    record.phase = "intent".into();
    record.reason = "即将交换两份完整应用，当前新版将保留在原旧版目录。".into();
    save(record)?;
    native::exchange(
        retained.parent(),
        &native::name(Path::new(&plan.draft.retained))?,
        installed.parent(),
        &native::name(Path::new(&plan.draft.path))?,
    )?;
    let current = AnchoredPath::open(Path::new(&plan.draft.path))?;
    let kept = AnchoredPath::open(Path::new(&plan.draft.retained))?;
    let x = current.target().metadata().map_err(|e| e.to_string())?;
    let y = kept.target().metadata().map_err(|e| e.to_string())?;
    let verify = Job::new(TaskKind::AppControl);
    let old_tree = native::tree(current.target(), None, &verify, true)?;
    let new_tree = native::tree(kept.target(), None, &verify, true)?;
    let saved = plan.source.recovery_receipt.as_ref().unwrap();
    if (x.dev(), x.ino()) != (b.dev(), b.ino())
        || (y.dev(), y.ino()) != (a.dev(), a.ino())
        || old_tree.content != saved.retained_tree.content
        || new_tree.content != saved.installed_tree.content
        || signing::continuity(Path::new(&plan.draft.path), Path::new(&plan.draft.retained))?
            != plan.draft.publisher
    {
        return Err("恢复交换后未完成双端核对，请检查实际应用；不会自动反向交换。".into());
    }
    current.validate()?;
    kept.validate()?;
    record.receipt = Some(Receipt {
        installed_device: x.dev().to_string(),
        installed_inode: x.ino().to_string(),
        retained_device: y.dev().to_string(),
        retained_inode: y.ino().to_string(),
        installed_hash: old_tree.content,
        retained_hash: new_tree.content,
    });
    record.state = "restored".into();
    record.reason =
        "旧版应用包已恢复到安装位置，新版包已保留。应用数据未回退，兼容性仍需自行核对。".into();
    Ok(())
}

pub(super) fn retirement_mapping(
    c: &rusqlite::Connection,
    id: &str,
) -> Result<Option<super::retirement::RestoredMapping>, String> {
    let rows = journal::list(c)?;
    let rows = rows
        .iter()
        .filter(|r| r.draft.source_id == id)
        .collect::<Vec<_>>();
    if rows
        .iter()
        .any(|r| ["running", "unknown", "interrupted"].contains(&r.state.as_str()))
    {
        return Err("此更新的恢复状态缺少可靠结果，保留包不能自动整理。".into());
    }
    let done = rows
        .iter()
        .filter(|r| r.state == "restored")
        .collect::<Vec<_>>();
    let row = match done.as_slice() {
        [] => return Ok(None),
        [r] => r,
        _ => return Err("保留版本的恢复收据存在歧义。".into()),
    };
    let p = row.receipt.as_ref().ok_or("缺少恢复后的双端收据。")?;
    Ok(Some(super::retirement::RestoredMapping {
        record_id: row.id.clone(),
        fingerprint: format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(row).map_err(|e| e.to_string())?)
        ),
        installed_device: p.installed_device.clone(),
        installed_inode: p.installed_inode.clone(),
        retained_device: p.retained_device.clone(),
        retained_inode: p.retained_inode.clone(),
        installed_hash: p.installed_hash.clone(),
        retained_hash: p.retained_hash.clone(),
        installed_version: row.draft.to_version.clone(),
        retained_version: row.draft.from_version.clone(),
    }))
}
