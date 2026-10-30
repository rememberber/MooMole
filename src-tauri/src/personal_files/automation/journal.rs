use super::*;
use rusqlite::{params, Connection};
const SCHEMA:&str="CREATE TABLE IF NOT EXISTS scheduled_file_policies(id TEXT PRIMARY KEY,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=262144));CREATE TABLE IF NOT EXISTS scheduled_file_runs(id TEXT PRIMARY KEY,state TEXT NOT NULL,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=65536));";
fn exists(db: &Connection) -> Result<bool, String> {
    let count:u32=db.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('scheduled_file_policies','scheduled_file_runs')",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if count != 0 && count != 2 {
        return Err("定期处理数据表不完整，原记录保留。".into());
    }
    Ok(count == 2)
}
fn full(db: &Connection) -> Result<(), String> {
    if db
        .pragma_query_value::<i64, _>(None, "synchronous", |r| r.get(0))
        .map_err(|e| e.to_string())?
        < 2
    {
        return Err("定期处理需要 FULL 同步记录。".into());
    }
    Ok(())
}
pub(super) fn policies(db: &Connection) -> Result<Vec<Saved>, String> {
    if !exists(db)? {
        return Ok(vec![]);
    }
    let mut q=db.prepare("SELECT id,CASE WHEN length(CAST(body AS BLOB))<=262144 THEN body END FROM scheduled_file_policies ORDER BY rowid LIMIT 9").map_err(|e|e.to_string())?;
    let rows = q
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let (id, body) = row.map_err(|e| e.to_string())?;
        let saved: Saved = serde_json::from_str(&body.ok_or("定期规则超过读取范围。")?)
            .map_err(|_| "定期规则无法核对。")?;
        saved.validate()?;
        if saved.policy.id != id {
            return Err("定期规则标识无法核对。".into());
        }
        out.push(saved);
    }
    if out.len() > 8 {
        return Err("本轮最多保存八条定期处理规则。".into());
    }
    Ok(out)
}
pub(super) fn records(db: &Connection) -> Result<Vec<Record>, String> {
    if !exists(db)? {
        return Ok(vec![]);
    }
    let mut q=db.prepare("SELECT id,state,CASE WHEN length(CAST(body AS BLOB))<=65536 THEN body END FROM scheduled_file_runs ORDER BY rowid DESC LIMIT 65").map_err(|e|e.to_string())?;
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let (id, state, body) = row.map_err(|e| e.to_string())?;
        let r: Record = serde_json::from_str(&body.ok_or("定期记录超过读取范围。")?)
            .map_err(|_| "定期记录无法核对。")?;
        r.rule.validate()?;
        if r.id != id
            || r.state != state
            || !valid_id(&r.id)
            || !valid_id(&r.policy_id)
            || !valid_id(&r.generation)
            || !["manual", "scheduled"].contains(&r.trigger.as_str())
            || !["running", "completed", "paused", "interrupted"].contains(&r.state.as_str())
            || r.started_at < 0
            || r.finished_at.is_some_and(|n| n < r.started_at)
            || r.matched_files > 2000
            || r.selected_files > 20
            || r.held_by_limits > 2000
            || r.execution_id.as_ref().is_some_and(|s| !valid_id(s))
            || r.reason.len() > 2000
        {
            return Err("定期处理记录版本或范围无法核对。".into());
        }
        out.push(r);
    }
    if out.len() > 64 {
        return Err("定期处理记录超过读取容量。".into());
    }
    Ok(out)
}
fn put_policy(db: &Connection, saved: &Saved) -> Result<(), String> {
    saved.validate()?;
    db.execute("INSERT INTO scheduled_file_policies(id,body) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body",params![saved.policy.id,serde_json::to_string(saved).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    Ok(())
}
fn put_record(db: &Connection, r: &Record) -> Result<(), String> {
    if db
        .execute(
            "UPDATE scheduled_file_runs SET state=?2,body=?3 WHERE id=?1 AND state='running'",
            params![
                r.id,
                r.state,
                serde_json::to_string(r).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?
        != 1
    {
        return Err("定期处理记录未保存，请核对历史。".into());
    }
    Ok(())
}
pub(super) fn save(db: &mut Connection, offered: Offered, home: &Path) -> Result<Policy, String> {
    full(db)?;
    let d = offered.draft;
    let now = db::now();
    if now < d.created_at
        || now >= d.expires_at
        || offered.issued.elapsed() >= Duration::from_secs(120)
    {
        return Err("定期规则预览已过期，请重新检查。".into());
    }
    let current = policies(db)?;
    let old = d
        .policy_id
        .as_ref()
        .map(|id| {
            current
                .iter()
                .find(|p| &p.policy.id == id)
                .ok_or("原定期规则已移除。".to_string())
        })
        .transpose()?;
    if old.is_none() && current.len() >= 8 {
        return Err("本轮最多保存八条定期处理规则。".into());
    }
    let (nodes, destination_nodes) = bind(&d.rule, home, &db::settings(db)?)?;
    if nodes != offered.nodes
        || destination_nodes != offered.destination_nodes
        || old.is_some_and(|p| {
            (p.policy.rule.root == d.rule.root && p.nodes != nodes)
                || (p.policy.rule.destination == d.rule.destination
                    && p.destination_nodes != destination_nodes)
        })
    {
        return Err("目录身份已变化，请移除旧规则后重新选择。".into());
    }
    if current
        .iter()
        .filter(|p| Some(&p.policy.id) != d.policy_id.as_ref())
        .any(|p| {
            nodes_overlap(&p.nodes, &nodes)
                || p.destination_nodes
                    .as_ref()
                    .is_some_and(|d| nodes_overlap(d, &nodes))
                || destination_nodes
                    .as_ref()
                    .is_some_and(|d| nodes_overlap(d, &p.nodes))
                || overlap(&p.policy.rule.root, &d.rule.root)
                || p.policy
                    .rule
                    .destination
                    .as_ref()
                    .is_some_and(|v| overlap(v, &d.rule.root))
                || d.rule
                    .destination
                    .as_ref()
                    .is_some_and(|v| overlap(v, &p.policy.rule.root))
        })
    {
        return Err("定期处理的源目录不能重叠，也不能把另一规则的目标作为源目录。".into());
    }
    let saved = Saved {
        version: 1,
        policy: Policy {
            id: d
                .policy_id
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            rule: d.rule,
            enabled: true,
            updated_at: now,
            last_attempt_at: None,
        },
        generation: uuid::Uuid::new_v4().to_string(),
        nodes,
        destination_nodes,
    };
    let tx = db.transaction().map_err(|e| e.to_string())?;
    tx.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    put_policy(&tx, &saved)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(saved.policy)
}
pub(super) fn pause(db: &Connection, id: &str) -> Result<(), String> {
    full(db)?;
    let mut saved = policies(db)?
        .into_iter()
        .find(|p| p.policy.id == id)
        .ok_or("定期规则已移除。")?;
    if saved.policy.enabled {
        saved.policy.enabled = false;
        saved.generation = uuid::Uuid::new_v4().to_string();
        put_policy(db, &saved)?;
    }
    Ok(())
}
pub(super) fn remove(db: &Connection, id: &str) -> Result<(), String> {
    full(db)?;
    if !policies(db)?.iter().any(|p| p.policy.id == id) {
        return Err("定期规则已移除。".into());
    }
    db.execute("DELETE FROM scheduled_file_policies WHERE id=?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub(super) fn begin(
    db: &mut Connection,
    saved: &Saved,
    id: &str,
    trigger: &str,
) -> Result<Record, String> {
    full(db)?;
    let current = policies(db)?
        .into_iter()
        .find(|p| {
            p.policy.id == saved.policy.id && p.generation == saved.generation && p.policy.enabled
        })
        .ok_or("定期规则已暂停或变化。")?;
    let now = db::now();
    let r = Record {
        id: id.into(),
        policy_id: saved.policy.id.clone(),
        generation: saved.generation.clone(),
        rule: saved.policy.rule.clone(),
        trigger: trigger.into(),
        started_at: now,
        finished_at: None,
        state: "running".into(),
        matched_files: 0,
        selected_files: 0,
        held_by_limits: 0,
        execution_id: None,
        reason: "扫描与处理尚未结束。".into(),
    };
    let mut updated = current;
    updated.policy.last_attempt_at = Some(now);
    let tx = db.transaction().map_err(|e| e.to_string())?;
    tx.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM scheduled_file_runs WHERE id IN(SELECT id FROM scheduled_file_runs WHERE state<>'running' ORDER BY rowid DESC LIMIT -1 OFFSET 63)",[]).map_err(|e|e.to_string())?;
    tx.execute(
        "INSERT INTO scheduled_file_runs(id,state,body) VALUES(?1,'running',?2)",
        params![id, serde_json::to_string(&r).map_err(|e| e.to_string())?],
    )
    .map_err(|e| e.to_string())?;
    put_policy(&tx, &updated)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(r)
}
fn modify(db: &Connection, id: &str, change: impl FnOnce(&mut Record)) -> Result<(), String> {
    full(db)?;
    let mut r = records(db)?
        .into_iter()
        .find(|r| r.id == id && r.state == "running")
        .ok_or("定期记录已变化。")?;
    change(&mut r);
    put_record(db, &r)
}
pub(super) fn selection(
    db: &Connection,
    id: &str,
    matched: usize,
    selected: usize,
    held: usize,
) -> Result<(), String> {
    modify(db, id, |r| {
        r.matched_files = matched;
        r.selected_files = selected;
        r.held_by_limits = held;
    })
}
pub(super) fn execution(db: &Connection, id: &str, execution: &str) -> Result<(), String> {
    modify(db, id, |r| r.execution_id = Some(execution.into()))
}
pub(super) fn finish(
    db: &mut Connection,
    id: &str,
    result: Result<(), String>,
) -> Result<(), String> {
    full(db)?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let mut r = records(&tx)?
        .into_iter()
        .find(|r| r.id == id && r.state == "running")
        .ok_or("定期处理记录已变化。")?;
    r.finished_at = Some(db::now().max(r.started_at));
    r.state = if result.is_ok() {
        "completed"
    } else {
        "paused"
    }
    .into();
    r.reason = match result {
        Ok(_) => {
            if r.selected_files == 0 {
                "没有符合本次规则及上限的文件，未作修改。".into()
            } else {
                "本次所选处理已完成，请在逐项历史核对实际收据。".into()
            }
        }
        Err(e) => super::super::writer::bounded(&e),
    };
    if r.state != "completed" {
        if let Some(saved) = policies(&tx)?
            .into_iter()
            .find(|p| p.policy.id == r.policy_id && p.generation == r.generation)
        {
            pause(&tx, &saved.policy.id)?;
        }
    }
    put_record(&tx, &r)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}
pub(super) fn reconcile(db: &mut Connection, active: &[String]) -> Result<(), String> {
    if !exists(db)? {
        return Ok(());
    }
    full(db)?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    for mut r in records(&tx)?
        .into_iter()
        .filter(|r| r.state == "running" && !active.contains(&r.id))
    {
        r.state = "interrupted".into();
        r.finished_at = Some(db::now().max(r.started_at));
        r.reason = "上次处理已中断，规则已暂停；请核对执行历史，不会自动重放。".into();
        if policies(&tx)?
            .iter()
            .any(|p| p.policy.id == r.policy_id && p.generation == r.generation)
        {
            pause(&tx, &r.policy_id)?;
        }
        put_record(&tx, &r)?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}
pub(super) fn initialize(db: &mut Connection) -> Result<(), String> {
    reconcile(db, &[])?;
    for p in policies(db)?.into_iter().filter(|p| p.policy.enabled) {
        pause(db, &p.policy.id)?;
    }
    Ok(())
}
