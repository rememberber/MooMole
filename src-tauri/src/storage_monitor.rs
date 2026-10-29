//! Opt-in local capacity observations. Never scans or processes personal files.
use crate::{cleanup_space::Sample, db, AppState};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
const GIB: u64 = 1024 * 1024 * 1024;
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS storage_monitor_preferences (id INTEGER PRIMARY KEY CHECK(id=1), version INTEGER NOT NULL, body TEXT NOT NULL); CREATE TABLE IF NOT EXISTS storage_monitor_samples (id INTEGER PRIMARY KEY, body TEXT NOT NULL CHECK(length(body)<=2048));";
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Config {
    enabled: bool,
    interval_minutes: u32,
    warn_below_bytes: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: 60,
            warn_below_bytes: 10 * GIB,
        }
    }
}
impl Config {
    fn validate(&self) -> Result<(), String> {
        if ![15, 60, 360].contains(&self.interval_minutes)
            || ![5 * GIB, 10 * GIB, 20 * GIB].contains(&self.warn_below_bytes)
        {
            return Err("空间观察间隔或提醒阈值无效。".into());
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    config: Config,
    generation: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    config: Config,
    samples: Vec<Sample>,
    collecting: bool,
    error: Option<String>,
}
#[derive(Default)]
pub(crate) struct Controller {
    collecting: AtomicBool,
    error: Mutex<Option<String>>,
}
fn exists(db: &db::Database, table: &str) -> Result<bool, String> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}
fn load(db: &db::Database) -> Result<Saved, String> {
    let default = || Saved {
        config: Config::default(),
        generation: "unconfigured".into(),
    };
    if !exists(db, "storage_monitor_preferences")? {
        return Ok(default());
    }
    let row: Option<(u32, Option<String>)> = db.query_row("SELECT version,CASE WHEN length(CAST(body AS BLOB))<=2048 THEN body END FROM storage_monitor_preferences WHERE id=1", [], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(|e| e.to_string())?;
    let Some((version, body)) = row else {
        return Ok(default());
    };
    if version != 1 {
        return Err("空间观察设置来自其他版本，请保留并使用相应版本打开。".into());
    }
    let saved: Saved = serde_json::from_str(&body.ok_or("空间观察设置超过读取范围。")?)
        .map_err(|_| "空间观察设置无法读取，未开启采集。")?;
    saved.config.validate()?;
    uuid::Uuid::parse_str(&saved.generation).map_err(|_| "空间观察设置标识无效。")?;
    Ok(saved)
}
fn samples(db: &db::Database) -> Result<Vec<Sample>, String> {
    if !exists(db, "storage_monitor_samples")? {
        return Ok(vec![]);
    }
    let mut s = db.prepare("SELECT CASE WHEN length(CAST(body AS BLOB))<=2048 THEN body END FROM (SELECT id,body FROM storage_monitor_samples ORDER BY id DESC LIMIT 720) ORDER BY id").map_err(|e| e.to_string())?;
    let rows = s
        .query_map([], |r| r.get::<_, Option<String>>(0))
        .map_err(|e| e.to_string())?;
    rows.map(|r| {
        let raw = r
            .map_err(|e| e.to_string())?
            .ok_or("空间样本超过读取范围。")?;
        let sample: Sample =
            serde_json::from_str(&raw).map_err(|_| "空间样本无法读取，原记录保留。")?;
        if !sample.valid() {
            return Err("空间样本数值无法核对，原记录保留。".into());
        }
        Ok(sample)
    })
    .collect()
}
fn save_config(db: &mut db::Database, config: Config) -> Result<Saved, String> {
    config.validate()?;
    let _ = load(db)?;
    if !db::settings(db)?.acknowledged {
        return Err("请先确认首次使用说明。".into());
    }
    let saved = Saved {
        config,
        generation: uuid::Uuid::new_v4().to_string(),
    };
    let body = serde_json::to_string(&saved).map_err(|e| e.to_string())?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    tx.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    tx.execute("INSERT INTO storage_monitor_preferences VALUES(1,1,?1) ON CONFLICT(id) DO UPDATE SET version=1,body=excluded.body", [body]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(saved)
}
fn save_sample(db: &mut db::Database, sample: &Sample) -> Result<(), String> {
    if !sample.valid() {
        return Err("当前容量未可靠测得，未写入样本。".into());
    }
    let body = serde_json::to_string(sample).map_err(|e| e.to_string())?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    tx.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO storage_monitor_samples(body) VALUES(?1)",
        params![body],
    )
    .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM storage_monitor_samples WHERE id NOT IN (SELECT id FROM storage_monitor_samples ORDER BY id DESC LIMIT 720)", []).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}
fn due(saved: &Saved, latest: Option<&Sample>, now: i64) -> bool {
    saved.config.enabled
        && latest.is_none_or(|s| {
            now < s.measured_at
                || now.saturating_sub(s.measured_at)
                    >= i64::from(saved.config.interval_minutes) * 60_000
        })
}
impl Controller {
    fn view(&self, db: &db::Database) -> Result<View, String> {
        Ok(View {
            config: load(db)?.config,
            samples: samples(db)?,
            collecting: self.collecting.load(Ordering::SeqCst),
            error: self.error.lock().map_err(|e| e.to_string())?.clone(),
        })
    }
    async fn collect(
        &self,
        database: &Arc<Mutex<db::Database>>,
        home: PathBuf,
        generation: Option<&str>,
    ) -> Result<(), String> {
        if self
            .collecting
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("空间观察仍在进行，请稍后刷新。".into());
        }
        struct Reset<'a>(&'a AtomicBool);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::SeqCst);
            }
        }
        let _reset = Reset(&self.collecting);
        let result = async {
            // A manual sample must also respect an unreadable/future format.
            // It cannot append/prune samples behind an unsupported setting.
            let _ = load(&*database.lock().map_err(|e| e.to_string())?)?;
            if !db::settings(&*database.lock().map_err(|e| e.to_string())?)?.acknowledged {
                return Err("请先确认首次使用说明。".into());
            }
            let sample = crate::cleanup_space::measure(home)
                .await
                .ok_or("未能测得本账户所在卷的可用空间，原样本保留。")?;
            let mut db = database.lock().map_err(|e| e.to_string())?;
            if let Some(generation) = generation {
                let current = load(&db)?;
                if !current.config.enabled || current.generation != generation {
                    return Ok(());
                }
            }
            save_sample(&mut db, &sample)
        }
        .await;
        *self.error.lock().map_err(|e| e.to_string())? = result.as_ref().err().cloned();
        result
    }
}
pub(crate) fn start(state: AppState) {
    tauri::async_runtime::spawn(async move {
        loop {
            if state.shutdown.closing() {
                break;
            }
            let scheduled = (|| -> Result<Option<String>, String> {
                let db = state.db.lock().map_err(|e| e.to_string())?;
                let saved = load(&db)?;
                if !saved.config.enabled || !db::settings(&db)?.acknowledged {
                    return Ok(None);
                }
                Ok(due(&saved, samples(&db)?.last(), db::now()).then_some(saved.generation))
            })();
            match scheduled {
                Ok(Some(generation)) => {
                    let _ = state
                        .storage_monitor
                        .collect(&state.db, state.home.clone(), Some(&generation))
                        .await;
                }
                Err(e) => {
                    if let Ok(mut error) = state.storage_monitor.error.lock() {
                        *error = Some(e);
                    }
                }
                _ => {}
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}
#[tauri::command]
pub(crate) fn get_storage_monitor(state: tauri::State<'_, AppState>) -> Result<View, String> {
    state
        .storage_monitor
        .view(&*state.db.lock().map_err(|e| e.to_string())?)
}
#[tauri::command]
pub(crate) async fn configure_storage_monitor(
    config: Config,
    state: tauri::State<'_, AppState>,
) -> Result<View, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出。".into());
    }
    let saved = save_config(&mut *state.db.lock().map_err(|e| e.to_string())?, config)?;
    if saved.config.enabled {
        let _ = state
            .storage_monitor
            .collect(&state.db, state.home.clone(), Some(&saved.generation))
            .await;
    }
    state
        .storage_monitor
        .view(&*state.db.lock().map_err(|e| e.to_string())?)
}
#[tauri::command]
pub(crate) async fn sample_storage_monitor(
    state: tauri::State<'_, AppState>,
) -> Result<View, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出。".into());
    }
    let _ = state
        .storage_monitor
        .collect(&state.db, state.home.clone(), None)
        .await;
    state
        .storage_monitor
        .view(&*state.db.lock().map_err(|e| e.to_string())?)
}
#[cfg(test)]
mod tests;
