//! Separate create-only journal. No saved record grants execution authority.
use super::*;
use rusqlite::{params, Connection};
fn exists(db: &Connection) -> Result<bool, String> {
    db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='personal_archives')", [], |r| r.get(0)).map_err(|e| e.to_string())
}
pub(super) fn list(db: &Connection) -> Result<Vec<Record>, String> {
    if !exists(db)? {
        return Ok(vec![]);
    }
    let mut query = db.prepare("SELECT id,state,CASE WHEN length(CAST(body AS BLOB))<=131072 THEN body END FROM personal_archives ORDER BY rowid DESC LIMIT 65").map_err(|e| e.to_string())?;
    let rows = query
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
        let (id, state, raw) = row.map_err(|e| e.to_string())?;
        let record: Record = serde_json::from_str(&raw.ok_or("文件归档记录超过读取范围。")?)
            .map_err(|_| "文件归档记录无法核对，保留原数据。")?;
        if record.id != id
            || record.state != state
            || record.entries.len() > 20
            || uuid::Uuid::parse_str(&id).is_err()
        {
            return Err("文件归档记录不一致，原数据保留。".into());
        }
        out.push(record);
    }
    if out.len() > 64 {
        return Err("文件归档记录超过本轮容量，原记录保留。".into());
    }
    Ok(out)
}
pub(super) fn insert(db: &mut db::Database, plan: &Plan, record: &Record) -> Result<(), String> {
    let t = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    t.execute_batch("CREATE TABLE IF NOT EXISTS personal_archives(id TEXT PRIMARY KEY, request_id TEXT NOT NULL UNIQUE, state TEXT NOT NULL, body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=131072), snapshot TEXT NOT NULL CHECK(length(CAST(snapshot AS BLOB))<=524288));").map_err(|e| e.to_string())?;
    let records = list(&t)?;
    if records.len() >= 64
        || records.iter().any(|r| {
            r.state == "running"
                || (r.reviewed_at.is_none()
                    && matches!(r.state.as_str(), "uncertain" | "interrupted"))
        })
    {
        return Err(
            "文件归档记录已满或存在待核实结果，请保留并核对原记录；未开始新的归档。".into(),
        );
    }
    t.execute(
        "INSERT INTO personal_archives VALUES(?1,?2,?3,?4,?5)",
        params![
            record.id,
            record.request_id,
            record.state,
            serde_json::to_string(record).map_err(|e| e.to_string())?,
            serde_json::to_string(plan).map_err(|e| e.to_string())?
        ],
    )
    .map_err(|e| e.to_string())?;
    t.commit().map_err(|e| e.to_string())
}
pub(super) fn acknowledge(db: &Connection, id: &str) -> Result<Record, String> {
    let mut record = list(db)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("归档记录不存在。")?;
    if !matches!(record.state.as_str(), "uncertain" | "interrupted") {
        return Err("此记录不需要未知结果核对。".into());
    }
    record.reviewed_at = Some(db::now());
    db.execute(
        "UPDATE personal_archives SET body=?2 WHERE id=?1 AND state IN ('uncertain','interrupted')",
        params![
            id,
            serde_json::to_string(&record).map_err(|e| e.to_string())?
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(record)
}
pub(super) fn save(db: &Connection, record: &Record) -> Result<(), String> {
    if db
        .execute(
            "UPDATE personal_archives SET state=?2,body=?3 WHERE id=?1 AND state='running'",
            params![
                record.id,
                record.state,
                serde_json::to_string(record).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?
        != 1
    {
        return Err("归档结果未能持久保存，请保留目标与历史核对；不要重复执行。".into());
    }
    Ok(())
}
pub(crate) fn reconcile(db: &mut Connection) -> Result<(), String> {
    if !exists(db)? {
        return Ok(());
    }
    for mut r in list(db)?.into_iter().filter(|r| r.state == "running") {
        r.state = "interrupted".into();
        r.finished_at = Some(db::now());
        r.reason =
            "应用曾中断；目标或临时归档可能存在，请核对，原文件保留。不会自动发布、删除或重放。"
                .into();
        for item in &mut r.entries {
            item.state = "uncertain".into();
        }
        save(db, &r)?;
    }
    Ok(())
}
