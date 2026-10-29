//! v8 expands the journal's operation kind without rewriting saved evidence.
use rusqlite::Connection;

pub(crate) fn upgrade(db: &Connection) -> Result<(), String> {
    // Refuse to silently discard custom triggers/indexes during table rebuild.
    let custom: i64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE tbl_name='execution_runs' AND (type='trigger' OR (type='index' AND sql IS NOT NULL AND name!='execution_one_running'))", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if custom != 0 {
        return Err("执行表含非预期索引或触发器，保留原数据库与升级前备份。".into());
    }
    db.execute_batch("CREATE TABLE execution_runs_v8 (
        id TEXT PRIMARY KEY, plan_id TEXT NOT NULL UNIQUE,
        method TEXT NOT NULL CHECK(method IN ('trash','move')),
        state TEXT NOT NULL CHECK(state IN ('running','completed','partial','noChanges','cancelled','interrupted')),
        created_at INTEGER NOT NULL, finished_at INTEGER,
        snapshot TEXT NOT NULL, snapshot_sha256 TEXT NOT NULL, reserved_bytes INTEGER NOT NULL
    );
    INSERT INTO execution_runs_v8 SELECT * FROM execution_runs;
    DROP TABLE execution_runs;
    ALTER TABLE execution_runs_v8 RENAME TO execution_runs;
    CREATE UNIQUE INDEX execution_one_running ON execution_runs(state) WHERE state='running';
    PRAGMA user_version=8;").map_err(|e| format!("执行记录升级失败，保留升级前备份：{e}"))?;
    let valid = db
        .prepare("PRAGMA foreign_key_check")
        .map_err(|e| e.to_string())?
        .query([])
        .map_err(|e| e.to_string())?
        .next()
        .map_err(|e| e.to_string())?
        .is_none();
    if !valid {
        return Err("执行记录升级后的外键核对失败，停止升级。".into());
    }
    Ok(())
}
