use super::*;
use rusqlite::{params, Connection};
const SCHEMA:&str="CREATE TABLE IF NOT EXISTS folder_schedules(id TEXT PRIMARY KEY,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=1048576),baseline TEXT CHECK(baseline IS NULL OR length(CAST(baseline AS BLOB))<=8388608)); CREATE TABLE IF NOT EXISTS folder_scan_runs(id TEXT PRIMARY KEY,state TEXT NOT NULL,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=524288));";
fn exists(db: &Connection) -> Result<bool, String> {
    let n:u32=db.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('folder_schedules','folder_scan_runs')",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if n != 0 && n != 2 {
        return Err("目录扫描表不完整，原记录保留。".into());
    }
    Ok(n == 2)
}
pub(super) fn watches(db: &Connection) -> Result<Vec<Saved>, String> {
    if !exists(db)? {
        return Ok(vec![]);
    }
    let mut s=db.prepare("SELECT id,CASE WHEN length(CAST(body AS BLOB))<=1048576 THEN body END FROM folder_schedules ORDER BY rowid LIMIT 9").map_err(|e|e.to_string())?;
    let rows = s
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut out = vec![];
    for r in rows {
        let (id, body) = r.map_err(|e| e.to_string())?;
        let saved: Saved = serde_json::from_str(&body.ok_or("目录扫描设置超过读取范围。")?)
            .map_err(|_| "目录扫描设置无法核对。")?;
        let c = &saved.config;
        if saved.version != 1
            || c.id != id
            || !valid_id(&id)
            || !valid_id(&saved.generation)
            || !valid_root(&c.root)
            || !valid_interval(c.interval_minutes)
            || c.updated_at < 0
            || c.last_attempt_at.is_some_and(|n| n < 0)
            || saved.nodes.is_empty()
            || saved.nodes.len() > 128
            || saved
                .nodes
                .last()
                .is_none_or(|n| n.path != Path::new(&c.root))
        {
            return Err("目录扫描设置版本或范围无法核对，未继续扫描。".into());
        }
        out.push(saved);
    }
    if out.len() > MAX_WATCHES {
        return Err("目录扫描任务超过本轮八个目录的容量。".into());
    }
    Ok(out)
}
pub(super) fn records(db: &Connection) -> Result<Vec<Record>, String> {
    if !exists(db)? {
        return Ok(vec![]);
    }
    let mut s=db.prepare("SELECT id,state,CASE WHEN length(CAST(body AS BLOB))<=524288 THEN body END FROM folder_scan_runs ORDER BY rowid DESC LIMIT 65").map_err(|e|e.to_string())?;
    let rows = s
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = vec![];
    for r in rows {
        let (id, state, body) = r.map_err(|e| e.to_string())?;
        let record: Record = serde_json::from_str(&body.ok_or("目录扫描记录超过读取范围。")?)
            .map_err(|_| "目录扫描记录无法核对。")?;
        if record.version != 1
            || record.id != id
            || !valid_id(&id)
            || !valid_id(&record.watch_id)
            || !valid_id(&record.generation)
            || !valid_root(&record.root)
            || serde_json::to_value(record.state).map_err(|e| e.to_string())? != state
            || record.started_at < 0
            || record.finished_at.is_some_and(|n| n < 0)
            || record.summary.as_ref().is_some_and(|s| {
                s.files > 2000
                    || s.visited > 10000
                    || s.changes.len() > MAX_CHANGES
                    || s.logical_bytes > 9_007_199_254_740_991
                    || s.changes.iter().any(|c| {
                        !valid_root(&c.path) || !Path::new(&c.path).starts_with(&record.root)
                    })
            })
        {
            return Err("目录扫描记录版本或范围无法核对，原记录保留。".into());
        }
        out.push(record);
    }
    if out.len() > MAX_RUNS {
        return Err("目录扫描历史超过读取容量。".into());
    }
    Ok(out)
}
fn state_text(state: State) -> String {
    serde_json::to_value(state)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}
