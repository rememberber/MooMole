mod activity_authorization;
mod ai_scan;
mod analysis_protocol;
mod app_associations;
mod app_background;
mod app_discovery;
mod app_icons;
mod app_inspection;
mod app_protection;
mod app_receipts;
mod app_shared_rules;
mod app_uninstall_rules;
mod clean_preview;
mod cleanup_preparation;
// Native cleanup requires a sealed cache/log confirmation and fresh per-item checks.
mod cleanup_target;
mod db;
mod descendant_rules;
mod diagnostics;
mod directory_probe;
mod engine;
mod engine_manifest;
mod execution;
mod execution_admission;
mod execution_archive;
mod execution_archive_commands;
mod execution_location;
mod execution_review;
#[cfg(test)]
mod execution_sandbox;
mod impact;
mod installers;
mod instance;
mod language;
mod log_cleanup;
mod maintenance;
mod mole_history;
#[cfg(test)]
mod native_trash_tests;
mod observation;
mod permissions;
mod process;
mod project_artifacts;
mod provenance;
mod review;
mod rules;
mod shutdown;
mod status_watch;
mod tasks;
mod tree;
mod uninstall_confirmation;
mod uninstall_execution;
mod uninstall_journal;
mod uninstall_plan;
mod uninstall_runtime;
mod uninstall_support;
mod uninstall_target;
mod update_download;
mod update_install;
mod update_preferences;
mod update_quarantine;
mod updates;
mod window_placement;

use db::{Settings, TaskRecord};
use engine::{Job, TaskKind};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{atomic::Ordering, Arc, Mutex},
    time::Duration,
};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<db::Database>>,
    jobs: Arc<Mutex<HashMap<String, Arc<Job>>>>,
    results: Arc<Mutex<tasks::Results>>,
    archive_slots: Arc<tokio::sync::Semaphore>,
    icon_slots: Arc<tokio::sync::Semaphore>,
    inspection_slots: Arc<tokio::sync::Semaphore>,
    maintenance_slots: Arc<tokio::sync::Semaphore>,
    project_slots: Arc<tokio::sync::Semaphore>,
    installer_slots: Arc<tokio::sync::Semaphore>,
    maintenance_reviews: Arc<Mutex<maintenance::review::Store>>,
    refresh_plans: Arc<Mutex<maintenance::refresh_plan::Store>>,
    cleanup_confirmations: Arc<Mutex<log_cleanup::Confirmations>>,
    uninstall_plans: Arc<Mutex<uninstall_plan::Store>>,
    uninstall_confirmations: Arc<Mutex<uninstall_confirmation::Confirmations>>,
    status_watch: Arc<Mutex<status_watch::Store>>,
    cleanup_visibility: Arc<Mutex<activity_authorization::Store>>,
    ai_scan: Arc<Mutex<ai_scan::Store>>,
    engine_root: PathBuf,
    home: PathBuf,
    shutdown: Arc<shutdown::Controller>,
    updates: Arc<updates::Checker>,
    window_placement: Arc<window_placement::Tracker>,
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
    system_language: String,
    history: Vec<TaskRecord>,
    engine: engine::EngineInfo,
    home: String,
    latest_clean_preview: Option<Value>,
    review_plans: Vec<review::PlanSummary>,
    executions: Vec<execution::Summary>,
    maintenance_refreshes: Vec<maintenance::refresh_journal::Summary>,
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
        system_language: language::system_language(),
        history: db::history(&db)?,
        engine: engine::info(&state.engine_root),
        home: state.home.to_string_lossy().into_owned(),
        latest_clean_preview: db::latest_scan(&db)?,
        review_plans: db::plans(&db)?,
        executions: execution::list(&db)?,
        maintenance_refreshes: maintenance::refresh_journal::list(&db)?,
    })
}

#[tauri::command]
fn save_settings(
    settings: Settings,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Settings, String> {
    let _operation = state.updates.installation.enter()?;
    let mut settings = settings;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let previous = db::settings(&db)?;
    settings.protected_paths = engine::protection_paths(
        &settings.protected_paths,
        &previous.protected_paths,
        &state.home,
    )?;
    db::save_settings(&db, &settings)
        .map_err(|error| format!("设置保存失败，请重试。诊断：{error}"))?;
    drop(db);
    if previous.language != settings.language {
        if let Err(error) = language::apply_menu(&app, &settings.language) {
            eprintln!("Unable to update native menu language: {error}");
        }
    }
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
async fn save_task_diagnostic(
    report: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let operation = state.updates.installation.enter()?;
    diagnostics::validate(&report)?;
    let language = language::saved(&state);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(language::text(
            &language,
            "保存诊断摘要",
            "Save diagnostic summary",
            "診断概要を保存",
        ))
        .set_file_name("MooMole-diagnostic.json")
        .add_filter(
            language::text(
                &language,
                "JSON 诊断摘要",
                "JSON diagnostic summary",
                "JSON 診断概要",
            ),
            &["json"],
        )
        .save_file(move |path| {
            let _ = sender.send(path);
        });
    let Some(path) = receiver.await.map_err(|_| "保存窗口已关闭，请重试。")? else {
        return Ok(false);
    };
    let path = path.into_path().map_err(|_| "请选择本机文件路径。")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        diagnostics::save(&path, &report)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(true)
}

#[tauri::command]
async fn read_project_roots(
    state: tauri::State<'_, AppState>,
) -> Result<project_artifacts::roots::Report, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出，请重新打开后读取。".into());
    }
    if !db::settings(&*state.db.lock().map_err(|_| "无法读取使用设置。")?)?.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    let permit = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "上次工程目录读取尚未结束，请稍后重试。")?;
    let operation = state.updates.installation.enter()?;
    let home = state.home.clone();
    let engine_root = state.engine_root.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let (_permit, _operation) = (permit, operation);
        let info = engine::info(&engine_root);
        if !info.available || info.version.as_deref() != Some("1.59.1") {
            return Err("Mole 引擎基线无法确认，请先检查引擎。".into());
        }
        project_artifacts::roots::read(&home)
    });
    tokio::time::timeout(Duration::from_secs(6), worker)
        .await
        .map_err(|_| "Mole 目录读取超时，系统读取可能仍在结束，请稍后重试。")?
        .map_err(|_| "Mole 目录读取未完成。")?
}

#[tauri::command]
async fn inspect_project_artifacts(
    root: String,
    state: tauri::State<'_, AppState>,
) -> Result<project_artifacts::Report, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出，请重新打开后扫描。".into());
    }
    let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取保护设置。")?)?;
    if !settings.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    let permit = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "上次工程目录扫描尚未结束，请稍后重试。")?;
    let operation = state.updates.installation.enter()?;
    let engine_root = state.engine_root.clone();
    let protected = settings.protected_paths.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let (_permit, _operation) = (permit, operation);
        let info = engine::info(&engine_root);
        if !info.available || info.version.as_deref() != Some("1.59.1") {
            return Err("Mole 引擎基线无法确认，请先检查引擎。".into());
        }
        project_artifacts::scan(std::path::Path::new(&root), &protected)
    });
    let report = tokio::time::timeout(Duration::from_secs(6), worker)
        .await
        .map_err(|_| "工程目录扫描超时，系统读取可能仍在结束，请稍后重试。")?
        .map_err(|_| "工程目录扫描未完成。")??;
    if db::settings(&*state.db.lock().map_err(|_| "无法读取当前保护设置。")?)?.protected_paths
        != settings.protected_paths
    {
        return Err("扫描期间保护设置已变化，请重新扫描。".into());
    }
    Ok(report)
}

