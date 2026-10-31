use super::*;
use rusqlite::{params, Connection};
pub(super) const TABLE:&str="CREATE TABLE IF NOT EXISTS application_update_restorations(id TEXT PRIMARY KEY,request_id TEXT NOT NULL UNIQUE,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=32768));";
fn valid(r: &Record) -> Result<(), String> {
    let uuid = |s: &str| uuid::Uuid::parse_str(s).is_ok();
    let path = |s: &str| {
        Path::new(s).is_absolute()
            && s.len() <= 4096
            && !s.chars().any(char::is_control)
            && !s.split('/').any(|p| p == "." || p == "..")
    };
    let p = Path::new(&r.draft.path);
    let b = Path::new(&r.draft.retained);
    let hash = |s: &str| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if !uuid(&r.id)
        || r.id != r.draft.id
        || !uuid(&r.request_id)
        || !uuid(&r.draft.source_id)
        || r.id == r.draft.source_id
        || !path(&r.draft.path)
        || !path(&r.draft.retained)
        || p.extension().is_none_or(|s| s != "app")
        || b.file_name() != p.file_name()
        || b.parent().and_then(Path::parent) != p.parent()
        || b.parent()
            .and_then(Path::file_name)
            .is_none_or(|s| s != format!(".moomole-app-update-{}", r.draft.source_id).as_str())
        || r.created_at < r.draft.created_at
        || r.draft.created_at < 0
        || r.draft.expires_at - r.draft.created_at != 120_000
        || r.draft.bytes > 8 * 1024 * 1024 * 1024
        || r.finished_at.is_some_and(|t| t < r.created_at)
        || r.reviewed_at.is_some_and(|t| t < r.created_at)
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
        || r.state == "restored"
            && (r.receipt.is_none() || r.phase != "finished" || r.finished_at.is_none())
        || r.receipt.as_ref().is_some_and(|p| {
            !hash(&p.installed_hash)
                || !hash(&p.retained_hash)
                || [
                    &p.installed_device,
                    &p.installed_inode,
                    &p.retained_device,
                    &p.retained_inode,
                ]
                .iter()
                .any(|v| v.parse::<u64>().ok().is_none_or(|n| n == 0))
        })
    {
        return Err("应用旧版恢复记录无法核对，原记录保留。".into());
    }
    Ok(())
}
pub(super) fn list(c: &Connection) -> Result<Vec<Record>, String> {
    let exists:i64=c.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='application_update_restorations'",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if exists == 0 {
        return Ok(vec![]);
    }
    let mut q=c.prepare("SELECT id,request_id,CASE WHEN length(CAST(body AS BLOB))<=32768 THEN body END FROM application_update_restorations ORDER BY rowid DESC LIMIT 65").map_err(|e|e.to_string())?;
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
        let r: Record = serde_json::from_str(&body.ok_or("旧版恢复记录超过容量。")?)
            .map_err(|_| "旧版恢复记录格式无法核对。")?;
        valid(&r)?;
        if r.id != id || r.request_id != request {
            return Err("旧版恢复记录标识不一致。".into());
        }
        out.push(r);
    }
    if out.len() > 64 {
        return Err("旧版恢复记录超过本轮容量。".into());
    }
    Ok(out)
}
pub(super) fn save(c: &Connection, r: &Record) -> Result<(), String> {
    valid(r)?;
    let full: i64 = c
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if full < 2 {
        return Err("应用旧版恢复需要 FULL 同步记录。".into());
    }
    let body = serde_json::to_string(r).map_err(|e| e.to_string())?;
    if body.len() > 32768 {
        return Err("旧版恢复记录超过容量。".into());
    }
    c.execute_batch(TABLE).map_err(|e| e.to_string())?;
    let n=c.execute("INSERT INTO application_update_restorations(id,request_id,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET body=excluded.body WHERE request_id=excluded.request_id",params![r.id,r.request_id,body]).map_err(|e|e.to_string())?;
    if n != 1 {
        return Err("旧版恢复记录已被其他请求占用。".into());
    }
    Ok(())
}
pub(in crate::application_update) fn admission(c: &Connection) -> Result<(), String> {
    let rows = list(c)?;
    if rows.len() >= 64
        || rows.iter().any(|r| {
            r.state == "running"
                || ["unknown", "interrupted"].contains(&r.state.as_str()) && r.reviewed_at.is_none()
        })
    {
        return Err("请先核对未完成的旧版恢复记录；本轮最多保留 64 条。".into());
    }
    Ok(())
}
pub(super) fn begin(c: &mut Connection, r: &Record) -> Result<(), String> {
    let tx = c
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    super::super::journal::admission(&tx)?;
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
            r.reason =
                "先前旧版恢复已中断，请检查安装位置与保留包；不会自动重放或反向交换。".into();
            save(c, r)?;
        }
    }
    Ok(rows)
}
