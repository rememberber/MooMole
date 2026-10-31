use super::*;
use rusqlite::{params, Connection};
pub(super) const TABLE:&str="CREATE TABLE IF NOT EXISTS folder_archives(id TEXT PRIMARY KEY,request_id TEXT NOT NULL UNIQUE,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=131072));";
fn valid(r: &Record) -> Result<(), String> {
    let id = |s: &str| uuid::Uuid::parse_str(s).is_ok();
    let hash = |s: &str| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    let d = &r.draft;
    let output = Path::new(&d.output_path);
    let parent = output.parent();
    if !id(&r.id)
        || r.id != d.id
        || !id(&r.request_id)
        || !ordinary(Path::new(&d.root))
        || !ordinary(output)
        || parent.is_none_or(|p| p.starts_with(&d.root))
        || output.extension().is_none_or(|s| s != "zip")
        || !hash(&d.source_sha256)
        || d.directories == 0
        || d.files.saturating_add(d.directories) > 10000
        || d.hidden > d.files.saturating_add(d.directories)
        || d.bytes > 8 * 1024 * 1024 * 1024
        || d.metadata_bytes > 256 * 1024 * 1024
        || d.bytes.saturating_add(d.metadata_bytes) > 8 * 1024 * 1024 * 1024
        || d.samples.len() > 50
        || d.samples.len() > d.files.saturating_add(d.directories)
        || d.samples.iter().any(|s| {
            s.name.len() > 4096
                || s.name.chars().any(char::is_control)
                || s.name.contains(['\\', ':'])
                || s.name
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == "..")
                || Path::new(&s.name)
                    .components()
                    .next()
                    .is_none_or(|p| Some(p.as_os_str()) != Path::new(&d.root).file_name())
        })
        || d.created_at < 0
        || d.expires_at.checked_sub(d.created_at) != Some(120_000)
        || r.created_at < d.created_at
        || r.finished_at.is_some_and(|t| t < r.created_at)
        || r.reviewed_at.is_some_and(|t| t < r.created_at)
        || r.reason.len() > 4000
        || Path::new(&r.temporary_path).parent() != parent
        || Path::new(&r.temporary_path)
            .file_name()
            .is_none_or(|s| s != format!(".MooMole-folder-archive-{}.partial", r.id).as_str())
        || ![
            "running",
            "completed",
            "retained",
            "cancelled",
            "unknown",
            "interrupted",
        ]
        .contains(&r.state.as_str())
        || !["building", "publishing", "finished"].contains(&r.phase.as_str())
        || r.state == "completed"
            && (r.artifact.is_none() || r.finished_at.is_none() || r.phase != "finished")
        || r.artifact.as_ref().is_some_and(|a| {
            !hash(&a.sha256)
                || a.bytes > 10 * 1024 * 1024 * 1024
                || a.bytes == 0
                || a.device.parse::<u64>().ok().is_none_or(|n| n == 0)
                || a.inode.parse::<u64>().ok().is_none_or(|n| n == 0)
        })
    {
        return Err("文件夹归档记录无法核对，保留原记录。".into());
    }
    Ok(())
}
pub(super) fn list(c: &Connection) -> Result<Vec<Record>, String> {
    let exists: i64 = c
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='folder_archives'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists == 0 {
        return Ok(vec![]);
    }
    let mut q=c.prepare("SELECT id,request_id,CASE WHEN length(CAST(body AS BLOB))<=131072 THEN body END FROM folder_archives ORDER BY rowid DESC LIMIT 65").map_err(|e|e.to_string())?;
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
        let r: Record = serde_json::from_str(&raw.ok_or("文件夹归档记录超过容量。")?)
            .map_err(|_| "文件夹归档记录格式无法核对。")?;
        valid(&r)?;
        if r.id != id || r.request_id != request {
            return Err("文件夹归档记录标识不一致。".into());
        }
        out.push(r);
    }
    if out.len() > 64 {
        return Err("文件夹归档记录超过本轮 64 条容量。".into());
    }
    Ok(out)
}
pub(super) fn save(c: &Connection, r: &Record) -> Result<(), String> {
    valid(r)?;
    let full: i64 = c
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if full < 2 {
        return Err("整份文件夹归档需要 FULL 同步记录。".into());
    }
    let raw = serde_json::to_string(r).map_err(|e| e.to_string())?;
    if raw.len() > 131072 {
        return Err("文件夹归档记录超过容量。".into());
    }
    c.execute_batch(TABLE).map_err(|e| e.to_string())?;
    let n=c.execute("INSERT INTO folder_archives(id,request_id,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET body=excluded.body WHERE request_id=excluded.request_id",params![r.id,r.request_id,raw]).map_err(|e|e.to_string())?;
    if n != 1 {
        return Err("文件夹归档记录被其他请求占用。".into());
    }
    Ok(())
}
pub(in crate::personal_files) fn admission(c: &Connection) -> Result<(), String> {
    let rows = list(c)?;
    if rows.len() >= 64
        || rows.iter().any(|r| {
            r.state == "running"
                || ["unknown", "interrupted"].contains(&r.state.as_str()) && r.reviewed_at.is_none()
        })
    {
        return Err("请先核对未完成的文件夹归档；本轮最多保留 64 条记录。".into());
    }
    Ok(())
}
pub(super) fn begin(c: &mut db::Database, r: &Record) -> Result<(), String> {
    let t = c
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    admission(&t)?;
    archive::journal::admission(&t)?;
    crate::execution_admission::reserve(&t, 131072)?;
    save(&t, r)?;
    t.commit().map_err(|e| e.to_string())
}
pub(super) fn reconcile(c: &Connection, active: &[String]) -> Result<Vec<Record>, String> {
    let mut rows = list(c)?;
    for r in &mut rows {
        if r.state == "running" && !active.contains(&r.request_id) {
            r.state = if r.phase == "publishing" {
                "unknown"
            } else {
                "interrupted"
            }
            .into();
            r.finished_at = Some(db::now().max(r.created_at));
            r.reason =
                "先前文件夹归档已中断，请核对目标和临时路径；不会自动发布、删除或重放。".into();
            save(c, r)?;
        }
    }
    Ok(rows)
}
