use super::*;
use crate::AppState;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
fn rules(state: &AppState) -> Result<Vec<String>, String> {
    let s = db::settings(&*state.db.lock().map_err(err)?)?;
    if !s.acknowledged {
        return Err("请先确认首次使用说明。".into());
    }
    Ok(s.protected_paths)
}
struct Stop(Arc<Job>);
impl Drop for Stop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
async fn work<T: Send + 'static>(
    app: tauri::AppHandle,
    state: &AppState,
    request: String,
    run: impl FnOnce(PathBuf, AppState, Arc<Job>) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let operation = state.updates.installation.enter()?;
    let slot = state
        .archive_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "另一项归档读写尚未完成，请稍后重试。")?;
    let (job, registered) = crate::application_update::commands::register(state, &request)?;
    let root = app
        .path()
        .app_data_dir()
        .map_err(err)?
        .join("execution-archives");
    let state = state.clone();
    let stop = Stop(job.clone());
    let mut task = tokio::task::spawn_blocking(move || {
        let _owned = (operation, slot, registered);
        run(root, state, job)
    });
    let result = tokio::select! {r=&mut task=>r.map_err(|_|"归档副本整理未取得完整结果，请刷新记录核查。")?,_=tokio::time::sleep(Duration::from_secs(30))=>return Err("归档副本检查等待超时，系统读取或文件窗口可能仍未结束；不会自动重试。".into())};
    drop(stop);
    result
}
#[tauri::command]
pub(crate) async fn prepare_archive_retirement(
    archive_id: String,
    request_id: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<Draft>, String> {
    valid_id(&archive_id)?;
    let dialog = app.clone();
    let language = crate::language::saved(&state);
    work(app, &state, request_id, move |root, state, job| {
        rules(&state)?;
        journal::admission(&*state.db.lock().map_err(err)?)?;
        state.archive_retirement.lock().map_err(err)?.plan = None;
        let header = view(&root, &archive_id)?.header;
        check(&job)?;
        let Some(file) = dialog
            .dialog()
            .file()
            .set_title(crate::language::text(
                &language,
                "先保存完整归档副本",
                "Save a complete archive copy first",
                "最初に完全なアーカイブコピーを保存",
            ))
            .set_file_name(format!("{archive_id}-exported.moomole-history"))
            .add_filter("MooMole", &["moomole-history"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        check(&job)?;
        let path = file.into_path().map_err(err)?;
        let plan = Plan::prepare(&root, &archive_id, &path, &rules(&state)?, &job)?;
        if plan.draft.header != header {
            return Err("归档在选择导出位置期间已变化，原副本保留。".into());
        }
        plan.validate(&root, &rules(&state)?, &job)?;
        let draft = plan.draft.clone();
        check(&job)?;
        state.archive_retirement.lock().map_err(err)?.plan = Some(plan);
        Ok(Some(draft))
    })
    .await
}
#[tauri::command]
pub(crate) fn discard_archive_retirement(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut store = state.archive_retirement.lock().map_err(err)?;
    if store.plan.as_ref().is_some_and(|p| p.draft.id == id) {
        store.plan = None
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn execute_archive_retirement(
    id: String,
    request_id: String,
    confirmed: bool,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Record, String> {
    if !confirmed {
        return Err("请单独确认仅处理本机归档副本，并保留完整导出副本。".into());
    }
    work(app, &state, request_id.clone(), move |root, state, job| {
        let plan = {
            let mut s = state.archive_retirement.lock().map_err(err)?;
            if s.plan.as_ref().is_none_or(|p| p.draft.id != id) {
                return Err("归档副本确认已失效，请重新核对。".into());
            }
            s.plan.take().unwrap()
        };
        execute(plan, &root, state.db.clone(), job, request_id, || {
            rules(&state)
        })
    })
    .await
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    records: Vec<Record>,
    active_requests: Vec<String>,
}
#[tauri::command]
pub(crate) fn get_archive_retirements(state: State<'_, AppState>) -> Result<Snapshot, String> {
    let active = state
        .jobs
        .lock()
        .map_err(err)?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let records = journal::reconcile(&*state.db.lock().map_err(err)?, &active)?;
    let active_requests = records
        .iter()
        .filter(|r| r.state == "running" && active.contains(&r.request_id))
        .map(|r| r.request_id.clone())
        .collect();
    Ok(Snapshot {
        records,
        active_requests,
    })
}
#[tauri::command]
pub(crate) fn acknowledge_archive_retirement(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let database = state.db.lock().map_err(err)?;
    let mut row = journal::list(&database)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("归档副本整理记录不存在。")?;
    if row.state != "unknown" {
        return Err("仅未知记录需要人工核查标记。".into());
    }
    row.reviewed_at = Some(db::now().max(row.created_at));
    journal::save(&database, &row)
}
