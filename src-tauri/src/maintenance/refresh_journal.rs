//! Durable per-step bookkeeping, NOT maintenance execution authority. Only a
//! newly begun live session can write; archive/history decoding cannot resume it.
#![allow(dead_code)]
use super::quicklook::{Attempt, Outcome, Step, Stop};
use crate::engine;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_ARCHIVE: usize = 16 * 1024;
const RESERVE: usize = 16 * 1024;
const TTL: i64 = 30_000;
pub(crate) const SCHEMA: &str = "
CREATE TABLE maintenance_runs (
    id TEXT PRIMARY KEY, plan_id TEXT NOT NULL UNIQUE,
    method TEXT NOT NULL CHECK(method='quickLookRefresh'),
    state TEXT NOT NULL CHECK(state IN ('running','completed','partial','noChanges','cancelled','needsReview','interrupted')),
    created_at INTEGER NOT NULL, finished_at INTEGER,
    snapshot TEXT NOT NULL CHECK(length(CAST(snapshot AS BLOB))<=16384),
    snapshot_sha256 TEXT NOT NULL, reserved_bytes INTEGER NOT NULL CHECK(reserved_bytes>0)
);
CREATE UNIQUE INDEX maintenance_one_running ON maintenance_runs(state) WHERE state='running';
CREATE TABLE maintenance_steps (
    run_id TEXT NOT NULL REFERENCES maintenance_runs(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position IN (0,1)), step TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','intent','commandSucceeded','notStarted','cancelled','uncertain')),
    finished_at INTEGER, exit_code INTEGER, stop TEXT,
    reason TEXT NOT NULL CHECK(length(CAST(reason AS BLOB))<=1024),
    PRIMARY KEY(run_id,position), UNIQUE(run_id,step)
);
CREATE TABLE maintenance_events (
    seq INTEGER PRIMARY KEY, run_id TEXT NOT NULL, position INTEGER NOT NULL,
    state TEXT NOT NULL, created_at INTEGER NOT NULL, exit_code INTEGER, stop TEXT,
    reason TEXT NOT NULL CHECK(length(CAST(reason AS BLOB))<=1024),
    FOREIGN KEY(run_id,position) REFERENCES maintenance_steps(run_id,position) ON DELETE CASCADE
);
CREATE INDEX maintenance_events_run ON maintenance_events(run_id,seq);
PRAGMA user_version=5;
";

