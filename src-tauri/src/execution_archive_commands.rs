use super::{execution_archive as archive, AppState};
use std::path::PathBuf;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

async fn work<T: Send + 'static>(
    app: tauri::AppHandle,
    state: &tauri::State<'_, AppState>,
    run: impl FnOnce(PathBuf) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出，请重新打开后操作归档。".into());
    }
    let _operation = state.updates.installation.enter()?;
    let _slot = state
        .archive_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "另一项归档读写尚未完成，请稍后重试。")?;
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("execution-archives");
    tauri::async_runtime::spawn_blocking(move || {
        let _ownership = (_operation, _slot);
        run(directory)
    })
    .await
    .map_err(|_| "归档操作未取得完整结果；请刷新活动记录和归档核对。".to_string())?
}
fn idle(state: &AppState) -> Result<(), String> {
    if state.shutdown.closing()
        || state
            .jobs
            .lock()
            .map_err(|_| "无法核对修改任务。")?
            .values()
            .any(|j| {
                matches!(
                    j.kind,
                    super::engine::TaskKind::LogTrash
                        | super::engine::TaskKind::AppTrash
                        | super::engine::TaskKind::MaintenanceRefresh
                )
            })
    {
        return Err("请等待修改任务结束后再归档执行记录。".into());
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn prepare_execution_archive(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<archive::Preview, String> {
    idle(&state)?;
    let db = state.db.clone();
    work(app, &state, move |_| {
        archive::prepare(&mut *db.lock().map_err(|_| "无法读取执行记录。")?)
    })
    .await
}
#[tauri::command]
pub(crate) async fn archive_execution_history(
    fingerprint: String,
    confirmed: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<archive::Header, String> {
    if !confirmed {
        return Err("请先确认归档范围及恢复证据的保留方式。".into());
    }
    idle(&state)?;
    let db = state.db.clone();
    let shutdown = state.shutdown.clone();
    let jobs = state.jobs.clone();
    work(app, &state, move |root| {
        if shutdown.closing()
            || jobs
                .lock()
                .map_err(|_| "无法核对修改任务。")?
                .values()
                .any(|j| {
                    matches!(
                        j.kind,
                        super::engine::TaskKind::LogTrash
                            | super::engine::TaskKind::AppTrash
                            | super::engine::TaskKind::MaintenanceRefresh
                    )
                })
        {
            return Err("任务状态已变化，请稍后重新检查归档。".into());
        }
        archive::archive(
            &mut *db.lock().map_err(|_| "无法归档执行记录。")?,
            &root,
            &fingerprint,
            super::db::now(),
        )
    })
    .await
}
#[tauri::command]
pub(crate) async fn list_execution_archives(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<archive::Listing, String> {
    work(app, &state, move |root| archive::list(&root)).await
}
#[tauri::command]
pub(crate) async fn read_execution_archive(
    archive_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<archive::View, String> {
    work(app, &state, move |root| archive::view(&root, &archive_id)).await
}
#[tauri::command]
pub(crate) async fn get_archived_execution(
    archive_id: String,
    kind: archive::Kind,
    run_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    work(app, &state, move |root| {
        archive::detail(&root, &archive_id, kind, &run_id)
    })
    .await
}
#[tauri::command]
pub(crate) async fn import_execution_archive(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Option<archive::Header>, String> {
    let dialog = app.clone();
    work(app, &state, move |root| {
        let Some(path) = dialog
            .dialog()
            .file()
            .set_title("打开执行归档（只读副本）")
            .add_filter("MooMole 执行归档", &["moomole-history"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|_| "所选归档路径无效。")?;
        archive::import(&root, &path).map(Some)
    })
    .await
}
#[tauri::command]
pub(crate) async fn export_execution_archive(
    archive_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let dialog = app.clone();
    work(app, &state, move |root| {
        archive::view(&root, &archive_id)?;
        let Some(path) = dialog
            .dialog()
            .file()
            .set_title("导出执行归档（包含路径与收据）")
            .set_file_name(format!("{archive_id}.moomole-history"))
            .add_filter("MooMole 执行归档", &["moomole-history"])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        archive::export(
            &root,
            &archive_id,
            &path.into_path().map_err(|_| "导出路径无效。")?,
        )?;
        Ok(true)
    })
    .await
}
#[tauri::command]
pub(crate) async fn reveal_execution_archives(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let root = work(app, &state, move |root| archive::folder(&root)).await?;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new("/usr/bin/open")
            .arg("--")
            .arg(root)
            .kill_on_drop(true)
            .status(),
    )
    .await
    .map_err(|_| "Finder 打开超时。")?
    .map_err(|e| e.to_string())?;
    if result.success() {
        Ok(())
    } else {
        Err("无法在 Finder 中打开归档目录。".into())
    }
}
