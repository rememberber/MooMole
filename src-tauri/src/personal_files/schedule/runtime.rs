use super::*;
use crate::AppState;
#[derive(Default)]
pub(crate) struct Controller {
    gate: Mutex<()>,
    active: Mutex<Option<Active>>,
    error: Mutex<Option<String>>,
}
struct Active {
    watch_id: String,
    request_id: String,
    job: Arc<Job>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    watches: Vec<Watch>,
    records: Vec<Record>,
    active_requests: Vec<String>,
    error: Option<String>,
}
fn requests(state: &AppState) -> Result<Vec<String>, String> {
    Ok(state
        .jobs
        .lock()
        .map_err(|e| e.to_string())?
        .iter()
        .filter(|(_, j)| j.kind == TaskKind::FolderScan)
        .map(|(id, _)| id.clone())
        .collect())
}
fn read(state: &AppState) -> Result<View, String> {
    let before = journal::records(&*state.db.lock().map_err(|e| e.to_string())?)?;
    let active = requests(state)?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    for r in before
        .iter()
        .filter(|r| r.state == State::Running && !active.contains(&r.id))
    {
        journal::interrupt(&db, &r.id)?;
    }
    Ok(View {
        watches: journal::watches(&db)?
            .into_iter()
            .map(|w| w.config)
            .collect(),
        records: journal::records(&db)?,
        active_requests: active,
        error: state
            .folder_schedules
            .error
            .lock()
            .map_err(|e| e.to_string())?
            .clone(),
    })
}
struct Registered {
    state: AppState,
    request: String,
}
impl Drop for Registered {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.folder_schedules.active.lock() {
            if active
                .as_ref()
                .is_some_and(|a| a.request_id == self.request)
            {
                *active = None;
            }
        }
        if let Ok(mut jobs) = self.state.jobs.lock() {
            jobs.remove(&self.request);
        }
    }
}
fn launch(
    state: &AppState,
    id: &str,
    request: String,
    trigger: Trigger,
    expected: Option<&str>,
) -> Result<(), String> {
    if !valid_id(&request) {
        return Err("目录扫描请求标识无效。".into());
    }
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "其他文件检查正在进行，定期扫描将延后。")?;
    let _gate = state
        .folder_schedules
        .gate
        .lock()
        .map_err(|e| e.to_string())?;
    let (watch, rules) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let settings = db::settings(&db)?;
        if !settings.acknowledged {
            return Err("请先确认首次使用说明。".into());
        }
        let watch = journal::watches(&db)?
            .into_iter()
            .find(|w| w.config.id == id)
            .ok_or("目录扫描任务已移除。")?;
        if expected.is_some_and(|g| g != watch.generation)
            || (trigger == Trigger::Scheduled && !due(&watch, db::now()))
        {
            return Err("定期扫描已暂停、设置变化或尚未到期。".into());
        }
        (watch, settings.protected_paths)
    };
    let job = Arc::new(Job::new(TaskKind::FolderScan));
    {
        let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
        if state.shutdown.closing()
            || jobs.len() >= 3
            || jobs.contains_key(&request)
            || jobs.values().any(|j| j.kind.exclusive_operation())
        {
            return Err("其他任务正在运行或应用正在退出，目录扫描将延后。".into());
        }
        jobs.insert(request.clone(), job.clone());
    }
    let owner = Registered {
        state: state.clone(),
        request: request.clone(),
    };
    let active = requests(state)?;
    let record = {
        let mut db = state.db.lock().map_err(|e| e.to_string())?;
        journal::reconcile(&db, &active)?;
        journal::begin(&mut db, &watch, trigger, &request, db::now())?
    };
    *state
        .folder_schedules
        .active
        .lock()
        .map_err(|e| e.to_string())? = Some(Active {
        watch_id: id.into(),
        request_id: request,
        job: job.clone(),
    });
    tauri::async_runtime::spawn_blocking(move || {
        let _ownership = (owner, operation, slot);
        let state = &_ownership.0.state;
        let observed = observe(&watch, &rules, job);
        let result = (|| {
            let mut db = state.db.lock().map_err(|e| e.to_string())?;
            journal::finish(&mut db, &watch, record, &rules, observed)
        })();
        if let Ok(mut error) = state.folder_schedules.error.lock() {
            *error = result.err();
        };
    });
    Ok(())
}
pub(crate) fn start(state: AppState) {
    tauri::async_runtime::spawn(async move {
        loop {
            if state.shutdown.closing() {
                break;
            }
            let due_watches = (|| {
                let db = state.db.lock().map_err(|e| e.to_string())?;
                Ok::<_, String>(
                    journal::watches(&db)?
                        .into_iter()
                        .filter(|w| due(w, db::now()))
                        .collect::<Vec<_>>(),
                )
            })();
            match due_watches {
                Ok(watches) => {
                    // Admission defers behind interactive work; no attempt is
                    // recorded until a worker and the exclusive registry exist.
                    for watch in watches {
                        if launch(
                            &state,
                            &watch.config.id,
                            uuid::Uuid::new_v4().to_string(),
                            Trigger::Scheduled,
                            Some(&watch.generation),
                        )
                        .is_ok()
                        {
                            break;
                        }
                    }
                }
                Err(e) => {
                    if let Ok(mut error) = state.folder_schedules.error.lock() {
                        *error = Some(e);
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}
#[tauri::command]
pub(crate) fn get_folder_schedules(state: tauri::State<'_, AppState>) -> Result<View, String> {
    read(&state)
}
#[tauri::command]
pub(crate) async fn configure_folder_schedule(
    edit: Edit,
    state: tauri::State<'_, AppState>,
) -> Result<Watch, String> {
    let operation = state.updates.installation.enter()?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let _gate = state
            .folder_schedules
            .gate
            .lock()
            .map_err(|e| e.to_string())?;
        if state.shutdown.closing() {
            return Err("应用正在退出。".into());
        }
        let watch = journal::configure(&mut *state.db.lock().map_err(|e| e.to_string())?, edit)?;
        if let Some(active) = state
            .folder_schedules
            .active
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .filter(|a| a.watch_id == watch.id)
        {
            active.job.cancel();
        }
        *state
            .folder_schedules
            .error
            .lock()
            .map_err(|e| e.to_string())? = None;
        Ok(watch)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn remove_folder_schedule(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let _operation = state.updates.installation.enter()?;
    let _gate = state
        .folder_schedules
        .gate
        .lock()
        .map_err(|e| e.to_string())?;
    if state.shutdown.closing() {
        return Err("应用正在退出。".into());
    }
    journal::remove(&*state.db.lock().map_err(|e| e.to_string())?, &id)?;
    if let Some(active) = state
        .folder_schedules
        .active
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .filter(|a| a.watch_id == id)
    {
        active.job.cancel();
    }
    Ok(())
}
#[tauri::command]
pub(crate) fn start_folder_scan(
    watch_id: String,
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    launch(&state, &watch_id, request_id, Trigger::Manual, None)
}
