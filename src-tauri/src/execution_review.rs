//! Human review releases admission only. Original outcomes/receipts stay intact.
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Deserialize;

pub(crate) const SCHEMA: &str = "
CREATE TABLE execution_reviews (
    run_id TEXT PRIMARY KEY REFERENCES execution_runs(id) ON DELETE CASCADE,
    snapshot_sha256 TEXT NOT NULL, reviewed_at INTEGER NOT NULL CHECK(typeof(reviewed_at)='integer' AND reviewed_at>=0)
);
CREATE TABLE uninstall_reviews (
    run_id TEXT PRIMARY KEY REFERENCES uninstall_runs(id) ON DELETE CASCADE,
    snapshot_sha256 TEXT NOT NULL, reviewed_at INTEGER NOT NULL CHECK(typeof(reviewed_at)='integer' AND reviewed_at>=0)
);
PRAGMA user_version=7;";

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Kind {
    Cleanup,
    Uninstall,
}
impl Kind {
    fn table(self) -> &'static str {
        match self {
            Self::Cleanup => "execution_reviews",
            Self::Uninstall => "uninstall_reviews",
        }
    }
}

pub(crate) fn reviewed_at(
    db: &Connection,
    kind: Kind,
    id: &str,
    digest: &str,
    eligible: bool,
) -> Result<Option<i64>, String> {
    let version: u32 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if version < 7 {
        return Ok(None);
    } // Read-only inspection of original pre-upgrade backups.
    let row: Option<(String, i64)> = db
        .query_row(
            &format!(
                "SELECT snapshot_sha256,reviewed_at FROM {} WHERE run_id=?1",
                kind.table()
            ),
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| format!("人工核查记录无法读取：{e}"))?;
    if row
        .as_ref()
        .is_some_and(|(hash, at)| !eligible || hash != digest || *at < 0)
    {
        return Err("人工核查与原未知结果不匹配，保留记录供核查。".into());
    }
    Ok(row.map(|(_, at)| at))
}

pub(crate) fn acknowledge(
    db: &mut Connection,
    kind: Kind,
    id: &str,
    expected: &str,
    at: i64,
) -> Result<(), String> {
    uuid::Uuid::parse_str(id).map_err(|_| "执行记录标识无效。")?;
    let sync: i64 = db
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let version: u32 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if version != 7 || sync < 2 || at < 0 {
        return Err("人工核查需要可靠的时间和 SQLite FULL 同步模式。".into());
    }
    let transaction = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let (digest, eligible) = match kind {
        Kind::Cleanup => {
            let detail = crate::execution::detail_in_snapshot(&transaction, id)?;
            (
                detail.snapshot_sha256,
                detail.summary.state == "interrupted" && detail.summary.uncertain_count > 0,
            )
        }
        Kind::Uninstall => {
            let detail = crate::uninstall_journal::detail_in_snapshot(&transaction, id)?;
            (
                detail.snapshot_sha256,
                detail.summary.state == "interrupted" && detail.summary.item_state == "uncertain",
            )
        }
    };
    if !eligible || digest != expected {
        return Err("请先核对当次已结束的未知结果，再明确确认；原结果不会改写或重试。".into());
    }
    transaction
        .execute(
            &format!(
                "INSERT INTO {} VALUES (?1,?2,?3) ON CONFLICT(run_id) DO NOTHING",
                kind.table()
            ),
            params![id, expected, at],
        )
        .map_err(|e| format!("人工核查无法保存：{e}"))?;
    transaction.commit().map_err(|e| e.to_string())
}