fn write_record(db: &Connection, r: &Record) -> Result<(), String> {
    if db
        .execute(
            "UPDATE folder_scan_runs SET state=?2,body=?3 WHERE id=?1 AND state='running'",
            params![
                r.id,
                state_text(r.state),
                serde_json::to_string(r).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?
        != 1
    {
        return Err("目录扫描结果未完整保存，请刷新历史。".into());
    }
    Ok(())
}
pub(super) fn configure(db: &mut Connection, edit: Edit) -> Result<Watch, String> {
    let current = watches(db)?;
    let settings = db::settings(db)?;
    if !settings.acknowledged || !valid_interval(edit.interval_minutes) || !valid_root(&edit.root) {
        return Err("请确认首次使用说明，并选择有效目录和扫描间隔。".into());
    }
    let old = edit
        .id
        .as_ref()
        .map(|id| {
            current
                .iter()
                .find(|w| &w.config.id == id)
                .ok_or("目录扫描任务已变化。".to_string())
        })
        .transpose()?;
    if old.is_none() && current.len() >= MAX_WATCHES {
        return Err("本轮最多保存八个扫描目录。".into());
    }
    // An unavailable volume or newly protected path must never prevent pause.
    let nodes = if old.is_some_and(|w| !edit.enabled && w.config.root == edit.root) {
        old.unwrap().nodes.clone()
    } else {
        bind(&edit.root, &settings.protected_paths)?
    };
    if old.is_some_and(|w| w.config.root == edit.root && w.nodes != nodes) {
        return Err("目录身份已变化，请移除旧扫描任务后重新选择并添加。".into());
    }
    if current.iter().any(|w| {
        Some(&w.config.id) != edit.id.as_ref()
            && (w.config.root == edit.root
                || w.nodes
                    .last()
                    .zip(nodes.last())
                    .is_some_and(|(a, b)| (a.dev, a.ino) == (b.dev, b.ino)))
    }) {
        return Err("这个物理目录已有扫描任务。".into());
    }
    let changed = old.is_none_or(|w| w.config.root != edit.root || w.nodes != nodes);
    let saved = Saved {
        version: 1,
        config: Watch {
            id: old
                .map(|w| w.config.id.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            root: edit.root,
            enabled: edit.enabled,
            interval_minutes: edit.interval_minutes,
            updated_at: db::now(),
            last_attempt_at: if changed || old.is_some_and(|w| !w.config.enabled && edit.enabled) {
                None
            } else {
                old.and_then(|w| w.config.last_attempt_at)
            },
        },
        generation: uuid::Uuid::new_v4().to_string(),
        nodes,
    };
    let t = db.transaction().map_err(|e| e.to_string())?;
    t.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    t.execute("INSERT INTO folder_schedules(id,body,baseline) VALUES(?1,?2,NULL) ON CONFLICT(id) DO UPDATE SET body=excluded.body,baseline=CASE WHEN ?3 THEN NULL ELSE folder_schedules.baseline END",params![saved.config.id,serde_json::to_string(&saved).map_err(|e|e.to_string())?,changed]).map_err(|e|e.to_string())?;
    t.commit().map_err(|e| e.to_string())?;
    Ok(saved.config)
}
pub(super) fn remove(db: &Connection, id: &str) -> Result<(), String> {
    if !watches(db)?.iter().any(|w| w.config.id == id) {
        return Err("目录扫描任务不存在。".into());
    }
    db.execute("DELETE FROM folder_schedules WHERE id=?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub(super) fn begin(
    db: &mut Connection,
    watch: &Saved,
    trigger: Trigger,
    request: &str,
    now: i64,
) -> Result<Record, String> {
    let t = db.transaction().map_err(|e| e.to_string())?;
    let mut current = watches(&t)?
        .into_iter()
        .find(|w| w.config.id == watch.config.id && w.generation == watch.generation)
        .ok_or("目录扫描设置已变化。")?;
    let rows = records(&t)?;
    if rows.iter().any(|r| r.state == State::Running) {
        return Err("目录扫描仍在进行。".into());
    }
    if trigger == Trigger::Scheduled && !due(&current, now) {
        return Err("目录扫描未到期或已暂停。".into());
    }
    let r = Record {
        version: 1,
        id: request.into(),
        watch_id: watch.config.id.clone(),
        generation: watch.generation.clone(),
        root: watch.config.root.clone(),
        trigger,
        started_at: now,
        finished_at: None,
        state: State::Running,
        summary: None,
        error: None,
    };
    current.config.last_attempt_at = Some(now);
    t.execute(
        "UPDATE folder_schedules SET body=?2 WHERE id=?1",
        params![
            current.config.id,
            serde_json::to_string(&current).map_err(|e| e.to_string())?
        ],
    )
    .map_err(|e| e.to_string())?;
    t.execute("DELETE FROM folder_scan_runs WHERE state!='running' AND id NOT IN (SELECT id FROM folder_scan_runs ORDER BY rowid DESC LIMIT 63)",[]).map_err(|e|e.to_string())?;
    t.execute(
        "INSERT INTO folder_scan_runs VALUES(?1,'running',?2)",
        params![r.id, serde_json::to_string(&r).map_err(|e| e.to_string())?],
    )
    .map_err(|e| e.to_string())?;
    t.commit().map_err(|e| e.to_string())?;
    Ok(r)
}
fn baseline(db: &Connection, watch: &Saved) -> Result<Option<Baseline>, String> {
    let (present,raw):(bool,Option<String>)=db.query_row("SELECT baseline IS NOT NULL,CASE WHEN length(CAST(baseline AS BLOB))<=8388608 THEN baseline END FROM folder_schedules WHERE id=?1",[&watch.config.id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
    if present && raw.is_none() {
        return Err("目录比较基线超过读取范围，原基线保留。".into());
    }
    let Some(raw) = raw else {
        return Ok(None);
    };
    let b: Baseline = serde_json::from_str(&raw).map_err(|_| "目录比较基线无法核对。")?;
    if b.version != 1
        || b.root != watch.config.root
        || b.nodes != watch.nodes
        || b.files.len() > 2000
        || b.files.iter().any(|(path, f)| {
            !valid_root(path)
                || !Path::new(path).starts_with(&b.root)
                || path == &b.root
                || f.size > 9_007_199_254_740_991
        })
    {
        return Err("目录比较基线版本或范围无法核对。".into());
    }
    Ok(Some(b))
}
pub(super) fn finish(
    db: &mut Connection,
    watch: &Saved,
    mut r: Record,
    rules: &[String],
    result: Result<Observed, String>,
) -> Result<Record, String> {
    let t = db.transaction().map_err(|e| e.to_string())?;
    r.finished_at = Some(db::now());
    let current = watches(&t)?
        .into_iter()
        .find(|w| w.config.id == watch.config.id && w.generation == watch.generation);
    let settings = db::settings(&t)?;
    if current.is_none() || !settings.acknowledged || settings.protected_paths != rules {
        r.state = State::Superseded;
        r.error = Some("设置或保护已变化，未保存旧扫描结果或替换比较基线。".into());
    } else {
        match result {
            Ok(observed) => {
                let previous = baseline(&t, watch)?;
                let summary = summarize(&observed, previous.as_ref())?;
                r.state = if observed.cancelled {
                    State::Cancelled
                } else if summary.complete {
                    State::Completed
                } else {
                    State::Partial
                };
                if summary.complete {
                    t.execute(
                        "UPDATE folder_schedules SET baseline=?2 WHERE id=?1",
                        params![
                            watch.config.id,
                            serde_json::to_string(&observed.baseline).map_err(|e| e.to_string())?
                        ],
                    )
                    .map_err(|e| e.to_string())?;
                }
                r.summary = Some(summary);
            }
            Err(error) => {
                r.state = State::Failed;
                r.error = Some(error.chars().take(8192).collect());
            }
        }
    }
    write_record(&t, &r)?;
    t.commit().map_err(|e| e.to_string())?;
    Ok(r)
}
pub(super) fn reconcile(db: &Connection, active: &[String]) -> Result<(), String> {
    for r in records(db)?
        .into_iter()
        .filter(|r| r.state == State::Running && !active.contains(&r.id))
    {
        interrupt(db, &r.id)?;
    }
    Ok(())
}
pub(super) fn interrupt(db: &Connection, id: &str) -> Result<(), String> {
    // Re-read under the DB lock; never overwrite a later terminal result.
    if let Some(mut r) = records(db)?
        .into_iter()
        .find(|latest| latest.id == id && latest.state == State::Running)
    {
        r.state = State::Interrupted;
        r.finished_at = Some(db::now());
        r.error = Some(
            "这次扫描曾中断；旧完整基线保留，不补跑该次任务。后续仍按已保存周期判断到期。".into(),
        );
        write_record(db, &r)?;
    }
    Ok(())
}
