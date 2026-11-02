use super::*;
use rusqlite::{params, Connection};
const TABLE:&str="CREATE TABLE IF NOT EXISTS managed_app_runs(id TEXT PRIMARY KEY,request_id TEXT NOT NULL UNIQUE,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=32768));";
fn valid(r: &Record) -> Result<(), String> {
    let uuid = |s: &str| uuid::Uuid::parse_str(s).is_ok();
    let clean =
        |s: &str, max: usize| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control);
    if !uuid(&r.id)
        || r.id != r.draft.id
        || !uuid(&r.request_id)
        || !uuid(&r.draft.task_id)
        || !valid_token(&r.draft.token)
        || !crate::uninstall_plan::absolute(&r.draft.path)
        || r.created_at < 0
        || r.draft.created_at < 0
        || r.created_at < r.draft.created_at
        || !clean(&r.draft.name, 1024)
        || !clean(&r.draft.bundle_id, 255)
        || !clean(&r.draft.installed_version, 128)
        || ![
            "/opt/homebrew/bin/brew",
            "/usr/local/bin/brew",
            "/usr/local/Homebrew/bin/brew",
        ]
        .contains(&r.draft.manager_path.as_str())
        || (r.state == "running"
            && (r.finished_at.is_some() || r.reviewed_at.is_some() || r.phase == "finished"))
        || (r.state != "running" && r.finished_at.is_none())
        || r.draft.expires_at.checked_sub(r.draft.created_at) != Some(120000)
        || r.reason.len() > 4000
        || !crate::uninstall_plan::hash(&r.draft.metadata_sha256)
        || r.finished_at.is_some_and(|t| t < r.created_at)
        || r.reviewed_at.is_some_and(|t| t < r.created_at)
        || ![
            "running",
            "removed",
            "retained",
            "cancelled",
            "unknown",
            "interrupted",
        ]
        .contains(&r.state.as_str())
        || !["checking", "intent", "finished"].contains(&r.phase.as_str())
    {
        return Err("Homebrew 操作记录无法核对，保留记录。".into());
    }
    Ok(())
}
pub(super) fn list(c: &Connection) -> Result<Vec<Record>, String> {
    let exists:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='managed_app_runs' AND type='table')",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if !exists {
        return Ok(vec![]);
    }
    let mut q=c.prepare("SELECT id,request_id,CASE WHEN length(CAST(body AS BLOB))<=32768 THEN body END FROM managed_app_runs ORDER BY rowid DESC LIMIT 65").map_err(|e|e.to_string())?;
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
        let r: Record = serde_json::from_str(&body.ok_or("Homebrew 记录超限。")?)
            .map_err(|_| "Homebrew 记录格式无效。")?;
        valid(&r)?;
        if r.id != id || r.request_id != request {
            return Err("Homebrew 记录标识不符。".into());
        }
        out.push(r)
    }
    if out.len() > 64 {
        return Err("Homebrew 操作记录超过容量。".into());
    }
    Ok(out)
}
pub(crate) fn reservations(c: &Connection) -> Result<(i64, i64), String> {
    let rows = list(c)?;
    Ok((
        rows.len() as i64,
        rows.iter()
            .filter(|r| {
                r.state == "running"
                    || (["unknown", "interrupted"].contains(&r.state.as_str())
                        && r.reviewed_at.is_none())
            })
            .count() as i64,
    ))
}
pub(super) fn save(c: &Connection, r: &Record) -> Result<(), String> {
    valid(r)?;
    let sync: i64 = c
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if sync < 2 {
        return Err("Homebrew 操作需要 FULL 同步记录。".into());
    }
    let body = serde_json::to_string(r).map_err(|e| e.to_string())?;
    if body.len() > 32768 {
        return Err("Homebrew 记录容量不足。".into());
    }
    c.execute_batch(TABLE).map_err(|e| e.to_string())?;
    let count=c.execute("INSERT INTO managed_app_runs(id,request_id,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET body=excluded.body WHERE request_id=excluded.request_id",params![r.id,r.request_id,body]).map_err(|e|e.to_string())?;
    if count != 1 {
        return Err("Homebrew 记录已占用。".into());
    }
    Ok(())
}
pub(super) fn begin(c: &mut Connection, r: &Record) -> Result<(), String> {
    let tx = c
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    crate::execution_admission::reserve(&tx, 32768)?;
    save(&tx, r)?;
    tx.commit().map_err(|e| e.to_string())
}
pub(super) fn reconcile(
    c: &Connection,
    active: &[String],
    observed: &[String],
) -> Result<Vec<Record>, String> {
    let mut rows = list(c)?;
    for r in &mut rows {
        if r.state == "running" && observed.contains(&r.id) && !active.contains(&r.request_id) {
            r.state = if r.phase == "intent" {
                "unknown"
            } else {
                "interrupted"
            }
            .into();
            r.finished_at = Some(db::now().max(r.created_at));
            r.reason =
                "先前 Homebrew 任务已中断；可能部分完成，请核对 App 及包管理器状态，不会自动重放。"
                    .into();
            save(c, r)?
        }
    }
    Ok(rows)
}
