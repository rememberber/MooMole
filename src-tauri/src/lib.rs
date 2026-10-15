mod clean_preview;
mod db;
mod engine;
#[cfg(test)]
mod execution_sandbox;
mod impact;
#[cfg(test)]
mod native_trash_tests;
mod review;
mod rules;
mod tasks;
mod tree;

use db::{Settings, TaskRecord};
use engine::{Job, TaskKind};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{atomic::Ordering, Arc, Mutex},
};
use tauri::{Emitter, Manager};

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<rusqlite::Connection>>,
    jobs: Arc<Mutex<HashMap<String, Arc<Job>>>>,
    results: Arc<Mutex<tasks::Results>>,
    engine_root: PathBuf,
    home: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bootstrap {
    settings: Settings,
    history: Vec<TaskRecord>,
    engine: engine::EngineInfo,
    home: String,
    latest_clean_preview: Option<Value>,
    review_plans: Vec<review::PlanSummary>,
}

#[tauri::command]
fn bootstrap(state: tauri::State<'_, AppState>) -> Result<Bootstrap, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(Bootstrap {
        settings: db::settings(&db)?,
        history: db::history(&db)?,
        engine: engine::info(&state.engine_root),
        home: state.home.to_string_lossy().into_owned(),
        latest_clean_preview: db::latest_scan(&db)?,
        review_plans: db::plans(&db)?,
    })
}

#[tauri::command]
fn save_settings(
    settings: Settings,
    state: tauri::State<'_, AppState>,
) -> Result<Settings, String> {
    let mut settings = settings;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let previous = db::settings(&db)?;
    settings.protected_paths = engine::protection_paths(
        &settings.protected_paths,
        &previous.protected_paths,
        &state.home,
    )?;
    db::save_settings(&db, &settings)?;
    Ok(settings)
}

#[tauri::command]
fn get_history(state: tauri::State<'_, AppState>) -> Result<Vec<TaskRecord>, String> {
    db::history(&*state.db.lock().map_err(|e| e.to_string())?)
}

#[tauri::command]
fn get_task_snapshot(
    task_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<tasks::Snapshot, String> {
    uuid::Uuid::parse_str(&task_id).map_err(|_| "任务 ID 无效。")?;
    if let Some(snapshot) = state
        .results
        .lock()
        .map_err(|e| e.to_string())?
        .get(&task_id)
    {
        return Ok(snapshot);
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let record = db::task(&db, &task_id)?.ok_or("任务不存在或已超过保留期限。")?;
    let active = state
        .jobs
        .lock()
        .map_err(|e| e.to_string())?
        .get(&task_id)
        .is_some_and(|job| job.kind.name() == record.kind);
    // Completion may have published its result after our first cache lookup.
    if let Some(snapshot) = state
        .results
        .lock()
        .map_err(|e| e.to_string())?
        .get(&task_id)
    {
        return Ok(snapshot);
    }
    Ok(tasks::from_record(record, active))
}

#[tauri::command]
fn start_task(
    task_id: String,
    kind: TaskKind,
    path: Option<String>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if kind == TaskKind::Review {
        return Err("请从审阅计划启动规则复查。".into());
    }
    uuid::Uuid::parse_str(&task_id).map_err(|_| "任务 ID 无效。")?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let settings = db::settings(&db)?;
    if !settings.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    let path = if kind == TaskKind::Analyze {
        Some(engine::directory(
            path.as_deref().ok_or("请选择扫描目录。")?,
            &state.home,
        )?)
    } else {
        None
    };
    let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
    if jobs.len() >= 3 || jobs.contains_key(&task_id) || jobs.values().any(|job| job.kind == kind) {
        return Err("已有同类任务正在运行，请等待或取消。".into());
    }
    db::start(
        &db,
        &task_id,
        kind.name(),
        path.as_ref().and_then(|p| p.to_str()),
    )?;
    let job = Arc::new(Job::new(kind));
    jobs.insert(task_id.clone(), job.clone());
    drop(jobs);
    drop(db);
    let state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let result = engine::run(
            &state.engine_root,
            &state.home,
            job.clone(),
            path,
            &settings.protected_paths,
        )
        .await;
        let result = if kind == TaskKind::Clean {
            result.and_then(|mut value| {
                if job.cancelled.load(Ordering::SeqCst) {
                    return Err("任务已取消。".into());
                }
                let preview = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
                let scan = review::capture(task_id.clone(), preview, db::now())?;
                let db = state.db.lock().map_err(|e| e.to_string())?;
                db::save_scan(&db, &scan)?;
                let stored = scan.preview();
                for key in ["scanId", "scannedAt", "expiresAt", "items"] {
                    value[key] = stored[key].clone();
                }
                Ok(value)
            })
        } else {
            result
        };
        let (status, payload, error) = match result {
            Ok(_) if job.cancelled.load(Ordering::SeqCst) => {
                ("cancelled", None, Some("任务已取消。".to_string()))
            }
            Ok(value) => ("succeeded", Some(value), None),
            Err(error) => (
                if job.cancelled.load(Ordering::SeqCst) {
                    "cancelled"
                } else {
                    "failed"
                },
                None,
                Some(error),
            ),
        };
        let message = error
            .as_deref()
            .unwrap_or("只读检查已完成，未删除或修改扫描目标。");
        let persisted = state
            .db
            .lock()
            .map_err(|e| e.to_string())
            .and_then(|db| db::finish(&db, &task_id, status, message));
        let event = tasks::Snapshot {
            id: task_id.clone(),
            kind: kind.name().into(),
            state: status.into(),
            result: payload,
            result_unavailable: None,
            error: persisted
                .err()
                .map(|e| format!("任务已结束，但历史记录写入失败：{e}"))
                .or(error),
        };
        // Publish the snapshot before dropping the running handle or emitting.
        // A late listener can retrieve the same terminal result by task ID.
        if let Ok(mut results) = state.results.lock() {
            results.remember(&event);
        }
        if let Ok(mut jobs) = state.jobs.lock() {
            jobs.remove(&task_id);
        }
        let _ = app.emit_to("main", "task-event", event);
    });
    Ok(())
}

