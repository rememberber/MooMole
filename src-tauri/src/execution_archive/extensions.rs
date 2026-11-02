//! Only the remaining journals already charged to the shared execution budget.
//! Their records are data; imported archives never enter a live controller.
use super::*;
pub(super) const TABLES: [(&str, usize); 4] = [
    ("mole_clean_runs", 10),
    ("mole_clean_events", 6),
    ("zip_extractions", 3),
    ("managed_app_runs", 3),
];
pub(super) const MAX_ROWS: usize = 131072;
pub(super) fn exists(c: &Connection, name: &str) -> Result<bool, String> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [name],
        |r| r.get(0),
    )
    .map_err(err)
}
pub(super) fn init(c: &Connection) -> Result<(), String> {
    c.execute_batch(crate::mole_clean::journal::SCHEMA)
        .map_err(err)?;
    c.execute_batch(crate::personal_files::zip_extraction::journal::TABLE)
        .map_err(err)?;
    c.execute_batch(crate::managed_apps::journal::TABLE)
        .map_err(err)
}
fn text(v: &Value, max: usize) -> Result<&str, String> {
    v.as_str()
        .filter(|s| s.len() <= max && !s.contains('\0'))
        .ok_or("归档字段不是可核对的文本。".into())
}
fn uuid(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok_and(|v| v.to_string() == s)
}
fn absolute(s: &str) -> bool {
    s.starts_with('/')
        && s.len() <= 4096
        && !s.contains('\0')
        && !s.split('/').any(|p| p == "." || p == "..")
}
fn time(v: &Value) -> Result<i64, String> {
    v.as_i64()
        .filter(|t| *t >= 0 && *t <= 9_007_199_254_740_991)
        .ok_or("归档时间字段无效。".into())
}
fn terminal(v: &Value, unknown: &[&str]) -> Result<(), String> {
    let state = text(&v["state"], 64)?;
    let created = time(&v["createdAt"])?;
    if state == "running" {
        return Err("请先结束正在运行的任务，再归档。".into());
    }
    let finished = time(&v["finishedAt"])?;
    if finished < created
        || (!v["reviewedAt"].is_null() && !time(&v["reviewedAt"]).is_ok_and(|t| t >= created))
        || (unknown.contains(&state) && v["reviewedAt"].is_null())
    {
        return Err("请先结束并人工核查完整清理、解压或 Homebrew 的未知结果，再归档。".into());
    }
    Ok(())
}
pub(super) fn summary(kind: Kind, v: &Value) -> Result<Value, String> {
    let (name, path, reason) = match kind {
        Kind::MoleClean => (
            "Mole",
            text(&v["plan"]["home"], 4096)?,
            v["error"].as_str().unwrap_or(""),
        ),
        Kind::Extraction => (
            match v["draft"]["format"].as_str().unwrap_or("zip") {
                "tar" => "TAR",
                "tarGzip" => "TAR.GZ",
                _ => "ZIP",
            },
            text(&v["draft"]["outputPath"], 4096)?,
            text(&v["reason"], 4000)?,
        ),
        Kind::ManagedApp => (
            text(&v["draft"]["token"], 256)?,
            text(&v["draft"]["path"], 4096)?,
            text(&v["reason"], 4000)?,
        ),
        _ => return Err("归档扩展类型无法核对。".into()),
    };
    let mut result_uncertain =
        ["unknown", "uncertain", "interrupted"].contains(&v["state"].as_str().unwrap_or(""));
    if kind == Kind::MoleClean {
        let mut pending = std::collections::HashSet::new();
        for e in v["events"].as_array().ok_or("完整清理事件缺失。")? {
            let token = e["token"].as_str().unwrap_or("");
            match e["kind"].as_str().unwrap_or("") {
                "intent" => {
                    pending.insert(token);
                }
                "removed" | "retained" | "uncertain" | "commandCompleted" => {
                    pending.remove(token);
                }
                _ => {}
            }
            result_uncertain |= e["kind"] == "uncertain";
        }
        result_uncertain |= !pending.is_empty();
    }
    Ok(
        serde_json::json!({"id":v["id"],"state":v["state"],"createdAt":v["createdAt"],"finishedAt":v["finishedAt"],"reviewedAt":v["reviewedAt"],"name":name,"path":path,"reason":reason,"resultUncertain":result_uncertain}),
    )
}
pub(super) fn mole(c: &Connection, id: &str) -> Result<Value, String> {
    // Bound every large field before allocating it, including the complete
    // event stream rather than the live UI's most recent 2,000 event window.
    let (plan_len,diagnostic_len,error_len):(i64,i64,i64)=c.query_row("SELECT length(CAST(plan AS BLOB)),length(CAST(diagnostic AS BLOB)),coalesce(length(CAST(error AS BLOB)),0) FROM mole_clean_runs WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(err)?;
    let (events,event_bytes):(i64,i64)=c.query_row("SELECT count(*),coalesce(sum(length(CAST(path AS BLOB))+length(CAST(detail AS BLOB))+length(CAST(token AS BLOB))+128),0) FROM mole_clean_events WHERE run_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(err)?;
    if !uuid(id)
        || plan_len > 2 * 1024 * 1024
        || diagnostic_len > 256 * 1024
        || error_len > 16384
        || events > 20000
        || event_bytes > 8 * 1024 * 1024
    {
        return Err("完整清理证据超过归档预算，未释放记录。".into());
    }
    let r = crate::mole_clean::journal::read_complete(c, id)?.ok_or("完整清理记录缺失。")?;
    let mut v = serde_json::to_value(&r).map_err(err)?;
    let raw: String = c
        .query_row("SELECT plan FROM mole_clean_runs WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .map_err(err)?;
    v["plan"] = serde_json::from_str(&raw).map_err(err)?;
    let p = &r.plan;
    if p.version != 1
        || !uuid(&p.id)
        || p.revision.len() != 40
        || !p
            .revision
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || p.engine_version.len() > 128
        || semver::Version::parse(&p.engine_version).is_err()
        || !absolute(&p.home)
        || p.method != "molePermanent"
        || p.created_at < 0
        || p.expires_at.checked_sub(p.created_at) != Some(120000)
        || p.protected_paths.len() > 2048
        || p.protected_paths.iter().any(|p| !absolute(p))
        || p.scope.len() != 15
        || p.scope
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != 15
        || p.scope.iter().any(|s| {
            ![
                "system",
                "userEssentials",
                "appCaches",
                "browsers",
                "cloudOffice",
                "developerTools",
                "appsUtilities",
                "virtualization",
                "applicationSupport",
                "appLeftovers",
                "appleSilicon",
                "firmware",
                "timeMachine",
                "largeFiles",
                "projectArtifacts",
            ]
            .contains(&s.as_str())
        })
        || p.created_at > 9_007_199_254_740_991
        || p.expires_at > 9_007_199_254_740_991
        || r.before_bytes.is_some_and(|n| n > 9_007_199_254_740_991)
        || r.after_bytes.is_some_and(|n| n > 9_007_199_254_740_991)
        || r.diagnostic.contains('\0')
        || r.error.as_ref().is_some_and(|e| e.contains('\0'))
        || r.id != id
        || r.events_truncated
        || ![
            "completed",
            "cancelled",
            "failed",
            "uncertain",
            "interrupted",
        ]
        .contains(&r.state.as_str())
    {
        return Err("完整清理归档的计划或结果无法核对。".into());
    }
    let mut pending = std::collections::HashMap::new();
    let mut tokens = std::collections::HashSet::new();
    let mut uncertain = false;
    for (i, e) in r.events.iter().enumerate() {
        if e.seq != i as i64 + 1
            || e.path.len() > 4096
            || (!e.path.is_empty() && !absolute(&e.path))
            || e.detail.len() > 8192
            || e.detail.contains('\0')
            || (!e.token.is_empty() && !uuid(&e.token))
            || ![
                "intent",
                "removed",
                "retained",
                "uncertain",
                "commandCompleted",
                "section",
                "upstream",
                "authorization",
                "complete",
            ]
            .contains(&e.kind.as_str())
        {
            return Err("完整清理事件序列、字段或范围无法核对。".into());
        }
        if e.kind == "intent" {
            if e.token.is_empty() || !tokens.insert(&e.token) {
                return Err("完整清理意图标识重复或缺失。".into());
            }
            pending.insert(e.token.as_str(), e.path.as_str());
        } else if ["removed", "retained", "uncertain", "commandCompleted"]
            .contains(&e.kind.as_str())
            && !e.token.is_empty()
        {
            if pending.remove(e.token.as_str()) != Some(e.path.as_str()) {
                return Err("完整清理结果缺少对应意图。".into());
            }
        } else if ["removed", "commandCompleted", "uncertain"].contains(&e.kind.as_str()) {
            return Err("完整清理结果缺少意图标识。".into());
        }
        uncertain |= e.kind == "uncertain";
    }
    terminal(&v, &["uncertain", "interrupted"])?;
    if (uncertain || !pending.is_empty()) && r.reviewed_at.is_none() {
        return Err("完整清理仍有未核查的意图或未知结果，未归档。".into());
    }
    Ok(v)
}
pub(super) fn records(c: &Connection, kind: Kind) -> Result<Vec<Value>, String> {
    match kind {
        Kind::MoleClean => {
            let runs = exists(c, "mole_clean_runs")?;
            let events = exists(c, "mole_clean_events")?;
            if !runs && !events {
                return Ok(vec![]);
            }
            if runs != events {
                return Err("完整清理表集合不完整，未归档。".into());
            }
            let orphans: i64 = c.query_row("SELECT count(*) FROM mole_clean_events e LEFT JOIN mole_clean_runs r ON r.id=e.run_id WHERE r.id IS NULL", [], |r| r.get(0)).map_err(err)?;
            if orphans != 0 {
                return Err("完整清理存在孤立事件，未归档。".into());
            }
            ids(c, "mole_clean_runs")?
                .iter()
                .map(|id| mole(c, id))
                .collect()
        }
        Kind::Extraction => crate::personal_files::zip_extraction::journal::list(c)?
            .iter()
            .map(|r| {
                let v = serde_json::to_value(r).map_err(err)?;
                terminal(&v, &["unknown", "interrupted"])?;
                Ok(v)
            })
            .collect(),
        Kind::ManagedApp => crate::managed_apps::journal::list(c)?
            .iter()
            .map(|r| {
                let v = serde_json::to_value(r).map_err(err)?;
                terminal(&v, &["unknown", "interrupted"])?;
                Ok(v)
            })
            .collect(),
        _ => Err("归档扩展类型无效。".into()),
    }
}
pub(super) fn entries(
    c: &Connection,
    out: &mut Vec<Entry>,
    counts: &mut Counts,
    reserved: &mut i64,
) -> Result<(), String> {
    for (kind, size) in [
        (Kind::MoleClean, crate::mole_clean::journal::RESERVED_BYTES),
        (Kind::Extraction, 131072),
        (Kind::ManagedApp, 32768),
    ] {
        for record in records(c, kind)? {
            let id = text(&record["id"], 64)?.to_owned();
            valid_id(&id)?;
            match kind {
                Kind::MoleClean => counts.mole_clean += 1,
                Kind::Extraction => counts.extraction += 1,
                Kind::ManagedApp => counts.managed_app += 1,
                _ => unreachable!(),
            }
            *reserved = reserved.checked_add(size).ok_or("归档预留容量溢出。")?;
            out.push(Entry {
                kind,
                id,
                summary: summary(kind, &record)?,
            });
        }
    }
    Ok(())
}
pub(super) fn detail(c: &Connection, kind: Kind, id: &str) -> Result<Value, String> {
    records(c, kind)?
        .into_iter()
        .find(|r| r["id"].as_str() == Some(id))
        .ok_or("归档中没有该扩展记录。".into())
}
