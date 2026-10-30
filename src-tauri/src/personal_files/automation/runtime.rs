use super::*;
use crate::AppState;
use std::sync::atomic::AtomicBool;
struct Active {
    policy: String,
    request: String,
    job: Arc<Job>,
}
struct Offers {
    generation: u64,
    offer: Option<Offered>,
}
pub(crate) struct Controller {
    gate: Mutex<()>,
    active: Mutex<Option<Active>>,
    offers: Mutex<Offers>,
    ready: AtomicBool,
    inhibited: Mutex<HashSet<String>>,
    clock: Mutex<(i64, Instant)>,
    boot: i64,
    error: Mutex<Option<String>>,
}
impl Default for Controller {
    fn default() -> Self {
        let now = db::now();
        Self {
            gate: Mutex::new(()),
            active: Mutex::new(None),
            offers: Mutex::new(Offers {
                generation: 0,
                offer: None,
            }),
            ready: AtomicBool::new(false),
            inhibited: Mutex::new(HashSet::new()),
            clock: Mutex::new((now, Instant::now())),
            boot: now,
            error: Mutex::new(None),
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    policies: Vec<Policy>,
    records: Vec<Record>,
    active_requests: Vec<String>,
    error: Option<String>,
}
fn ready(state: &AppState) -> Result<(), String> {
    if !state.scheduled_files.ready.load(Ordering::SeqCst) || state.shutdown.closing() {
        return Err("定期处理正在恢复状态或应用正在退出。".into());
    }
    Ok(())
}
fn active_requests(state: &AppState) -> Result<Vec<String>, String> {
    let a = state
        .scheduled_files
        .active
        .lock()
        .map_err(|e| e.to_string())?;
    let jobs = state.jobs.lock().map_err(|e| e.to_string())?;
    Ok(a.as_ref()
        .filter(|a| jobs.contains_key(&a.request))
        .map(|a| vec![a.request.clone()])
        .unwrap_or_default())
}
fn clock_check(state: &AppState) -> Result<bool, String> {
    let mut clock = state
        .scheduled_files
        .clock
        .lock()
        .map_err(|e| e.to_string())?;
    let drift = (db::now() as i128 - clock.0 as i128 - clock.1.elapsed().as_millis() as i128).abs();
    if drift > 300_000 {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        for p in journal::policies(&db)?
            .into_iter()
            .filter(|p| p.policy.enabled)
        {
            state
                .scheduled_files
                .inhibited
                .lock()
                .map_err(|e| e.to_string())?
                .insert(p.policy.id.clone());
            journal::pause(&db, &p.policy.id)?;
        }
        if let Some(a) = state
            .scheduled_files
            .active
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
        {
            a.job.cancel();
        }
        *clock = (db::now(), Instant::now());
        *state
            .scheduled_files
            .error
            .lock()
            .map_err(|e| e.to_string())? =
            Some("时间或运行基准明显变化，定期处理已暂停；请重新预览启用。".into());
        return Ok(false);
    }
    Ok(true)
}
struct Registered {
    state: AppState,
    request: String,
}
impl Drop for Registered {
    fn drop(&mut self) {
        if let Ok(mut a) = self.state.scheduled_files.active.lock() {
            if a.as_ref().is_some_and(|a| a.request == self.request) {
                *a = None;
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
    trigger: &str,
    expected: Option<&str>,
) -> Result<(), String> {
    ready(state)?;
    if !valid_id(&request) {
        return Err("定期处理请求标识无效。".into());
    }
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "其他文件检查正在进行，定期处理将延后。")?;
    let _gate = state
        .scheduled_files
        .gate
        .lock()
        .map_err(|e| e.to_string())?;
    if !clock_check(state)? {
        return Err("时间或运行基准明显变化，定期处理已暂停；请重新预览启用。".into());
    }
    if state
        .scheduled_files
        .inhibited
        .lock()
        .map_err(|e| e.to_string())?
        .contains(id)
    {
        return Err("此规则在本次运行中已暂停，请重新预览启用。".into());
    }
    let saved = {
        let active = active_requests(state)?;
        let mut db = state.db.lock().map_err(|e| e.to_string())?;
        journal::reconcile(&mut db, &active)?;
        let p = journal::policies(&db)?
            .into_iter()
            .find(|p| p.policy.id == id && p.policy.enabled)
            .ok_or("定期规则已暂停或移除。")?;
        if expected.is_some_and(|g| g != p.generation)
            || (trigger == "scheduled" && !due(&p, db::now(), state.scheduled_files.boot))
        {
            return Err("规则已变化或尚未到期。".into());
        }
        p
    };
    let job = Arc::new(Job::new(TaskKind::PersonalTrash));
    {
        let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
        if state.shutdown.closing()
            || jobs.len() >= 3
            || jobs.contains_key(&request)
            || jobs.values().any(|j| j.kind.exclusive_operation())
        {
            return Err("其他任务正在运行，定期处理将延后。".into());
        }
        jobs.insert(request.clone(), job.clone());
    }
    let owner = Registered {
        state: state.clone(),
        request: request.clone(),
    };
    journal::begin(
        &mut *state.db.lock().map_err(|e| e.to_string())?,
        &saved,
        &request,
        trigger,
    )?;
    *state
        .scheduled_files
        .active
        .lock()
        .map_err(|e| e.to_string())? = Some(Active {
        policy: id.into(),
        request: request.clone(),
        job: job.clone(),
    });
    tauri::async_runtime::spawn(async move {
        let _ownership = (owner, operation, slot);
        let state = &_ownership.0.state;
        let result = process(
            state.db.clone(),
            state.home.clone(),
            state.engine_root.clone(),
            saved.clone(),
            request.clone(),
            job,
        )
        .await;
        if result.is_err() {
            if let Ok(mut i) = state.scheduled_files.inhibited.lock() {
                i.insert(saved.policy.id.clone());
            }
        }
        let saved = state
            .db
            .lock()
            .map_err(|e| e.to_string())
            .and_then(|mut db| journal::finish(&mut db, &request, result));
        if let Ok(mut error) = state.scheduled_files.error.lock() {
            *error = saved.err();
        };
    });
    Ok(())
}
pub(crate) fn start(state: AppState) {
    tauri::async_runtime::spawn(async move {
        // A standing rule is explicitly enabled per App session. Startup always
        // pauses old rules, including when a prior stop could not be persisted.
        while !state.shutdown.closing() {
            let initialized = (|| {
                let _gate = state
                    .scheduled_files
                    .gate
                    .lock()
                    .map_err(|e| e.to_string())?;
                let mut db = state.db.lock().map_err(|e| e.to_string())?;
                journal::initialize(&mut db)?;
                Ok::<_, String>(())
            })();
            if initialized.is_ok() {
                state.scheduled_files.ready.store(true, Ordering::SeqCst);
                break;
            }
            if let Ok(mut e) = state.scheduled_files.error.lock() {
                *e = initialized.err();
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
        while !state.shutdown.closing() {
            let due_policies = (|| {
                let _gate = state
                    .scheduled_files
                    .gate
                    .lock()
                    .map_err(|e| e.to_string())?;
                if !clock_check(&state)? {
                    return Err("时间或运行基准明显变化，定期处理已暂停；请重新预览启用。".into());
                }
                let active = active_requests(&state)?;
                let mut db = state.db.lock().map_err(|e| e.to_string())?;
                journal::reconcile(&mut db, &active)?;
                Ok::<_, String>(
                    journal::policies(&db)?
                        .into_iter()
                        .filter(|p| due(p, db::now(), state.scheduled_files.boot))
                        .collect::<Vec<_>>(),
                )
            })();
            match due_policies {
                Ok(policies) => {
                    for p in policies {
                        if launch(
                            &state,
                            &p.policy.id,
                            uuid::Uuid::new_v4().to_string(),
                            "scheduled",
                            Some(&p.generation),
                        )
                        .is_ok()
                        {
                            break;
                        }
                    }
                }
                Err(e) => {
                    if let Ok(mut error) = state.scheduled_files.error.lock() {
                        *error = Some(e);
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}
#[tauri::command]
pub(crate) fn get_scheduled_files(state: tauri::State<'_, AppState>) -> Result<View, String> {
    ready(&state)?;
    let _gate = state
        .scheduled_files
        .gate
        .lock()
        .map_err(|e| e.to_string())?;
    let active = active_requests(&state)?;
    let mut db = state.db.lock().map_err(|e| e.to_string())?;
    journal::reconcile(&mut db, &active)?;
    let inhibited = state
        .scheduled_files
        .inhibited
        .lock()
        .map_err(|e| e.to_string())?;
    Ok(View {
        policies: journal::policies(&db)?
            .into_iter()
            .map(|p| {
                let mut policy = p.policy;
                if inhibited.contains(&policy.id) {
                    policy.enabled = false;
                }
                policy
            })
            .collect(),
        records: journal::records(&db)?,
        active_requests: active,
        error: state
            .scheduled_files
            .error
            .lock()
            .map_err(|e| e.to_string())?
            .clone(),
    })
}
#[tauri::command]
pub(crate) async fn prepare_scheduled_files(
    edit: Edit,
    state: tauri::State<'_, AppState>,
) -> Result<Draft, String> {
    ready(&state)?;
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "文件检查仍在进行。")?;
    let ticket = {
        let mut s = state
            .scheduled_files
            .offers
            .lock()
            .map_err(|e| e.to_string())?;
        s.generation = s.generation.checked_add(1).ok_or("请重新打开应用。")?;
        s.offer = None;
        s.generation
    };
    let home = state.home.clone();
    let settings = db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?;
    let offered = tauri::async_runtime::spawn_blocking(move || {
        let (_operation, _slot) = (operation, slot);
        preview(edit, &home, &settings)
    })
    .await
    .map_err(|e| e.to_string())??;
    let draft = offered.draft.clone();
    let mut s = state
        .scheduled_files
        .offers
        .lock()
        .map_err(|e| e.to_string())?;
    if s.generation != ticket {
        return Err("定期规则检查已被取代。".into());
    }
    s.offer = Some(offered);
    Ok(draft)
}
#[tauri::command]
pub(crate) fn discard_scheduled_files(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut s = state
        .scheduled_files
        .offers
        .lock()
        .map_err(|e| e.to_string())?;
    if s.offer.as_ref().is_some_and(|o| o.draft.id == id) {
        s.offer = None;
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn enable_scheduled_files(
    id: String,
    confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Policy, String> {
    ready(&state)?;
    if !confirmed {
        return Err("请先核对规则，并确认允许后续符合规则的文件自动处理。".into());
    }
    let operation = state.updates.installation.enter()?;
    let slot = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "请等待当前文件任务结束。")?;
    let offered = {
        let mut s = state
            .scheduled_files
            .offers
            .lock()
            .map_err(|e| e.to_string())?;
        if s.offer.as_ref().is_none_or(|o| o.draft.id != id) {
            return Err("定期规则确认已使用、撤下或被取代。".into());
        }
        s.offer.take().ok_or("定期规则确认不存在。")?
    };
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (_operation, _slot) = (operation, slot);
        let _gate = state
            .scheduled_files
            .gate
            .lock()
            .map_err(|e| e.to_string())?;
        ready(&state)?;
        clock_check(&state)?;
        let p = journal::save(
            &mut *state.db.lock().map_err(|e| e.to_string())?,
            offered,
            &state.home,
        )?;
        state
            .scheduled_files
            .inhibited
            .lock()
            .map_err(|e| e.to_string())?
            .remove(&p.id);
        *state
            .scheduled_files
            .error
            .lock()
            .map_err(|e| e.to_string())? = None;
        Ok(p)
    })
    .await
    .map_err(|e| e.to_string())?
}
fn stop(state: &AppState, id: &str, remove: bool) -> Result<(), String> {
    ready(state)?;
    let _operation = state.updates.installation.enter()?;
    let _gate = state
        .scheduled_files
        .gate
        .lock()
        .map_err(|e| e.to_string())?;
    state
        .scheduled_files
        .inhibited
        .lock()
        .map_err(|e| e.to_string())?
        .insert(id.into());
    if let Some(a) = state
        .scheduled_files
        .active
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .filter(|a| a.policy == id)
    {
        a.job.cancel();
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    if remove {
        journal::remove(&db, id)
    } else {
        journal::pause(&db, id)
    }
}
#[tauri::command]
pub(crate) fn pause_scheduled_files(
    policy_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    stop(&state, &policy_id, false)
}
#[tauri::command]
pub(crate) fn remove_scheduled_files(
    policy_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    stop(&state, &policy_id, true)
}
#[tauri::command]
pub(crate) fn run_scheduled_files(
    policy_id: String,
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    launch(&state, &policy_id, request_id, "manual", None)
}
