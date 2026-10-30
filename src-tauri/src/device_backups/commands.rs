use super::*;
use crate::AppState;
use std::sync::Mutex;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    phase: String,
    request_id: Option<String>,
    report: Option<Report>,
    error: Option<String>,
    records: Vec<Record>,
    active_requests: Vec<String>,
}
fn context(state: &AppState) -> Result<Context, String> {
    Ok(Context {
        home: state.home.clone(),
        settings: db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?,
    })
}
fn register(state: &AppState, request: &str, kind: TaskKind) -> Result<Arc<Job>, String> {
    if !id(request) {
        return Err("设备备份请求标识无效。".into());
    }
    let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
    if state.shutdown.closing()
        || jobs.len() >= 3
        || jobs.contains_key(request)
        || jobs.values().any(|j| j.kind.exclusive_operation())
    {
        return Err("请先结束当前任务，再检查或处理设备备份。".into());
    }
    let job = Arc::new(Job::new(kind));
    jobs.insert(request.into(), job.clone());
    Ok(job)
}
struct Registered {
    state: AppState,
    request: String,
}
impl Drop for Registered {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.state.jobs.lock() {
            jobs.remove(&self.request);
        }
    }
}
#[tauri::command]
pub(crate) fn get_device_backups(state: tauri::State<'_, AppState>) -> Result<View, String> {
    let (phase, request, report, error) = {
        let s = state.device_backups.lock().map_err(|e| e.to_string())?;
        (
            s.phase.to_string(),
            s.request.clone(),
            s.scan.as_ref().map(|s| s.report.clone()),
            s.error.clone(),
        )
    };
    let active = state
        .jobs
        .lock()
        .map_err(|e| e.to_string())?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let records = journal::reconcile(&*state.db.lock().map_err(|e| e.to_string())?, &active)?;
    Ok(View {
        phase,
        request_id: request.clone(),
        report,
        error,
        active_requests: request.filter(|r| active.contains(r)).into_iter().collect(),
        records,
    })
}
#[tauri::command]
pub(crate) fn scan_device_backups(
    directory: Option<String>,
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "其他文件检查正在进行。")?;
    let ctx = context(&state)?;
    let path = directory.map(PathBuf::from).unwrap_or_else(|| {
        ctx.home
            .join("Library/Application Support/MobileSync/Backup")
    });
    let job = register(&state, &request_id, TaskKind::PersonalScan)?;
    {
        let mut s = state.device_backups.lock().map_err(|e| e.to_string())?;
        *s = Store {
            phase: "scanning",
            request: Some(request_id.clone()),
            ..Default::default()
        };
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ownership = (
            operation,
            slot,
            Registered {
                state: state.clone(),
                request: request_id.clone(),
            },
        );
        let result = collect(&ctx, &path, &job);
        if let Ok(mut s) = state.device_backups.lock() {
            if s.request.as_deref() == Some(&request_id) {
                match result {
                    Ok(scan) => {
                        s.scan = Some(scan);
                        s.phase = "ready";
                    }
                    Err(e) => {
                        s.error = Some(e);
                        s.phase = "failed";
                    }
                }
            }
        }
    });
    Ok(())
}
#[tauri::command]
pub(crate) fn prepare_device_backup(
    scan_id: String,
    item_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Draft, String> {
    let _operation = state.updates.installation.enter()?;
    if state.shutdown.closing()
        || state
            .jobs
            .lock()
            .map_err(|e| e.to_string())?
            .values()
            .any(|j| j.kind.exclusive_operation())
    {
        return Err("请先结束当前任务。".into());
    }
    journal::admission(&*state.db.lock().map_err(|e| e.to_string())?)?;
    let ctx = context(&state)?;
    state
        .device_backups
        .lock()
        .map_err(|e| e.to_string())?
        .prepare(&scan_id, &item_id, &ctx)
}
#[tauri::command]
pub(crate) fn discard_device_backup(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state
        .device_backups
        .lock()
        .map_err(|e| e.to_string())?
        .offers
        .remove(&id);
    Ok(())
}
#[derive(Clone)]
pub(super) struct Worker {
    pub(super) db: Arc<Mutex<db::Database>>,
    pub(super) home: PathBuf,
    pub(super) engine_root: PathBuf,
}
impl Worker {
    fn context(&self) -> Result<Context, String> {
        Ok(Context {
            home: self.home.clone(),
            settings: db::settings(&*self.db.lock().map_err(|e| e.to_string())?)?,
        })
    }
}
pub(super) async fn run(
    state: Worker,
    plan: Plan,
    request: String,
    job: Arc<Job>,
) -> Result<(), String> {
    let mut r = Record {
        revision: REVISION.into(),
        id: plan.id.clone(),
        request_id: request,
        created_at: db::now(),
        finished_at: None,
        state: "running".into(),
        phase: "checking".into(),
        plan: plan.clone(),
        reason: "正在复查整份备份与当前保护；尚未发出系统处理请求。".into(),
        receipt: None,
        reviewed_at: None,
    };
    {
        let mut db = state.db.lock().map_err(|e| e.to_string())?;
        journal::begin(&mut db, &r)?;
    }
    let checked = async {
        // Bind the real Mole whitelist around its read-only path checks. These
        // do not turn the backup into a cache or assert that it is unused.
        let mut stamp = crate::personal_files::writer::policy_stamp(&state.home)?;
        let requested = vec![(
            plan.item.row.id.clone(),
            plan.item.row.path.clone(),
            "unsupported".into(),
        )];
        let mut checks =
            crate::rules::run(&state.engine_root, &state.home, &requested, job.clone()).await?;
        let current = crate::personal_files::writer::policy_stamp(&state.home)?;
        if current != stamp {
            stamp = current;
            checks =
                crate::rules::run(&state.engine_root, &state.home, &requested, job.clone()).await?;
        }
        if checks
            .remove(&plan.item.row.id)
            .is_none_or(|c| c.code != "passed")
        {
            return Err("Mole 路径保护要求保留，或检查未完整通过。".to_string());
        }
        let ctx = state.context()?;
        let worker = state.clone();
        let control = job.clone();
        let saved = r.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut intent = saved;
            native(&plan, &ctx, &control, || {
                if control.cancelled.load(Ordering::SeqCst) {
                    return Err("已在系统请求前取消，备份保留。".into());
                }
                let fresh = worker.context()?;
                if !fresh.settings.acknowledged
                    || fresh.settings.protected_paths != ctx.settings.protected_paths
                    || crate::personal_files::writer::policy_stamp(&worker.home)? != stamp
                {
                    return Err("保护设置在复查后变化，备份保留。".into());
                }
                intent.phase = "intent".into();
                intent.reason = "已记录整份备份的系统处理意图；尚不能据此判断结果。".into();
                journal::save(&*worker.db.lock().map_err(|e| e.to_string())?, &intent)
            })
        })
        .await
        .map_err(|e| e.to_string())?
    }
    .await;
    match checked {
        Ok((status, receipt, reason)) => {
            r.state = status;
            r.receipt = receipt;
            r.reason = reason;
        }
        Err(e) => {
            let intended = journal::list(&*state.db.lock().map_err(|e| e.to_string())?)?
                .iter()
                .any(|saved| saved.id == r.id && saved.phase == "intent");
            r.state = if intended {
                "unknown"
            } else if job.cancelled.load(Ordering::SeqCst) {
                "cancelled"
            } else {
                "retained"
            }
            .into();
            r.reason = if intended {
                "已持久记录系统处理意图，但工作结果未能完整返回；请核对原位置和废纸篓，不会自动重试。".into()
            } else {
                e
            };
        }
    }
    r.phase = "finished".into();
    r.finished_at = Some(db::now().max(r.created_at));
    journal::save(&*state.db.lock().map_err(|e| e.to_string())?, &r)
}
#[tauri::command]
pub(crate) fn execute_device_backup(
    id: String,
    request_id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if !confirmed {
        return Err("请先确认整份备份的影响与恢复边界。".into());
    }
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "其他文件检查正在进行。")?;
    let job = register(&state, &request_id, TaskKind::PersonalTrash)?;
    let registered = Registered {
        state: state.inner().clone(),
        request: request_id.clone(),
    };
    let plan = state
        .device_backups
        .lock()
        .map_err(|e| e.to_string())?
        .take(&id)?;
    {
        let mut s = state.device_backups.lock().map_err(|e| e.to_string())?;
        s.phase = "processing";
        s.request = Some(request_id.clone());
        s.error = None;
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let _ownership = (operation, slot, registered);
        let worker = Worker {
            db: state.db.clone(),
            home: state.home.clone(),
            engine_root: state.engine_root.clone(),
        };
        let result = run(worker, plan, request_id.clone(), job).await;
        if let Ok(mut s) = state.device_backups.lock() {
            if s.request.as_deref() == Some(&request_id) {
                s.phase = if result.is_ok() { "ready" } else { "failed" };
                s.error = result.err();
                s.scan = None;
            }
        }
    });
    Ok(())
}
#[tauri::command]
pub(crate) fn acknowledge_device_backup(
    id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let _operation = state.updates.installation.enter()?;
    if !confirmed {
        return Err("请先核对原位置、废纸篓及实际文件状态。".into());
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut r = journal::list(&db)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("记录不存在。")?;
    if !["unknown", "interrupted"].contains(&r.state.as_str()) {
        return Err("本记录无需人工核对确认。".into());
    }
    r.reviewed_at = Some(db::now().max(r.created_at));
    journal::save(&db, &r)
}
