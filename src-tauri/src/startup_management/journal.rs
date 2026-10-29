use super::*;
use rusqlite::{params, Connection};
fn exists(db: &Connection) -> Result<bool, String> {
    db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='startup_management')",[],|r|r.get(0)).map_err(|e|e.to_string())
}
pub(super) fn list(db: &Connection) -> Result<Vec<Record>, String> {
    if !exists(db)? {
        return Ok(vec![]);
    }
    let mut s=db.prepare("SELECT id,state,CASE WHEN length(CAST(body AS BLOB))<=524288 THEN body END FROM startup_management ORDER BY rowid DESC LIMIT 65").map_err(|e|e.to_string())?;
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
    for row in rows {
        let (id, state, body) = row.map_err(|e| e.to_string())?;
        let r: Record = serde_json::from_str(&body.ok_or("启动管理记录超过读取范围。")?)
            .map_err(|_| "启动管理记录格式无法核对。")?;
        if r.id != id || r.state != state || uuid::Uuid::parse_str(&id).is_err() {
            return Err("启动管理记录不一致，保留原记录。".into());
        }
        out.push(r);
    }
    if out.len() > 64 {
        return Err("启动管理记录超过本轮容量，原数据保留。".into());
    }
    Ok(out)
}
pub(super) fn resume_proof(
    records: &[Record],
    path: &Path,
    witness: &Witness,
    sha256: &str,
) -> bool {
    let Some(active) = path.to_str().and_then(|s| s.strip_suffix(SUFFIX)) else {
        return false;
    };
    records
        .iter()
        .find(|r| r.configuration_changed && (r.path == active || r.target == active))
        .is_some_and(|r| {
            r.action == Action::Pause
                && r.target == path.to_string_lossy()
                && r.sha256 == sha256
                && r.moved_witness.as_ref() == Some(witness)
                && r.receipt.as_ref().is_some_and(|receipt| {
                    witness.nodes.last().is_some_and(|n| {
                        receipt.destination == r.target
                            && receipt.device == n.dev
                            && receipt.inode == n.ino
                    })
                })
        })
}
pub(super) fn insert(db: &mut db::Database, p: &Plan, r: &Record) -> Result<(), String> {
    let t = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    t.execute_batch("CREATE TABLE IF NOT EXISTS startup_management(id TEXT PRIMARY KEY,request_id TEXT NOT NULL UNIQUE,state TEXT NOT NULL,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=524288),snapshot TEXT NOT NULL CHECK(length(CAST(snapshot AS BLOB))<=1048576));").map_err(|e|e.to_string())?;
    let rows = list(&t)?;
    if rows.len() >= 64
        || rows.iter().any(|r| {
            r.state == "running"
                || (r.reviewed_at.is_none()
                    && matches!(r.state.as_str(), "partial" | "uncertain" | "interrupted"))
        })
    {
        return Err("启动管理存在未核对结果或记录已满，请先保留并核对历史。".into());
    }
    t.execute(
        "INSERT INTO startup_management VALUES(?1,?2,?3,?4,?5)",
        params![
            r.id,
            r.request_id,
            r.state,
            serde_json::to_string(r).map_err(|e| e.to_string())?,
            serde_json::to_string(p).map_err(|e| e.to_string())?
        ],
    )
    .map_err(|e| e.to_string())?;
    t.commit().map_err(|e| e.to_string())
}
pub(super) fn save(db: &Connection, r: &Record) -> Result<(), String> {
    if db
        .execute(
            "UPDATE startup_management SET state=?2,body=?3 WHERE id=?1 AND state='running'",
            params![
                r.id,
                r.state,
                serde_json::to_string(r).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?
        != 1
    {
        return Err("启动管理结果未完整保存，请核对原文件与记录，不要重复操作。".into());
    }
    Ok(())
}
pub(super) fn acknowledge(db: &Connection, id: &str) -> Result<Record, String> {
    let mut r = list(db)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("启动管理记录不存在。")?;
    if !matches!(r.state.as_str(), "partial" | "uncertain" | "interrupted") {
        return Err("此记录不需要未知结果核对。".into());
    }
    r.reviewed_at = Some(db::now());
    db.execute("UPDATE startup_management SET body=?2 WHERE id=?1 AND state IN ('partial','uncertain','interrupted')",params![id,serde_json::to_string(&r).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    Ok(r)
}
pub(crate) fn reconcile(db: &mut Connection) -> Result<(), String> {
    for mut r in list(db)?.into_iter().filter(|r| r.state == "running") {
        r.state = "interrupted".into();
        r.finished_at = Some(db::now());
        r.reason =
            "上次启动管理曾中断；请核对配置两处路径和后台状态，不会自动恢复、继续或重试。".into();
        save(db, &r)?;
    }
    Ok(())
}
