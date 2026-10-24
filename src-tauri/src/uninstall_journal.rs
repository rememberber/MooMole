//! Durable single-App accounting, not uninstall authority. Archive decoding
//! NEVER reconstructs a live Plan/Session; only the sealed executor writes.
#![allow(dead_code)]
use crate::{
    uninstall_plan::{absolute, hash, Archive, Plan},
    uninstall_target::{Outcome, Receipt},
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};

const MAX_SNAPSHOT: usize = 256 * 1024;
const MAX_RECEIPT: usize = 8192;
const RESERVE: usize = 32 * 1024;

pub(crate) const SCHEMA: &str="
CREATE TABLE uninstall_runs (
    id TEXT PRIMARY KEY, plan_id TEXT NOT NULL UNIQUE,
    method TEXT NOT NULL CHECK(method='appBundleTrash'),
    state TEXT NOT NULL CHECK(state IN ('running','completed','noChanges','cancelled','interrupted')),
    item_state TEXT NOT NULL CHECK(item_state IN ('pending','intent','moved','skipped','failed','cancelled','notStarted','uncertain')),
    created_at INTEGER NOT NULL, finished_at INTEGER,
    snapshot TEXT NOT NULL CHECK(length(CAST(snapshot AS BLOB))<=262144),
    snapshot_sha256 TEXT NOT NULL, reserved_bytes INTEGER NOT NULL CHECK(reserved_bytes>0),
    reason TEXT NOT NULL CHECK(length(CAST(reason AS BLOB))<=1024),
    receipt TEXT CHECK(receipt IS NULL OR length(CAST(receipt AS BLOB))<=8192)
);
CREATE UNIQUE INDEX uninstall_one_running ON uninstall_runs(state) WHERE state='running';
CREATE TABLE uninstall_events (
    seq INTEGER PRIMARY KEY, run_id TEXT NOT NULL REFERENCES uninstall_runs(id) ON DELETE CASCADE,
    state TEXT NOT NULL, created_at INTEGER NOT NULL,
    reason TEXT NOT NULL CHECK(length(CAST(reason AS BLOB))<=1024),
    receipt TEXT CHECK(receipt IS NULL OR length(CAST(receipt AS BLOB))<=8192)
);
CREATE INDEX uninstall_events_run ON uninstall_events(run_id,seq);
PRAGMA user_version=4;
";