#[tauri::command]
async fn inspect_installers(
    root: String,
    state: tauri::State<'_, AppState>,
) -> Result<installers::Report, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出，请重新打开后扫描。".into());
    }
    let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取保护设置。")?)?;
    if !settings.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    let permit = state
        .installer_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "上次安装包目录扫描尚未结束，请稍后重试。")?;
    let operation = state.updates.installation.enter()?;
    let engine_root = state.engine_root.clone();
    let protected = settings.protected_paths.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let (_permit, _operation) = (permit, operation);
        let info = engine::info(&engine_root);
        if !info.available || info.version.as_deref() != Some("1.59.1") {
            return Err("Mole 引擎基线无法确认，请先检查引擎。".into());
        }
        installers::scan(std::path::Path::new(&root), &protected)
    });
    let report = tokio::time::timeout(Duration::from_secs(6), worker)
        .await
        .map_err(|_| "安装包目录扫描超时，系统读取可能仍在结束，请稍后重试。")?
        .map_err(|_| "安装包目录扫描未完成。")??;
    if db::settings(&*state.db.lock().map_err(|_| "无法读取当前保护设置。")?)?.protected_paths
        != settings.protected_paths
    {
        return Err("扫描期间保护设置已变化，请重新扫描。".into());
    }
    Ok(report)
}

#[tauri::command]
async fn inspect_installer_zip(
    root: String,
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<installers::zip::Inspection, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出，请重新打开后检查。".into());
    }
    let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取保护设置。")?)?;
    if !settings.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    let permit = state
        .installer_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "上次安装包读取尚未结束，请稍后重试。")?;
    let operation = state.updates.installation.enter()?;
    let engine_root = state.engine_root.clone();
    let worker = tauri::async_runtime::spawn(async move {
        let (_permit, _operation) = (permit, operation);
        let prepared = tauri::async_runtime::spawn_blocking(move || {
            let info = engine::info(&engine_root);
            if !info.available || info.version.as_deref() != Some("1.59.1") {
                return Err("Mole 引擎基线无法确认。".into());
            }
            installers::zip::Prepared::open(
                std::path::Path::new(&root),
                std::path::Path::new(&path),
            )
        })
        .await
        .map_err(|_| "ZIP 无法准备只读检查。".to_string())??;
        installers::zip::inspect(prepared).await
    });
    let report = tokio::time::timeout(Duration::from_secs(12), worker)
        .await
        .map_err(|_| "ZIP 检查未及时结束；系统读取可能仍在结束，请稍后重试。")?
        .map_err(|_| "ZIP 检查未完成。")??;
    if db::settings(&*state.db.lock().map_err(|_| "无法读取当前保护设置。")?)?.protected_paths
        != settings.protected_paths
    {
        return Err("检查期间保护设置已变化，请重新检查。".into());
    }
    Ok(report)
}

#[tauri::command]
async fn inspect_installer_zip_data(
    root: String,
    path: String,
    expected_stamp: String,
    state: tauri::State<'_, AppState>,
) -> Result<installers::zip::DataInspection, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出，请重新打开后检查。".into());
    }
    let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取保护设置。")?)?;
    if !settings.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    let permit = state
        .installer_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "上次安装包读取尚未结束，请稍后重试。")?;
    let operation = state.updates.installation.enter()?;
    let engine_root = state.engine_root.clone();
    let worker = tauri::async_runtime::spawn(async move {
        let (_permit, _operation) = (permit, operation);
        let prepared = tauri::async_runtime::spawn_blocking(move || {
            let info = engine::info(&engine_root);
            if !info.available || info.version.as_deref() != Some("1.59.1") {
                return Err("Mole 引擎基线无法确认。".into());
            }
            installers::zip::Prepared::open_data(
                std::path::Path::new(&root),
                std::path::Path::new(&path),
                &expected_stamp,
            )
        })
        .await
        .map_err(|_| "无法准备 ZIP 数据校验。".to_string())??;
        installers::zip::inspect_data(prepared).await
    });
    let report = tokio::time::timeout(Duration::from_secs(12), worker)
        .await
        .map_err(|_| "ZIP 数据校验未及时结束；系统读取可能仍在结束，请稍后重试。")?
        .map_err(|_| "ZIP 数据校验未完成。")??;
    if db::settings(&*state.db.lock().map_err(|_| "无法读取当前保护设置。")?)?.protected_paths
        != settings.protected_paths
    {
        return Err("检查期间保护设置已变化，请重新检查。".into());
    }
    Ok(report)
}

#[tauri::command]
async fn inspect_installer_connection(
    root: String,
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<installers::connection::Inspection, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出，请重新打开后检查。".into());
    }
    let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取保护设置。")?)?;
    if !settings.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    let permit = state
        .installer_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "上次安装包读取尚未结束，请稍后重试。")?;
    let operation = state.updates.installation.enter()?;
    let engine_root = state.engine_root.clone();
    let worker = tauri::async_runtime::spawn(async move {
        let (_permit, _operation) = (permit, operation);
        let prepared = tauri::async_runtime::spawn_blocking(move || {
            let info = engine::info(&engine_root);
            if !info.available || info.version.as_deref() != Some("1.59.1") {
                return Err("Mole 引擎基线无法确认。".into());
            }
            installers::connection::Prepared::open(
                std::path::Path::new(&root),
                std::path::Path::new(&path),
            )
        })
        .await
        .map_err(|_| "无法准备映像连接只读检查。".to_string())??;
        installers::connection::inspect(prepared).await
    });
    let report = tokio::time::timeout(Duration::from_secs(12), worker)
        .await
        .map_err(|_| "映像连接检查未及时结束；系统读取可能仍在结束，请稍后重试。")?
        .map_err(|_| "映像连接检查未完成。")??;
    if db::settings(&*state.db.lock().map_err(|_| "无法读取当前保护设置。")?)?.protected_paths
        != settings.protected_paths
    {
        return Err("检查期间保护设置已变化，请重新检查。".into());
    }
    Ok(report)
}

#[tauri::command]
async fn inspect_installer_usage(
    root: String,
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<installers::usage::Inspection, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出，请重新打开后检查。".into());
    }
    let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取保护设置。")?)?;
    if !settings.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    let permit = state
        .installer_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "上次安装包读取尚未结束，请稍后重试。")?;
    let operation = state.updates.installation.enter()?;
    let engine_root = state.engine_root.clone();
    let worker = tauri::async_runtime::spawn(async move {
        let (_permit, _operation) = (permit, operation);
        let prepared = tauri::async_runtime::spawn_blocking(move || {
            let info = engine::info(&engine_root);
            if !info.available || info.version.as_deref() != Some("1.59.1") {
                return Err("Mole 引擎基线无法确认。".into());
            }
            installers::usage::Prepared::open(
                std::path::Path::new(&root),
                std::path::Path::new(&path),
            )
        })
        .await
        .map_err(|_| "无法准备文件占用只读检查。".to_string())??;
        installers::usage::inspect(prepared).await
    });
    let report = tokio::time::timeout(Duration::from_secs(12), worker)
        .await
        .map_err(|_| "文件占用检查未及时结束；系统读取可能仍在结束，请稍后重试。")?
        .map_err(|_| "文件占用检查未完成。")??;
    if db::settings(&*state.db.lock().map_err(|_| "无法读取当前保护设置。")?)?.protected_paths
        != settings.protected_paths
    {
        return Err("检查期间保护设置已变化，请重新检查。".into());
    }
    Ok(report)
}