#[tauri::command]
fn list_review_plans(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<review::PlanSummary>, String> {
    db::plans(&*state.db.lock().map_err(|e| e.to_string())?)
}

#[tauri::command]
async fn create_review_plan(
    scan_id: String,
    selected_ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<review::PlanView, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (scan, settings) = {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            (db::scan(&db, &scan_id)?, db::settings(&db)?)
        };
        if !settings.acknowledged {
            return Err("请先阅读首次使用说明。".into());
        }
        let now = db::now();
        let plan = review::create(&scan, &selected_ids, &settings.protected_paths, now)?;
        db::save_plan(&*state.db.lock().map_err(|e| e.to_string())?, &plan)?;
        Ok(review::check(&plan, &settings.protected_paths, now))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_review_plan(
    plan_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<review::PlanView, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (plan, settings) = {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            (db::plan(&db, &plan_id)?, db::settings(&db)?)
        };
        Ok(review::check(&plan, &settings.protected_paths, db::now()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn check_review_rules(
    plan_id: String,
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<review::PlanView, String> {
    uuid::Uuid::parse_str(&request_id).map_err(|_| "复查任务 ID 无效。")?;
    let state = state.inner().clone();
    let job = Arc::new(Job::new(TaskKind::Review));
    {
        let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
        if jobs.len() >= 3
            || jobs.contains_key(&request_id)
            || jobs.values().any(|j| j.kind == TaskKind::Review)
        {
            return Err("已有规则复查正在运行，请等待或取消。".into());
        }
        jobs.insert(request_id.clone(), job.clone());
    }
    let result = async {
        let (plan, settings) = {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            (db::plan(&db, &plan_id)?, db::settings(&db)?)
        };
        if !settings.acknowledged {
            return Err("请先阅读首次使用说明。".into());
        }
        let local = review::check(&plan, &settings.protected_paths, db::now());
        let requested: Vec<_> = local
            .items
            .iter()
            .filter(|i| i.state == "unchanged")
            .map(|i| (i.id.clone(), i.path.clone()))
            .collect();
        if requested.is_empty() {
            return Ok(local);
        }
        let checks = rules::run(&state.engine_root, &requested, job.clone()).await?;
        let settings = db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?;
        // Re-read local identities, expiry and client protection after the probes.
        let view = review::check(&plan, &settings.protected_paths, db::now());
        if job.cancelled.load(Ordering::SeqCst) {
            return Err("规则复查已取消。".into());
        }
        Ok(review::apply_rule_checks(view, &checks, db::now()))
    }
    .await;
    if let Ok(mut jobs) = state.jobs.lock() {
        jobs.remove(&request_id);
    }
    result
}

#[tauri::command]
fn cancel_task(task_id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    if let Some(job) = state.jobs.lock().map_err(|e| e.to_string())?.get(&task_id) {
        job.cancel();
    }
    Ok(())
}

#[tauri::command]
async fn reveal_path(path: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let home = state.home.clone();
    let path = if path == "~" {
        home
    } else if let Some(relative) = path.strip_prefix("~/") {
        home.join(relative)
    } else {
        PathBuf::from(path)
    };
    if !path.is_absolute() {
        return Err("路径无效。".into());
    }
    let path = path.canonicalize().map_err(|e| format!("无法访问：{e}"))?;
    let mut command = tokio::process::Command::new("/usr/bin/open");
    command.args(["-R", "--"]).arg(path).kill_on_drop(true);
    let status = tokio::time::timeout(std::time::Duration::from_secs(5), command.status())
        .await
        .map_err(|_| "Finder 打开超时。")?
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("无法在 Finder 中显示。".into());
    }
    Ok(())
}

#[tauri::command]
async fn open_official() -> Result<(), String> {
    let status = tokio::process::Command::new("/usr/bin/open")
        .arg("https://mole.fit/zh/")
        .status()
        .await
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("无法打开官方产品页面。".into())
    }
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data)?;
            let connection =
                db::open(&data.join("moomole.sqlite")).map_err(std::io::Error::other)?;
            let bundled = app.path().resource_dir()?.join("mole");
            let engine_root = if bundled.join("UPSTREAM.json").is_file() {
                bundled
            } else if cfg!(debug_assertions) {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole")
            } else {
                bundled
            };
            app.manage(AppState {
                db: Arc::new(Mutex::new(connection)),
                jobs: Arc::new(Mutex::new(HashMap::new())),
                results: Arc::new(Mutex::new(tasks::Results::default())),
                engine_root,
                home: app.path().home_dir()?,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            save_settings,
            get_history,
            get_task_snapshot,
            list_review_plans,
            create_review_plan,
            get_review_plan,
            check_review_rules,
            start_task,
            cancel_task,
            reveal_path,
            open_official
        ])
        .build(tauri::generate_context!())
        .expect("无法启动 MooMole");
    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            if let Some(state) = app.try_state::<AppState>() {
                if let Ok(jobs) = state.jobs.lock() {
                    for job in jobs.values() {
                        job.cancel();
                        job.kill_group();
                    }
                }
            }
        }
    });
}
