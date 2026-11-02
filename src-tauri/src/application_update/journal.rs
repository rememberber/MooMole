use super::*;
use rusqlite::{params, Connection};
const TABLE:&str="CREATE TABLE IF NOT EXISTS application_update_runs(id TEXT PRIMARY KEY,request_id TEXT NOT NULL UNIQUE,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=32768));";
fn valid(r: &Record) -> Result<(), String> {
    let uuid = |v: &str| uuid::Uuid::parse_str(v).is_ok();
    let abs = |v: &str| {
        Path::new(v).is_absolute()
            && v.len() <= 4096
            && !v.chars().any(char::is_control)
            && !v.split('/').any(|p| p == "." || p == "..")
    };
    let path = Path::new(&r.draft.path);
    let stage = Path::new(&r.backup).parent();
    if !uuid(&r.id)
        || !uuid(&r.request_id)
        || r.id != r.draft.id
        || !uuid(&r.draft.task_id)
        || !abs(&r.draft.path)
        || !abs(&r.draft.replacement)
        || !abs(&r.backup)
        || stage.and_then(Path::parent) != path.parent()
        || Path::new(&r.backup).file_name() != path.file_name()
        || stage
            .and_then(Path::file_name)
            .is_none_or(|n| n != format!(".moomole-app-update-{}", r.id).as_str())
        || r.created_at < 0
        || r.draft.created_at < 0
        || r.draft.expires_at - r.draft.created_at != 120_000
        || r.finished_at.is_some_and(|t| t < r.created_at)
        || r.reviewed_at.is_some_and(|t| t < r.created_at)
        || r.reason.len() > 4000
        || r.draft.bytes > 8 * 1024 * 1024 * 1024
        || ![
            "running",
            "updated",
            "retained",
            "cancelled",
            "unknown",
            "interrupted",
        ]
        .contains(&r.state.as_str())
        || !["checking", "copying", "intent", "finished"].contains(&r.phase.as_str())
    {
        return Err("应用更新记录无法核对，原记录保留。".into());
    }
    Ok(())
}
pub(super) fn list(c: &Connection) -> Result<Vec<Record>, String> {
    let exists:i64=c.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='application_update_runs'",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if exists == 0 {
        return Ok(vec![]);
    }
    let mut q=c.prepare("SELECT id,request_id,CASE WHEN length(CAST(body AS BLOB))<=32768 THEN body END FROM application_update_runs ORDER BY rowid DESC LIMIT 65").map_err(|e|e.to_string())?;
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
        let (id, request, body) = row.map_err(|e| e.to_string())?;
        let r: Record = serde_json::from_str(&body.ok_or("应用更新记录超过容量。")?)
            .map_err(|_| "应用更新记录格式无法核对。")?;
        valid(&r)?;
        if r.id != id || r.request_id != request {
            return Err("应用更新记录标识不一致。".into());
        }
        out.push(r);
    }
    if out.len() > 64 {
        return Err("应用更新记录超过本轮容量，原记录保留。".into());
    }
    Ok(out)
}
pub(super) fn save(c: &Connection, r: &Record) -> Result<(), String> {
    valid(r)?;
    let full: i64 = c
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if full < 2 {
        return Err("应用更新需要 FULL 同步记录。".into());
    }
    let body = serde_json::to_string(r).map_err(|e| e.to_string())?;
    if body.len() > 32768 {
        return Err("应用更新记录超过容量。".into());
    }
    c.execute_batch(TABLE).map_err(|e| e.to_string())?;
    let n=c.execute("INSERT INTO application_update_runs(id,request_id,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET body=excluded.body WHERE request_id=excluded.request_id",params![r.id,r.request_id,body]).map_err(|e|e.to_string())?;
    if n != 1 {
        return Err("应用更新记录已被占用。".into());
    }
    Ok(())
}
pub(super) fn admission(c: &Connection) -> Result<(), String> {
    super::retirement::journal::admission(c)?;
    recovery::journal::admission(c)?;
    let rows = list(c)?;
    if rows.len() >= 64
        || rows.iter().any(|r| {
            r.state == "running"
                || ["unknown", "interrupted"].contains(&r.state.as_str()) && r.reviewed_at.is_none()
        })
    {
        return Err("请先核对未完成的应用更新记录；本轮最多保留 64 条，不自动删除旧版。".into());
    }
    Ok(())
}
pub(super) fn begin(c: &mut Connection, r: &Record) -> Result<(), String> {
    let tx = c
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    admission(&tx)?;
    crate::execution_admission::reserve(&tx, 32768)?;
    save(&tx, r)?;
    tx.commit().map_err(|e| e.to_string())
}
pub(super) fn reconcile(c: &Connection, active: &[String]) -> Result<Vec<Record>, String> {
    let mut rows = list(c)?;
    for r in &mut rows {
        if r.state == "running" && !active.contains(&r.request_id) {
            r.state = if r.phase == "intent" {
                "unknown"
            } else {
                "interrupted"
            }
            .into();
            r.finished_at = Some(db::now().max(r.created_at));
            r.reason = "先前更新已中断；请核对安装位置和暂存/旧版位置，不会自动重放或回滚。".into();
            save(c, r)?;
        }
    }
    Ok(rows)
}