// Constructed by the future sealed executor AFTER consuming its confirmation.
// Its existence is not permission to call a command; every spawn needs a fresh
// gate. Neither this type nor Session implements Deserialize.
pub(crate) struct Selection(Archive);
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Archive {
    version: u32,
    plan_id: String,
    confirmation_id: String,
    action: String,
    revision: String,
    context_sha256: String,
    selected: Vec<Step>,
    created_at: i64,
    expires_at: i64,
}
impl Archive {
    fn validate(&self) -> Result<(), String> {
        uuid::Uuid::parse_str(&self.plan_id).map_err(|_| "维护计划标识损坏。")?;
        uuid::Uuid::parse_str(&self.confirmation_id).map_err(|_| "维护确认标识损坏。")?;
        if self.version != 1
            || self.action != "cache_refresh"
            || !hex(&self.revision, 40)
            || !hex(&self.context_sha256, 64)
            || !matches!(
                self.selected.as_slice(),
                [Step::ThumbnailCache]
                    | [Step::Generators]
                    | [Step::ThumbnailCache, Step::Generators]
            )
            || self.created_at < 0
            || self.expires_at <= self.created_at
            || self.expires_at > self.created_at.saturating_add(TTL)
        {
            return Err("维护快照范围、版本或有效期无法核对。".into());
        }
        Ok(())
    }
    fn live_at(&self, at: i64) -> Result<(), String> {
        self.validate()?;
        if self.revision != engine::REVISION || at < self.created_at || at >= self.expires_at {
            return Err("维护确认已过期、时间回退或引擎变化，请重新检查。".into());
        }
        Ok(())
    }
}
impl Selection {
    pub(crate) fn new(
        plan_id: &str,
        confirmation_id: &str,
        selected: Vec<Step>,
        context_sha256: &str,
        created_at: i64,
        expires_at: i64,
    ) -> Result<Self, String> {
        let archive = Archive {
            version: 1,
            plan_id: plan_id.into(),
            confirmation_id: confirmation_id.into(),
            action: "cache_refresh".into(),
            revision: engine::REVISION.into(),
            context_sha256: context_sha256.into(),
            selected,
            created_at,
            expires_at,
        };
        archive.validate()?;
        Ok(Self(archive))
    }
}
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
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub selected_count: usize,
    pub succeeded_count: usize,
    pub uncertain_count: usize,
    pub not_started_count: usize,
    pub retained_for_review: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Item {
    pub position: usize,
    pub step: Step,
    pub state: String,
    pub finished_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub stop: Option<Stop>,
    pub reason: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Event {
    pub position: usize,
    pub state: String,
    pub created_at: i64,
    pub exit_code: Option<i32>,
    pub stop: Option<Stop>,
    pub reason: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Detail {
    #[serde(flatten)]
    pub summary: Summary,
    pub revision: String,
    pub snapshot_sha256: String,
    pub items: Vec<Item>,
    pub events: Vec<Event>,
}
struct Stored {
    id: String,
    state: String,
    created_at: i64,
    finished_at: Option<i64>,
    archive: Archive,
    digest: String,
    items: Vec<Item>,
    events: Vec<Event>,
}
impl Stored {
    fn summary(&self) -> Summary {
        Summary {
            id: self.id.clone(),
            plan_id: self.archive.plan_id.clone(),
            method: "quickLookRefresh",
            state: self.state.clone(),
            created_at: self.created_at,
            finished_at: self.finished_at,
            selected_count: self.items.len(),
            succeeded_count: self
                .items
                .iter()
                .filter(|i| i.state == "commandSucceeded")
                .count(),
            uncertain_count: self.items.iter().filter(|i| i.state == "uncertain").count(),
            not_started_count: self
                .items
                .iter()
                .filter(|i| matches!(i.state.as_str(), "notStarted" | "cancelled"))
                .count(),
            retained_for_review: matches!(
                self.state.as_str(),
                "running" | "interrupted" | "needsReview"
            ) || self.items.iter().any(|i| i.state == "uncertain"),
        }
    }
}
fn sql(e: rusqlite::Error) -> String {
    format!("维护记录无法写入或读取，停止后续操作：{e}")
}
fn hex(value: &str, n: usize) -> bool {
    value.len() == n
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn tx(db: &mut Connection) -> Result<Transaction<'_>, String> {
    if db
        .pragma_query_value::<i64, _>(None, "synchronous", |r| r.get(0))
        .map_err(sql)?
        < 2
    {
        return Err("维护记录需要 SQLite FULL 同步模式。".into());
    }
    db.transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql)
}
fn reason(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 1024 || value.contains('\0') {
        Err("维护结果原因无效或超过上限。".into())
    } else {
        Ok(())
    }
}
fn stop(raw: Option<String>) -> Result<Option<Stop>, String> {
    match raw.as_deref() {
        None => Ok(None),
        Some("cancelled") => Ok(Some(Stop::Cancelled)),
        Some("timedOut") => Ok(Some(Stop::TimedOut)),
        _ => Err("维护停止原因无法识别。".into()),
    }
}
fn stop_text(value: Option<Stop>) -> Option<&'static str> {
    value.map(|s| match s {
        Stop::Cancelled => "cancelled",
        Stop::TimedOut => "timedOut",
    })
}
fn result_valid(state: &str, code: Option<i32>, stopped: Option<Stop>) -> bool {
    match state {
        "commandSucceeded" => code == Some(0) && stopped.is_none(),
        "pending" | "intent" | "notStarted" => code.is_none() && stopped.is_none(),
        "cancelled" => code.is_none() && stopped == Some(Stop::Cancelled),
        "uncertain" => code != Some(0) || stopped.is_some(),
        _ => false,
    }
}
fn terminal(state: &str) -> bool {
    !matches!(state, "pending" | "intent")
}
fn final_state(items: &[Item]) -> &'static str {
    if items.iter().any(|i| i.state == "uncertain") {
        "needsReview"
    } else if items.iter().all(|i| i.state == "commandSucceeded") {
        "completed"
    } else if items.iter().any(|i| i.state == "cancelled") {
        "cancelled"
    } else if items.iter().any(|i| i.state == "commandSucceeded") {
        "partial"
    } else {
        "noChanges"
    }
}
fn load(db: &Connection, id: &str) -> Result<Stored, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "维护记录标识无效。")?;
    let raw = db.query_row("SELECT plan_id,method,state,created_at,finished_at,CASE WHEN length(CAST(snapshot AS BLOB))<=16384 THEN snapshot ELSE NULL END,snapshot_sha256,reserved_bytes FROM maintenance_runs WHERE id=?1", [id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,Option<i64>>(4)?,r.get::<_,Option<String>>(5)?,r.get::<_,String>(6)?,r.get::<_,i64>(7)?))).optional().map_err(sql)?.ok_or("维护记录不存在或已按期限清理。")?;
    let (plan, method, state, created_at, finished_at, body, digest, reserved) = raw;
    let body = body.ok_or("维护快照超过记录上限。")?;
    if hash(&body) != digest || reserved != (body.len() + RESERVE) as i64 {
        return Err("维护快照摘要或容量损坏。".into());
    }
    let archive: Archive = serde_json::from_str(&body).map_err(|_| "维护快照无法解码。")?;
    archive.validate()?;
    if plan != archive.plan_id
        || method != "quickLookRefresh"
        || created_at < archive.created_at
        || created_at >= archive.expires_at
    {
        return Err("维护快照与原计划不一致。".into());
    }
    let mut q = db.prepare("SELECT position,step,state,finished_at,exit_code,stop,reason FROM maintenance_steps WHERE run_id=?1 ORDER BY position LIMIT 3").map_err(sql)?;
    let items = q
        .query_map([id], |r| {
            Ok((
                r.get::<_, usize>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<i32>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
            ))
        })
        .map_err(sql)?
        .map(|r| {
            let (position, key, state, finished_at, exit_code, stopped, why) = r.map_err(sql)?;
            let step = *archive
                .selected
                .get(position)
                .ok_or("维护步骤超出原选择。")?;
            let stopped = stop(stopped)?;
            reason(&why)?;
            if key != step.id()
                || !result_valid(&state, exit_code, stopped)
                || terminal(&state) != finished_at.is_some()
            {
                return Err("维护步骤结果无法核对。".into());
            }
            Ok(Item {
                position,
                step,
                state,
                finished_at,
                exit_code,
                stop: stopped,
                reason: why,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if items.len() != archive.selected.len()
        || items.iter().enumerate().any(|(p, i)| i.position != p)
    {
        return Err("维护步骤遗漏或重复。".into());
    }
    let mut q = db.prepare("SELECT position,state,created_at,exit_code,stop,reason FROM maintenance_events WHERE run_id=?1 ORDER BY seq LIMIT 7").map_err(sql)?;
    let events = q
        .query_map([id], |r| {
            Ok((
                r.get::<_, usize>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<i32>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
            ))
        })
        .map_err(sql)?
        .map(|r| {
            let (position, state, created_at, exit_code, stopped, why) = r.map_err(sql)?;
            reason(&why)?;
            let stopped = stop(stopped)?;
            if position >= items.len()
                || created_at < 0
                || !result_valid(&state, exit_code, stopped)
            {
                return Err("维护事件内容无法核对。".into());
            }
            Ok(Event {
                position,
                state,
                created_at,
                exit_code,
                stop: stopped,
                reason: why,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if events.len() > 6
        || events.len() < items.len()
        || events[..items.len()]
            .iter()
            .enumerate()
            .any(|(position, e)| e.position != position || e.state != "pending")
    {
        return Err("维护事件超过固定步骤上限。".into());
    }
    let mut previous = vec!["pending"; items.len()];
    for e in &events {
        if e.state == "intent"
            && previous[..e.position]
                .iter()
                .any(|state| *state != "commandSucceeded")
        {
            return Err("维护命令事件顺序与前一步结果不一致。".into());
        }
        previous[e.position] = &e.state;
    }
    for item in &items {
        let chain: Vec<_> = events
            .iter()
            .filter(|e| e.position == item.position)
            .collect();
        if chain.is_empty()
            || chain.len() > 3
            || chain[0].state != "pending"
            || chain[0].created_at != created_at
            || chain.windows(2).any(|e| {
                !matches!(
                    (e[0].state.as_str(), e[1].state.as_str()),
                    ("pending", "intent" | "notStarted" | "cancelled")
                        | (
                            "intent",
                            "commandSucceeded" | "uncertain" | "notStarted" | "cancelled"
                        )
                )
            })
            || chain.last().is_none_or(|e| {
                e.state != item.state
                    || e.exit_code != item.exit_code
                    || e.stop != item.stop
                    || e.reason != item.reason
                    || item.finished_at.is_some_and(|at| at != e.created_at)
            })
        {
            return Err("维护逐步结果与事件不一致。".into());
        }
    }
    if finished_at.is_some_and(|at| at < 0)
        || (state == "running") != finished_at.is_none()
        || (state != "running"
            && (items.iter().any(|i| !terminal(&i.state))
                || (state != "interrupted" && state != final_state(&items))))
    {
        return Err("维护批次与逐步结果不一致。".into());
    }
    Ok(Stored {
        id: id.into(),
        state,
        created_at,
        finished_at,
        archive,
        digest,
        items,
        events,
    })
}
fn active(db: &Connection, session: &Session) -> Result<Stored, String> {
    let stored = load(db, session.id())?;
    if stored.state != "running"
        || stored.archive.plan_id != session.plan_id
        || stored.digest != session.digest
    {
        return Err("维护会话已结束或与原选择不一致。".into());
    }
    Ok(stored)
}
fn event(db: &Connection, id: &str, item: &Item, at: i64) -> Result<(), String> {
    if at < 0 {
        return Err("维护记录时间无效。".into());
    }
    reason(&item.reason)?;
    db.execute("INSERT INTO maintenance_events(run_id,position,state,created_at,exit_code,stop,reason) VALUES (?1,?2,?3,?4,?5,?6,?7)",params![id,item.position,item.state,at,item.exit_code,stop_text(item.stop),item.reason]).map_err(sql)?;
    Ok(())
}
fn update(db: &Connection, id: &str, item: &Item, at: i64) -> Result<(), String> {
    db.execute("UPDATE maintenance_steps SET state=?3,finished_at=?4,exit_code=?5,stop=?6,reason=?7 WHERE run_id=?1 AND position=?2",params![id,item.position,item.state,item.finished_at,item.exit_code,stop_text(item.stop),item.reason]).map_err(sql)?;
    event(db, id, item, at)
}
pub(crate) fn begin(
    db: &mut Connection,
    selection: &Selection,
    request_id: &str,
    at: i64,
) -> Result<Session, String> {
    uuid::Uuid::parse_str(request_id).map_err(|_| "维护请求标识无效。")?;
    selection.0.live_at(at)?;
    let body = serde_json::to_string(&selection.0).map_err(|_| "无法固定维护选择。")?;
    if body.len() > MAX_ARCHIVE {
        return Err("维护快照超过上限。".into());
    }
    let digest = hash(&body);
    let t = tx(db)?;
    crate::execution_admission::reserve(&t, (body.len() + RESERVE) as i64)?;
    t.execute(
        "INSERT INTO maintenance_runs VALUES (?1,?2,'quickLookRefresh','running',?3,NULL,?4,?5,?6)",
        params![
            request_id,
            selection.0.plan_id,
            at,
            body,
            digest,
            (body.len() + RESERVE) as i64
        ],
    )
    .map_err(sql)?;
    for (position, step) in selection.0.selected.iter().enumerate() {
        let item = Item {
            position,
            step: *step,
            state: "pending".into(),
            finished_at: None,
            exit_code: None,
            stop: None,
            reason: "尚未开始".into(),
        };
        t.execute(
            "INSERT INTO maintenance_steps VALUES (?1,?2,?3,'pending',NULL,NULL,NULL,?4)",
            params![request_id, position, step.id(), item.reason],
        )
        .map_err(sql)?;
        event(&t, request_id, &item, at)?;
    }
    t.commit().map_err(sql)?;
    Ok(Session {
        id: request_id.into(),
        plan_id: selection.0.plan_id.clone(),
        digest,
    })
}
pub(crate) fn intent(
    db: &mut Connection,
    session: &Session,
    step: Step,
    at: i64,
) -> Result<(), String> {
    let t = tx(db)?;
    let stored = active(&t, session)?;
    stored.archive.live_at(at)?;
    let mut item = stored
        .items
        .iter()
        .find(|i| i.step == step)
        .cloned()
        .ok_or("步骤不在原选择中。")?;
    if item.state != "pending"
        || stored.items[..item.position]
            .iter()
            .any(|i| i.state != "commandSucceeded")
    {
        return Err("维护步骤已开始或前一步未明确成功。".into());
    }
    item.state = "intent".into();
    item.reason = "已提交意图，尚无完成证据".into();
    update(&t, session.id(), &item, at)?;
    t.commit().map_err(sql)
}
pub(crate) fn check_intent(
    db: &Connection,
    session: &Session,
    step: Step,
    at: i64,
) -> Result<(), String> {
    let t = db.unchecked_transaction().map_err(sql)?;
    let stored = active(&t, session)?;
    stored.archive.live_at(at)?;
    if !stored
        .items
        .iter()
        .any(|i| i.step == step && i.state == "intent")
    {
        return Err("维护步骤尚无已提交意图。".into());
    }
    t.commit().map_err(sql)
}
pub(crate) fn record_attempt(
    db: &mut Connection,
    session: &Session,
    attempt: &Attempt,
    at: i64,
) -> Result<(), String> {
    let t = tx(db)?;
    let stored = active(&t, session)?;
    let mut item = stored
        .items
        .iter()
        .find(|i| i.step == attempt.step)
        .cloned()
        .ok_or("结果不属于原选择。")?;
    if !matches!(item.state.as_str(), "pending" | "intent")
        || (item.state != "intent" && attempt.outcome != Outcome::NotStarted)
    {
        return Err("维护结果缺少意图或已被记录，不能重放。".into());
    }
    item.state = match attempt.outcome {
        Outcome::CommandSucceeded => "commandSucceeded",
        Outcome::Uncertain => "uncertain",
        Outcome::NotStarted if attempt.stop == Some(Stop::Cancelled) => "cancelled",
        Outcome::NotStarted => "notStarted",
    }
    .into();
    if !result_valid(&item.state, attempt.exit_code, attempt.stop) {
        return Err("维护命令结果矛盾。".into());
    }
    item.exit_code = attempt.exit_code;
    item.stop = attempt.stop;
    item.reason = attempt.reason.into();
    item.finished_at = Some(at);
    // Results are collected even AFTER expiry or a wall-clock rollback. Seq,
    // not wall time, orders facts. Fresh authority is required only to start.
    update(&t, session.id(), &item, at)?;
    t.commit().map_err(sql)
}
pub(crate) fn finish(
    db: &mut Connection,
    session: &Session,
    cancelled: bool,
    at: i64,
) -> Result<(), String> {
    if at < 0 {
        return Err("维护结束时间无效。".into());
    }
    let t = tx(db)?;
    let mut stored = active(&t, session)?;
    if stored.items.iter().any(|i| i.state == "intent") {
        return Err("维护命令仍有未收集意图，不能结束或放弃结果。".into());
    }
    for item in stored.items.iter_mut().filter(|i| i.state == "pending") {
        item.state = if cancelled { "cancelled" } else { "notStarted" }.into();
        item.stop = cancelled.then_some(Stop::Cancelled);
        item.finished_at = Some(at);
        item.reason = if cancelled {
            "用户取消，后续步骤未启动"
        } else {
            "前序条件或结果不足，后续步骤未启动"
        }
        .into();
        update(&t, session.id(), item, at)?;
    }
    t.execute(
        "UPDATE maintenance_runs SET state=?2,finished_at=?3 WHERE id=?1",
        params![session.id(), final_state(&stored.items), at],
    )
    .map_err(sql)?;
    t.commit().map_err(sql)
}
fn interrupt(db: &Connection, session: &Session, at: i64) -> Result<(), String> {
    let stored = active(db, session)?;
    for mut item in stored.items.into_iter().filter(|i| !terminal(&i.state)) {
        let intent = item.state == "intent";
        item.state = if intent { "uncertain" } else { "notStarted" }.into();
        item.reason = if intent {
            "已记录意图但缺少结果，禁止自动重试"
        } else {
            "维护已中断，此步骤未启动"
        }
        .into();
        item.finished_at = Some(at);
        update(db, session.id(), &item, at)?;
    }
    db.execute(
        "UPDATE maintenance_runs SET state='interrupted',finished_at=?2 WHERE id=?1",
        params![session.id(), at],
    )
    .map_err(sql)?;
    Ok(())
}
// Startup or a fully unwound worker only; NEVER abandon a live system command.
pub(crate) fn reconcile(db: &mut Connection, at: i64) -> Result<(), String> {
    let t = tx(db)?;
    let sessions = {
        let mut q = t.prepare("SELECT id,plan_id,snapshot_sha256 FROM maintenance_runs WHERE state='running' LIMIT 65").map_err(sql)?;
        let rows = q
            .query_map([], |r| {
                Ok(Session {
                    id: r.get(0)?,
                    plan_id: r.get(1)?,
                    digest: r.get(2)?,
                })
            })
            .map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)?
    };
    if sessions.len() > 1 {
        return Err("存在多个运行中的维护批次，保留原始记录。".into());
    }
    for session in sessions {
        interrupt(&t, &session, at)?;
    }
    t.commit().map_err(sql)
}
pub(crate) fn stop_worker(db: &mut Connection, session: &Session, at: i64) -> Result<(), String> {
    let t = tx(db)?;
    interrupt(&t, session, at)?;
    t.commit().map_err(sql)
}
pub(crate) fn detail(db: &Connection, id: &str) -> Result<Detail, String> {
    let t = db.unchecked_transaction().map_err(sql)?;
    let stored = load(&t, id)?;
    let summary = stored.summary();
    t.commit().map_err(sql)?;
    Ok(Detail {
        summary,
        revision: stored.archive.revision,
        snapshot_sha256: stored.digest,
        items: stored.items,
        events: stored.events,
    })
}
pub(crate) fn list(db: &Connection) -> Result<Vec<Summary>, String> {
    let t = db.unchecked_transaction().map_err(sql)?;
    let ids = {
        let mut q = t
            .prepare("SELECT id FROM maintenance_runs ORDER BY created_at DESC,rowid DESC LIMIT 64")
            .map_err(sql)?;
        let rows = q.query_map([], |r| r.get::<_, String>(0)).map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)?
    };
    let out = ids
        .iter()
        .map(|id| load(&t, id).map(|s| s.summary()))
        .collect();
    t.commit().map_err(sql)?;
    out
}
pub(crate) fn prune(db: &Transaction<'_>, before: i64) -> Result<(), String> {
    // Validate ALL candidates before deleting any. The settings/startup owner
    // supplies the surrounding transaction, so corruption or a later failure
    // cannot partially prune evidence or persist only part of new settings.
    let ids = {
        let mut q = db.prepare("SELECT id FROM maintenance_runs WHERE state IN ('completed','partial','noChanges','cancelled') AND finished_at<?1 AND NOT EXISTS(SELECT 1 FROM maintenance_steps i WHERE i.run_id=maintenance_runs.id AND i.state IN ('intent','uncertain')) LIMIT 65").map_err(sql)?;
        let rows = q
            .query_map([before], |r| r.get::<_, String>(0))
            .map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)?
    };
    if ids.len() > 64 {
        return Err("维护保留记录超过上限，不能自动丢弃证据。".into());
    }
    for id in ids {
        load(db, &id)?;
    }
    db.execute("DELETE FROM maintenance_runs WHERE state IN ('completed','partial','noChanges','cancelled') AND finished_at<?1 AND NOT EXISTS(SELECT 1 FROM maintenance_steps i WHERE i.run_id=maintenance_runs.id AND i.state IN ('intent','uncertain'))",[before]).map_err(sql)?;
    Ok(())
}

#[cfg(test)]
mod tests;
