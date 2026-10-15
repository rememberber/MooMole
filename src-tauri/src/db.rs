use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub theme: String,
    pub retention_days: u32,
    pub protected_paths: Vec<String>,
    pub acknowledged: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            retention_days: 30,
            protected_paths: vec![],
            acknowledged: false,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: String,
    pub kind: String,
    pub path: Option<String>,
    pub state: String,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub message: Option<String>,
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub fn open(path: &Path) -> Result<Connection, String> {
    let mut connection = Connection::open(path).map_err(|e| e.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if version > 1 {
        return Err("数据库来自更新版本，请使用新版 MooMole 打开。".into());
    }
    if version == 0 {
        let migration = connection.transaction().map_err(|e| e.to_string())?;
        migration.execute_batch("CREATE TABLE settings (id INTEGER PRIMARY KEY CHECK(id = 1), value TEXT NOT NULL);
            CREATE TABLE tasks (id TEXT PRIMARY KEY, kind TEXT NOT NULL, path TEXT, state TEXT NOT NULL,
                created_at INTEGER NOT NULL, finished_at INTEGER, message TEXT);
            CREATE INDEX tasks_created ON tasks(created_at DESC);
            PRAGMA user_version = 1;").map_err(|e| e.to_string())?;
        migration.commit().map_err(|e| e.to_string())?;
    }
    connection.execute("UPDATE tasks SET state = 'interrupted', finished_at = ?1, message = '应用退出，任务未完成；需要重新检查。' WHERE state IN ('queued', 'running')", [now()]).map_err(|e| e.to_string())?;
    prune(&connection, settings(&connection)?.retention_days)?;
    Ok(connection)
}

pub fn settings(connection: &Connection) -> Result<Settings, String> {
    match connection.query_row::<String, _, _>(
        "SELECT value FROM settings WHERE id = 1",
        [],
        |row| row.get(0),
    ) {
        Ok(value) => serde_json::from_str(&value).map_err(|e| format!("设置无法读取：{e}")),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(Settings::default()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save_settings(connection: &Connection, settings: &Settings) -> Result<(), String> {
    if !["light", "dark", "system"].contains(&settings.theme.as_str())
        || ![7, 30, 90].contains(&settings.retention_days)
        || settings.protected_paths.len() > 100
    {
        return Err("设置内容无效。".into());
    }
    connection.execute("INSERT INTO settings VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET value = excluded.value", [serde_json::to_string(settings).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
    prune(connection, settings.retention_days)
}

fn prune(connection: &Connection, days: u32) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM tasks WHERE created_at < ?1 AND state NOT IN ('queued', 'running')",
            [now() - i64::from(days) * 86_400_000],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn start(
    connection: &Connection,
    id: &str,
    kind: &str,
    path: Option<&str>,
) -> Result<(), String> {
    connection.execute("INSERT INTO tasks (id, kind, path, state, created_at) VALUES (?1, ?2, ?3, 'running', ?4)", params![id, kind, path, now()]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn finish(connection: &Connection, id: &str, state: &str, message: &str) -> Result<(), String> {
    connection
        .execute(
            "UPDATE tasks SET state = ?2, message = ?3, finished_at = ?4 WHERE id = ?1",
            params![id, state, message, now()],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn history(connection: &Connection) -> Result<Vec<TaskRecord>, String> {
    let mut statement = connection.prepare("SELECT id, kind, path, state, created_at, finished_at, message FROM tasks ORDER BY created_at DESC LIMIT 200").map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(TaskRecord {
                id: row.get(0)?,
                kind: row.get(1)?,
                path: row.get(2)?,
                state: row.get(3)?,
                created_at: row.get(4)?,
                finished_at: row.get(5)?,
                message: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restart_recovers_running_tasks_without_changing_completed_tasks() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.sqlite");
        {
            let connection = open(&path).unwrap();
            start(&connection, "running", "analyze", Some("/tmp")).unwrap();
            start(&connection, "completed", "status", None).unwrap();
            finish(&connection, "completed", "succeeded", "done").unwrap();
            let value = Settings {
                acknowledged: true,
                ..Settings::default()
            };
            save_settings(&connection, &value).unwrap();
        }
        let connection = open(&path).unwrap();
        let tasks = history(&connection).unwrap();
        assert_eq!(
            tasks.iter().find(|t| t.id == "running").unwrap().state,
            "interrupted"
        );
        assert_eq!(
            tasks.iter().find(|t| t.id == "completed").unwrap().state,
            "succeeded"
        );
        assert!(settings(&connection).unwrap().acknowledged);
    }

    #[test]
    fn invalid_settings_do_not_overwrite_valid_settings() {
        let directory = tempfile::tempdir().unwrap();
        let connection = open(&directory.path().join("app.sqlite")).unwrap();
        let invalid = Settings {
            theme: "arbitrary".into(),
            ..Settings::default()
        };
        assert!(save_settings(&connection, &invalid).is_err());
        assert_eq!(settings(&connection).unwrap().theme, "system");
    }
}