// Only begin creates this handle. Historical reads cannot resume it or execute.
#[derive(Debug)]
pub(crate) struct Session {
    id: String,
    plan_id: String,
    digest: String,
}
impl Session {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Summary {
    pub id: String,
    pub plan_id: String,
    pub method: &'static str,
    pub state: String,
    pub item_state: String,
    pub name: String,
    pub path: String,
    pub bundle_id: String,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub retained_for_review: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Event {
    pub state: String,
    pub created_at: i64,
    pub reason: String,
    pub receipt: Option<Receipt>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Detail {
    #[serde(flatten)]
    pub summary: Summary,
    pub revision: String,
    pub snapshot_sha256: String,
    pub reason: String,
    pub receipt: Option<Receipt>,
    pub events: Vec<Event>,
}

struct Stored {
    id: String,
    plan_id: String,
    state: String,
    item_state: String,
    created_at: i64,
    finished_at: Option<i64>,
    archive: Archive,
    digest: String,
    reason: String,
    receipt: Option<Receipt>,
}
impl Stored {
    fn summary(&self) -> Summary {
        Summary {
            id: self.id.clone(),
            plan_id: self.plan_id.clone(),
            method: "appBundleTrash",
            state: self.state.clone(),
            item_state: self.item_state.clone(),
            name: self.archive.name().into(),
            path: self.archive.path().into(),
            bundle_id: self.archive.bundle_id().into(),
            created_at: self.created_at,
            finished_at: self.finished_at,
            retained_for_review: matches!(self.state.as_str(), "running" | "interrupted")
                || matches!(self.item_state.as_str(), "moved" | "uncertain" | "intent"),
        }
    }
}
fn sql(e: rusqlite::Error) -> String {
    format!("卸载记录无法写入或读取；停止后续操作：{e}")
}
fn tx(db: &mut Connection) -> Result<Transaction<'_>, String> {
    let sync: i64 = db
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(sql)?;
    if sync < 2 {
        return Err("卸载记录需要 SQLite FULL 同步模式。".into());
    }
    db.transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql)
}
fn reason(why: &str) -> Result<(), String> {
    if why.trim().is_empty() || why.len() > 1024 || why.contains('\0') {
        Err("卸载结果原因无效或超过上限。".into())
    } else {
        Ok(())
    }
}
fn receipt(archive: &Archive, state: &str, receipt: Option<&Receipt>) -> Result<(), String> {
    if state == "moved" && receipt.is_none() {
        return Err("已移入结果缺少实际 App 收据。".into());
    }
    if receipt.is_some() && !matches!(state, "moved" | "uncertain") {
        return Err("未尝试操作的状态不能附带位置收据。".into());
    }
    if let Some(r) = receipt {
        if r.version != 1
            || !absolute(&r.actual.destination)
            || r.actual.destination == archive.path()
            || r.actual.inode == 0
            || r.observed_inventory.as_ref().is_some_and(|i| {
                i.version != 1 || !hash(&i.digest) || i.entries > crate::tree::MAX_ENTRIES
            })
        {
            return Err("App 收据位置、版本或身份无效。".into());
        }
        if state == "moved" {
            let root = archive
                .snapshot()
                .root
                .nodes
                .last()
                .ok_or("快照缺少 App 根身份。")?;
            if (r.actual.device, r.actual.inode) != (root.dev, root.ino)
                || r.observed_inventory.as_ref() != Some(&archive.snapshot().inventory)
            {
                return Err("App 已移入收据与固定包身份/清单不一致，不能记录为成功。".into());
            }
        }
        if serde_json::to_vec(r)
            .map_err(|_| "无法记录 App 收据。")?
            .len()
            > MAX_RECEIPT
        {
            return Err("App 收据超过记录上限。".into());
        }
    }
    Ok(())
}
fn decode_receipt(raw: Option<String>) -> Result<Option<Receipt>, String> {
    raw.map(|raw| {
        if raw.len() > MAX_RECEIPT {
            return Err("App 收据超过记录上限。".into());
        }
        serde_json::from_str(&raw).map_err(|_| "App 收据损坏，保留记录供核查。".into())
    })
    .transpose()
}
fn load(db: &Connection, id: &str) -> Result<Stored, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "卸载记录标识无效。")?;
    let raw=db.query_row("SELECT plan_id,method,state,item_state,created_at,finished_at,
        CASE WHEN length(CAST(snapshot AS BLOB))<=262144 THEN snapshot ELSE NULL END,snapshot_sha256,reserved_bytes,reason,
        CASE WHEN receipt IS NULL OR length(CAST(receipt AS BLOB))<=8192 THEN receipt ELSE 'oversized' END
        FROM uninstall_runs WHERE id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,i64>(4)?,r.get::<_,Option<i64>>(5)?,r.get::<_,Option<String>>(6)?,r.get::<_,String>(7)?,r.get::<_,i64>(8)?,r.get::<_,String>(9)?,r.get::<_,Option<String>>(10)?))).optional().map_err(sql)?.ok_or("卸载记录不存在或已按期限清理。")?;
    let (
        plan_id,
        method,
        state,
        item_state,
        created_at,
        finished_at,
        body,
        digest,
        reserved,
        why,
        raw_receipt,
    ) = raw;
    let body = body.ok_or("卸载快照超过记录上限。")?;
    if body.len() > MAX_SNAPSHOT
        || format!("{:x}", Sha256::digest(body.as_bytes())) != digest
        || reserved != (body.len() + RESERVE) as i64
    {
        return Err("卸载快照摘要或容量字段损坏；保留记录供核查。".into());
    }
    let archive: Archive =
        serde_json::from_str(&body).map_err(|_| "卸载快照无法读取；保留记录供核查。")?;
    archive.validate()?;
    if method != "appBundleTrash" || archive.id() != plan_id || !archive.contains_time(created_at) {
        return Err("卸载操作类型或计划标识不一致。".into());
    }
    let expected = match item_state.as_str() {
        "pending" | "intent" => "running",
        "moved" => "completed",
        "skipped" | "failed" => "noChanges",
        "cancelled" => "cancelled",
        "uncertain" | "notStarted" => "interrupted",
        _ => return Err("卸载项目状态无法识别。".into()),
    };
    if expected != state || (state == "running") != finished_at.is_none() {
        return Err("卸载批次与项目状态不一致。".into());
    }
    reason(&why)?;
    let decoded = decode_receipt(raw_receipt)?;
    receipt(&archive, &item_state, decoded.as_ref())?;
    Ok(Stored {
        id: id.into(),
        plan_id,
        state,
        item_state,
        created_at,
        finished_at,
        archive,
        digest,
        reason: why,
        receipt: decoded,
    })
}
fn active(db: &Connection, session: &Session) -> Result<Stored, String> {
    let row = load(db, &session.id)?;
    if row.state != "running" || row.plan_id != session.plan_id || row.digest != session.digest {
        return Err("卸载会话已结束或与原计划不一致，不能继续操作。".into());
    }
    Ok(row)
}
fn event(
    db: &Connection,
    id: &str,
    state: &str,
    why: &str,
    receipt: Option<&str>,
    at: i64,
) -> Result<(), String> {
    db.execute("INSERT INTO uninstall_events(run_id,state,created_at,reason,receipt) VALUES (?1,?2,?3,?4,?5)",params![id,state,at,why,receipt]).map_err(sql)?;
    Ok(())
}

