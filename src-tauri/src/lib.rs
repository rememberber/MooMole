mod clean_preview;
mod db;
mod descendant_rules;
mod diagnostics;
mod engine;
mod engine_manifest;
mod execution;
#[cfg(test)]
mod execution_sandbox;
mod impact;
mod instance;
mod mole_history;
#[cfg(test)]
mod native_trash_tests;
mod observation;
mod permissions;
mod process;
mod provenance;
mod review;
mod rules;
mod tasks;
mod tree;
mod update_download;
mod update_quarantine;
mod updates;

use db::{Settings, TaskRecord};
use engine::{Job, TaskKind};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<db::Database>>,
    jobs: Arc<Mutex<HashMap<String, Arc<Job>>>>,
    results: Arc<Mutex<tasks::Results>>,
    engine_root: PathBuf,
    home: PathBuf,
    // 0 accepts tasks, 1 drains cancellations, 2 permits the final exit.
    shutdown: Arc<AtomicU8>,
    updates: Arc<updates::Checker>,
}

// A startup issue has no database or task handles. Its message also backs the
// loading window while the native sheet explains why this instance must close.
struct StartupIssue {
    message: String,
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
    executions: Vec<execution::Summary>,
}

#[tauri::command]
fn bootstrap(app: tauri::AppHandle) -> Result<Bootstrap, String> {
    if let Some(issue) = app.try_state::<StartupIssue>() {
        return Err(issue.message.clone());
    }
    let state = app
        .try_state::<AppState>()
        .ok_or("本机工作区尚未初始化。")?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(Bootstrap {
        settings: db::settings(&db)?,
        history: db::history(&db)?,
        engine: engine::info(&state.engine_root),
        home: state.home.to_string_lossy().into_owned(),
        latest_clean_preview: db::latest_scan(&db)?,
        review_plans: db::plans(&db)?,
        executions: execution::list(&db)?,
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
fn prepare_task_diagnostic(
    task_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    uuid::Uuid::parse_str(&task_id).map_err(|_| "任务 ID 无效。")?;
    let task = db::task(&*state.db.lock().map_err(|e| e.to_string())?, &task_id)?
        .ok_or("任务不存在或已超过保留期限，请刷新列表。")?;
    diagnostics::prepare(&task, engine::info(&state.engine_root).version)
}

#[tauri::command]
async fn save_task_diagnostic(report: String, app: tauri::AppHandle) -> Result<bool, String> {
    diagnostics::validate(&report)?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("保存诊断摘要")
        .set_file_name("MooMole-diagnostic.json")
        .add_filter("JSON 诊断摘要", &["json"])
        .save_file(move |path| {
            let _ = sender.send(path);
        });
    let Some(path) = receiver.await.map_err(|_| "保存窗口已关闭，请重试。")? else {
        return Ok(false);
    };
    let path = path.into_path().map_err(|_| "请选择本机文件路径。")?;
    tauri::async_runtime::spawn_blocking(move || diagnostics::save(&path, &report))
        .await
        .map_err(|e| e.to_string())??;
    Ok(true)
}

#[tauri::command]
fn list_executions(state: tauri::State<'_, AppState>) -> Result<Vec<execution::Summary>, String> {
    execution::list(&*state.db.lock().map_err(|e| e.to_string())?)
}

#[tauri::command]
fn get_execution(
    run_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<execution::Detail, String> {
    execution::detail(&*state.db.lock().map_err(|e| e.to_string())?, &run_id)
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
    if state.shutdown.load(Ordering::SeqCst) != 0 {
        return Err("应用正在退出，请重新打开后再开始检查。".into());
    }
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
        if state.shutdown.load(Ordering::SeqCst) != 0 {
            return Err("应用正在退出，请重新打开后再开始检查。".into());
        }
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
        let prepare_plan = plan.clone();
        let home = state.home.clone();
        let (local, batch) = tauri::async_runtime::spawn_blocking(move || {
            let local = review::check(&prepare_plan, &settings.protected_paths, db::now());
            let batch = descendant_rules::prepare(&prepare_plan, &local, &home);
            (local, batch)
        })
        .await
        .map_err(|e| e.to_string())?;
        if batch.requested.is_empty() {
            return Ok(local);
        }
        if job.cancelled.load(Ordering::SeqCst) {
            return Err("规则复查已取消。".into());
        }
        let checks = rules::run(
            &state.engine_root,
            &state.home,
            &batch.requested,
            job.clone(),
        )
        .await?;
        let settings = db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?;
        // Re-read local identities, expiry and client protection after the probes.
        let view = tauri::async_runtime::spawn_blocking(move || {
            review::check(&plan, &settings.protected_paths, db::now())
        })
        .await
        .map_err(|e| e.to_string())?;
        if job.cancelled.load(Ordering::SeqCst) {
            return Err("规则复查已取消。".into());
        }
        Ok(descendant_rules::apply(
            review::apply_rule_checks(view, &checks, db::now()),
            &batch,
            &checks,
        ))
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

#[tauri::command]
async fn check_updates(state: tauri::State<'_, AppState>) -> Result<updates::Check, String> {
    state.updates.check().await
}

#[tauri::command]
fn get_update_snapshot(state: tauri::State<'_, AppState>) -> Result<updates::Snapshot, String> {
    state.updates.snapshot()
}
#[tauri::command]
fn download_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<updates::Snapshot, String> {
    let root = app
        .path()
        .app_cache_dir()
        .map_err(|_| "无法定位应用缓存。")?;
    std::fs::create_dir_all(&root).map_err(|_| "无法创建应用缓存。")?;
    state.updates.start(root.join("pending-updates"))
}
#[tauri::command]
fn cancel_update_download(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.updates.cancel()
}
#[tauri::command]
async fn reveal_update_download(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let path = state.updates.ready_path().await?;
    let mut command = tokio::process::Command::new("/usr/bin/open");
    command.args(["-R", "--"]).arg(path).kill_on_drop(true);
    let result = tokio::time::timeout(Duration::from_secs(5), command.status())
        .await
        .map_err(|_| "Finder 打开超时。")?
        .map_err(|_| "无法打开 Finder。")?;
    if result.success() {
        Ok(())
    } else {
        Err("无法打开 Finder，请重试。".into())
    }
}

#[tauri::command]
async fn open_update_releases() -> Result<(), String> {
    let mut command = tokio::process::Command::new("/usr/bin/open");
    command.arg(updates::RELEASES).kill_on_drop(true);
    let status = tokio::time::timeout(Duration::from_secs(5), command.status())
        .await
        .map_err(|_| "发布页打开超时，请重试。")?
        .map_err(|_| "无法打开发布页，请稍后重试。")?;
    if status.success() {
        Ok(())
    } else {
        Err("无法打开发布页，请稍后重试。".into())
    }
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data)?;
            let connection = match db::open_owned(&data.join("moomole.sqlite")) {
                Ok(connection) => connection,
                Err(error) => {
                    let busy = matches!(error, db::OpenError::Instance(instance::Error::Busy));
                    let title = if busy {
                        "MooMole 已在运行"
                    } else {
                        "无法打开 MooMole 本机数据"
                    };
                    let message = error.to_string();
                    app.manage(StartupIssue {
                        message: message.clone(),
                    });
                    let handle = app.handle().clone();
                    // DialogExt::show is nonblocking; blocking_show must not run
                    // on Tauri's main thread. No AppState or tasks are admitted.
                    let mut dialog = app.dialog().message(message).title(title).kind(if busy {
                        tauri_plugin_dialog::MessageDialogKind::Info
                    } else {
                        tauri_plugin_dialog::MessageDialogKind::Error
                    });
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.set_title(title);
                        // A parentless macOS dialog is a system-hosted alert.
                        // Keep the inert startup window visible and attach an
                        // accessible sheet instead of hiding its only window.
                        dialog = dialog.parent(&window);
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                    dialog.show(move |_| handle.exit(if busy { 0 } else { 1 }));
                    return Ok(());
                }
            };
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
                shutdown: Arc::new(AtomicU8::new(0)),
                updates: Arc::new(updates::Checker::default()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            save_settings,
            get_history,
            prepare_task_diagnostic,
            save_task_diagnostic,
            list_executions,
            get_execution,
            get_task_snapshot,
            list_review_plans,
            create_review_plan,
            get_review_plan,
            check_review_rules,
            start_task,
            cancel_task,
            reveal_path,
            open_official,
            check_updates,
            get_update_snapshot,
            download_update,
            cancel_update_download,
            reveal_update_download,
            open_update_releases
        ])
        .build(tauri::generate_context!())
        .expect("无法启动 MooMole");
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, code, .. } = &event {
            if let Some(state) = app.try_state::<AppState>() {
                if state.shutdown.load(Ordering::SeqCst) != 2 {
                    let jobs = state.jobs.lock().unwrap_or_else(|e| e.into_inner());
                    state.updates.shutdown();
                    if jobs.is_empty() && !state.updates.active() {
                        state.shutdown.store(2, Ordering::SeqCst);
                    } else {
                        let first = state.shutdown.compare_exchange(
                            0,
                            1,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        );
                        if first != Err(2) {
                            api.prevent_exit();
                        }
                        if first.is_ok() {
                            for job in jobs.values() {
                                job.cancel();
                            }
                            let state = state.inner().clone();
                            let handle = app.clone();
                            let code = code.unwrap_or(0);
                            tauri::async_runtime::spawn(async move {
                                let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
                                loop {
                                    let done = {
                                        let jobs =
                                            state.jobs.lock().unwrap_or_else(|e| e.into_inner());
                                        if tokio::time::Instant::now() >= deadline {
                                            for job in jobs.values() {
                                                job.kill_group();
                                            }
                                            true
                                        } else {
                                            jobs.is_empty() && !state.updates.active()
                                        }
                                    };
                                    if done {
                                        break;
                                    }
                                    tokio::time::sleep(Duration::from_millis(50)).await;
                                }
                                state.shutdown.store(2, Ordering::SeqCst);
                                handle.exit(code);
                            });
                        }
                    }
                }
            }
        }
        if matches!(event, tauri::RunEvent::Exit) {
            if let Some(state) = app.try_state::<AppState>() {
                // macOS's native Quit selector can deliver Exit directly,
                // bypassing ExitRequested. Cocoa has committed to termination,
                // so give the independent async runtime the same bounded drain
                // here before returning from applicationWillTerminate.
                if state.shutdown.load(Ordering::SeqCst) != 2 {
                    {
                        let jobs = state.jobs.lock().unwrap_or_else(|e| e.into_inner());
                        state.shutdown.store(1, Ordering::SeqCst);
                        state.updates.shutdown();
                        for job in jobs.values() {
                            job.cancel();
                        }
                    }
                    let deadline = std::time::Instant::now() + Duration::from_secs(6);
                    loop {
                        let done = {
                            let jobs = state.jobs.lock().unwrap_or_else(|e| e.into_inner());
                            if std::time::Instant::now() >= deadline {
                                for job in jobs.values() {
                                    job.kill_group();
                                }
                                true
                            } else {
                                jobs.is_empty() && !state.updates.active()
                            }
                        };
                        if done {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    state.shutdown.store(2, Ordering::SeqCst);
                }
            }
        }
    });
}
