//! Durable accounting only: this module never touches cleanup targets. Mutators
//! are internal building blocks, NOT execution permission. The log executor
//! independently checks sealed confirmation, module rules and native identity.
#[cfg(test)]
use crate::execution_admission::{MAX_RESERVED, MAX_RUNS};
use crate::review::Plan;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

const MAX_REASON: usize = 1024;
const MAX_RECEIPT: usize = 4096;
const MAX_PLAN: usize = 8 * 1024 * 1024;
// Covers three bounded events, current receipt/reason and row overhead per item.
const RESERVE_PER_ITEM: i64 = 32 * 1024;

pub const SCHEMA: &str = "
CREATE TABLE execution_runs (
    id TEXT PRIMARY KEY, plan_id TEXT NOT NULL UNIQUE,
    method TEXT NOT NULL CHECK(method = 'trash'),
    state TEXT NOT NULL CHECK(state IN ('running','completed','partial','noChanges','cancelled','interrupted')),
    created_at INTEGER NOT NULL, finished_at INTEGER,
    snapshot TEXT NOT NULL, snapshot_sha256 TEXT NOT NULL, reserved_bytes INTEGER NOT NULL
);
CREATE UNIQUE INDEX execution_one_running ON execution_runs(state) WHERE state = 'running';
CREATE TABLE execution_items (
    run_id TEXT NOT NULL REFERENCES execution_runs(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL, ordinal INTEGER NOT NULL, path TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','intent','moved','skipped','failed','cancelled','notStarted','uncertain')),
    reason TEXT NOT NULL, receipt TEXT,
    PRIMARY KEY(run_id, item_id), UNIQUE(run_id, ordinal)
);
CREATE TABLE execution_events (
    seq INTEGER PRIMARY KEY, run_id TEXT NOT NULL, item_id TEXT NOT NULL,
    state TEXT NOT NULL, created_at INTEGER NOT NULL, reason TEXT NOT NULL, receipt TEXT,
    FOREIGN KEY(run_id, item_id) REFERENCES execution_items(run_id, item_id) ON DELETE CASCADE
);
CREATE INDEX execution_events_run ON execution_events(run_id, seq);
PRAGMA user_version = 3;
";

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    // Actual URL path returned by the future native API; never a guessed Trash path.
    pub destination: String,
    #[serde(with = "decimal")]
    pub device: u64,
    #[serde(with = "decimal")]
    pub inode: u64,
}

// File identifiers may exceed JavaScript's exact integer range. Keep them
// decimal strings in SQLite JSON and IPC; they are not floating-point values.
mod decimal {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub plan_id: String,
    pub method: String,
    pub state: String,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub item_count: usize,
    pub moved_count: usize,
    pub uncertain_count: usize,
    pub retained_for_review: bool,
    pub reviewed_at: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub path: String,
    pub state: String,
    pub reason: String,
    pub receipt: Option<Receipt>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub item_id: String,
    pub state: String,
    pub created_at: i64,
    pub reason: String,
    pub receipt: Option<Receipt>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    #[serde(flatten)]
    pub summary: Summary,
    pub revision: String,
    pub protocol: u64,
    pub snapshot_sha256: String,
    pub items: Vec<Item>,
    pub events: Vec<Event>,
}

fn sql(error: rusqlite::Error) -> String {
    format!("执行记录无法写入或读取；停止后续操作：{error}")
}

fn transaction(db: &mut Connection) -> Result<Transaction<'_>, String> {
    // An intent is only acknowledged after SQLite commits it. FULL also applies
    // to WAL commits. This is not certification against power loss/hardware lies.
    let sync: i64 = db
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(sql)?;
    if sync < 2 {
        return Err("执行记录需要 SQLite FULL 同步模式。".into());
    }
    db.transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql)
}

fn reason(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > MAX_REASON || value.contains('\0') {
        return Err("逐项原因为空或超过记录上限。".into());
    }
    Ok(())
}

fn absolute(value: &str) -> bool {
    let path = Path::new(value);
    path.is_absolute()
        && path.parent().is_some()
        && !value.contains('\0')
        && !path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
}

