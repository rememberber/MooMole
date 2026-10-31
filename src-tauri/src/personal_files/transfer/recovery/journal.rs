use super::*;
use rusqlite::{params, Connection};
const TABLE:&str="CREATE TABLE IF NOT EXISTS transfer_restorations(id TEXT PRIMARY KEY,request_id TEXT NOT NULL UNIQUE,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=32768));";
fn valid(r: &Recovery) -> Result<(), String> {
    let uuid = |v: &str| uuid::Uuid::parse_str(v).is_ok();
    let path =
        |v: &str| ordinary(Path::new(v)) && v.len() <= 4096 && !v.chars().any(char::is_control);
    if !uuid(&r.id)
        || !uuid(&r.request_id)
        || r.id != r.draft.id
        || !uuid(&r.draft.run_id)
        || !uuid(&r.draft.item_id)
        || !path(&r.draft.copy_path)
        || !path(&r.draft.original_path)
        || !Path::new(&r.temporary_path).is_absolute()
        || r.temporary_path.len() > 4096
        || r.temporary_path.chars().any(char::is_control)
        || r.draft.copy_path == r.draft.original_path
        || Path::new(&r.temporary_path).parent() != Path::new(&r.draft.original_path).parent()
        || Path::new(&r.temporary_path)
            .file_name()
            .is_none_or(|s| s != format!(".moomole-restore-{}.tmp", r.id).as_str())
        || r.draft.created_at < 0
        || r.draft.expires_at - r.draft.created_at != 120_000
        || r.created_at < r.draft.created_at
        || r.finished_at.is_some_and(|t| t < r.created_at)
        || r.reviewed_at.is_some_and(|t| t < r.created_at)
        || r.draft.bytes > 16 * 1024 * 1024 * 1024
        || r.reason.len() > 4000
        || ![
            "running",
            "restored",
            "retained",
            "cancelled",
            "unknown",
            "interrupted",
        ]
        .contains(&r.state.as_str())
        || !["checking", "intent", "finished"].contains(&r.phase.as_str())
        || r.state == "restored" && r.receipt.is_none()
        || r.receipt
            .as_ref()
            .is_some_and(|p| p.destination != r.draft.original_path || p.inode == 0)
    {
        return Err("跨磁盘恢复记录无法核对，原记录保留。".into());
    }
    Ok(())
}
pub(super) fn list(c: &Connection) -> Result<Vec<Recovery>, String> {
    let exists:i64=c.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='transfer_restorations'",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if exists == 0 {
        return Ok(vec![]);
    }
    let mut q=c.prepare("SELECT id,request_id,CASE WHEN length(CAST(body AS BLOB))<=32768 THEN body END FROM transfer_restorations ORDER BY rowid DESC LIMIT 65").map_err(|e|e.to_string())?;
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = vec![];
    for row in rows {
        let (id, request, raw) = row.map_err(|e| e.to_string())?;
        let r: Recovery = serde_json::from_str(&raw.ok_or("恢复记录超过容量。")?)
            .map_err(|_| "恢复记录格式无法核对。")?;
        valid(&r)?;
        if r.id != id || r.request_id != request {
            return Err("恢复记录标识不一致。".into());
        }
        out.push(r);
    }
    if out.len() > 64 {
        return Err("恢复记录超过本轮容量。".into());
    }
    Ok(out)
}
pub(super) fn save(c: &Connection, r: &Recovery) -> Result<(), String> {
    valid(r)?;
    let full: i64 = c
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if full < 2 {
        return Err("恢复操作需要 FULL 同步记录。".into());
    }
    let body = serde_json::to_string(r).map_err(|e| e.to_string())?;
    if body.len() > 32768 {
        return Err("恢复记录超过容量。".into());
    }
    c.execute_batch(TABLE).map_err(|e| e.to_string())?;
    let n=c.execute("INSERT INTO transfer_restorations(id,request_id,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET body=excluded.body WHERE request_id=excluded.request_id",params![r.id,r.request_id,body]).map_err(|e|e.to_string())?;
    if n != 1 {
        return Err("恢复记录被其他请求占用。".into());
    }
    Ok(())
}
pub(super) fn admission(c: &Connection) -> Result<(), String> {
    let rows = list(c)?;
    if rows.len() >= 64
        || rows.iter().any(|r| {
            r.state == "running"
                || ["unknown", "interrupted"].contains(&r.state.as_str()) && r.reviewed_at.is_none()
        })
    {
        return Err("请先核对未完成的跨磁盘恢复记录；本轮最多保留 64 条。".into());
    }
    Ok(())
}
pub(super) fn begin(c: &mut db::Database, r: &Recovery) -> Result<(), String> {
    let tx = c
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    admission(&tx)?;
    crate::execution_admission::reserve(&tx, 32768)?;
    save(&tx, r)?;
    tx.commit().map_err(|e| e.to_string())
}
pub(super) fn reconcile(c: &Connection, active: &[String]) -> Result<Vec<Recovery>, String> {
    let mut out = list(c)?;
    for r in &mut out {
        if r.state == "running" && !active.contains(&r.request_id) {
            r.state = if r.phase == "intent" {
                "unknown"
            } else {
                "interrupted"
            }
            .into();
            r.finished_at = Some(db::now().max(r.created_at));
            r.reason = "先前恢复已中断，请核对原位置、副本及临时路径；不会自动继续或重试。".into();
            save(c, r)?;
        }
    }
    Ok(out)
}
