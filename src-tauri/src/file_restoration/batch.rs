//! A single durable restoration run for an explicit bounded selection from one
//! local source run. Every file keeps its own receipt and original directory.
use super::*;
use std::collections::HashSet;
pub(crate) const MODULE: &str = "fileRestorationBatch";
const REVISION: &str = "MooMole.file-restoration.batch.v1";
const MAX: usize = 20;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Plan {
    pub module: String,
    pub protocol: u64,
    pub revision: String,
    pub id: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub plans: Vec<super::Plan>,
    #[serde(skip)]
    issued: Option<Instant>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Draft {
    id: String,
    source_run_id: String,
    items: Vec<DraftItem>,
    created_at: i64,
    expires_at: i64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DraftItem {
    item_id: String,
    source_item_id: String,
    current_path: String,
    original_path: String,
}
impl Plan {
    pub(crate) fn validate_record(&self) -> Result<(), String> {
        if self.module != MODULE
            || self.protocol != 1
            || self.revision != REVISION
            || uuid::Uuid::parse_str(&self.id).is_err()
            || self.created_at < 0
            || self.expires_at != self.created_at + 30_000
            || self.plans.is_empty()
            || self.plans.len() > MAX
        {
            return Err("批量恢复计划或选择数量无法核对。".into());
        }
        let mut ids = HashSet::new();
        let mut sources = HashSet::new();
        let mut current = HashSet::new();
        let mut original = HashSet::new();
        for p in &self.plans {
            p.validate_record()?;
            if p.source.run_id != self.plans[0].source.run_id
                || p.created_at < self.created_at
                || p.expires_at < self.expires_at
                || !ids.insert(&p.item.id)
                || !sources.insert(&p.source.item_id)
                || !current.insert(&p.item.path)
                || !original.insert(&p.source.original_path)
            {
                return Err("批量恢复包含重复项目或不一致的原记录。".into());
            }
        }
        if current.iter().any(|p| original.contains(p)) {
            return Err("当前文件与其他恢复目标重叠，不能合并恢复。".into());
        }
        Ok(())
    }
    fn fresh(&self) -> Result<(), String> {
        self.validate_record()?;
        if db::now() < self.created_at
            || db::now() >= self.expires_at
            || self
                .issued
                .is_none_or(|i| i.elapsed() >= Duration::from_secs(30))
        {
            return Err("批量恢复确认已过期，请重新选择。".into());
        }
        Ok(())
    }
    fn draft(&self) -> Draft {
        Draft {
            id: self.id.clone(),
            source_run_id: self.plans[0].source.run_id.clone(),
            created_at: self.created_at,
            expires_at: self.expires_at,
            items: self
                .plans
                .iter()
                .map(|p| DraftItem {
                    item_id: p.item.id.clone(),
                    source_item_id: p.source.item_id.clone(),
                    current_path: p.item.path.clone(),
                    original_path: p.source.original_path.clone(),
                })
                .collect(),
        }
    }
}
fn prepare(
    detail: &execution::Detail,
    ids: &[String],
    home: &Path,
    settings: &db::Settings,
) -> Result<Plan, String> {
    if ids.is_empty() || ids.len() > MAX || ids.iter().collect::<HashSet<_>>().len() != ids.len() {
        return Err("请选择 1 至 20 个不同的可靠完成项目。".into());
    }
    let created_at = db::now();
    let issued = Instant::now();
    let plans = ids
        .iter()
        .map(|id| super::prepare(detail, id, home, settings))
        .collect::<Result<Vec<_>, _>>()?;
    let plan = Plan {
        module: MODULE.into(),
        protocol: 1,
        revision: REVISION.into(),
        id: uuid::Uuid::new_v4().to_string(),
        created_at,
        expires_at: created_at + 30_000,
        plans,
        issued: Some(issued),
    };
    plan.fresh()?;
    Ok(plan)
}
fn run(
    database: Arc<std::sync::Mutex<db::Database>>,
    home: PathBuf,
    plan: Plan,
    job: Arc<Job>,
    after_item: impl Fn(usize),
) -> Result<execution::Detail, String> {
    plan.fresh()?;
    let run = {
        let mut db = database.lock().map_err(|e| e.to_string())?;
        let settings = db::settings(&db)?;
        if !settings.acknowledged
            || unsafe { libc::geteuid() } == 0
            || unsafe { libc::geteuid() } != unsafe { libc::getuid() }
        {
            return Err("批量恢复仅支持已确认使用说明的当前普通账户。".into());
        }
        execution::begin_batch_restoration(&mut db, &plan, db::now())?
    };
    let result = (|| -> Result<(), String> {
        job.bind_execution(&run)?;
        for (index, p) in plan.plans.iter().enumerate() {
            if job.cancelled.load(Ordering::SeqCst) {
                break;
            }
            let prepared = (|| {
                let db = database.lock().map_err(|e| e.to_string())?;
                let settings = db::settings(&db)?;
                p.validate_source(&db)?;
                p.validate(&home, &settings)?;
                movement::Prepared::new(&p.item, &p.movement, &p.source.original_path, &settings)
            })();
            match prepared {
                Err(e) => execution::skip(
                    &mut *database.lock().map_err(|e| e.to_string())?,
                    &run,
                    &p.item.id,
                    &bounded(&e),
                    db::now(),
                )?,
                Ok(target) => {
                    execution::intent(
                        &mut *database.lock().map_err(|e| e.to_string())?,
                        &run,
                        &p.item.id,
                        db::now(),
                    )?;
                    let moved = target.move_checked(|| {
                        if job.cancelled.load(Ordering::SeqCst) {
                            return Err("系统调用前已取消恢复。".into());
                        }
                        let db = database.lock().map_err(|e| e.to_string())?;
                        p.validate_source(&db)?;
                        p.validate(&home, &db::settings(&db)?)
                    });
                    let (outcome, reason) =
                        moved.unwrap_or_else(|e| (execution::Outcome::Failed, e));
                    execution::complete_item(
                        &mut *database.lock().map_err(|e| e.to_string())?,
                        &run,
                        &p.item.id,
                        outcome,
                        &bounded(&reason),
                        db::now(),
                    )?;
                }
            }
            after_item(index);
        }
        execution::finish(
            &mut *database.lock().map_err(|e| e.to_string())?,
            &run,
            job.cancelled.load(Ordering::SeqCst),
            db::now(),
        )
    })();
    if let Err(e) = result {
        if let Ok(mut db) = database.lock() {
            let _ = execution::stop(&mut db, &run, db::now());
        }
        return Err(format!("{e} 请在历史核对恢复批次 {run}，不要重复操作。"));
    }
    execution::detail(&*database.lock().map_err(|e| e.to_string())?, &run)
}

#[tauri::command]
pub(crate) async fn prepare_file_restoration_batch(
    run_id: String,
    item_ids: Vec<String>,
    state: tauri::State<'_, crate::AppState>,
) -> Result<Draft, String> {
    let operation = state.updates.installation.enter()?;
    let permit = state
        .project_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "文件检查仍在进行。")?;
    if item_ids.len() > MAX {
        return Err("每批最多恢复 20 项。".into());
    }
    let ticket = state
        .file_restorations
        .lock()
        .map_err(|e| e.to_string())?
        .begin()?;
    let (detail, settings) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        (execution::detail(&db, &run_id)?, db::settings(&db)?)
    };
    let home = state.home.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || {
        let (_operation, _permit) = (operation, permit);
        prepare(&detail, &item_ids, &home, &settings)
    })
    .await
    .map_err(|e| e.to_string())??;
    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let settings = db::settings(&db)?;
        for p in &plan.plans {
            p.validate_source(&db)?;
            p.validate(&state.home, &settings)?;
        }
    }
    plan.fresh()?;
    let draft = plan.draft();
    let mut store = state.file_restorations.lock().map_err(|e| e.to_string())?;
    if store.generation != ticket {
        return Err("恢复检查已被取代，请重新选择。".into());
    }
    store.batch = Some(plan);
    Ok(draft)
}
#[tauri::command]
pub(crate) async fn execute_file_restoration_batch(
    id: String,
    request_id: String,
    confirmed: bool,
    state: tauri::State<'_, crate::AppState>,
) -> Result<execution::Detail, String> {
    if !confirmed {
        return Err("请先逐项核对当前文件与原位置并确认批量恢复。".into());
    }
    uuid::Uuid::parse_str(&request_id).map_err(|_| "恢复请求标识无效。")?;
    let operation = state.updates.installation.enter()?;
    let plan = {
        let mut store = state.file_restorations.lock().map_err(|e| e.to_string())?;
        if store.batch.as_ref().is_none_or(|p| p.id != id) {
            return Err("恢复确认已撤下、取代或已使用。".into());
        }
        store.batch.take().ok_or("恢复确认不存在。")?
    };
    plan.fresh()?;
    let job = Arc::new(Job::new(crate::engine::TaskKind::FileRestore));
    {
        let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
        if state.shutdown.closing()
            || jobs.len() >= 3
            || jobs.contains_key(&request_id)
            || jobs.values().any(|j| j.kind.exclusive_operation())
        {
            return Err("请等待当前任务结束后再恢复。".into());
        }
        jobs.insert(request_id.clone(), job.clone());
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let result = run(state.db.clone(), state.home.clone(), plan, job, |_| {});
        if let Ok(mut jobs) = state.jobs.lock() {
            jobs.remove(&request_id);
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
#[cfg(test)]
mod tests;
