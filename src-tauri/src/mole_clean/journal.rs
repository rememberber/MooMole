use super::plan::Plan;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
pub(crate) const RESERVED_BYTES: i64 = 12 * 1024 * 1024;
pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS mole_clean_runs(id TEXT PRIMARY KEY, plan TEXT NOT NULL, state TEXT NOT NULL, created_at INTEGER NOT NULL, finished_at INTEGER, before_bytes INTEGER, after_bytes INTEGER, error TEXT, diagnostic TEXT NOT NULL DEFAULT '', reviewed_at INTEGER); CREATE TABLE IF NOT EXISTS mole_clean_events(run_id TEXT NOT NULL REFERENCES mole_clean_runs(id), seq INTEGER NOT NULL, kind TEXT NOT NULL, path TEXT NOT NULL, detail TEXT NOT NULL, token TEXT NOT NULL, PRIMARY KEY(run_id,seq));";
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub seq: i64,
    pub kind: String,
    pub path: String,
    pub detail: String,
    pub token: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: String,
    pub plan: Plan,
    pub state: String,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub before_bytes: Option<u64>,
    pub after_bytes: Option<u64>,
    pub error: Option<String>,
    pub diagnostic: String,
    pub reviewed_at: Option<i64>,
    pub events: Vec<Event>,
    pub events_truncated: bool,
}
pub fn init(db: &Connection) -> Result<(), String> {
    db.execute_batch(SCHEMA).map_err(|e| e.to_string())
}
pub fn recover(db: &Connection) -> Result<(), String> {
    init(db)?;
    db.execute("UPDATE mole_clean_runs SET state='interrupted',finished_at=?1,error='应用退出前未取得完整结果；未完成的处理需要核查，不会自动重试。' WHERE state='running'", [crate::db::now()]).map_err(|e|e.to_string())?;
    Ok(())
}
pub fn create(
    db: &mut Connection,
    id: &str,
    plan: &Plan,
    before: Option<u64>,
) -> Result<(), String> {
    init(db)?;
    let t = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    crate::execution_admission::reserve(&t, RESERVED_BYTES)?;
    let blocked: bool=t.query_row("SELECT EXISTS(SELECT 1 FROM mole_clean_runs WHERE state='running' OR (state IN ('interrupted','uncertain') AND reviewed_at IS NULL))",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if blocked {
        return Err("完整清理仍有运行中或待核查记录，请先查看清理记录。".into());
    }
    let count: i64 = t
        .query_row("SELECT count(*) FROM mole_clean_runs", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if count >= 64 {
        return Err("完整清理记录已达上限，请先导出并整理历史。".into());
    }
    t.execute("INSERT INTO mole_clean_runs(id,plan,state,created_at,before_bytes) VALUES(?1,?2,'running',?3,?4)",params![id,serde_json::to_string(plan).map_err(|e|e.to_string())?,crate::db::now(),before.map(|b|b as i64)]).map_err(|e|e.to_string())?;
    t.commit().map_err(|e| e.to_string())
}
pub fn append(
    db: &Connection,
    id: &str,
    kind: &str,
    path: &str,
    detail: &str,
    token: &str,
) -> Result<(), String> {
    let (count,size):(i64,i64)=db.query_row("SELECT count(*),coalesce(sum(length(CAST(path AS BLOB))+length(CAST(detail AS BLOB))+length(token)+128),0) FROM mole_clean_events WHERE run_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
    if count >= 20_000
        || size + (path.len() + detail.len() + token.len() + 128) as i64 > 8 * 1024 * 1024
    {
        return Err("完整清理记录达到容量边界，已停止后续操作。".into());
    }
    db.execute("INSERT INTO mole_clean_events(run_id,seq,kind,path,detail,token) SELECT ?1,?2,?3,?4,?5,?6 WHERE EXISTS(SELECT 1 FROM mole_clean_runs WHERE id=?1 AND state='running')",params![id,count+1,kind,path,detail,token]).map_err(|e|e.to_string()).and_then(|n|if n==1{Ok(())}else{Err("清理批次已结束，未继续操作。".into())})
}
pub fn finish(
    db: &Connection,
    id: &str,
    state: &str,
    after: Option<u64>,
    error: Option<&str>,
) -> Result<(), String> {
    db.execute("UPDATE mole_clean_runs SET state=?2,finished_at=?3,after_bytes=?4,error=?5 WHERE id=?1 AND state='running'",params![id,state,crate::db::now(),after.map(|b|b as i64),error]).map_err(|e|e.to_string())?;
    Ok(())
}
pub fn read(db: &Connection, id: &str) -> Result<Option<Record>, String> {
    read_limited(db, id, 2000)
}
pub(crate) fn read_complete(db: &Connection, id: &str) -> Result<Option<Record>, String> {
    let r = read_limited(db, id, 20000)?;
    if r.as_ref().is_some_and(|r| r.events_truncated) {
        return Err("完整清理事件超过归档范围，未释放记录。".into());
    }
    Ok(r)
}
fn read_limited(db: &Connection, id: &str, limit: usize) -> Result<Option<Record>, String> {
    init(db)?;
    let raw=db.query_row("SELECT plan,state,created_at,finished_at,before_bytes,after_bytes,error,diagnostic,reviewed_at FROM mole_clean_runs WHERE id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,Option<i64>>(3)?,r.get::<_,Option<u64>>(4)?,r.get::<_,Option<u64>>(5)?,r.get::<_,Option<String>>(6)?,r.get::<_,String>(7)?,r.get::<_,Option<i64>>(8)?))).optional().map_err(|e|e.to_string())?;
    let Some((
        plan,
        state,
        created_at,
        finished_at,
        before_bytes,
        after_bytes,
        error,
        diagnostic,
        reviewed_at,
    )) = raw
    else {
        return Ok(None);
    };
    let mut s=db.prepare(&format!("SELECT seq,kind,path,detail,token FROM mole_clean_events WHERE run_id=?1 ORDER BY seq DESC LIMIT {}",limit+1)).map_err(|e|e.to_string())?;
    let mut events = s
        .query_map([id], |r| {
            Ok(Event {
                seq: r.get(0)?,
                kind: r.get(1)?,
                path: r.get(2)?,
                detail: r.get(3)?,
                token: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let events_truncated = events.len() > limit;
    events.truncate(limit);
    events.reverse();
    Ok(Some(Record {
        id: id.into(),
        plan: serde_json::from_str(&plan).map_err(|e| e.to_string())?,
        state,
        created_at,
        finished_at,
        before_bytes,
        after_bytes,
        error,
        diagnostic,
        reviewed_at,
        events,
        events_truncated,
    }))
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub state: String,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub event_count: i64,
    pub reviewed_at: Option<i64>,
}
pub fn list(db: &Connection) -> Result<Vec<Summary>, String> {
    init(db)?;
    let mut s=db.prepare("SELECT id,state,created_at,finished_at,(SELECT count(*) FROM mole_clean_events WHERE run_id=r.id),reviewed_at FROM mole_clean_runs r ORDER BY created_at DESC LIMIT 64").map_err(|e|e.to_string())?;
    let values = s
        .query_map([], |r| {
            Ok(Summary {
                id: r.get(0)?,
                state: r.get(1)?,
                created_at: r.get(2)?,
                finished_at: r.get(3)?,
                event_count: r.get(4)?,
                reviewed_at: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string());
    values
}
pub fn before(db: &Connection, id: &str, available: Option<u64>) -> Result<(), String> {
    db.execute(
        "UPDATE mole_clean_runs SET before_bytes=?2 WHERE id=?1 AND state='running'",
        params![id, available.map(|v| v as i64)],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn diagnostics(db: &Connection, id: &str, text: &str) -> Result<(), String> {
    let text = crate::engine::plain_terminal_text(text);
    let mut end = text.len().min(256 * 1024);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let bounded = &text[..end];
    db.execute(
        "UPDATE mole_clean_runs SET diagnostic=?2 WHERE id=?1",
        params![id, bounded],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn acknowledge(db: &Connection, id: &str, last_seq: i64, now: i64) -> Result<(), String> {
    let n=db.execute("UPDATE mole_clean_runs SET reviewed_at=?3 WHERE id=?1 AND state<>'running' AND coalesce((SELECT max(seq) FROM mole_clean_events WHERE run_id=?1),0)=?2",params![id,last_seq,now]).map_err(|e|e.to_string())?;
    if n != 1 {
        return Err("清理记录已变化或尚未结束，请重新核对。".into());
    }
    Ok(())
}
pub fn remove_reviewed(db: &mut Connection, id: &str, last_seq: i64) -> Result<(), String> {
    let t = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let eligible:bool=t.query_row("SELECT EXISTS(SELECT 1 FROM mole_clean_runs WHERE id=?1 AND state<>'running' AND reviewed_at IS NOT NULL AND coalesce((SELECT max(seq) FROM mole_clean_events WHERE run_id=?1),0)=?2)",params![id,last_seq],|r|r.get(0)).map_err(|e|e.to_string())?;
    if !eligible {
        return Err("请先核查已结束的完整清理记录；变化后的记录不能清除。".into());
    }
    t.execute("DELETE FROM mole_clean_events WHERE run_id=?1", [id])
        .map_err(|e| e.to_string())?;
    t.execute("DELETE FROM mole_clean_runs WHERE id=?1", [id])
        .map_err(|e| e.to_string())?;
    t.commit().map_err(|e| e.to_string())
}

pub fn finish_with_diagnostics(
    db: &mut Connection,
    id: &str,
    state: &str,
    after: Option<u64>,
    error: Option<&str>,
    text: &str,
) -> Result<(), String> {
    let t = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    diagnostics(&t, id, text)?;
    finish(&t, id, state, after, error)?;
    t.commit().map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, crate::db::Database, Plan) {
        let temp = tempfile::tempdir().unwrap();
        let db = crate::db::open_owned(&temp.path().join("state.sqlite")).unwrap();
        let now = crate::db::now();
        let p = Plan {
            version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            revision: crate::engine::REVISION.into(),
            engine_version: "1.65.0".into(),
            home: temp.path().to_str().unwrap().into(),
            system: false,
            method: "molePermanent".into(),
            protected_paths: vec![],
            created_at: now,
            expires_at: now + 120_000,
            scope: vec![],
        };
        (temp, db, p)
    }
    #[test]
    fn intent_survives_restart_and_blocks_mutation_until_exact_review() {
        let (_temp, mut db, p) = fixture();
        let id = uuid::Uuid::new_v4().to_string();
        create(&mut db, &id, &p, Some(4096)).unwrap();
        append(&db, &id, "intent", "/owned/cache", "safe_remove", "token").unwrap();
        assert!(create(&mut db, &uuid::Uuid::new_v4().to_string(), &p, None).is_err());
        recover(&db).unwrap();
        let r = read(&db, &id).unwrap().unwrap();
        assert_eq!(r.state, "interrupted");
        assert_eq!(r.events[0].kind, "intent");
        assert!(acknowledge(&db, &id, 0, crate::db::now()).is_err());
        assert!(create(&mut db, &uuid::Uuid::new_v4().to_string(), &p, None).is_err());
        acknowledge(&db, &id, 1, crate::db::now()).unwrap();
        create(&mut db, &uuid::Uuid::new_v4().to_string(), &p, None).unwrap();
    }
    #[test]
    fn results_and_space_commit_together_and_only_reviewed_history_is_clearable() {
        let (_temp, mut db, p) = fixture();
        let id = uuid::Uuid::new_v4().to_string();
        create(&mut db, &id, &p, Some(4096)).unwrap();
        append(&db, &id, "intent", "/owned/cache", "safe_remove", "token").unwrap();
        append(
            &db,
            &id,
            "removed",
            "/owned/cache",
            "safe_remove: 0",
            "token",
        )
        .unwrap();
        assert!(remove_reviewed(&mut db, &id, 2).is_err());
        finish_with_diagnostics(
            &mut db,
            &id,
            "completed",
            Some(8192),
            None,
            "upstream diagnostic",
        )
        .unwrap();
        let r = read(&db, &id).unwrap().unwrap();
        assert_eq!(r.after_bytes, Some(8192));
        assert_eq!(r.diagnostic, "upstream diagnostic");
        assert_eq!(r.events.len(), 2);
        assert!(append(&db, &id, "intent", "/unapproved", "safe_remove", "other").is_err());
        acknowledge(&db, &id, 2, crate::db::now()).unwrap();
        remove_reviewed(&mut db, &id, 2).unwrap();
        assert!(read(&db, &id).unwrap().is_none());
    }
    #[test]
    fn full_command_records_share_the_other_executors_admission_gate() {
        let (_temp, mut db, p) = fixture();
        let id = uuid::Uuid::new_v4().to_string();
        create(&mut db, &id, &p, None).unwrap();
        {
            let t = db.transaction().unwrap();
            assert!(crate::execution_admission::reserve(&t, 1024).is_err());
        }
        finish(
            &db,
            &id,
            "uncertain",
            None,
            Some("interrupted owner command"),
        )
        .unwrap();
        {
            let t = db.transaction().unwrap();
            assert!(crate::execution_admission::reserve(&t, 1024).is_err());
        }
        acknowledge(&db, &id, 0, crate::db::now()).unwrap();
        let t = db.transaction().unwrap();
        assert!(crate::execution_admission::reserve(&t, 1024).is_ok());
    }
}