pub(crate) fn begin(db: &mut Connection, plan: &Plan, at: i64) -> Result<Session, String> {
    begin_bound(db, plan, &uuid::Uuid::new_v4().to_string(), at)
}
pub(crate) fn begin_bound(
    db: &mut Connection,
    plan: &Plan,
    request_id: &str,
    at: i64,
) -> Result<Session, String> {
    uuid::Uuid::parse_str(request_id).map_err(|_| "卸载请求标识无效。")?;
    let archive = plan.archive();
    archive.live_at(at)?;
    let body = serde_json::to_string(&archive).map_err(|_| "无法固定卸载记录。")?;
    if body.len() > MAX_SNAPSHOT {
        return Err("卸载快照超过 256 KiB 上限。".into());
    }
    let transaction = tx(db)?;
    crate::execution_admission::reserve(&transaction, (body.len() + RESERVE) as i64)?;
    let id = request_id.to_owned();
    let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
    transaction.execute("INSERT INTO uninstall_runs VALUES (?1,?2,'appBundleTrash','running','pending',?3,NULL,?4,?5,?6,'尚未开始',NULL)",params![id,archive.id(),at,body,digest,(body.len()+RESERVE) as i64]).map_err(sql)?;
    event(&transaction, &id, "pending", "尚未开始", None, at)?;
    transaction.commit().map_err(sql)?;
    Ok(Session {
        id,
        plan_id: archive.id().into(),
        digest,
    })
}
pub(crate) fn intent(db: &mut Connection, session: &Session, at: i64) -> Result<(), String> {
    let transaction = tx(db)?;
    let stored = active(&transaction, session)?;
    stored.archive.live_at(at)?;
    if stored.item_state != "pending" {
        return Err("App 已有操作意图，不能重复提交。".into());
    }
    transaction.execute("UPDATE uninstall_runs SET item_state='intent',reason='已记录意图，尚无完成证据' WHERE id=?1",[session.id()]).map_err(sql)?;
    event(
        &transaction,
        session.id(),
        "intent",
        "已记录意图，尚无完成证据",
        None,
        at,
    )?;
    transaction.commit().map_err(sql)
}
pub(crate) fn check_intent(db: &Connection, session: &Session, at: i64) -> Result<(), String> {
    let stored = active(db, session)?;
    stored.archive.live_at(at)?;
    if stored.item_state != "intent" {
        return Err("尚无已提交的卸载操作意图。".into());
    }
    Ok(())
}

