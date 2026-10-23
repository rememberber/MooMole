//! Optional additive preference storage; leaves the v3 settings JSON unchanged.
use rusqlite::{Connection, OptionalExtension};
const READ_ERROR: &str = "无法读取自动下载设置。";

pub fn load(db: &Connection) -> Result<bool, String> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='update_preferences')",
        [], |r| r.get(0),
    ).map_err(|_| READ_ERROR)?;
    if !exists {
        return Ok(false);
    }
    match db
        .query_row(
            "SELECT auto_download FROM update_preferences WHERE id=1",
            [],
            |r| r.get::<_, i64>(0),
        )
        .optional()
        .map_err(|_| READ_ERROR)?
    {
        None | Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(READ_ERROR.into()),
    }
}
pub fn save(db: &Connection, enabled: bool) -> Result<(), String> {
    let result = (|| -> rusqlite::Result<()> {
        let tx = db.unchecked_transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS update_preferences (id INTEGER PRIMARY KEY CHECK(id=1), auto_download INTEGER NOT NULL CHECK(auto_download IN (0,1)))")?;
        tx.execute("INSERT INTO update_preferences VALUES (1,?1) ON CONFLICT(id) DO UPDATE SET auto_download=excluded.auto_download", [enabled])?;
        tx.commit()
    })();
    result.map_err(|_| "自动下载设置未能保存，已保留原值，请重试。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_is_off_without_creating_table_and_preference_survives_reopen() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db.sqlite");
        let db = crate::db::open_fixture(&path).unwrap();
        let schema_before: u32 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert!(!load(&db).unwrap());
        assert_eq!(
            db.query_row::<i64, _, _>(
                "SELECT count(*) FROM sqlite_master WHERE name='update_preferences'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
            0
        );
        save(&db, true).unwrap();
        drop(db);
        let db = crate::db::open_fixture(&path).unwrap();
        assert!(load(&db).unwrap());
        save(&db, false).unwrap();
        assert!(!load(&db).unwrap());
        assert_eq!(
            db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            schema_before
        );
    }
    #[test]
    fn old_settings_json_and_old_style_sql_keep_working_and_preserve_the_preference() {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Legacy {
            theme: String,
            retention_days: u32,
            protected_paths: Vec<String>,
            acknowledged: bool,
        }
        let temp = tempfile::tempdir().unwrap();
        let db = crate::db::open_fixture(&temp.path().join("db.sqlite")).unwrap();
        crate::db::save_settings(&db, &crate::db::Settings::default()).unwrap();
        let before: String = db
            .query_row("SELECT value FROM settings WHERE id=1", [], |r| r.get(0))
            .unwrap();
        save(&db, true).unwrap();
        let after: String = db
            .query_row("SELECT value FROM settings WHERE id=1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before, after);
        let legacy: Legacy = serde_json::from_str(&after).unwrap();
        assert_eq!(legacy.theme, "system");
        assert_eq!(legacy.retention_days, 30);
        assert!(legacy.protected_paths.is_empty());
        assert!(!legacy.acknowledged);
        let changed = after.replace("system", "dark");
        db.execute(
            "INSERT INTO settings VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET value=excluded.value",
            [changed],
        )
        .unwrap();
        assert_eq!(crate::db::settings(&db).unwrap().theme, "dark");
        assert!(load(&db).unwrap());
    }
    #[test]
    fn locked_or_readonly_save_preserves_old_value_and_invalid_values_never_enable_download() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db.sqlite");
        let db = crate::db::open_fixture(&path).unwrap();
        db.busy_timeout(std::time::Duration::from_millis(20))
            .unwrap();
        save(&db, false).unwrap();
        let other = Connection::open(&path).unwrap();
        other.execute_batch("BEGIN IMMEDIATE").unwrap();
        assert!(save(&db, true).is_err());
        assert!(!load(&db).unwrap());
        other.execute_batch("ROLLBACK").unwrap();
        let readonly =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert!(save(&readonly, true).is_err());
        assert!(!load(&db).unwrap());
        db.execute_batch(
            "PRAGMA ignore_check_constraints=ON; UPDATE update_preferences SET auto_download=2;",
        )
        .unwrap();
        assert!(load(&db).is_err());
        save(&db, false).unwrap();
        assert!(!load(&db).unwrap());
    }
}
