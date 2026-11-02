use super::*;
use rusqlite::{params, Connection};
pub(super) const TABLE:&str="CREATE TABLE IF NOT EXISTS application_retained_retirements(id TEXT PRIMARY KEY,request_id TEXT NOT NULL UNIQUE,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=32768));";
fn valid(r: &Record) -> Result<(), String> {
    let d = &r.draft;
    let id = |s: &str| uuid::Uuid::parse_str(s).is_ok_and(|v| v.to_string() == s);
    let abs = |s: &str| {
        Path::new(s).is_absolute()
            && s.len() <= 4096
            && !s.chars().any(char::is_control)
            && !s.split('/').any(|p| p == "." || p == "..")
    };
    if !id(&r.id)
        || !id(&r.request_id)
        || !id(&d.source_id)
        || r.id != d.id
        || d.id == d.source_id
        || ![&d.installed, &d.retained, &d.stage].iter().all(|s| abs(s))
        || Path::new(&d.retained).parent() != Some(Path::new(&d.stage))
        || Path::new(&d.stage).parent() != Path::new(&d.installed).parent()
        || Path::new(&d.retained).file_name() != Path::new(&d.installed).file_name()
        || Path::new(&d.stage)
            .file_name()
            .is_none_or(|n| n != format!(".moomole-app-update-{}", d.source_id).as_str())
        || d.expires_at.checked_sub(d.created_at) != Some(120000)
        || d.created_at < 0
        || r.created_at < d.created_at
        || r.finished_at.is_some_and(|t| t < r.created_at)
        || r.reviewed_at.is_some_and(|t| t < r.created_at)
        || d.bytes > 8 * 1024 * 1024 * 1024
        || r.reason.len() > 4000
        || !["running", "moved", "retained", "cancelled", "unknown"].contains(&r.state.as_str())
        || !["checking", "intent", "finished"].contains(&r.phase.as_str())
        || (r.state == "running") != r.finished_at.is_none()
        || (r.phase == "finished") != (r.state != "running")
        || r.state == "moved" && r.receipt.is_none()
        || r.stage_removed && r.state != "moved"
        || r.receipt
            .as_ref()
            .is_some_and(|s| !abs(&s.destination) || s.inode == 0)
    {
        return Err("保留版本整理记录无法核对，原记录保留。".into());
    }
    Ok(())
}
pub(super) fn list(c: &Connection) -> Result<Vec<Record>, String> {
    let exists:u32=c.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='application_retained_retirements'",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if exists == 0 {
        return Ok(vec![]);
    }
    let mut q=c.prepare("SELECT id,request_id,CASE WHEN length(CAST(body AS BLOB))<=32768 THEN body END FROM application_retained_retirements ORDER BY rowid DESC LIMIT 65").map_err(|e|e.to_string())?;
    let mut out = vec![];
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
        let r: Record =
            serde_json::from_str(&body.ok_or("保留版本记录过大。")?).map_err(|e| e.to_string())?;
        valid(&r)?;
        if r.id != id || r.request_id != request {
            return Err("保留版本记录标识不符。".into());
        }
        out.push(r);
    }
    if out.len() > 64 {
        return Err("保留版本整理记录达到容量，请保留记录核查。".into());
    }
    Ok(out)
}
pub(super) fn save(c: &Connection, r: &Record) -> Result<(), String> {
    valid(r)?;
    let full: u32 = c
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if full < 2 {
        return Err("保留版本整理需要 FULL 同步记录。".into());
    }
    let body = serde_json::to_string(r).map_err(|e| e.to_string())?;
    if body.len() > 32768 {
        return Err("保留版本记录过大。".into());
    }
    c.execute_batch(TABLE).map_err(|e| e.to_string())?;
    if c.execute("INSERT INTO application_retained_retirements(id,request_id,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET body=excluded.body WHERE request_id=excluded.request_id",params![r.id,r.request_id,body]).map_err(|e|e.to_string())?!=1{return Err("保留版本记录已被占用。".into())}
    Ok(())
}
pub(in crate::application_update) fn admission(c: &Connection) -> Result<(), String> {
    let rows = list(c)?;
    if rows.len() >= 64
        || rows
            .iter()
            .any(|r| r.state == "running" || r.state == "unknown" && r.reviewed_at.is_none())
    {
        Err("请先核查未完成或未知的保留版本整理结果。".into())
    } else {
        Ok(())
    }
}
pub(super) fn begin(c: &mut Connection, r: &Record) -> Result<(), String> {
    let tx = c
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    super::super::journal::admission(&tx)?;
    crate::execution_admission::reserve(&tx, 32768)?;
    block_source(&tx, &r.draft.source_id)?;
    if list(&tx)?
        .iter()
        .any(|v| v.id == r.id || v.request_id == r.request_id)
    {
        return Err("保留版本确认已使用。".into());
    }
    save(&tx, r)?;
    tx.commit().map_err(|e| e.to_string())
}
pub(in crate::application_update) fn reconcile(
    c: &Connection,
    active: &[String],
) -> Result<Vec<Record>, String> {
    let mut rows = list(c)?;
    for r in &mut rows {
        if r.state == "running" && !active.contains(&r.request_id) {
            r.state = "unknown".into();
            r.phase = "finished".into();
            r.finished_at = Some(db::now().max(r.created_at));
            r.reason = "保留版本整理已中断，请检查安装位置、保留目录与废纸篓；不会重放。".into();
            save(c, r)?
        }
    }
    Ok(rows)
}
