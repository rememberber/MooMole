use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

// Field order matters: close SQLite before releasing directory ownership.
// AppState clones hold the whole handle, including during task shutdown.
#[derive(Debug)]
pub struct Database {
    connection: Connection,
    _owner: crate::instance::Lease,
}
impl std::ops::Deref for Database {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.connection
    }
}
impl std::ops::DerefMut for Database {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }
}

#[derive(Debug)]
pub enum OpenError {
    Instance(crate::instance::Error),
    Database(String),
}
impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Instance(error) => error.fmt(f),
            Self::Database(error) => f.write_str(error),
        }
    }
}

pub fn open_owned(path: &Path) -> Result<Database, OpenError> {
    let directory = path
        .parent()
        .filter(|p| p.is_absolute())
        .ok_or_else(|| OpenError::Database("应用数据路径必须是绝对路径。".into()))?;
    let owner = crate::instance::acquire(directory).map_err(OpenError::Instance)?;
    let connection = open(path).map_err(OpenError::Database)?;
    Ok(Database {
        connection,
        _owner: owner,
    })
}

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

// Raw initialization is private: production callers must own the data directory
// BEFORE opening SQLite, backing up, migrating, reconciling or pruning records.
fn open(path: &Path) -> Result<Connection, String> {
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
    if version > 6 {
        return Err("数据库来自更新版本，请使用新版 MooMole 打开。".into());
    }
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| e.to_string())?;
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(|e| e.to_string())?;
    connection
        .pragma_update(None, "fullfsync", "ON")
        .map_err(|e| e.to_string())?;
    connection
        .pragma_update(None, "checkpoint_fullfsync", "ON")
        .map_err(|e| e.to_string())?;
    if version > 0 && version < 6 {
        // Back up the original schema before ANY migration. VACUUM INTO includes WAL data.
        let backup =
            path.with_file_name(format!("moomole-before-v6-{}.sqlite", uuid::Uuid::new_v4()));
        connection
            .execute(
                "VACUUM INTO ?1",
                [backup.to_str().ok_or("数据库备份路径无法编码。")?],
            )
            .map_err(|e| format!("升级前备份失败，保留原数据库：{e}"))?;
    }
    if version < 6 {
        let migration = connection.transaction().map_err(|e| e.to_string())?;
        if version == 0 {
            migration.execute_batch("CREATE TABLE settings (id INTEGER PRIMARY KEY CHECK(id = 1), value TEXT NOT NULL);
                CREATE TABLE tasks (id TEXT PRIMARY KEY, kind TEXT NOT NULL, path TEXT, state TEXT NOT NULL,
                    created_at INTEGER NOT NULL, finished_at INTEGER, message TEXT);
                CREATE INDEX tasks_created ON tasks(created_at DESC);").map_err(|e| e.to_string())?;
        }
        if version < 2 {
            migration.execute_batch("CREATE TABLE clean_scans (id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, body TEXT NOT NULL);
                CREATE TABLE review_plans (id TEXT PRIMARY KEY, scan_id TEXT NOT NULL REFERENCES clean_scans(id) ON DELETE CASCADE,
                    created_at INTEGER NOT NULL, body TEXT NOT NULL);
                CREATE INDEX clean_scans_created ON clean_scans(created_at DESC);
                CREATE INDEX review_plans_created ON review_plans(created_at DESC);").map_err(|e| e.to_string())?;
        }
        if version < 3 {
            migration
                .execute_batch(crate::execution::SCHEMA)
                .map_err(|e| e.to_string())?;
        }
        if version < 4 {
            migration
                .execute_batch(crate::uninstall_journal::SCHEMA)
                .map_err(|e| e.to_string())?;
        }
        if version < 5 {
            migration
                .execute_batch(crate::maintenance::refresh_journal::SCHEMA)
                .map_err(|e| e.to_string())?;
        }
        migration
            .execute_batch(crate::maintenance::refresh_journal::REVIEW_SCHEMA)
            .map_err(|e| e.to_string())?;
        migration.commit().map_err(|e| e.to_string())?;
    }
    crate::execution::reconcile(&mut connection, now())?;
    crate::uninstall_journal::reconcile(&mut connection, now())?;
    crate::maintenance::refresh_journal::reconcile(&mut connection, now())?;
    connection.execute("UPDATE tasks SET state = 'interrupted', finished_at = ?1, message = '应用退出，任务未完成；需要重新检查。' WHERE state IN ('queued', 'running')", [now()]).map_err(|e| e.to_string())?;
    prune(&connection, settings(&connection)?.retention_days)?;
    Ok(connection)
}

// Fault-injection and independent-connection fixtures deliberately bypass the
// application instance boundary. This API is absent from production builds.
#[cfg(test)]
pub(crate) fn open_fixture(path: &Path) -> Result<Connection, String> {
    open(path)
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
    // Settings and every retention/cascade deletion form one operation. A later
    // failure must not persist a new preference or an earlier partial prune.
    let transaction = connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    transaction.execute("INSERT INTO settings VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET value = excluded.value", [serde_json::to_string(settings).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
    prune_in_transaction(&transaction, settings.retention_days)?;
    transaction.commit().map_err(|e| e.to_string())
}

fn prune(connection: &Connection, days: u32) -> Result<(), String> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    prune_in_transaction(&transaction, days)?;
    transaction.commit().map_err(|e| e.to_string())
}

fn prune_in_transaction(transaction: &rusqlite::Transaction<'_>, days: u32) -> Result<(), String> {
    let before = now() - i64::from(days) * 86_400_000;
    transaction
        .execute(
            "DELETE FROM tasks WHERE created_at < ?1 AND state NOT IN ('queued', 'running')",
            [before],
        )
        .map_err(|e| e.to_string())?;
    transaction
        .execute("DELETE FROM clean_scans WHERE created_at < ?1", [before])
        .map_err(|e| e.to_string())?;
    prune_reviews(transaction)?;
    crate::execution::prune(transaction, before)?;
    crate::uninstall_journal::prune(transaction, before)?;
    crate::maintenance::refresh_journal::prune(transaction, before)?;
    Ok(())
}

fn prune_reviews(connection: &Connection) -> Result<(), String> {
    connection.execute("DELETE FROM review_plans WHERE id NOT IN (SELECT id FROM review_plans ORDER BY created_at DESC, rowid DESC LIMIT 20)", []).map_err(|e| e.to_string())?;
    connection.execute("DELETE FROM clean_scans WHERE id NOT IN (SELECT id FROM clean_scans ORDER BY created_at DESC, rowid DESC LIMIT 10)", []).map_err(|e| e.to_string())?;
    Ok(())
}

fn review_body<T: Serialize>(value: &T) -> Result<String, String> {
    let body = serde_json::to_string(value).map_err(|e| e.to_string())?;
    if body.len() > 8 * 1024 * 1024 {
        return Err("审阅数据超过 8 MiB 存储上限。".into());
    }
    Ok(body)
}

pub fn save_scan(connection: &Connection, scan: &crate::review::Scan) -> Result<(), String> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    transaction
        .execute(
            "INSERT INTO clean_scans VALUES (?1, ?2, ?3)",
            params![scan.id, scan.created_at, review_body(scan)?],
        )
        .map_err(|e| e.to_string())?;
    prune_reviews(&transaction)?;
    transaction.commit().map_err(|e| e.to_string())
}

pub fn scan(connection: &Connection, id: &str) -> Result<crate::review::Scan, String> {
    let body: String = connection
        .query_row("SELECT body FROM clean_scans WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .map_err(|_| "这份扫描已不存在或已被回收，请重新扫描。")?;
    serde_json::from_str(&body).map_err(|e| format!("扫描数据无法读取：{e}"))
}

pub fn latest_scan(connection: &Connection) -> Result<Option<serde_json::Value>, String> {
    let id: Option<String> = connection
        .query_row(
            "SELECT id FROM clean_scans ORDER BY created_at DESC, rowid DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    id.map(|id| scan(connection, &id).map(|scan| scan.preview()))
        .transpose()
}

pub fn save_plan(connection: &Connection, plan: &crate::review::Plan) -> Result<(), String> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    transaction
        .execute(
            "INSERT INTO review_plans VALUES (?1, ?2, ?3, ?4)",
            params![plan.id, plan.scan_id, plan.created_at, review_body(plan)?],
        )
        .map_err(|e| e.to_string())?;
    prune_reviews(&transaction)?;
    transaction.commit().map_err(|e| e.to_string())
}

pub fn plan(connection: &Connection, id: &str) -> Result<crate::review::Plan, String> {
    let body: String = connection
        .query_row("SELECT body FROM review_plans WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .map_err(|_| "这份计划已不存在或已被回收。")?;
    serde_json::from_str(&body).map_err(|e| format!("审阅计划无法读取：{e}"))
}

pub fn plans(connection: &Connection) -> Result<Vec<crate::review::PlanSummary>, String> {
    let mut statement = connection
        .prepare("SELECT body FROM review_plans ORDER BY created_at DESC, rowid DESC LIMIT 20")
        .map_err(|e| e.to_string())?;
    let bodies = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    bodies
        .map(|body| {
            serde_json::from_str::<crate::review::Plan>(&body.map_err(|e| e.to_string())?)
                .map(|p| p.summary())
                .map_err(|e| e.to_string())
        })
        .collect()
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

pub fn task(connection: &Connection, id: &str) -> Result<Option<TaskRecord>, String> {
    connection.query_row("SELECT id, kind, path, state, created_at, finished_at, message FROM tasks WHERE id = ?1", [id], |row| {
        Ok(TaskRecord { id: row.get(0)?, kind: row.get(1)?, path: row.get(2)?, state: row.get(3)?, created_at: row.get(4)?, finished_at: row.get(5)?, message: row.get(6)? })
    }).optional().map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "db/retention_tests.rs"]
mod retention_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn v1_migration_backs_up_settings_and_history_before_upgrading() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("app.sqlite");
        {
            let legacy = Connection::open(&path).unwrap();
            legacy.execute_batch("CREATE TABLE settings (id INTEGER PRIMARY KEY, value TEXT NOT NULL);
                CREATE TABLE tasks (id TEXT PRIMARY KEY, kind TEXT NOT NULL, path TEXT, state TEXT NOT NULL,
                    created_at INTEGER NOT NULL, finished_at INTEGER, message TEXT); PRAGMA user_version=1;").unwrap();
            let settings = Settings {
                acknowledged: true,
                ..Settings::default()
            };
            legacy
                .execute(
                    "INSERT INTO settings VALUES (1, ?1)",
                    [serde_json::to_string(&settings).unwrap()],
                )
                .unwrap();
            start(&legacy, "legacy-task", "status", None).unwrap();
            finish(&legacy, "legacy-task", "succeeded", "legacy").unwrap();
        }
        let upgraded = open(&path).unwrap();
        assert!(settings(&upgraded).unwrap().acknowledged);
        assert_eq!(history(&upgraded).unwrap()[0].id, "legacy-task");
        assert_eq!(
            upgraded
                .pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            6
        );
        let backups: Vec<_> = std::fs::read_dir(scratch.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("moomole-before-v6-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        let backup = Connection::open(&backups[0]).unwrap();
        assert_eq!(
            backup
                .pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            1
        );
        assert_eq!(
            history(&backup).unwrap()[0].message.as_deref(),
            Some("legacy")
        );
    }

    #[test]
    fn v2_wal_backup_precedes_migration_and_is_not_repeated() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("app.sqlite");
        let legacy = open(&path).unwrap();
        legacy.execute_batch("DROP TABLE maintenance_reviews; DROP TABLE maintenance_events; DROP TABLE maintenance_steps; DROP TABLE maintenance_runs; DROP TABLE uninstall_events; DROP TABLE uninstall_runs; DROP TABLE execution_events; DROP TABLE execution_items; DROP TABLE execution_runs; PRAGMA user_version=2;").unwrap();
        start(&legacy, "wal-only", "status", None).unwrap();
        finish(&legacy, "wal-only", "succeeded", "preserve WAL data").unwrap();
        let upgraded = open(&path).unwrap();
        assert_eq!(history(&upgraded).unwrap()[0].id, "wal-only");
        let backups: Vec<_> = std::fs::read_dir(scratch.path())
            .unwrap()
            .map(|p| p.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("moomole-before-v6-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        let backup = Connection::open(&backups[0]).unwrap();
        assert_eq!(
            backup
                .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            2
        );
        assert_eq!(
            history(&backup).unwrap()[0].message.as_deref(),
            Some("preserve WAL data")
        );
        drop(upgraded);
        drop(open(&path).unwrap());
        assert_eq!(
            std::fs::read_dir(scratch.path())
                .unwrap()
                .filter(|p| p
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("moomole-before-v6-"))
                .count(),
            1
        );
    }

    #[test]
    fn failed_migration_rolls_back_all_schema_changes_and_future_version_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("app.sqlite");
        let legacy = Connection::open(&path).unwrap();
        legacy.execute_batch("CREATE TABLE settings (id INTEGER PRIMARY KEY,value TEXT); CREATE TABLE tasks (id TEXT PRIMARY KEY,kind TEXT,path TEXT,state TEXT,created_at INTEGER,finished_at INTEGER,message TEXT); CREATE TABLE execution_runs (sentinel TEXT); INSERT INTO execution_runs VALUES ('keep'); PRAGMA user_version=1;").unwrap();
        assert!(open(&path).is_err());
        assert_eq!(
            legacy
                .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            1
        );
        assert_eq!(
            legacy
                .query_row::<String, _, _>("SELECT sentinel FROM execution_runs", [], |r| r.get(0))
                .unwrap(),
            "keep"
        );
        assert_eq!(
            legacy
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM sqlite_master WHERE name='clean_scans'",
                    [],
                    |r| r.get(0)
                )
                .unwrap(),
            0
        );
        legacy.pragma_update(None, "user_version", 99).unwrap();
        assert!(open(&path).unwrap_err().contains("新版"));
        assert_eq!(
            legacy
                .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            99
        );
    }

    #[cfg(unix)]
    #[test]
    fn backup_creation_failure_blocks_migration() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("app.sqlite");
        let legacy = open(&path).unwrap();
        legacy.execute_batch("DROP TABLE maintenance_reviews; DROP TABLE maintenance_events; DROP TABLE maintenance_steps; DROP TABLE maintenance_runs; DROP TABLE uninstall_events; DROP TABLE uninstall_runs; DROP TABLE execution_events; DROP TABLE execution_items; DROP TABLE execution_runs; PRAGMA user_version=2;").unwrap();
        // Keep WAL/SHM open, allowing the original DB to open while a new backup cannot be created.
        std::fs::set_permissions(scratch.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let result = open(&path);
        std::fs::set_permissions(scratch.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.unwrap_err().contains("备份失败"));
        assert_eq!(
            legacy
                .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            2
        );
        assert_eq!(
            legacy
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM sqlite_master WHERE name='execution_runs'",
                    [],
                    |r| r.get(0)
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn saved_scans_and_plans_survive_restart_and_retain_foreign_key_bounds() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("app.sqlite");
        let scan = crate::review::Scan {
            id: "scan".into(),
            created_at: now(),
            expires_at: now() + 900_000,
            revision: crate::engine::REVISION.into(),
            protocol: 1,
            system_included: false,
            items: vec![],
        };
        let plan = crate::review::Plan {
            id: "plan".into(),
            scan_id: scan.id.clone(),
            created_at: now(),
            expires_at: scan.expires_at,
            revision: scan.revision.clone(),
            protocol: 1,
            items: vec![],
        };
        {
            let db = open(&path).unwrap();
            assert!(save_plan(&db, &plan).is_err());
            save_scan(&db, &scan).unwrap();
            save_plan(&db, &plan).unwrap();
        }
        let db = open(&path).unwrap();
        assert_eq!(plans(&db).unwrap()[0].id, "plan");
        assert_eq!(latest_scan(&db).unwrap().unwrap()["scanId"], "scan");
        for i in 0..25 {
            let mut next = plan.clone();
            next.id = format!("plan-{i}");
            next.created_at += i + 1;
            save_plan(&db, &next).unwrap();
        }
        assert_eq!(plans(&db).unwrap().len(), 20);
        for i in 0..10 {
            let mut next = scan.clone();
            next.id = format!("scan-{i}");
            next.created_at += i + 1;
            save_scan(&db, &next).unwrap();
        }
        assert!(super::scan(&db, "scan").is_err());
        assert!(plans(&db).unwrap().is_empty());
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT count(*) FROM clean_scans", [], |row| row.get(0))
                .unwrap(),
            10
        );
    }
    #[test]
    fn owned_startup_refuses_before_open_and_releases_on_initialization_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.sqlite");
        let owner = crate::instance::acquire(directory.path()).unwrap();
        assert!(matches!(
            open_owned(&path),
            Err(OpenError::Instance(crate::instance::Error::Busy))
        ));
        assert!(!path.exists(), "busy startup must not even create SQLite");
        drop(owner);
        let future = Connection::open(&path).unwrap();
        future.pragma_update(None, "user_version", 99).unwrap();
        drop(future);
        assert!(
            matches!(open_owned(&path), Err(OpenError::Database(message)) if message.contains("新版"))
        );
        let next = crate::instance::acquire(directory.path())
            .expect("failed SQLite initialization must release ownership");
        assert!(open_owned(Path::new("relative.sqlite")).is_err());
        drop(next);
    }

    #[test]
    fn shared_database_handle_keeps_live_tasks_owned_until_all_workers_drop() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.sqlite");
        let app = std::sync::Arc::new(std::sync::Mutex::new(open_owned(&path).unwrap()));
        {
            let db = app.lock().unwrap();
            start(&db, "running", "clean", None).unwrap();
            start(&db, "completed", "status", None).unwrap();
            finish(&db, "completed", "succeeded", "done").unwrap();
        }
        let worker = app.clone();
        drop(app);
        assert!(matches!(
            open_owned(&path),
            Err(OpenError::Instance(crate::instance::Error::Busy))
        ));
        assert_eq!(
            history(&worker.lock().unwrap())
                .unwrap()
                .iter()
                .find(|t| t.id == "running")
                .unwrap()
                .state,
            "running"
        );
        drop(worker);
        let next = open_owned(&path).unwrap();
        let tasks = history(&next).unwrap();
        assert_eq!(
            tasks.iter().find(|t| t.id == "running").unwrap().state,
            "interrupted"
        );
        assert_eq!(
            tasks.iter().find(|t| t.id == "completed").unwrap().state,
            "succeeded"
        );
    }

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
