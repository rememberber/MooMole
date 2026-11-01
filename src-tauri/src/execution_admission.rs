//! Called inside BEGIN IMMEDIATE by each operation-specific journal. This is
//! durable admission, not runtime permission or a target lock.
use rusqlite::Transaction;
pub(crate) const MAX_RUNS: i64 = 64;
pub(crate) const MAX_RESERVED: i64 = 64 * 1024 * 1024;

pub(crate) fn reserve(db: &Transaction<'_>, reserved: i64) -> Result<(), String> {
    let (mut count,mut used,mut blocked,invalid):(i64,i64,i64,i64)=db.query_row("SELECT count(*),coalesce(sum(reserved_bytes),0),coalesce(sum(blocked),0),coalesce(sum(reserved_bytes<=0),0) FROM (
        SELECT reserved_bytes,state='running' OR EXISTS(SELECT 1 FROM execution_items i WHERE i.run_id=r.id AND i.state='uncertain' AND NOT EXISTS(SELECT 1 FROM execution_reviews v WHERE v.run_id=r.id AND v.snapshot_sha256=r.snapshot_sha256 AND typeof(v.reviewed_at)='integer' AND v.reviewed_at>=0)) AS blocked FROM execution_runs r
        UNION ALL SELECT reserved_bytes,state='running' OR (item_state='uncertain' AND NOT EXISTS(SELECT 1 FROM uninstall_reviews v WHERE v.run_id=r.id AND v.snapshot_sha256=r.snapshot_sha256 AND typeof(v.reviewed_at)='integer' AND v.reviewed_at>=0)) AS blocked FROM uninstall_runs r
        UNION ALL SELECT reserved_bytes,state='running' OR EXISTS(SELECT 1 FROM maintenance_steps i WHERE i.run_id=r.id AND (i.state='intent' OR (i.state='uncertain' AND NOT EXISTS(SELECT 1 FROM maintenance_reviews v WHERE v.run_id=r.id AND v.snapshot_sha256=r.snapshot_sha256 AND typeof(v.reviewed_at)='integer' AND v.reviewed_at>=0)))) AS blocked FROM maintenance_runs r
    )",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|format!("无法核对修改任务准入；停止操作：{e}"))?;
    let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='mole_clean_runs' AND type='table')",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if exists {
        let (runs,pending):(i64,i64)=db.query_row("SELECT count(*),coalesce(sum(state='running' OR (state IN ('interrupted','uncertain') AND reviewed_at IS NULL)),0) FROM mole_clean_runs",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
        count += runs;
        used += runs * crate::mole_clean::journal::RESERVED_BYTES;
        blocked += pending;
    }
    let (extractions, pending) = crate::personal_files::zip_extraction::journal::reservations(db)?;
    count += extractions;
    used += extractions * 131072;
    blocked += pending;
    if invalid != 0 || reserved <= 0 {
        return Err("执行记录容量字段无效；保留记录供核查。".into());
    }
    if blocked != 0 {
        return Err(
            "已有运行中或结果待核实的清理、卸载或维护批次。请在操作历史核查，暂不能开始新的修改。"
                .into(),
        );
    }
    if count >= MAX_RUNS
        || used
            .checked_add(reserved)
            .is_none_or(|total| total > MAX_RESERVED)
    {
        return Err(
            "执行记录达到共享容量上限；请在操作历史的归档入口保存恢复证据，再释放活动容量。".into(),
        );
    }
    Ok(())
}