fn event(
    tx: &Transaction<'_>,
    run: &str,
    item: &str,
    state: &str,
    why: &str,
    receipt: Option<&str>,
    at: i64,
) -> Result<(), String> {
    tx.execute("INSERT INTO execution_events(run_id,item_id,state,created_at,reason,receipt) VALUES (?1,?2,?3,?4,?5,?6)", params![run,item,state,at,why,receipt]).map_err(sql)?;
    Ok(())
}

fn running(tx: &Transaction<'_>, run: &str) -> Result<(), String> {
    let state: Option<String> = tx
        .query_row("SELECT state FROM execution_runs WHERE id=?1", [run], |r| {
            r.get(0)
        })
        .optional()
        .map_err(sql)?;
    if state.as_deref() != Some("running") {
        return Err("执行批次不存在或已经停止。".into());
    }
    Ok(())
}

// These internal methods intentionally have no production caller yet. Keeping
// accounting separate prevents the read-only GUI from obtaining write authority.
#[allow(dead_code)]
pub fn begin(db: &mut Connection, plan: &Plan, at: i64) -> Result<String, String> {
    if uuid::Uuid::parse_str(&plan.id).is_err()
        || plan.items.is_empty()
        || plan.items.len() > 500
        || plan.revision != crate::engine::REVISION
        || plan.protocol != crate::clean_preview::PROTOCOL
        || at < plan.created_at
        || at >= plan.expires_at
    {
        return Err("无法为无效、过期或版本不匹配的计划建立执行记录。".into());
    }
    let mut ids = std::collections::HashSet::new();
    let mut paths = std::collections::HashSet::new();
    for item in &plan.items {
        if item.candidate.id.is_empty()
            || item.candidate.id.len() > 256
            || item.candidate.path.len() > 4096
            || !ids.insert(&item.candidate.id)
            || !absolute(&item.candidate.path)
            || !paths.insert(&item.candidate.path)
        {
            return Err("计划存在重复项目或无效路径。".into());
        }
    }
    let body = serde_json::to_string(plan).map_err(|e| e.to_string())?;
    if body.len() > MAX_PLAN {
        return Err("执行计划超过 8 MiB 上限。".into());
    }
    let reserved = body.len() as i64 + plan.items.len() as i64 * RESERVE_PER_ITEM;
    let tx = transaction(db)?;
    crate::execution_admission::reserve(&tx, reserved)?;
    let id = uuid::Uuid::new_v4().to_string();
    let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
    tx.execute(
        "INSERT INTO execution_runs VALUES (?1,?2,'trash','running',?3,NULL,?4,?5,?6)",
        params![id, plan.id, at, body, digest, reserved],
    )
    .map_err(sql)?;
    for (ordinal, item) in plan.items.iter().enumerate() {
        tx.execute(
            "INSERT INTO execution_items VALUES (?1,?2,?3,?4,'pending','尚未开始',NULL)",
            params![id, item.candidate.id, ordinal as i64, item.candidate.path],
        )
        .map_err(sql)?;
    }
    tx.commit().map_err(sql)?;
    Ok(id)
}

#[allow(dead_code)]
pub fn intent(db: &mut Connection, run: &str, item: &str, at: i64) -> Result<(), String> {
    let tx = transaction(db)?;
    running(&tx, run)?;
    let inflight: i64 = tx
        .query_row(
            "SELECT count(*) FROM execution_items WHERE run_id=?1 AND state='intent'",
            [run],
            |r| r.get(0),
        )
        .map_err(sql)?;
    if inflight != 0 {
        return Err("上一项仍有未完成意图，不能开始下一项。".into());
    }
    let changed = tx.execute("UPDATE execution_items SET state='intent',reason='已记录意图，尚无完成证据' WHERE run_id=?1 AND item_id=?2 AND state='pending'", params![run,item]).map_err(sql)?;
    if changed != 1 {
        return Err("项目不存在或已处理，不能重复执行。".into());
    }
    event(
        &tx,
        run,
        item,
        "intent",
        "已记录意图，尚无完成证据",
        None,
        at,
    )?;
    tx.commit().map_err(sql)
}

