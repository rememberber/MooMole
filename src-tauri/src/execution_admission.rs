//! Called inside BEGIN IMMEDIATE by each operation-specific journal. This is
//! durable admission, not runtime permission or a target lock.
use rusqlite::Transaction;
pub(crate) const MAX_RUNS: i64 = 64;
pub(crate) const MAX_RESERVED: i64 = 64 * 1024 * 1024;

pub(crate) fn reserve(db: &Transaction<'_>, reserved: i64) -> Result<(), String> {
    let (count,used,blocked,invalid):(i64,i64,i64,i64)=db.query_row("SELECT count(*),coalesce(sum(reserved_bytes),0),coalesce(sum(blocked),0),coalesce(sum(reserved_bytes<=0),0) FROM (
        SELECT reserved_bytes,state='running' OR EXISTS(SELECT 1 FROM execution_items i WHERE i.run_id=r.id AND i.state='uncertain') AS blocked FROM execution_runs r
        UNION ALL SELECT reserved_bytes,state='running' OR item_state='uncertain' AS blocked FROM uninstall_runs
    )",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|format!("无法核对修改任务准入；停止操作：{e}"))?;
    if invalid != 0 || reserved <= 0 {
        return Err("执行记录容量字段无效；保留记录供核查。".into());
    }
    if blocked != 0 {
        return Err("已有运行中或结果待核实的清理/卸载批次，不能开始新的修改操作。".into());
    }
    if count >= MAX_RUNS
        || used
            .checked_add(reserved)
            .is_none_or(|total| total > MAX_RESERVED)
    {
        return Err("执行记录达到共享容量上限；需要先处理保留记录，不能丢弃恢复证据。".into());
    }
    Ok(())
}