#[tauri::command]
async fn inspect_installer_image(
    root: String,
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<installers::image::Inspection, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出，请重新打开后检查。".into());
    }
    let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取保护设置。")?)?;
    if !settings.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    let permit = state
        .installer_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "上次安装包读取尚未结束，请稍后重试。")?;
    let operation = state.updates.installation.enter()?;
    let engine_root = state.engine_root.clone();
    let worker = tauri::async_runtime::spawn(async move {
        let (_permit, _operation) = (permit, operation);
        let prepared = tauri::async_runtime::spawn_blocking(move || {
            let info = engine::info(&engine_root);
            if !info.available || info.version.as_deref() != Some("1.59.1") {
                return Err("Mole 引擎基线无法确认。".into());
            }
            installers::image::Prepared::open(
                std::path::Path::new(&root),
                std::path::Path::new(&path),
            )
        })
        .await
        .map_err(|_| "无法准备DMG 只读校验。".to_string())??;
        installers::image::inspect(prepared).await
    });
    let report = tokio::time::timeout(Duration::from_secs(12), worker)
        .await
        .map_err(|_| "DMG 校验未及时结束；系统读取可能仍在结束，请稍后重试。")?
        .map_err(|_| "DMG 校验未完成。")??;
    if db::settings(&*state.db.lock().map_err(|_| "无法读取当前保护设置。")?)?.protected_paths
        != settings.protected_paths
    {
        return Err("检查期间保护设置已变化，请重新检查。".into());
    }
    Ok(report)
}

#[tauri::command]
async fn inspect_project_git(
    root: String,
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<project_artifacts::git_index::Report, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出，请重新打开后扫描。".into());
    }
    let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取保护设置。")?)?;
    if !settings.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    let permit = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "上次工程目录读取尚未结束，请稍后重试。")?;
    let operation = state.updates.installation.enter()?;
    let engine_root = state.engine_root.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let (_permit, _operation) = (permit, operation);
        let info = engine::info(&engine_root);
        if !info.available || info.version.as_deref() != Some("1.59.1") {
            return Err("Mole 引擎基线无法确认，请先检查引擎。".into());
        }
        project_artifacts::git_index::inspect(
            std::path::Path::new(&root),
            std::path::Path::new(&path),
        )
    });
    let report = tokio::time::timeout(Duration::from_secs(6), worker)
        .await
        .map_err(|_| "工程目录读取超时，系统读取可能仍在结束，请稍后重试。")?
        .map_err(|_| "工程目录读取未完成。")??;
    if db::settings(&*state.db.lock().map_err(|_| "无法读取当前保护设置。")?)?.protected_paths
        != settings.protected_paths
    {
        return Err("读取期间保护设置已变化，请重新扫描。".into());
    }
    Ok(report)
}

#[tauri::command]
async fn measure_project_artifact(
    root: String,
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<project_artifacts::measurement::Measurement, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出，请重新打开后扫描。".into());
    }
    let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取保护设置。")?)?;
    if !settings.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    let permit = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "上次工程目录读取尚未结束，请稍后重试。")?;
    let operation = state.updates.installation.enter()?;
    let engine_root = state.engine_root.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let (_permit, _operation) = (permit, operation);
        let info = engine::info(&engine_root);
        if !info.available || info.version.as_deref() != Some("1.59.1") {
            return Err("Mole 引擎基线无法确认，请先检查引擎。".into());
        }
        project_artifacts::measurement::measure(
            std::path::Path::new(&root),
            std::path::Path::new(&path),
        )
    });
    let report = tokio::time::timeout(Duration::from_secs(6), worker)
        .await
        .map_err(|_| "工程目录读取超时，系统读取可能仍在结束，请稍后重试。")?
        .map_err(|_| "工程目录读取未完成。")??;
    if db::settings(&*state.db.lock().map_err(|_| "无法读取当前保护设置。")?)?.protected_paths
        != settings.protected_paths
    {
        return Err("读取期间保护设置已变化，请重新扫描。".into());
    }
    Ok(report)
}

#[tauri::command]
async fn inspect_maintenance(
    state: tauri::State<'_, AppState>,
) -> Result<maintenance::Report, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出，请重新打开后检查。".into());
    }
    let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取设置。")?)?;
    if !settings.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    let permit = state
        .maintenance_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "上次维护检查仍未结束，请稍后重试。")?;
    let operation = state.updates.installation.enter()?;
    let root = state.engine_root.clone();
    let home = state.home.clone();
    let protected_paths = settings.protected_paths.clone();
    let generation = state
        .maintenance_reviews
        .lock()
        .map_err(|_| "无法记录本次维护检查。")?
        .begin()?;
    let worker = tauri::async_runtime::spawn_blocking(move || {
        // Hold both leases until the actual worker returns, even if the IPC
        // deadline expires. A timed-out OS read is not cancelled by dropping it.
        let (_permit, _operation) = (permit, operation);
        let info = engine::info(&root);
        if !info.available || info.version.as_deref() != Some("1.59.1") {
            return Err("Mole 引擎基线无法确认，请先检查引擎。".into());
        }
        maintenance::inspect(&home, &protected_paths)
    });
    let result = tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .map_err(|_| "维护检查超时，未确认的状态仍为未知；系统读取可能尚未结束。".to_string())?
        .map_err(|_| "维护检查未完成，请重试。".to_string())??;
    if db::settings(&*state.db.lock().map_err(|_| "无法读取当前保护设置。")?)?.protected_paths
        != settings.protected_paths
    {
        return Err("检查期间保护目录已变化，请重新检查。".into());
    }
    state
        .maintenance_reviews
        .lock()
        .map_err(|_| "无法保存本次维护检查。")?
        .publish(generation, result.clone(), settings.protected_paths)?;
    Ok(result)
}

#[tauri::command]
fn prepare_maintenance_review(
    observation_id: String,
    selected_ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<maintenance::review::Review, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出，请重新打开后再审阅。".into());
    }
    let _operation = state.updates.installation.enter()?;
    let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取保护设置。")?)?;
    if !settings.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    state
        .maintenance_reviews
        .lock()
        .map_err(|_| "无法读取维护检查。")?
        .prepare(
            &observation_id,
            &selected_ids,
            &settings.protected_paths,
            db::now(),
        )
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
async fn reveal_execution_item(
    run_id: String,
    item_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let _operation = state.updates.installation.enter()?;
    let database = state.db.clone();
    let location = tauri::async_runtime::spawn_blocking(move || {
        let detail = execution::detail(&*database.lock().map_err(|e| e.to_string())?, &run_id)?;
        execution_location::Location::prepare(&detail, &item_id)
    })
    .await
    .map_err(|_| "收据位置核对未完成，请重试。")??;
    location.reveal().await
}

