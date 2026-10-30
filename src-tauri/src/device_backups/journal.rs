use super::*;
use rusqlite::{params, Connection};
const TABLE:&str="CREATE TABLE IF NOT EXISTS device_backup_runs(id TEXT PRIMARY KEY,request_id TEXT NOT NULL UNIQUE,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=262144));";
fn exists(c: &Connection) -> Result<bool, String> {
    c.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='device_backup_runs'",
        [],
        |r| r.get::<_, u32>(0),
    )
    .map(|n| n == 1)
    .map_err(|e| e.to_string())
}
fn validate(r: &Record) -> Result<(), String> {
    if r.revision != REVISION
        || r.plan.revision != REVISION
        || !id(&r.id)
        || r.id != r.plan.id
        || !id(&r.request_id)
        || !id(&r.plan.scan_id)
        || !id(&r.plan.item.row.id)
        || r.created_at < 0
        || r.finished_at.is_some_and(|t| t < r.created_at)
        || r.reviewed_at.is_some_and(|t| t < r.created_at)
        || !Path::new(&r.plan.root).is_absolute()
        || Path::new(&r.plan.item.row.path).parent() != Some(Path::new(&r.plan.root))
        || r.plan.item.snapshot.entries > 500_000
        || r.plan.item.snapshot.bytes > 9_007_199_254_740_991
        || r.plan.item.snapshot.digest.len() != 64
        || r.reason.len() > 4000
        || ![
            "running",
            "moved",
            "retained",
            "cancelled",
            "unknown",
            "interrupted",
        ]
        .contains(&r.state.as_str())
        || !["checking", "intent", "finished"].contains(&r.phase.as_str())
        || r.state == "moved" && r.receipt.is_none()
        || r.receipt.as_ref().is_some_and(|s| {
            !Path::new(&s.destination).is_absolute() || s.destination.len() > 4096 || s.inode == 0
        })
    {
        return Err("设备备份记录无法核对，原记录保留。".into());
    }
    Ok(())
}
pub(super) fn list(c: &Connection) -> Result<Vec<Record>, String> {
    if !exists(c)? {
        return Ok(vec![]);
    }
    let mut q=c.prepare("SELECT id,request_id,CASE WHEN length(CAST(body AS BLOB))<=262144 THEN body END FROM device_backup_runs ORDER BY rowid DESC LIMIT 65").map_err(|e|e.to_string())?;
    let mut records = vec![];
    for row in q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
    {
        let (id, request, body) = row.map_err(|e| e.to_string())?;
        let r: Record = serde_json::from_str(&body.ok_or("设备备份记录过大。")?)
            .map_err(|_| "设备备份记录格式无法核对。")?;
        validate(&r)?;
        if r.id != id || r.request_id != request {
            return Err("设备备份记录标识不一致。".into());
        }
        records.push(r);
    }
    if records.len() > 64 {
        return Err("设备备份记录超过本轮容量，原记录保留。".into());
    }
    Ok(records)
}
pub(super) fn save(c: &Connection, r: &Record) -> Result<(), String> {
    validate(r)?;
    if c.pragma_query_value::<i64, _>(None, "synchronous", |r| r.get(0))
        .map_err(|e| e.to_string())?
        < 2
    {
        return Err("设备备份处理需要 FULL 同步记录。".into());
    }
    let body = serde_json::to_string(r).map_err(|e| e.to_string())?;
    if body.len() > 262144 {
        return Err("设备备份记录超过容量。".into());
    }
    c.execute_batch(TABLE).map_err(|e| e.to_string())?;
    c.execute("INSERT INTO device_backup_runs(id,request_id,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET body=excluded.body WHERE request_id=excluded.request_id",params![r.id,r.request_id,body]).map_err(|e|e.to_string()).and_then(|n|if n==1{Ok(())}else{Err("设备备份记录被其他请求占用。".into())})
}
pub(super) fn admission(c: &Connection) -> Result<(), String> {
    let records = list(c)?;
    if records.len() >= 64
        || records.iter().any(|r| {
            r.state == "running"
                || ["unknown", "interrupted"].contains(&r.state.as_str()) && r.reviewed_at.is_none()
        })
    {
        return Err("请先核对未完成的设备备份记录；本轮最多保留 64 条，不自动删除收据。".into());
    }
    Ok(())
}
pub(super) fn begin(c: &mut Connection, r: &Record) -> Result<(), String> {
    let tx = c
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    admission(&tx)?;
    crate::execution_admission::reserve(&tx, 262144)?;
    save(&tx, r)?;
    tx.commit().map_err(|e| e.to_string())
}
pub(super) fn reconcile(c: &Connection, active: &[String]) -> Result<Vec<Record>, String> {
    let mut records = list(c)?;
    for r in &mut records {
        if r.state == "running" && !active.contains(&r.request_id) {
            r.state = if r.phase == "intent" {
                "unknown"
            } else {
                "interrupted"
            }
            .into();
            r.finished_at = Some(db::now().max(r.created_at));
            r.reason = "先前请求已中断；请核对原位置和实际收据，不自动重放。".into();
            save(c, r)?;
        }
    }
    Ok(records)
}
