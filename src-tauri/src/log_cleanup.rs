//! Narrow user-cache and single-log execution. No IPC accepts file paths or frontend eligibility.
//! Short-lived confirmations bind a server-owned subset and the entire saved
//! source plan. Every item re-runs REAL rules before recording intent and acting.
use crate::activity_authorization::selected_logs;
use crate::{cleanup_preparation, db, engine::Job, execution, provenance, review, rules};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{atomic::Ordering, Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmationItem {
    pub id: String,
    pub path: String,
    pub size_bytes: u64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Confirmation {
    pub version: u64,
    pub id: String,
    pub plan_id: String,
    pub snapshot_sha256: String,
    pub method: &'static str,
    pub created_at: i64,
    pub expires_at: i64,
    pub retained_count: usize,
    pub requires_authorization: bool,
    pub items: Vec<ConfirmationItem>,
}
pub struct Authorized {
    confirmation: Confirmation,
    source_digest: String,
    plan: review::Plan,
}
#[derive(Default)]
pub struct Confirmations {
    entries: HashMap<String, Authorized>,
}
fn digest(plan: &review::Plan) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(plan).map_err(|e| e.to_string())?)
    ))
}
impl Confirmations {
    pub fn revoke_plan(&mut self, plan: &str) {
        self.entries.retain(|_, e| e.plan.id != plan);
    }
    pub fn discard(&mut self, id: &str) {
        self.entries.remove(id);
    }
    pub fn issue(
        &mut self,
        plan: &review::Plan,
        report: &cleanup_preparation::Report,
        now: i64,
    ) -> Result<Option<Confirmation>, String> {
        self.revoke_plan(&plan.id);
        self.entries
            .retain(|_, e| now >= e.confirmation.created_at && now < e.confirmation.expires_at);
        if report.observed_count == 0 && report.authorization_count == 0 {
            return Ok(None);
        }
        if report.version != cleanup_preparation::VERSION
            || report.plan_id != plan.id
            || !report.read_only
            || report.method != "trash"
            || !["userLogFiles", "userCachesAndLogs"].contains(&report.scope)
            || now < report.checked_at
            || now >= report.expires_at
            || report.expires_at > plan.expires_at
            || report.expires_at > report.checked_at.saturating_add(30_000)
            || report.items.len() != plan.items.len()
            || report
                .observed_count
                .saturating_add(report.authorization_count)
                > 20
            || report
                .items
                .iter()
                .zip(&plan.items)
                .any(|(a, b)| a.id != b.candidate.id)
        {
            return Err("日志确认范围无法与本次检查核对，请重新复查。".into());
        }
        let ids: HashSet<_> = report
            .items
            .iter()
            .filter(|i| matches!(i.code, "observed" | "authorizationRequired"))
            .map(|i| &i.id)
            .collect();
        if ids.len() != report.observed_count + report.authorization_count
            || report.items.iter().filter(|i| i.code == "observed").count() != report.observed_count
            || report
                .items
                .iter()
                .filter(|i| i.code == "authorizationRequired")
                .count()
                != report.authorization_count
        {
            return Err("日志确认项目重复或计数不匹配。".into());
        }
        let mut subset = plan.clone();
        subset.items.retain(|i| ids.contains(&i.candidate.id));
        let items = subset
            .items
            .iter()
            .map(|i| {
                let w = i.witness.as_ref().ok_or("日志确认缺少文件身份。")?;
                if w.size > 9_007_199_254_740_991
                    || !["userCache", "userLogs"].contains(&provenance::binding(
                        i.candidate.provenance.as_ref(),
                        &i.candidate.path,
                    ))
                    || (i.candidate.is_directory
                        && provenance::binding(i.candidate.provenance.as_ref(), &i.candidate.path)
                            != "userCache")
                {
                    return Err("日志确认包含不支持的范围。".to_string());
                }
                Ok(ConfirmationItem {
                    id: i.candidate.id.clone(),
                    path: i.candidate.path.clone(),
                    size_bytes: if i.candidate.is_directory {
                        i.candidate
                            .size
                            .filter(|size| *size <= 9_007_199_254_740_991)
                            .ok_or("缓存目录大小未知，保留。")?
                    } else {
                        w.size
                    },
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if items.is_empty() || self.entries.len() >= 8 {
            return Err("确认窗口已达到上限，请关闭旧窗口后重试。".into());
        }
        let confirmation = Confirmation {
            version: 2,
            id: uuid::Uuid::new_v4().to_string(),
            plan_id: plan.id.clone(),
            snapshot_sha256: digest(&subset)?,
            method: "trash",
            created_at: now,
            expires_at: report.expires_at,
            retained_count: plan.items.len() - items.len(),
            requires_authorization: report.authorization_count > 0,
            items,
        };
        self.entries.insert(
            confirmation.id.clone(),
            Authorized {
                confirmation: confirmation.clone(),
                source_digest: digest(plan)?,
                plan: subset,
            },
        );
        Ok(Some(confirmation))
    }
    pub fn take(&mut self, id: &str, now: i64) -> Result<Authorized, String> {
        let entry = self
            .entries
            .remove(id)
            .ok_or("确认不存在、已撤销或已使用，请重新复查。")?;
        if now < entry.confirmation.created_at || now >= entry.confirmation.expires_at {
            return Err("确认已过期或系统时间回退，请重新复查。".into());
        }
        Ok(entry)
    }
}
fn bounded(reason: &str) -> String {
    let mut end = reason.len().min(1000);
    while !reason.is_char_boundary(end) {
        end -= 1;
    }
    let value = reason[..end].replace('\0', " ");
    if value.trim().is_empty() {
        "本项条件无法确认，保留。".into()
    } else {
        value
    }
}
impl Authorized {
    pub fn plan_id(&self) -> &str {
        &self.plan.id
    }
    pub fn validate_source(&self, source: &review::Plan, now: i64) -> Result<(), String> {
        if now < self.confirmation.created_at
            || now >= self.confirmation.expires_at
            || source.id != self.plan.id
            || digest(source)? != self.source_digest
            || digest(&self.plan)? != self.confirmation.snapshot_sha256
        {
            return Err("确认的计划、选择或有效期已变化，请重新扫描并复查。".into());
        }
        Ok(())
    }
}

pub async fn run(
    database: Arc<Mutex<db::Database>>,
    root: PathBuf,
    home: PathBuf,
    authorized: Authorized,
    job: Arc<Job>,
) -> Result<execution::Detail, String> {
    let engine_root = root.clone();
    let user_home = home.clone();
    run_with(database, home, authorized, job, move |requested, job| {
        let root = engine_root.clone();
        let home = user_home.clone();
        async move { rules::run(&root, &home, &requested, job).await }
    })
    .await
}

async fn run_with<F, Fut>(
    database: Arc<Mutex<db::Database>>,
    home: PathBuf,
    authorized: Authorized,
    job: Arc<Job>,
    check: F,
) -> Result<execution::Detail, String>
where
    F: FnMut(Vec<(String, String, String)>, Arc<Job>) -> Fut,
    Fut: std::future::Future<Output = Result<HashMap<String, rules::RuleCheck>, String>>,
{
    run_observed(
        database,
        home,
        authorized,
        job,
        check,
        #[cfg(test)]
        None,
    )
    .await
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum Phase {
    Begun,
    Intent,
    NativeReturned,
    ItemCommitted,
    Finished,
}
#[cfg(test)]
type Checkpoint = Arc<dyn Fn(Phase, &str, Option<&execution::Receipt>) + Send + Sync>;

async fn run_observed<F, Fut>(
    database: Arc<Mutex<db::Database>>,
    home: PathBuf,
    authorized: Authorized,
    job: Arc<Job>,
    check: F,
    #[cfg(test)] checkpoint: Option<Checkpoint>,
) -> Result<execution::Detail, String>
where
    F: FnMut(Vec<(String, String, String)>, Arc<Job>) -> Fut,
    Fut: std::future::Future<Output = Result<HashMap<String, rules::RuleCheck>, String>>,
{
    run_authorized(
        database,
        home,
        authorized,
        job,
        check,
        request_observation,
        #[cfg(test)]
        checkpoint,
    )
    .await
}

async fn request_observation(
    plan: review::Plan,
    home: PathBuf,
    job: Arc<Job>,
) -> Result<selected_logs::Evidence, String> {
    #[cfg(not(test))]
    {
        selected_logs::observe(&plan, &home, job).await
    }
    #[cfg(test)]
    {
        let _ = (plan, home, job);
        Err("自动回归不发起系统授权，文件保留。".into())
    }
}

async fn run_authorized<F, Fut, A, AFut>(
    database: Arc<Mutex<db::Database>>,
    home: PathBuf,
    authorized: Authorized,
    job: Arc<Job>,
    mut check: F,
    observe: A,
    #[cfg(test)] checkpoint: Option<Checkpoint>,
) -> Result<execution::Detail, String>
where
    F: FnMut(Vec<(String, String, String)>, Arc<Job>) -> Fut,
    Fut: std::future::Future<Output = Result<HashMap<String, rules::RuleCheck>, String>>,
    A: FnOnce(review::Plan, PathBuf, Arc<Job>) -> AFut,
    AFut: std::future::Future<Output = Result<selected_logs::Evidence, String>>,
{
    let requires_authorization = authorized.confirmation.requires_authorization;
    let source_digest = authorized.source_digest.clone();
    let plan = authorized.plan;
    let run = {
        let mut db = database.lock().map_err(|e| e.to_string())?;
        let source = db::plan(&db, &plan.id)?;
        // Check the full source even though the immutable run snapshot is a subset.
        if digest(&source)? != authorized.source_digest
            || db::now() >= authorized.confirmation.expires_at
            || db::now() < authorized.confirmation.created_at
            || !db::settings(&db)?.acknowledged
        {
            return Err("确认已失效、计划变化或首次说明未确认，请重新复查。".into());
        }
        execution::begin(&mut db, &plan, db::now())?
    };
    let started = Instant::now();
    let result: Result<(), String> = async {
        job.bind_execution(&run)?;
        #[cfg(test)]
        if let Some(hook) = &checkpoint {
            hook(Phase::Begun, &run, None);
        }
        let mut guards = HashMap::new();
        if requires_authorization {
            let observation = observe(plan.clone(), home.clone(), job.clone())
                .await
                .and_then(|evidence| evidence.into_guards(&plan, &home, db::now()));
            match observation {
                Ok(observed) => guards = observed,
                Err(reason) => {
                    let mut db = database.lock().map_err(|e| e.to_string())?;
                    if !job.cancelled.load(Ordering::SeqCst) {
                        for item in &plan.items {
                            execution::skip(
                                &mut db,
                                &run,
                                &item.candidate.id,
                                &bounded(&format!("当次授权占用检查未完成，保留：{reason}")),
                                db::now(),
                            )?;
                        }
                    }
                    execution::finish(
                        &mut db,
                        &run,
                        job.cancelled.load(Ordering::SeqCst),
                        db::now(),
                    )?;
                    return Ok(());
                }
            }
        }
        for item in &plan.items {
            if job.cancelled.load(Ordering::SeqCst) {
                execution::finish(
                    &mut *database.lock().map_err(|e| e.to_string())?,
                    &run,
                    true,
                    db::now(),
                )?;
                return Ok(());
            }
            if started.elapsed() > Duration::from_secs(120) {
                return Err("本批处理超时；已停止未开始项目，请查看历史。".into());
            }
            // Rebuild descendants from the saved tree on EVERY execution pass.
            let one = review::Plan {
                items: vec![item.clone()],
                ..plan.clone()
            };
            let settings = db::settings(&*database.lock().map_err(|e| e.to_string())?)?;
            let local = review::check(&one, &settings.protected_paths, db::now());
            let batch = crate::descendant_rules::prepare(&one, &local, &home);
            let requested = batch.requested.clone();
            let mut checks = match check(requested, job.clone()).await {
                Ok(checks) => checks,
                Err(_) if job.cancelled.load(Ordering::SeqCst) => {
                    execution::finish(
                        &mut *database.lock().map_err(|e| e.to_string())?,
                        &run,
                        true,
                        db::now(),
                    )?;
                    return Ok(());
                }
                Err(reason) => {
                    execution::skip(
                        &mut *database.lock().map_err(|e| e.to_string())?,
                        &run,
                        &item.candidate.id,
                        &bounded(&format!("当次规则复查失败，保留：{reason}")),
                        db::now(),
                    )?;
                    continue;
                }
            };
            let guard = if requires_authorization {
                let observed = guards.remove(&item.candidate.id);
                let applied = observed
                    .as_ref()
                    .ok_or("缺少当次对象占用检查。".to_string())
                    .and_then(|g| g.apply(&plan, &mut checks));
                if let Err(reason) = applied {
                    execution::skip(
                        &mut *database.lock().map_err(|e| e.to_string())?,
                        &run,
                        &item.candidate.id,
                        &bounded(&reason),
                        db::now(),
                    )?;
                    continue;
                }
                observed
            } else {
                None
            };
            let db = database.clone();
            let home = home.clone();
            let job = job.clone();
            let run = run.clone();
            let source_digest = source_digest.clone();
            let one = review::Plan {
                items: vec![item.clone()],
                ..plan.clone()
            };
            #[cfg(test)]
            let checkpoint = checkpoint.clone();
            let stopped = tauri::async_runtime::spawn_blocking(move || -> Result<bool, String> {
                let settings = db::settings(&*db.lock().map_err(|e| e.to_string())?)?;
                let now = db::now();
                let view = crate::descendant_rules::apply(
                    review::apply_rule_checks(
                        review::check(&one, &settings.protected_paths, now),
                        &checks,
                        now,
                    ),
                    &batch,
                    &checks,
                );
                let target = cleanup_preparation::prepare_one(&one, &view, &home, now, || {
                    job.cancelled.load(Ordering::SeqCst)
                });
                if job.cancelled.load(Ordering::SeqCst) {
                    execution::finish(
                        &mut *db.lock().map_err(|e| e.to_string())?,
                        &run,
                        true,
                        db::now(),
                    )?;
                    return Ok(true);
                }
                let id = &one.items[0].candidate.id;
                let target = match target {
                    Ok(target) => target,
                    Err(reason) => {
                        execution::skip(
                            &mut *db.lock().map_err(|e| e.to_string())?,
                            &run,
                            id,
                            &bounded(&reason),
                            db::now(),
                        )?;
                        return Ok(false);
                    }
                };
                {
                    let mut db = db.lock().map_err(|e| e.to_string())?;
                    let latest = db::settings(&db)?;
                    if job.cancelled.load(Ordering::SeqCst) {
                        execution::finish(&mut db, &run, true, db::now())?;
                        return Ok(true);
                    }
                    if latest.protected_paths != settings.protected_paths
                        || !latest.acknowledged
                        || digest(&db::plan(&db, &one.id)?)? != source_digest
                        || db::now() < one.created_at
                        || db::now() >= one.expires_at
                    {
                        execution::skip(
                            &mut db,
                            &run,
                            id,
                            "保护或有效期已变化，本项保留。",
                            db::now(),
                        )?;
                        return Ok(false);
                    }
                    if started.elapsed() > Duration::from_secs(120) {
                        return Err("本批处理超时，停止后续项目。".into());
                    }
                    if let Some(observed) = &guard {
                        if let Err(reason) = observed.validate() {
                            execution::skip(&mut db, &run, id, &bounded(&reason), db::now())?;
                            return Ok(false);
                        }
                    }
                    execution::intent(&mut db, &run, id, db::now())?;
                }
                #[cfg(test)]
                if let Some(hook) = &checkpoint {
                    hook(Phase::Intent, &run, None);
                }
                // Intent marks this item in flight. Always collect its outcome,
                // including when a cancellation arrives during the native call.
                let attempt = if let Some(observed) = &guard {
                    target.move_to_trash_checked(|| observed.validate())
                } else {
                    target.move_to_trash()
                };
                let (outcome, reason) = match attempt {
                    Ok(attempt) => (attempt.outcome, attempt.reason),
                    Err(reason) => (
                        execution::Outcome::Failed,
                        format!("最终检查拒绝，未调用废纸篓接口：{reason}"),
                    ),
                };
                #[cfg(test)]
                if let Some(hook) = &checkpoint {
                    let receipt = match &outcome {
                        execution::Outcome::Moved(receipt) => Some(receipt),
                        execution::Outcome::Uncertain(receipt) => receipt.as_ref(),
                        execution::Outcome::Failed => None,
                    };
                    hook(Phase::NativeReturned, &run, receipt);
                }
                let uncertain = matches!(outcome, execution::Outcome::Uncertain(_));
                execution::complete_item(
                    &mut *db.lock().map_err(|e| e.to_string())?,
                    &run,
                    id,
                    outcome,
                    &bounded(&reason),
                    db::now(),
                )?;
                #[cfg(test)]
                if let Some(hook) = &checkpoint {
                    hook(Phase::ItemCommitted, &run, None);
                }
                Ok(uncertain)
            })
            .await
            .map_err(|e| e.to_string())??;
            if stopped {
                return Ok(());
            }
        }
        execution::finish(
            &mut *database.lock().map_err(|e| e.to_string())?,
            &run,
            job.cancelled.load(Ordering::SeqCst),
            db::now(),
        )?;
        #[cfg(test)]
        if let Some(hook) = &checkpoint {
            hook(Phase::Finished, &run, None);
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        let mut db = database.lock().map_err(|e| e.to_string())?;
        let _ = execution::stop(&mut db, &run, db::now());
        return Err(format!(
            "{error} 批次 {run} 的实际结果请在历史中核对，不能自动重试。"
        ));
    }
    execution::detail(&*database.lock().map_err(|e| e.to_string())?, &run)
}

#[cfg(test)]
mod tests;