#[tauri::command]
fn acknowledge_execution_review(
    kind: execution_review::Kind,
    run_id: String,
    snapshot_sha256: String,
    reviewed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if !reviewed {
        return Err("请先核对原对象、废纸篓和逐项结果，再明确确认。".into());
    }
    let _operation = state.updates.installation.enter()?;
    if state.shutdown.closing()
        || state
            .jobs
            .lock()
            .map_err(|_| "无法核对执行 worker。")?
            .contains_key(&run_id)
    {
        return Err("结果仍在收集中或应用正在退出，请稍后核查。".into());
    }
    execution_review::acknowledge(
        &mut *state.db.lock().map_err(|_| "无法保存人工核查。")?,
        kind,
        &run_id,
        &snapshot_sha256,
        db::now(),
    )
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
    let operation = state.updates.installation.enter()?;
    if matches!(
        kind,
        TaskKind::Review
            | TaskKind::LogTrash
            | TaskKind::AppTrash
            | TaskKind::MaintenanceRefresh
            | TaskKind::StatusWatch
            | TaskKind::VisibilityProbe
            | TaskKind::AiScan
    ) {
        return Err("请使用该功能的专用操作入口。".into());
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
    if state.shutdown.closing() {
        return Err("应用正在退出，请重新打开后再开始检查。".into());
    }
    if jobs.len() >= 3
        || jobs.contains_key(&task_id)
        || jobs.values().any(|job| {
            job.kind == kind
                || job.kind == TaskKind::AiScan
                || (kind == TaskKind::Status && job.kind == TaskKind::StatusWatch)
        })
    {
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
        let _operation = operation;
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

// Optional decoration: failure leaves the list usable with its generic icon.
// Permits live in the actual worker, including after a UI/timeout stops waiting.
#[tauri::command]
async fn application_icon(
    task_id: String,
    index: usize,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, String> {
    Ok(async {
        if state.shutdown.closing() || state.updates.installation.active() {
            return None;
        }
        uuid::Uuid::parse_str(&task_id).ok()?;
        let path = state.results.lock().ok()?.app_path(&task_id, index)?;
        app_icons::load(path, state.icon_slots.clone()).await
    }
    .await)
}
#[tauri::command]
async fn inspect_application(
    app: tauri::AppHandle,
    task_id: String,
    index: usize,
    state: tauri::State<'_, AppState>,
) -> Result<app_inspection::Inspection, String> {
    if state.shutdown.closing() || state.updates.installation.active() {
        return Err("当前暂不能检查应用状态，请稍后重试。".into());
    }
    uuid::Uuid::parse_str(&task_id).map_err(|_| "应用清单无效，请重新读取。")?;
    let (path, bundle_id, name, candidates) = {
        let mut results = state.results.lock().map_err(|_| "无法读取应用清单。")?;
        let (path, id, name) = results
            .app_target(&task_id, index)
            .ok_or("这份应用清单已不可用，请重新读取列表。")?;
        (path, id, name, results.app_candidates(&task_id))
    };
    let mut result = app_inspection::load(
        path.clone(),
        bundle_id,
        name,
        candidates,
        state.engine_root.clone(),
        state.home.clone(),
        state.inspection_slots.clone(),
    )
    .await?;
    // Read after the asynchronous observation: a newly saved directory must not
    // be missed because a filesystem/AppKit read took longer than the settings save.
    let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取当前保护目录。")?)?;
    let executable = std::env::current_exe().ok();
    result.attach_protection(
        &path,
        executable
            .as_deref()
            .and_then(app_protection::containing_app),
        &app.config().identifier,
        &state.home,
        &settings.protected_paths,
    )?;
    Ok(result)
}

fn uninstall_context(
    state: &AppState,
    current_id: &str,
) -> Result<uninstall_plan::Context, String> {
    let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取保护设置。")?)?;
    if !settings.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    let executable = std::env::current_exe().ok();
    Ok(uninstall_plan::Context {
        home: state.home.clone(),
        engine_root: state.engine_root.clone(),
        current_id: current_id.into(),
        current_app: executable
            .as_deref()
            .and_then(app_protection::containing_app)
            .map(std::path::Path::to_path_buf),
        protected_paths: settings.protected_paths,
    })
}

async fn observe_uninstall_plan(
    state: AppState,
    ctx: uninstall_plan::Context,
    source: uninstall_plan::Source,
    existing: Option<uninstall_plan::Plan>,
) -> Result<uninstall_plan::View, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出，请重新打开后再准备卸载计划。".into());
    }
    let permit = state
        .inspection_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "已有应用检查尚未结束，请稍后重试。")?;
    let operation = state.updates.installation.enter()?;
    let path = source.path().to_string();
    let generation = state
        .uninstall_plans
        .lock()
        .map_err(|_| "无法准备卸载计划。")?
        .begin(&path, existing.as_ref().map(uninstall_plan::Plan::id))?;
    state
        .uninstall_confirmations
        .lock()
        .map_err(|_| "无法撤下旧卸载确认。")?
        .revoke_path(&path);
    // The worker retains admission/ownership if the WebView observer disappears
    // or the IPC timeout fires. No native handles cross this thread boundary.
    let worker = tauri::async_runtime::spawn(async move {
        let (_permit, _operation) = (permit, operation);
        let result = async {
            let work_source = source.clone();
            let work_ctx = ctx.clone();
            let plan = tauri::async_runtime::spawn_blocking(move || {
                if let Some(plan) = existing {
                    plan.recheck(&work_ctx, db::now())?;
                    Ok(plan)
                } else {
                    uninstall_plan::Plan::prepare(work_source, &work_ctx)
                }
            })
            .await
            .map_err(|_| "应用快照检查未完成。".to_string())??;
            if state.shutdown.closing() {
                return Err("应用正在退出，未保留本次卸载计划。".into());
            }
            let latest = uninstall_context(&state, &ctx.current_id)?;
            if latest.protected_paths != ctx.protected_paths {
                return Err("检查期间保护设置已变化，请重新准备卸载计划。".into());
            }
            let (task_id, index) = plan.source_address();
            let current = state
                .results
                .lock()
                .map_err(|_| "无法重新核对应用清单。")?
                .uninstall_target(task_id, index)?;
            if !plan.source_matches(&current) || !plan.source_matches(&source) {
                return Err("原应用清单已变化，请重新读取后再准备卸载计划。".into());
            }
            state
                .uninstall_plans
                .lock()
                .map_err(|_| "无法保留卸载计划。")?
                .publish(generation, plan, db::now())
        }
        .await;
        if let Ok(mut store) = state.uninstall_plans.lock() {
            store.finish_attempt(&path, generation);
        }
        result
    });
    tokio::time::timeout(Duration::from_secs(12), worker)
        .await
        .map_err(|_| "应用计划检查未及时完成；系统只读检查可能仍在结束，请稍后重试。")?
        .map_err(|_| "应用计划检查未完成。".to_string())?
}

#[tauri::command]
async fn prepare_application_uninstall_plan(
    app: tauri::AppHandle,
    task_id: String,
    index: usize,
    state: tauri::State<'_, AppState>,
) -> Result<uninstall_plan::View, String> {
    let ctx = uninstall_context(&state, &app.config().identifier)?;
    let source = state
        .results
        .lock()
        .map_err(|_| "无法读取应用清单。")?
        .uninstall_target(&task_id, index)?;
    observe_uninstall_plan(state.inner().clone(), ctx, source, None).await
}

#[tauri::command]
async fn recheck_application_uninstall_plan(
    app: tauri::AppHandle,
    plan_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<uninstall_plan::View, String> {
    uuid::Uuid::parse_str(&plan_id).map_err(|_| "卸载计划标识无效。")?;
    let ctx = uninstall_context(&state, &app.config().identifier)?;
    let plan = state
        .uninstall_plans
        .lock()
        .map_err(|_| "无法读取卸载计划。")?
        .get(&plan_id, db::now())?;
    let (task_id, index) = plan.source_address();
    let source = state
        .results
        .lock()
        .map_err(|_| "无法核对原应用清单。")?
        .uninstall_target(task_id, index)?;
    observe_uninstall_plan(state.inner().clone(), ctx, source, Some(plan)).await
}

#[tauri::command]
fn discard_application_uninstall_plan(
    plan_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state
        .uninstall_plans
        .lock()
        .map_err(|_| "无法撤下卸载计划。")?
        .discard(&plan_id);
    state
        .uninstall_confirmations
        .lock()
        .map_err(|_| "无法撤下卸载确认。")?
        .revoke_plan(&plan_id);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UninstallReview {
    runtime: uninstall_runtime::Report,
    support: uninstall_support::Report,
    confirmation: Option<uninstall_confirmation::Confirmation>,
}

#[tauri::command]
async fn prepare_application_uninstall_confirmation(
    app: tauri::AppHandle,
    plan_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<UninstallReview, String> {
    uuid::Uuid::parse_str(&plan_id).map_err(|_| "卸载计划标识无效。")?;
    if state.shutdown.closing() {
        return Err("应用正在退出，不能复查卸载。".into());
    }
    let ctx = uninstall_context(&state, &app.config().identifier)?;
    let plan = state
        .uninstall_plans
        .lock()
        .map_err(|_| "无法读取卸载计划。")?
        .get(&plan_id, db::now())?;
    let permit = state
        .inspection_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "已有应用检查尚未结束，请稍后重试。")?;
    let operation = state.updates.installation.enter()?;
    let generation = state
        .uninstall_confirmations
        .lock()
        .map_err(|_| "无法准备卸载复查。")?
        .begin_review(&plan_id, db::now())?;
    let state = state.inner().clone();
    let worker = tauri::async_runtime::spawn(async move {
        let (_permit, _operation) = (permit, operation);
        let result = async {
            let observed_plan = plan.clone();
            let observed_ctx = ctx.clone();
            let (support, observation) = tauri::async_runtime::spawn_blocking(move || {
                let support = uninstall_support::observe(&observed_plan, &observed_ctx)?;
                // The expensive metadata scope precedes the short-lived
                // process pass. Both retain their own original deadlines.
                let runtime = uninstall_runtime::observe(&observed_plan, &observed_ctx)?;
                Ok::<_, String>((support, runtime))
            })
            .await
            .map_err(|_| "卸载运行检查未完成。".to_string())??;
            if state.shutdown.closing() {
                return Err("应用正在退出，本次复查已撤回。".into());
            }
            let latest = uninstall_context(&state, &ctx.current_id)?;
            if latest.protected_paths != ctx.protected_paths
                || latest.current_app != ctx.current_app
            {
                return Err("复查期间保护设置或客户端位置变化，请重新准备计划。".into());
            }
            let published = state
                .uninstall_plans
                .lock()
                .map_err(|_| "无法核对当前卸载计划。")?
                .get(&plan_id, db::now())?;
            if uninstall_runtime::fingerprint(&published)? != uninstall_runtime::fingerprint(&plan)?
            {
                return Err("原卸载计划已变化，本次复查撤回。".into());
            }
            let (task_id, index) = plan.source_address();
            let source = state
                .results
                .lock()
                .map_err(|_| "无法核对原应用清单。")?
                .uninstall_target(task_id, index)?;
            if !plan.source_matches(&source) {
                return Err("原应用清单已变化，请重新读取。".into());
            }
            let confirmation = state
                .uninstall_confirmations
                .lock()
                .map_err(|_| "无法保存本次确认范围。")?
                .publish(
                    generation,
                    &plan,
                    &latest,
                    &observation,
                    &support,
                    db::now(),
                )?;
            Ok(UninstallReview {
                runtime: observation.report().clone(),
                support: support.report().clone(),
                confirmation,
            })
        }
        .await;
        if let Ok(mut store) = state.uninstall_confirmations.lock() {
            store.finish_review(&plan_id, generation);
        }
        result
    });
    tokio::time::timeout(Duration::from_secs(12), worker)
        .await
        .map_err(|_| "运行检查未及时完成；实际只读 worker 可能仍在结束，请稍后重试。")?
        .map_err(|_| "卸载复查未完成。".to_string())?
}

#[tauri::command]
fn discard_application_uninstall_confirmation(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state
        .uninstall_confirmations
        .lock()
        .map_err(|_| "无法撤下卸载确认。")?
        .discard(&id);
    Ok(())
}

// Returns no frontend-supplied path or eligibility. Every final gate reacquires
// the still-published plan, retained Mole source and current user protections.
fn live_uninstall_context(
    state: &AppState,
    plan: &uninstall_plan::Plan,
    current_id: &str,
) -> Result<uninstall_plan::Context, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出，本次卸载停止。".into());
    }
    let ctx = uninstall_context(state, current_id)?;
    let published = state
        .uninstall_plans
        .lock()
        .map_err(|_| "无法核对当前卸载计划。")?
        .get(plan.id(), db::now())?;
    if uninstall_runtime::fingerprint(&published)? != uninstall_runtime::fingerprint(plan)? {
        return Err("原卸载计划已变化，请重新准备。".into());
    }
    let (task, index) = plan.source_address();
    let source = state
        .results
        .lock()
        .map_err(|_| "无法核对原应用清单。")?
        .uninstall_target(task, index)?;
    if !plan.source_matches(&source) {
        return Err("原应用清单已变化或过期，请重新读取。".into());
    }
    Ok(ctx)
}

#[tauri::command]
async fn execute_application_uninstall(
    plan_id: String,
    confirmation_id: String,
    request_id: String,
    ordinary_app_confirmed: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<uninstall_journal::Detail, String> {
    uuid::Uuid::parse_str(&request_id).map_err(|_| "卸载请求标识无效。")?;
    uuid::Uuid::parse_str(&plan_id).map_err(|_| "卸载计划标识无效。")?;
    if !ordinary_app_confirmed {
        return Err(
            "请先确认这是首批支持的普通 App；受管理或带专用服务的应用应使用对应卸载方式。".into(),
        );
    }
    let operation = state.updates.installation.enter()?;
    let current_id = app.config().identifier.clone();
    let ctx = uninstall_context(&state, &current_id)?;
    let plan = state
        .uninstall_plans
        .lock()
        .map_err(|_| "无法读取卸载计划。")?
        .get(&plan_id, db::now())?;
    let job = Arc::new(Job::new(TaskKind::AppTrash));
    {
        let mut jobs = state.jobs.lock().map_err(|_| "无法登记卸载任务。")?;
        if state.shutdown.closing()
            || jobs.len() >= 3
            || jobs.contains_key(&request_id)
            || jobs.values().any(|j| j.kind.exclusive_operation())
        {
            return Err("已有修改/权限检查任务，或应用正在退出，请稍后重试。".into());
        }
        jobs.insert(request_id.clone(), job.clone());
    }
    let selection = state
        .uninstall_confirmations
        .lock()
        .map_err(|_| "无法取用卸载确认。".to_string())
        .and_then(|mut s| s.take(&confirmation_id, &plan, &ctx, db::now()));
    let selection = match selection {
        Ok(value) => value,
        Err(error) => {
            if let Ok(mut jobs) = state.jobs.lock() {
                jobs.remove(&request_id);
            }
            return Err(error);
        }
    };
    let state = state.inner().clone();
    let receive = uninstall_execution::dispatch(
        request_id.clone(),
        state.jobs.clone(),
        job.clone(),
        operation,
        async move {
            let gate_state = state.clone();
            let gate_plan = plan.clone();
            let result = uninstall_execution::run(
                state.db.clone(),
                request_id.clone(),
                plan,
                selection,
                job,
                move || live_uninstall_context(&gate_state, &gate_plan, &current_id),
            )
            .await;
            result
        },
    );
    receive
        .await
        .map_err(|_| "卸载响应中断，请按请求标识读取卸载记录，不要自动重试。".to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UninstallSnapshot {
    version: u32,
    request_id: String,
    active: bool,
    record: Option<uninstall_journal::Detail>,
}
#[tauri::command]
fn get_application_uninstall(
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<UninstallSnapshot, String> {
    uuid::Uuid::parse_str(&request_id).map_err(|_| "卸载请求标识无效。")?;
    let active = || {
        state
            .jobs
            .lock()
            .map(|jobs| {
                jobs.get(&request_id)
                    .is_some_and(|j| j.kind == TaskKind::AppTrash)
            })
            .map_err(|_| "无法核对卸载任务。".to_string())
    };
    let was_active = active()?;
    let read_record = || {
        let db = state.db.lock().map_err(|_| "无法读取卸载记录。")?;
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM uninstall_runs WHERE id=?1)",
                [&request_id],
                |r| r.get(0),
            )
            .map_err(|_| "无法核对卸载记录。")?;
        if exists {
            Ok::<_, String>(Some(uninstall_journal::detail(&db, &request_id)?))
        } else {
            Ok(None)
        }
    };
    let mut record = read_record()?;
    let is_active = active()?;
    if !is_active
        && (was_active
            || record
                .as_ref()
                .is_some_and(|r| r.summary.state == "running"))
    {
        // The worker publishes before removing its handle. If it completed
        // during this read, fetch its durable terminal result once more.
        record = read_record()?;
    }
    // An absent request is an explicit read result, never permission to retry.
    // The UI must allow an initial admission race before treating absence as
    // terminal; the execution call and registry remain independently owned.
    Ok(UninstallSnapshot {
        version: 1,
        request_id,
        active: is_active,
        record,
    })
}
#[tauri::command]
fn list_application_uninstalls(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<uninstall_journal::Summary>, String> {
    uninstall_journal::list(&*state.db.lock().map_err(|_| "无法读取卸载历史。")?)
}
#[tauri::command]
fn cancel_application_uninstall(
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    uuid::Uuid::parse_str(&request_id).map_err(|_| "卸载请求标识无效。")?;
    let jobs = state.jobs.lock().map_err(|_| "无法核对卸载任务。")?;
    let job = jobs
        .get(&request_id)
        .filter(|j| j.kind == TaskKind::AppTrash)
        .ok_or("此卸载任务已结束，请读取实际记录。")?;
    job.cancel();
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
    let operation = state.updates.installation.enter()?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
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
    state
        .cleanup_confirmations
        .lock()
        .map_err(|e| e.to_string())?
        .revoke_plan(&plan_id);
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
    let _operation = state.updates.installation.enter()?;
    state
        .cleanup_confirmations
        .lock()
        .map_err(|e| e.to_string())?
        .revoke_plan(&plan_id);
    uuid::Uuid::parse_str(&request_id).map_err(|_| "复查任务 ID 无效。")?;
    let state = state.inner().clone();
    let job = Arc::new(Job::new(TaskKind::Review));
    {
        let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
        if state.shutdown.closing() {
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
        let finish_state = state.clone();
        let finish_job = job.clone();
        // Keep native reference preparation on this blocking thread. No native
        // handles cross IPC; a separate short-lived server confirmation binds scope.
        let view = tauri::async_runtime::spawn_blocking(move || -> Result<_, String> {
            let settings = db::settings(&*finish_state.db.lock().map_err(|e| e.to_string())?)?;
            let checked = db::now();
            let mut view = descendant_rules::apply(
                review::apply_rule_checks(
                    review::check(&plan, &settings.protected_paths, checked),
                    &checks,
                    checked,
                ),
                &batch,
                &checks,
            );
            let mut preparation =
                cleanup_preparation::observe(&plan, &view, &finish_state.home, checked, || {
                    finish_job.cancelled.load(Ordering::SeqCst)
                });
            let latest = db::settings(&*finish_state.db.lock().map_err(|e| e.to_string())?)?;
            let finished = db::now();
            if latest.protected_paths != settings.protected_paths
                || finished < checked
                || finished >= preparation.expires_at
            {
                view = descendant_rules::apply(
                    review::apply_rule_checks(
                        review::check(&plan, &latest.protected_paths, finished),
                        &checks,
                        checked,
                    ),
                    &batch,
                    &checks,
                );
                preparation.withdraw("准备期间保护或有效期已变化，请重新复查 Mole 规则。");
            }
            view.cleanup_preparation = Some(preparation);
            if !finish_job.cancelled.load(Ordering::SeqCst) {
                view.cleanup_confirmation = finish_state
                    .cleanup_confirmations
                    .lock()
                    .map_err(|e| e.to_string())?
                    .issue(&plan, view.cleanup_preparation.as_ref().unwrap(), db::now())?;
            }
            Ok(view)
        })
        .await
        .map_err(|e| e.to_string())??;
        if job.cancelled.load(Ordering::SeqCst) {
            return Err("规则复查已取消。".into());
        }
        Ok(view)
    }
    .await;
    if let Ok(mut jobs) = state.jobs.lock() {
        jobs.remove(&request_id);
    }
    result
}

#[tauri::command]
fn discard_cleanup_confirmation(
    confirmation_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state
        .cleanup_confirmations
        .lock()
        .map_err(|e| e.to_string())?
        .discard(&confirmation_id);
    Ok(())
}

#[tauri::command]
async fn execute_log_cleanup(
    confirmation_id: String,
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<execution::Detail, String> {
    let operation = state.updates.installation.enter()?;
    uuid::Uuid::parse_str(&request_id).map_err(|_| "处理任务 ID 无效。")?;
    let job = Arc::new(Job::new(TaskKind::LogTrash));
    {
        let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
        if state.shutdown.closing()
            || jobs.len() >= 3
            || jobs.contains_key(&request_id)
            || jobs.values().any(|j| j.kind.exclusive_operation())
        {
            return Err("已有清理正在进行或应用正在退出，请稍后重试。".into());
        }
        jobs.insert(request_id.clone(), job.clone());
    }
    let authorized = (|| {
        state
            .cleanup_confirmations
            .lock()
            .map_err(|e| e.to_string())?
            .take(&confirmation_id, db::now())
    })();
    let authorized = match authorized {
        Ok(entry) => entry,
        Err(error) => {
            state
                .jobs
                .lock()
                .map_err(|e| e.to_string())?
                .remove(&request_id);
            return Err(error);
        }
    };
    let validation = (|| {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        authorized.validate_source(&db::plan(&db, authorized.plan_id())?, db::now())
    })();
    if let Err(error) = validation {
        state
            .jobs
            .lock()
            .map_err(|e| e.to_string())?
            .remove(&request_id);
        return Err(error);
    }
    let state = state.inner().clone();
    let (send, receive) = tokio::sync::oneshot::channel();
    // The worker owns its admission/job even if the WebView disappears. Closing
    // the dialog requests cancellation but never discards an in-flight outcome.
    tauri::async_runtime::spawn(async move {
        let _operation = operation;
        let result = log_cleanup::run(
            state.db.clone(),
            state.engine_root.clone(),
            state.home.clone(),
            authorized,
            job,
        )
        .await;
        if let Ok(mut jobs) = state.jobs.lock() {
            jobs.remove(&request_id);
        }
        let _ = send.send(result);
    });
    receive
        .await
        .map_err(|_| "处理响应中断，请在历史中核对实际结果。".to_string())?
}

#[tauri::command]
fn cancel_log_cleanup(run_id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    uuid::Uuid::parse_str(&run_id).map_err(|_| "执行批次 ID 无效。")?;
    // Only the exact live binding may be cancelled. Never use the current
    // LogTrash job alone: a late click must not cancel a newer batch.
    let jobs = state.jobs.lock().map_err(|e| e.to_string())?;
    for job in jobs.values() {
        if job.kind == TaskKind::LogTrash && job.cancel_execution(&run_id) {
            return Ok(());
        }
    }
    Err("该批次已结束或无法继续跟踪，请刷新执行记录核对结果。".into())
}

#[tauri::command]
fn cancel_task(task_id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    if let Some(job) = state.jobs.lock().map_err(|e| e.to_string())?.get(&task_id) {
        job.cancel();
    }
    Ok(())
}

#[tauri::command]
fn get_cleanup_visibility(
    state: tauri::State<'_, AppState>,
) -> Result<activity_authorization::Snapshot, String> {
    Ok(state
        .cleanup_visibility
        .lock()
        .map_err(|_| "无法读取权限检查状态。")?
        .snapshot())
}

#[tauri::command]
fn start_cleanup_visibility(
    state: tauri::State<'_, AppState>,
) -> Result<activity_authorization::Snapshot, String> {
    let operation = state.updates.installation.enter()?;
    if !db::settings(&*state.db.lock().map_err(|_| "无法读取使用设置。")?)?.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    let mut jobs = state.jobs.lock().map_err(|_| "无法确认当前任务。")?;
    if state.shutdown.closing()
        || jobs.len() >= 3
        || jobs.values().any(|j| j.kind.exclusive_operation())
    {
        return Err("当前正在处理文件、检查权限或退出，请稍后重试。".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let job = Arc::new(Job::new(TaskKind::VisibilityProbe));
    let snapshot = state
        .cleanup_visibility
        .lock()
        .map_err(|_| "无法开始权限检查。")?
        .begin(id.clone(), db::now())?;
    jobs.insert(id.clone(), job.clone());
    drop(jobs);
    let state = state.inner().clone();
    // Own the operation beyond the IPC observer/dialog. A UI reload only reads
    // the snapshot; it never replays authorization or starts another process.
    tauri::async_runtime::spawn(async move {
        let _operation = operation;
        let (result, diagnostic) = activity_authorization::run(job.clone()).await;
        if let Ok(mut jobs) = state.jobs.lock() {
            if let Ok(mut store) = state.cleanup_visibility.lock() {
                store.finish_with_diagnostic(
                    &id,
                    if job.cancelled.load(Ordering::SeqCst) {
                        activity_authorization::Observation::Cancelled
                    } else {
                        result
                    },
                    db::now(),
                    diagnostic,
                );
            }
            jobs.remove(&id);
        }
    });
    Ok(snapshot)
}

#[tauri::command]
fn cancel_cleanup_visibility(
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<activity_authorization::Snapshot, String> {
    let jobs = state.jobs.lock().map_err(|_| "无法读取检查任务。")?;
    let job = jobs
        .get(&request_id)
        .filter(|j| j.kind == TaskKind::VisibilityProbe)
        .ok_or("这次检查已结束，请刷新查看结果。")?;
    let snapshot = state
        .cleanup_visibility
        .lock()
        .map_err(|_| "无法读取检查状态。")?
        .cancel(&request_id)?;
    job.cancel();
    Ok(snapshot)
}

#[tauri::command]
fn start_status_watch(
    session_id: String,
    interval_seconds: u64,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let interval = status_watch::Interval::try_from(interval_seconds)?;
    let operation = state.updates.installation.enter()?;
    uuid::Uuid::parse_str(&session_id).map_err(|_| "采集会话 ID 无效。")?;
    let database = state.db.lock().map_err(|e| e.to_string())?;
    if !db::settings(&database)?.acknowledged {
        return Err("请先确认首次使用说明。".into());
    }
    let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
    if state.shutdown.closing()
        || jobs.len() >= 3
        || jobs.contains_key(&session_id)
        || jobs.values().any(|j| {
            matches!(
                j.kind,
                TaskKind::Status | TaskKind::StatusWatch | TaskKind::AiScan
            )
        })
    {
        return Err("已有状态采集正在运行或应用正在退出，请稍后重试。".into());
    }
    let mut store = state.status_watch.lock().map_err(|e| e.to_string())?;
    store.begin(&session_id, interval)?;
    if let Err(error) = db::start(&database, &session_id, "statuswatch", None) {
        store.finish(&session_id, Some(error.clone()));
        return Err(error);
    }
    let job = Arc::new(Job::new(TaskKind::StatusWatch));
    jobs.insert(session_id.clone(), job.clone());
    drop(store);
    drop(jobs);
    drop(database);
    let state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let _operation = operation;
        let result = status_watch::run(
            &state.engine_root,
            &state.home,
            session_id.clone(),
            job.clone(),
            state.status_watch.clone(),
            interval,
        )
        .await;
        let cancelled = job.cancelled.load(Ordering::SeqCst);
        let error = if cancelled {
            None
        } else {
            Some(
                result
                    .err()
                    .unwrap_or("Mole 状态流已结束，请重新开始。".into()),
            )
        };
        let message = error
            .as_deref()
            .unwrap_or("持续采集已停止，最后快照仅供查阅。");
        let persisted = state.db.lock().map_err(|e| e.to_string()).and_then(|db| {
            db::finish(
                &db,
                &session_id,
                if cancelled { "cancelled" } else { "failed" },
                message,
            )
        });
        if let Ok(mut store) = state.status_watch.lock() {
            store.finish(
                &session_id,
                persisted
                    .err()
                    .map(|e| format!("采集已停止，历史写入失败：{e}"))
                    .or(error),
            );
        }
        if let Ok(mut jobs) = state.jobs.lock() {
            jobs.remove(&session_id);
        }
    });
    Ok(())
}

#[tauri::command]
fn get_status_watch(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<status_watch::Snapshot, String> {
    state
        .status_watch
        .lock()
        .map_err(|e| e.to_string())?
        .read(&session_id)
}

#[tauri::command]
async fn stop_status_watch(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if let Some(job) = state
        .jobs
        .lock()
        .map_err(|e| e.to_string())?
        .get(&session_id)
        .filter(|j| j.kind == TaskKind::StatusWatch)
    {
        job.cancel();
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if !state
            .jobs
            .lock()
            .map_err(|e| e.to_string())?
            .get(&session_id)
            .is_some_and(|j| j.kind == TaskKind::StatusWatch)
        {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("仍在等待采集进程停止，请稍后重试。".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
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
async fn check_updates(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<updates::Snapshot, String> {
    let operation = state.updates.installation.enter()?;
    let path = app
        .path()
        .app_cache_dir()
        .map_err(|_| "无法定位本地更新缓存。")?
        .join("pending-updates");
    let checker = state.updates.clone();
    let db = state.db.clone();
    tauri::async_runtime::spawn(async move {
        let _operation = operation;
        checker
            .check(Some(path.clone()), || {
                // Serialize the live preference decision with saves; toggling off after
                // this point affects subsequent checks, not an already accepted download.
                let db = db.lock().map_err(|_| "无法读取本机设置。")?;
                checker.after_check(update_preferences::load(&db), path)?;
                Ok(())
            })
            .await
    })
    .await
    .map_err(|_| "更新检查未完成，请重试。")?
}

#[tauri::command]
fn set_update_auto_download(
    enabled: bool,
    state: tauri::State<'_, AppState>,
) -> Result<updates::Snapshot, String> {
    let _operation = state.updates.installation.enter()?;
    let db = state.db.lock().map_err(|_| "无法保存本机设置。")?;
    update_preferences::save(&db, enabled)?;
    state.updates.set_preference(Ok(enabled))?;
    state.updates.snapshot()
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
    let _operation = state.updates.installation.enter()?;
    let root = app
        .path()
        .app_cache_dir()
        .map_err(|_| "无法定位应用缓存。")?;
    state.updates.start(root.join("pending-updates"))
}
#[tauri::command]
fn cancel_update_download(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.updates.cancel()
}
#[tauri::command]
async fn reveal_update_download(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let _operation = state.updates.installation.enter()?;
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
async fn install_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let state = state.inner().clone();
    // Remains owned by the backend even if the settings panel is closed.
    tauri::async_runtime::spawn(async move {
        state
            .updates
            .install(
                || async {
                    tauri::async_runtime::spawn_blocking(move || {
                        let db = state.db.lock().map_err(|_| "无法确认本机数据保存状态。")?;
                        update_install::prepare_data(&db)
                    })
                    .await
                    .map_err(|_| "本机数据保存检查未完成。")?
                },
                update_install::open_dmg,
                || app.exit(0),
            )
            .await
    })
    .await
    .map_err(|_| "安装准备未完成，应用保持运行。")?
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
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(
                    event,
                    tauri::WindowEvent::Moved(_)
                        | tauri::WindowEvent::Resized(_)
                        | tauri::WindowEvent::ScaleFactorChanged { .. }
                        | tauri::WindowEvent::CloseRequested { .. }
                )
            {
                if let Some(state) = window.try_state::<AppState>() {
                    if !state.shutdown.closing() {
                        state.window_placement.capture(window);
                    }
                }
            }
            if matches!(event, tauri::WindowEvent::Destroyed)
                || (matches!(event, tauri::WindowEvent::Resized(_))
                    && window.is_minimized().unwrap_or(false))
            {
                if let Some(state) = window.try_state::<AppState>() {
                    if let Ok(jobs) = state.jobs.lock() {
                        for job in jobs
                            .values()
                            .filter(|job| job.kind == TaskKind::StatusWatch)
                        {
                            job.cancel();
                        }
                    }
                }
            }
        })
        .setup(|app| {
            language::apply_menu(app.handle(), "system")?;
            let data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data)?;
            let connection = match db::open_owned(&data.join("moomole.sqlite")) {
                Ok(connection) => connection,
                Err(error) => {
                    let busy = matches!(error, db::OpenError::Instance(instance::Error::Busy));
                    let title = if busy {
                        language::text(
                            "system",
                            "MooMole 已在运行",
                            "MooMole is already running",
                            "MooMole は起動済みです",
                        )
                    } else {
                        language::text(
                            "system",
                            "无法打开 MooMole 本机数据",
                            "Unable to open MooMole local data",
                            "MooMole のローカルデータを開けません",
                        )
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
            let updates = Arc::new(updates::Checker::default());
            updates.set_preference(update_preferences::load(&connection))?;
            let placement = window_placement::load(&connection).unwrap_or_else(|error| {
                eprintln!("窗口偏好无法读取，使用默认窗口：{error}");
                None
            });
            let window_placement = Arc::new(window_placement::Tracker::default());
            if let Ok(cache) = app.path().app_cache_dir() {
                updates.schedule_maintenance(cache.join("pending-updates"));
            }
            app.manage(AppState {
                db: Arc::new(Mutex::new(connection)),
                jobs: Arc::new(Mutex::new(HashMap::new())),
                results: Arc::new(Mutex::new(tasks::Results::default())),
                archive_slots: Arc::new(tokio::sync::Semaphore::new(1)),
                icon_slots: Arc::new(tokio::sync::Semaphore::new(2)),
                inspection_slots: Arc::new(tokio::sync::Semaphore::new(2)),
                maintenance_slots: Arc::new(tokio::sync::Semaphore::new(1)),
                project_slots: Arc::new(tokio::sync::Semaphore::new(1)),
                installer_slots: Arc::new(tokio::sync::Semaphore::new(1)),
                maintenance_reviews: Arc::new(Mutex::new(maintenance::review::Store::default())),
                refresh_plans: Arc::new(Mutex::new(maintenance::refresh_plan::Store::default())),
                cleanup_confirmations: Arc::new(Mutex::new(log_cleanup::Confirmations::default())),
                uninstall_plans: Arc::new(Mutex::new(uninstall_plan::Store::default())),
                uninstall_confirmations: Arc::new(Mutex::new(
                    uninstall_confirmation::Confirmations::default(),
                )),
                status_watch: Arc::new(Mutex::new(status_watch::Store::default())),
                cleanup_visibility: Arc::new(Mutex::new(activity_authorization::Store::default())),
                ai_scan: Arc::new(Mutex::new(ai_scan::Store::default())),
                engine_root,
                home: app.path().home_dir()?,
                shutdown: Arc::new(shutdown::Controller::default()),
                updates,
                window_placement: window_placement.clone(),
            });
            let preference = language::saved(&app.state::<AppState>());
            language::apply_menu(app.handle(), &preference)?;
            // Restore only after database ownership, without holding a mutex
            // during native size/move calls or overwriting settings/history.
            if let Some(window) = app.get_webview_window("main") {
                window_placement.restore(&window.as_ref().window(), placement);
                window.show()?;
                window_placement.capture(&window.as_ref().window());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            ai_scan::detect_ai_agents,
            ai_scan::ai_scan_skill,
            ai_scan::start_ai_scan,
            ai_scan::get_ai_scan,
            ai_scan::cancel_ai_scan,
            save_settings,
            start_cleanup_visibility,
            get_cleanup_visibility,
            cancel_cleanup_visibility,
            get_history,
            prepare_task_diagnostic,
            save_task_diagnostic,
            list_executions,
            get_execution,
            acknowledge_execution_review,
            execution_archive_commands::prepare_execution_archive,
            execution_archive_commands::archive_execution_history,
            execution_archive_commands::list_execution_archives,
            execution_archive_commands::read_execution_archive,
            execution_archive_commands::get_archived_execution,
            execution_archive_commands::import_execution_archive,
            execution_archive_commands::export_execution_archive,
            execution_archive_commands::reveal_execution_archives,
            reveal_execution_item,
            get_task_snapshot,
            application_icon,
            inspect_application,
            prepare_application_uninstall_plan,
            recheck_application_uninstall_plan,
            discard_application_uninstall_plan,
            prepare_application_uninstall_confirmation,
            discard_application_uninstall_confirmation,
            execute_application_uninstall,
            get_application_uninstall,
            list_application_uninstalls,
            cancel_application_uninstall,
            inspect_maintenance,
            maintenance::refresh_commands::prepare_quicklook_refresh,
            maintenance::refresh_commands::discard_quicklook_refresh,
            maintenance::refresh_commands::execute_quicklook_refresh,
            maintenance::refresh_commands::get_quicklook_refresh,
            maintenance::refresh_commands::list_quicklook_refreshes,
            maintenance::refresh_commands::cancel_quicklook_refresh,
            maintenance::refresh_commands::acknowledge_quicklook_refresh,
            inspect_project_artifacts,
            read_project_roots,
            inspect_installers,
            inspect_installer_zip,
            inspect_installer_zip_data,
            inspect_installer_connection,
            inspect_installer_usage,
            inspect_installer_image,
            measure_project_artifact,
            inspect_project_git,
            prepare_maintenance_review,
            list_review_plans,
            create_review_plan,
            get_review_plan,
            check_review_rules,
            discard_cleanup_confirmation,
            execute_log_cleanup,
            cancel_log_cleanup,
            start_task,
            cancel_task,
            start_status_watch,
            get_status_watch,
            stop_status_watch,
            reveal_path,
            open_official,
            check_updates,
            get_update_snapshot,
            set_update_auto_download,
            download_update,
            cancel_update_download,
            reveal_update_download,
            install_update,
            open_update_releases
        ])
        .build(tauri::generate_context!())
        .expect("无法启动 MooMole");
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, code, .. } = &event {
            if let Some(state) = app.try_state::<AppState>() {
                if !state.shutdown.closing() {
                    if let Some(window) = app.get_webview_window("main") {
                        state.window_placement.capture(&window.as_ref().window());
                    }
                }
                let request = state.shutdown.request(&state.jobs, &state.updates);
                if let Ok(_operation) = state.updates.installation.enter() {
                    state.window_placement.flush(&state.db);
                }
                match request {
                    shutdown::Request::Exit => {}
                    shutdown::Request::Waiting => api.prevent_exit(),
                    shutdown::Request::Drain => {
                        api.prevent_exit();
                        let state = state.inner().clone();
                        let handle = app.clone();
                        let code = code.unwrap_or(0);
                        tauri::async_runtime::spawn(async move {
                            while state.shutdown.poll(&state.jobs, &state.updates)
                                == shutdown::Outcome::Waiting
                            {
                                tokio::time::sleep(Duration::from_millis(50)).await;
                            }
                            handle.exit(code);
                        });
                    }
                }
            }
        }
        if matches!(event, tauri::RunEvent::Exit) {
            if let Some(state) = app.try_state::<AppState>() {
                // Cocoa may skip ExitRequested. If it follows an earlier
                // request, reuse that drain's deadline instead of adding 6s.
                state.shutdown.request(&state.jobs, &state.updates);
                while state.shutdown.poll(&state.jobs, &state.updates) == shutdown::Outcome::Waiting
                {
                    std::thread::sleep(Duration::from_millis(50));
                }
                if let Ok(_operation) = state.updates.installation.enter() {
                    state.window_placement.flush(&state.db);
                }
            }
        }
    });
}
