use super::*;
pub(super) const TABLE:&str="CREATE TABLE IF NOT EXISTS archive_retirements(id TEXT PRIMARY KEY,request_id TEXT NOT NULL UNIQUE,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=32768));";
fn valid(r: &Record) -> Result<(), String> {
    let d = &r.draft;
    valid_id(&r.id)?;
    valid_id(&r.request_id)?;
    valid_id(&d.header.id)?;
    if r.id != d.id
        || d.expires_at - d.created_at != 120000
        || d.created_at < 0
        || r.created_at < d.created_at
        || r.finished_at.is_some_and(|t| t < r.created_at)
        || r.reviewed_at.is_some_and(|t| t < r.created_at)
        || d.bytes == 0
        || d.bytes > (MAX_BODY + MAX_HEADER) as u64
        || d.sha256.len() != 64
        || !d.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || d.path == d.exported
        || Path::new(&d.path)
            .file_name()
            .is_none_or(|s| s != format!("{}.moomole-history", d.header.id).as_str())
        || !Path::new(&d.path).is_absolute()
        || !Path::new(&d.exported).is_absolute()
        || r.reason.len() > 4096
        || !["running", "moved", "retained", "cancelled", "unknown"].contains(&r.state.as_str())
        || !["checking", "intent", "finished"].contains(&r.phase.as_str())
        || (r.state == "running") != (r.finished_at.is_none())
        || (r.state == "moved" && r.receipt.is_none())
        || r.receipt.as_ref().is_some_and(|p| {
            !Path::new(&p.destination).is_absolute() || p.destination.len() > 4096 || p.inode == 0
        })
    {
        return Err("归档副本整理记录无法核对，原记录保留。".into());
    }
    Ok(())
}
pub(super) fn list(c: &Connection) -> Result<Vec<Record>, String> {
    let exists: u32 = c
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='archive_retirements'",
            [],
            |r| r.get(0),
        )
        .map_err(err)?;
    if exists == 0 {
        return Ok(vec![]);
    }
    let mut q=c.prepare("SELECT id,request_id,CASE WHEN length(CAST(body AS BLOB))<=32768 THEN body END FROM archive_retirements ORDER BY rowid DESC LIMIT 65").map_err(err)?;
    let mut out = vec![];
    for row in q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(err)?
    {
        let (id, request, body) = row.map_err(err)?;
        let r: Record = serde_json::from_str(&body.ok_or("归档副本记录过大。")?).map_err(err)?;
        valid(&r)?;
        if r.id != id || r.request_id != request {
            return Err("归档副本记录标识不符。".into());
        }
        out.push(r);
    }
    if out.len() > 64 {
        return Err("归档副本整理记录已达容量，请保留记录。".into());
    }
    Ok(out)
}
pub(super) fn save(c: &Connection, r: &Record) -> Result<(), String> {
    valid(r)?;
    let full: u32 = c
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(err)?;
    if full < 2 {
        return Err("归档副本整理需要 FULL 同步记录。".into());
    }
    let body = serde_json::to_string(r).map_err(err)?;
    if body.len() > 32768 {
        return Err("归档副本记录过大。".into());
    }
    c.execute_batch(TABLE).map_err(err)?;
    if c.execute("INSERT INTO archive_retirements(id,request_id,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET body=excluded.body WHERE request_id=excluded.request_id",rusqlite::params![r.id,r.request_id,body]).map_err(err)?!=1{return Err("归档副本记录已被占用。".into())}
    Ok(())
}
pub(super) fn admission(c: &Connection) -> Result<(), String> {
    let rows = list(c)?;
    if rows.len() >= 64
        || rows
            .iter()
            .any(|r| r.state == "running" || r.state == "unknown" && r.reviewed_at.is_none())
    {
        Err("请先核查归档副本整理的进行中或未知结果。".into())
    } else {
        Ok(())
    }
}
pub(super) fn begin(c: &mut Connection, r: &Record) -> Result<(), String> {
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)?;
    admission(&tx)?;
    if list(&tx)?
        .iter()
        .any(|old| old.id == r.id || old.request_id == r.request_id)
    {
        return Err("归档副本确认已经使用。".into());
    }
    save(&tx, r)?;
    tx.commit().map_err(err)
}
pub(crate) fn reconcile(c: &Connection, active: &[String]) -> Result<Vec<Record>, String> {
    let mut rows = list(c)?;
    for r in &mut rows {
        if r.state == "running" && !active.contains(&r.request_id) {
            r.state = "unknown".into();
            r.phase = "finished".into();
            r.finished_at = Some(db::now().max(r.created_at));
            r.reason =
                "上次归档副本整理已中断，请核对本机副本、导出副本和废纸篓；不会重放。".into();
            save(c, r)?;
        }
    }
    Ok(rows)
}