fn settle(
    db: &mut Connection,
    session: &Session,
    from: &str,
    state: &str,
    why: &str,
    value: Option<Receipt>,
    at: i64,
) -> Result<(), String> {
    reason(why)?;
    let transaction = tx(db)?;
    let stored = active(&transaction, session)?;
    if stored.item_state != from {
        return Err("卸载项目已结束或转换顺序无效。".into());
    }
    receipt(&stored.archive, state, value.as_ref())?;
    let raw = value
        .map(|r| serde_json::to_string(&r).map_err(|_| "无法记录实际收据。".to_string()))
        .transpose()?;
    let run = match state {
        "moved" => "completed",
        "uncertain" | "notStarted" => "interrupted",
        "cancelled" => "cancelled",
        _ => "noChanges",
    };
    transaction.execute("UPDATE uninstall_runs SET state=?2,item_state=?3,finished_at=?4,reason=?5,receipt=?6 WHERE id=?1",params![session.id(),run,state,at,why,raw]).map_err(sql)?;
    event(&transaction, session.id(), state, why, raw.as_deref(), at)?;
    transaction.commit().map_err(sql)
}
pub(crate) fn skip(
    db: &mut Connection,
    session: &Session,
    why: &str,
    at: i64,
) -> Result<(), String> {
    settle(db, session, "pending", "skipped", why, None, at)
}
pub(crate) fn cancel(db: &mut Connection, session: &Session, at: i64) -> Result<(), String> {
    settle(
        db,
        session,
        "pending",
        "cancelled",
        "开始前取消，未调用卸载 API",
        None,
        at,
    )
}
// Only for a target/gate Err strictly before the native API, even after intent.
pub(crate) fn refused_before_call(
    db: &mut Connection,
    session: &Session,
    why: &str,
    at: i64,
) -> Result<(), String> {
    settle(db, session, "intent", "failed", why, None, at)
}
// Only a proven pre-API cancellation, never an in-flight Foundation call.
pub(crate) fn cancelled_before_call(
    db: &mut Connection,
    session: &Session,
    at: i64,
) -> Result<(), String> {
    settle(
        db,
        session,
        "intent",
        "cancelled",
        "最终检查前收到取消，未调用卸载 API",
        None,
        at,
    )
}
pub(crate) fn complete(
    db: &mut Connection,
    session: &Session,
    outcome: Outcome,
    why: &str,
    at: i64,
) -> Result<(), String> {
    match outcome {
        Outcome::Moved(r) => settle(db, session, "intent", "moved", why, Some(r), at),
        Outcome::Uncertain(r) => settle(db, session, "intent", "uncertain", why, r, at),
    }
}
pub(crate) fn reconcile(db: &mut Connection, at: i64) -> Result<(), String> {
    let transaction = tx(db)?;
    let ids = {
        let mut stmt = transaction
            .prepare("SELECT id,item_state FROM uninstall_runs WHERE state='running'")
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)?
    };
    for (id, state) in ids {
        interrupt(&transaction, &id, &state, at)?;
    }
    transaction.commit().map_err(sql)
}
fn interrupt(transaction: &Transaction<'_>, id: &str, state: &str, at: i64) -> Result<(), String> {
    let (to, why) = match state {
        "pending" => ("notStarted", "卸载批次中断，此 App 未开始处理"),
        "intent" => (
            "uncertain",
            "已有卸载意图但没有可靠完成记录，结果待核实，禁止自动重试",
        ),
        _ => return Err("运行中的卸载记录状态无效，保留原始记录。".into()),
    };
    transaction.execute("UPDATE uninstall_runs SET state='interrupted',item_state=?2,finished_at=?3,reason=?4 WHERE id=?1",params![id,to,at,why]).map_err(sql)?;
    event(transaction, id, to, why, None, at)
}
// Worker-fault collection only: never use this to abandon an in-flight native
// call. Its owner must retain task/update/exit leases and collect that outcome.
pub(crate) fn stop(db: &mut Connection, session: &Session, at: i64) -> Result<(), String> {
    let transaction = tx(db)?;
    let stored = active(&transaction, session)?;
    interrupt(&transaction, session.id(), &stored.item_state, at)?;
    transaction.commit().map_err(sql)
}
pub(crate) fn prune(db: &Connection, before: i64) -> Result<(), String> {
    db.execute("DELETE FROM uninstall_runs WHERE state NOT IN ('running','interrupted') AND finished_at<?1 AND item_state NOT IN ('moved','intent','uncertain')",[before]).map_err(sql)?;
    Ok(())
}
pub(crate) fn detail(db: &Connection, id: &str) -> Result<Detail, String> {
    let transaction = db.unchecked_transaction().map_err(sql)?;
    let stored = load(&transaction, id)?;
    let events = {
        let mut s=transaction.prepare("SELECT state,created_at,reason,receipt FROM uninstall_events WHERE run_id=?1 ORDER BY seq LIMIT 4").map_err(sql)?;
        let rows = s
            .query_map([id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(sql)?;
        rows.map(|r| {
            let (state, created_at, why, raw) = r.map_err(sql)?;
            reason(&why)?;
            let value = decode_receipt(raw)?;
            receipt(&stored.archive, &state, value.as_ref())?;
            Ok(Event {
                state,
                created_at,
                reason: why,
                receipt: value,
            })
        })
        .collect::<Result<Vec<_>, String>>()?
    };
    if events.is_empty()
        || events.len() > 3
        || events[0].state != "pending"
        || events[0].created_at != stored.created_at
        || events.windows(2).any(|e| {
            !matches!(
                (e[0].state.as_str(), e[1].state.as_str()),
                ("pending", "intent" | "skipped" | "cancelled" | "notStarted")
                    | ("intent", "moved" | "failed" | "uncertain" | "cancelled")
            )
        })
        || events.last().is_none_or(|e| {
            e.state != stored.item_state
                || e.reason != stored.reason
                || e.receipt != stored.receipt
                || (stored.finished_at.is_some() && Some(e.created_at) != stored.finished_at)
        })
    {
        return Err("卸载逐项结果与事件顺序不一致，保留记录供核查。".into());
    }
    transaction.commit().map_err(sql)?;
    Ok(Detail {
        summary: stored.summary(),
        revision: stored.archive.revision().into(),
        snapshot_sha256: stored.digest,
        reason: stored.reason,
        receipt: stored.receipt,
        events,
    })
}
pub(crate) fn list(db: &Connection) -> Result<Vec<Summary>, String> {
    // The current UI does not consume these yet. No raw archive/identity fields
    // cross the future read-only summary boundary.
    let mut statement = db
        .prepare("SELECT id FROM uninstall_runs ORDER BY created_at DESC,rowid DESC LIMIT 64")
        .map_err(sql)?;
    let ids = statement
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    ids.iter()
        .map(|id| detail(db, id).map(|d| d.summary))
        .collect()
}

#[cfg(test)]
mod tests;