#[allow(dead_code)]
pub enum Outcome {
    Moved(Receipt),
    // Only for an OS error known to have caused no change; otherwise Uncertain.
    Failed,
    // Preserve an observed destination even if the result cannot be verified.
    Uncertain(Option<Receipt>),
}

#[allow(dead_code)]
pub fn complete_item(
    db: &mut Connection,
    run: &str,
    item: &str,
    outcome: Outcome,
    why: &str,
    at: i64,
) -> Result<(), String> {
    reason(why)?;
    let (state, raw_receipt) = match outcome {
        Outcome::Moved(receipt) => ("moved", Some(receipt)),
        Outcome::Failed => ("failed", None),
        Outcome::Uncertain(receipt) => ("uncertain", receipt),
    };
    let receipt = raw_receipt
        .map(|receipt| -> Result<String, String> {
            if !absolute(&receipt.destination) || receipt.inode == 0 {
                return Err("实际目标或身份收据无效。".into());
            }
            let body = serde_json::to_string(&receipt).map_err(|e| e.to_string())?;
            if body.len() > MAX_RECEIPT {
                return Err("实际目标收据超过上限。".into());
            }
            Ok(body)
        })
        .transpose()?;
    let tx = transaction(db)?;
    running(&tx, run)?;
    let changed = tx.execute("UPDATE execution_items SET state=?3,reason=?4,receipt=?5 WHERE run_id=?1 AND item_id=?2 AND state='intent'", params![run,item,state,why,receipt]).map_err(sql)?;
    if changed != 1 {
        return Err("没有已提交的项目意图，不能记录操作结果。".into());
    }
    event(&tx, run, item, state, why, receipt.as_deref(), at)?;
    if state == "uncertain" {
        interrupt(&tx, run, at)?;
    }
    tx.commit().map_err(sql)
}

#[allow(dead_code)]
pub fn skip(db: &mut Connection, run: &str, item: &str, why: &str, at: i64) -> Result<(), String> {
    reason(why)?;
    let tx = transaction(db)?;
    running(&tx, run)?;
    // Do not continue checking other items while a previous operation is unresolved.
    let changed = tx.execute("UPDATE execution_items SET state='skipped',reason=?3 WHERE run_id=?1 AND item_id=?2 AND state='pending' AND NOT EXISTS(SELECT 1 FROM execution_items WHERE run_id=?1 AND state='intent')",params![run,item,why]).map_err(sql)?;
    if changed != 1 {
        return Err("项目不存在、已处理或仍有未完成意图。".into());
    }
    event(&tx, run, item, "skipped", why, None, at)?;
    tx.commit().map_err(sql)
}

