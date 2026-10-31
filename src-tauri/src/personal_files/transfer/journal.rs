use super::*;
use rusqlite::{params, Connection};
fn exists(db: &Connection) -> Result<bool, String> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='file_transfers')",
        [],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}
pub(super) fn list(db: &Connection) -> Result<Vec<Record>, String> {
    if !exists(db)? {
        return Ok(vec![]);
    }
    let mut s=db.prepare("SELECT id,state,CASE WHEN length(CAST(body AS BLOB))<=524288 THEN body END FROM file_transfers ORDER BY rowid DESC LIMIT 65").map_err(|e|e.to_string())?;
    let rows = s
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = vec![];
    for r in rows {
        let (id, state, body) = r.map_err(|e| e.to_string())?;
        let record: Record = serde_json::from_str(&body.ok_or("跨磁盘记录超过读取范围。")?)
            .map_err(|_| "跨磁盘记录无法核对。")?;
        if record.protocol != 1
            || record.revision != REVISION
            || record.id != id
            || record.state != state
            || record.items.len() > 20
            || uuid::Uuid::parse_str(&id).is_err()
        {
            return Err("跨磁盘记录不一致，请保留原记录。".into());
        }
        out.push(record);
    }
    if out.len() > 64 {
        return Err("跨磁盘移动记录超过本轮容量。".into());
    }
    Ok(out)
}
pub(super) fn snapshot(db: &Connection, id: &str) -> Result<Plan, String> {
    let raw:Option<String>=db.query_row("SELECT CASE WHEN length(CAST(snapshot AS BLOB))<=8388608 THEN snapshot END FROM file_transfers WHERE id=?1",[id],|r|r.get(0)).map_err(|_|"原跨磁盘计划不存在。")?;
    let plan: Plan = serde_json::from_str(&raw.ok_or("原跨磁盘计划超过范围。")?)
        .map_err(|_| "原跨磁盘计划无法核对。")?;
    if plan.protocol != 1 || plan.revision != REVISION || plan.source.id != id {
        return Err("原跨磁盘计划标识不一致。".into());
    }
    plan.source.validate_record()?;
    plan.movement.validate_record(&plan.source.items)?;
    Ok(plan)
}
pub(super) fn insert(db: &mut db::Database, plan: &Plan, r: &Record) -> Result<(), String> {
    let t = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    t.execute_batch("CREATE TABLE IF NOT EXISTS file_transfers(id TEXT PRIMARY KEY,request_id TEXT NOT NULL UNIQUE,state TEXT NOT NULL,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=524288),snapshot TEXT NOT NULL CHECK(length(CAST(snapshot AS BLOB))<=8388608));").map_err(|e|e.to_string())?;
    let rows = list(&t)?;
    if rows.len() >= 64
        || rows.iter().any(|r| {
            r.state == "running"
                || (r.reviewed_at.is_none()
                    && matches!(r.state.as_str(), "partial" | "uncertain" | "interrupted"))
        })
    {
        return Err("跨磁盘移动存在待核对结果或记录已满，请先核对历史。".into());
    }
    t.execute(
        "INSERT INTO file_transfers VALUES(?1,?2,?3,?4,?5)",
        params![
            r.id,
            r.request_id,
            r.state,
            serde_json::to_string(r).map_err(|e| e.to_string())?,
            serde_json::to_string(plan).map_err(|e| e.to_string())?
        ],
    )
    .map_err(|e| e.to_string())?;
    t.commit().map_err(|e| e.to_string())
}
pub(super) fn save(db: &Connection, r: &Record) -> Result<(), String> {
    if db
        .execute(
            "UPDATE file_transfers SET state=?2,body=?3 WHERE id=?1 AND state='running'",
            params![
                r.id,
                r.state,
                serde_json::to_string(r).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?
        != 1
    {
        return Err("跨磁盘移动结果未完整保存，请核对两端位置，不要重复操作。".into());
    }
    Ok(())
}
pub(super) fn acknowledge(db: &Connection, id: &str) -> Result<Record, String> {
    let mut r = list(db)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("跨磁盘移动记录不存在。")?;
    if !matches!(r.state.as_str(), "partial" | "uncertain" | "interrupted") {
        return Err("此记录不需要未知结果核对。".into());
    }
    r.reviewed_at = Some(db::now());
    db.execute("UPDATE file_transfers SET body=?2 WHERE id=?1 AND state IN ('partial','uncertain','interrupted')",params![id,serde_json::to_string(&r).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    Ok(r)
}
pub(crate) fn reconcile(db: &mut Connection) -> Result<(), String> {
    for r in list(db)?.into_iter().filter(|r| r.state == "running") {
        interrupt(db, &r.id)?;
    }
    Ok(())
}
pub(super) fn interrupt(db: &Connection, id: &str) -> Result<(), String> {
    // Re-read after checking live ownership, preserving any later saved receipt.
    if let Some(mut r) = list(db)?
        .into_iter()
        .find(|r| r.id == id && r.state == "running")
    {
        r.state = "interrupted".into();
        r.finished_at = Some(db::now());
        r.reason =
            "跨磁盘操作曾中断，请核对目标、原件和临时路径；不会自动清理、继续或重试。".into();
        for e in &mut r.items {
            if matches!(e.state.as_str(), "copying" | "copyVerified" | "trashIntent") {
                e.state = "uncertain".into();
            } else if e.state == "pending" {
                e.state = "notStarted".into();
            }
        }
        save(db, &r)?;
    }
    Ok(())
}
