use super::*;
use crate::AppState;
fn context(state: &AppState, current: &str) -> Result<Context, String> {
    if !db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?.acknowledged {
        return Err("请先确认首次使用说明。".into());
    }
    crate::app_control::context(state, current)
}
struct Stop(Arc<Job>);
impl Drop for Stop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
async fn work<T: Send + 'static>(
    state: &AppState,
    current: String,
    request: String,
    action: impl FnOnce(AppState, Context, Arc<Job>) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "其他文件或应用检查仍在进行，请稍后重试。")?;
    let (job, registered) = super::super::commands::register(state, &request)?;
    let ctx = context(state, &current)?;
    let state = state.clone();
    let stop = Stop(job.clone());
    let mut task = tokio::task::spawn_blocking(move || {
        let _owned = (operation, slot, registered);
        objc2::rc::autoreleasepool(|_| action(state, ctx, job))
    });
    let result = tokio::select! {r=&mut task=>r.map_err(|_|"保留版本整理未取得完整结果，请刷新并核查。")?,_=tokio::time::sleep(Duration::from_secs(130))=>return Err("保留版本检查等待超时，系统读取仍可能未结束；不会自动重试。".into())};
    drop(stop);
    result
}
#[tauri::command]
pub(crate) async fn prepare_retained_application(
    source_id: String,
    request_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Draft, String> {
    let current = app.config().identifier.clone();
    work(
        &state,
        current.clone(),
        request_id,
        move |state, ctx, job| {
            let a = {
                let c = state.db.lock().map_err(|e| e.to_string())?;
                super::super::journal::admission(&c)?;
                block_source(&c, &source_id)?;
                source(&c, &source_id)?
            };
            {
                let mut store = state
                    .application_updates
                    .lock()
                    .map_err(|e| e.to_string())?;
                store.plan = None;
                store.restoration = None;
                store.retirement = None;
            }
            let plan = Plan::prepare(a, ctx, &job)?;
            let current_context = context(&state, &current)?;
            plan.check(
                &*state.db.lock().map_err(|e| e.to_string())?,
                &current_context,
                &job,
            )?;
            let draft = plan.draft.clone();
            cancelled(&job)?;
            state
                .application_updates
                .lock()
                .map_err(|e| e.to_string())?
                .retirement = Some(plan);
            Ok(draft)
        },
    )
    .await
}
#[tauri::command]
pub(crate) fn discard_retained_application(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut store = state
        .application_updates
        .lock()
        .map_err(|e| e.to_string())?;
    if store.retirement.as_ref().is_some_and(|p| p.draft.id == id) {
        store.retirement = None
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn execute_retained_application(
    id: String,
    request_id: String,
    confirmed: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Record, String> {
    if !confirmed {
        return Err("请单独确认只处理所列保留版本，并了解此恢复副本将不再可用。".into());
    }
    let current = app.config().identifier.clone();
    work(
        &state,
        current.clone(),
        request_id.clone(),
        move |state, _, job| {
            let plan = {
                let mut s = state
                    .application_updates
                    .lock()
                    .map_err(|e| e.to_string())?;
                if s.retirement.as_ref().is_none_or(|p| p.draft.id != id) {
                    return Err("保留版本确认已失效，请重新检查。".into());
                }
                s.retirement.take().unwrap()
            };
            run(
                plan,
                request_id,
                &job,
                &state.db,
                || context(&state, &current),
                |_| {},
            )
        },
    )
    .await
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    records: Vec<Record>,
    active_requests: Vec<String>,
}
#[tauri::command]
pub(crate) fn get_retained_application_history(
    state: tauri::State<'_, AppState>,
) -> Result<View, String> {
    let active = state
        .jobs
        .lock()
        .map_err(|e| e.to_string())?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let records = journal::reconcile(&*state.db.lock().map_err(|e| e.to_string())?, &active)?;
    let active_requests = records
        .iter()
        .filter(|r| r.state == "running" && active.contains(&r.request_id))
        .map(|r| r.request_id.clone())
        .collect();
    Ok(View {
        records,
        active_requests,
    })
}
#[tauri::command]
pub(crate) fn acknowledge_retained_application(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let c = state.db.lock().map_err(|e| e.to_string())?;
    let mut row = journal::list(&c)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("保留版本整理记录不存在。")?;
    if row.state != "unknown" {
        return Err("仅未知整理结果需要人工核查。".into());
    }
    row.reviewed_at = Some(db::now().max(row.created_at));
    journal::save(&c, &row)
}