#[allow(dead_code)]
pub fn finish(db: &mut Connection, run: &str, cancel: bool, at: i64) -> Result<(), String> {
    let tx = transaction(db)?;
    running(&tx, run)?;
    let pending: i64 = tx.query_row("SELECT count(*) FROM execution_items WHERE run_id=?1 AND state IN ('intent','pending')", [run], |r| r.get(0)).map_err(sql)?;
    let inflight: i64 = tx
        .query_row(
            "SELECT count(*) FROM execution_items WHERE run_id=?1 AND state='intent'",
            [run],
            |r| r.get(0),
        )
        .map_err(sql)?;
    if inflight != 0 || (pending != 0 && !cancel) {
        return Err("批次仍有未完成项目，不能提前结束。".into());
    }
    if cancel {
        terminal_pending(&tx, run, "cancelled", "开始前取消，未尝试操作", at)?;
    }
    let (total, moved): (i64, i64) = tx
        .query_row(
            "SELECT count(*),coalesce(sum(state='moved'),0) FROM execution_items WHERE run_id=?1",
            [run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(sql)?;
    let state = if cancel && pending > 0 {
        "cancelled"
    } else if moved == total {
        "completed"
    } else if moved > 0 {
        "partial"
    } else {
        "noChanges"
    };
    tx.execute(
        "UPDATE execution_runs SET state=?2,finished_at=?3 WHERE id=?1",
        params![run, state, at],
    )
    .map_err(sql)?;
    tx.commit().map_err(sql)
}

fn terminal_pending(
    tx: &Transaction<'_>,
    run: &str,
    state: &str,
    why: &str,
    at: i64,
) -> Result<(), String> {
    tx.execute("INSERT INTO execution_events(run_id,item_id,state,created_at,reason) SELECT run_id,item_id,?2,?3,?4 FROM execution_items WHERE run_id=?1 AND state='pending' ORDER BY ordinal", params![run,state,at,why]).map_err(sql)?;
    tx.execute(
        "UPDATE execution_items SET state=?2,reason=?3 WHERE run_id=?1 AND state='pending'",
        params![run, state, why],
    )
    .map_err(sql)?;
    Ok(())
}

fn interrupt(tx: &Transaction<'_>, run: &str, at: i64) -> Result<(), String> {
    let why = "只有操作意图，没有可靠完成记录；结果待核实，禁止自动重试";
    tx.execute("INSERT INTO execution_events(run_id,item_id,state,created_at,reason) SELECT run_id,item_id,'uncertain',?2,?3 FROM execution_items WHERE run_id=?1 AND state='intent' ORDER BY ordinal",params![run,at,why]).map_err(sql)?;
    tx.execute(
        "UPDATE execution_items SET state='uncertain',reason=?2 WHERE run_id=?1 AND state='intent'",
        params![run, why],
    )
    .map_err(sql)?;
    terminal_pending(tx, run, "notStarted", "批次中断，未开始此项", at)?;
    tx.execute(
        "UPDATE execution_runs SET state='interrupted',finished_at=?2 WHERE id=?1",
        params![run, at],
    )
    .map_err(sql)?;
    Ok(())
}

pub fn reconcile(db: &mut Connection, at: i64) -> Result<(), String> {
    let tx = transaction(db)?;
    let runs: Vec<String> = {
        let mut statement = tx
            .prepare("SELECT id FROM execution_runs WHERE state='running'")
            .map_err(sql)?;
        let values = statement.query_map([], |r| r.get(0)).map_err(sql)?;
        values.collect::<Result<_, _>>().map_err(sql)?
    };
    for run in runs {
        interrupt(&tx, &run, at)?;
    }
    tx.commit().map_err(sql)
}

pub(crate) fn stop(db: &mut Connection, run: &str, at: i64) -> Result<(), String> {
    let tx = transaction(db)?;
    running(&tx, run)?;
    interrupt(&tx, run, at)?;
    tx.commit().map_err(sql)
}

pub fn prune(db: &Connection, before: i64) -> Result<(), String> {
    // Receipts and uncertain outcomes outlive normal history retention. Review
    // plan/scan pruning has no foreign key path into these independent records.
    db.execute("DELETE FROM execution_runs WHERE state NOT IN ('running','interrupted') AND finished_at < ?1 AND NOT EXISTS(SELECT 1 FROM execution_items WHERE run_id=execution_runs.id AND state IN ('moved','uncertain','intent'))",[before]).map_err(sql)?;
    Ok(())
}

const SUMMARY_QUERY: &str = "SELECT r.id,r.plan_id,r.method,r.state,r.created_at,r.finished_at,count(i.item_id),coalesce(sum(i.state='moved'),0),coalesce(sum(i.state='uncertain'),0) FROM execution_runs r JOIN execution_items i ON i.run_id=r.id";
fn summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<Summary> {
    let moved: usize = row.get(7)?;
    let uncertain: usize = row.get(8)?;
    let state: String = row.get(3)?;
    let retained =
        moved > 0 || uncertain > 0 || matches!(state.as_str(), "running" | "interrupted");
    Ok(Summary {
        id: row.get(0)?,
        plan_id: row.get(1)?,
        method: row.get(2)?,
        state,
        created_at: row.get(4)?,
        finished_at: row.get(5)?,
        item_count: row.get(6)?,
        moved_count: moved,
        uncertain_count: uncertain,
        retained_for_review: retained,
        reviewed_at: None,
    })
}

pub fn list(db: &Connection) -> Result<Vec<Summary>, String> {
    let mut statement = db
        .prepare(&format!(
            "{SUMMARY_QUERY} GROUP BY r.id ORDER BY r.created_at DESC,r.rowid DESC LIMIT 64"
        ))
        .map_err(sql)?;
    let rows = statement.query_map([], summary).map_err(sql)?;
    let mut records = rows.collect::<Result<Vec<_>, _>>().map_err(sql)?;
    for row in &mut records {
        let digest: String = db
            .query_row(
                "SELECT snapshot_sha256 FROM execution_runs WHERE id=?1",
                [&row.id],
                |r| r.get(0),
            )
            .map_err(sql)?;
        row.reviewed_at = crate::execution_review::reviewed_at(
            db,
            crate::execution_review::Kind::Cleanup,
            &row.id,
            &digest,
            row.state == "interrupted" && row.uncertain_count > 0,
        )?;
    }
    Ok(records)
}

pub fn detail(db: &Connection, id: &str) -> Result<Detail, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "执行记录 ID 无效。")?;
    // All fields are read from the same snapshot, even with another writer.
    let tx = db.unchecked_transaction().map_err(sql)?;
    let result = detail_in_snapshot(&tx, id)?;
    tx.commit().map_err(sql)?;
    Ok(result)
}
pub(crate) fn detail_in_snapshot(db: &Connection, id: &str) -> Result<Detail, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "执行记录 ID 无效。")?;
    let mut summary = db
        .query_row(
            &format!("{SUMMARY_QUERY} WHERE r.id=?1 GROUP BY r.id"),
            [id],
            summary,
        )
        .optional()
        .map_err(sql)?
        .ok_or("执行记录不存在或已按保留期限清理。")?;
    let (body, digest): (String, String) = db
        .query_row(
            "SELECT snapshot,snapshot_sha256 FROM execution_runs WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(sql)?;
    if body.len() > MAX_PLAN || format!("{:x}", Sha256::digest(body.as_bytes())) != digest {
        return Err("执行计划快照损坏，保留原始记录供核查。".into());
    }
    let plan: Plan = serde_json::from_str(&body).map_err(|_| "执行计划快照无法读取。")?;
    let items = {
        let mut s = db.prepare("SELECT item_id,path,state,reason,receipt FROM execution_items WHERE run_id=?1 ORDER BY ordinal").map_err(sql)?;
        let rows = s
            .query_map([id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })
            .map_err(sql)?;
        rows.map(|r| {
            let (id, path, state, reason, receipt) = r.map_err(sql)?;
            Ok(Item {
                id,
                path,
                state,
                reason,
                receipt: decode_receipt(receipt)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?
    };
    if plan.id != summary.plan_id
        || items.len() != plan.items.len()
        || items.iter().zip(&plan.items).any(|(item, expected)| {
            item.id != expected.candidate.id || item.path != expected.candidate.path
        })
    {
        return Err("逐项记录与不可变计划不一致，保留原始记录供核查。".into());
    }
    let events = {
        let mut s = db.prepare("SELECT item_id,state,created_at,reason,receipt FROM execution_events WHERE run_id=?1 ORDER BY seq").map_err(sql)?;
        let rows = s
            .query_map([id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })
            .map_err(sql)?;
        rows.map(|r| {
            let (item_id, state, created_at, reason, receipt) = r.map_err(sql)?;
            Ok(Event {
                item_id,
                state,
                created_at,
                reason,
                receipt: decode_receipt(receipt)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?
    };
    summary.reviewed_at = crate::execution_review::reviewed_at(
        db,
        crate::execution_review::Kind::Cleanup,
        id,
        &digest,
        summary.state == "interrupted" && summary.uncertain_count > 0,
    )?;
    Ok(Detail {
        summary,
        revision: plan.revision,
        protocol: plan.protocol,
        snapshot_sha256: digest,
        items,
        events,
    })
}
fn decode_receipt(body: Option<String>) -> Result<Option<Receipt>, String> {
    body.map(|body| {
        if body.len() > MAX_RECEIPT {
            return Err("收据超过记录上限。".into());
        }
        serde_json::from_str(&body).map_err(|_| "操作收据损坏；保留记录供核查。".into())
    })
    .transpose()
}

#[cfg(test)]
mod tests;
