use super::*;
use crate::{db, engine::Job, execution, observation::AnchoredPath, rules};
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
};

// Bind the real whitelist (or its physical absence) around the rules runner
// and final native gate. It is never parsed as a list of deletion targets.
fn policy_stamp(home: &Path) -> Result<(PathBuf, Witness, bool), String> {
    let path = home.join(".config/mole/whitelist");
    let mut nearest = path.as_path();
    loop {
        match std::fs::symlink_metadata(nearest) {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                nearest = nearest.parent().ok_or("Mole 保护配置无法确认。")?
            }
            Err(_) => return Err("Mole 保护配置无法读取，保留文件。".into()),
        }
    }
    let held = AnchoredPath::open(nearest)?;
    let meta = held.target().metadata().map_err(|e| e.to_string())?;
    let present = nearest == path;
    if !held.local_materialized()
        || (present && (!meta.is_file() || meta.len() > 64 * 1024))
        || (!present && !meta.is_dir())
    {
        return Err("Mole 保护配置不是可核对的本地文件，保留。".into());
    }
    Ok((nearest.into(), held.witness, present))
}
fn validate(item: &Item, plan: &Plan, settings: &db::Settings, now: i64) -> Result<(), String> {
    if !settings.acknowledged
        || now < plan.created_at
        || now >= plan.expires_at
        || !ordinary(Path::new(&item.path))
        || !Path::new(&item.path).starts_with(&plan.root)
        || protected(Path::new(&item.path), &settings.protected_paths)?
    {
        return Err("文件受保护、范围变化或计划过期，本项保留。".into());
    }
    let held = AnchoredPath::open(Path::new(&item.path))?;
    let meta = held.target().metadata().map_err(|e| e.to_string())?;
    if held.witness != item.witness
        || !meta.is_file()
        || meta.nlink() != 1
        || !held.local_materialized()
    {
        return Err("个人文件的身份、内容或本地状态已变化，请重新扫描。".into());
    }
    if let Some(movement) = &plan.movement {
        movement.validate_directory(settings)?;
        if movement
            .targets
            .iter()
            .find(|t| t.id == item.id)
            .and_then(|t| t.destination.as_ref())
            .is_some_and(|p| protected(Path::new(p), &settings.protected_paths).unwrap_or(true))
        {
            return Err("移动目标已受保护，本项保留。".into());
        }
    }
    Ok(())
}
use std::os::unix::fs::MetadataExt;

enum PreparedMutation {
    Trash(crate::cleanup_target::PreparedFile),
    Move(movement::Prepared),
}
impl PreparedMutation {
    fn perform(
        self,
        gate: impl FnOnce() -> Result<(), String>,
    ) -> Result<(execution::Outcome, String), String> {
        match self {
            Self::Trash(target) => target
                .move_to_trash_checked(gate)
                .map(|v| (v.outcome, v.reason)),
            Self::Move(target) => target.move_checked(gate),
        }
    }
}

pub(super) async fn run(
    state: crate::AppState,
    plan: Plan,
    job: Arc<Job>,
    request: String,
) -> Result<execution::Detail, String> {
    run_with(
        state.db.clone(),
        state.home.clone(),
        plan,
        job,
        move |requested, job| {
            let root = state.engine_root.clone();
            let home = state.home.clone();
            async move { rules::run(&root, &home, &requested, job).await }
        },
        move |run| {
            if let Ok(mut store) = state.personal_files.lock() {
                if store.request_id.as_deref() == Some(request.as_str()) {
                    store.run_id = Some(run.into());
                }
            }
        },
    )
    .await
}

async fn run_with<F, Fut>(
    database: Arc<std::sync::Mutex<db::Database>>,
    home: PathBuf,
    plan: Plan,
    job: Arc<Job>,
    mut check: F,
    on_begin: impl FnOnce(&str),
) -> Result<execution::Detail, String>
where
    F: FnMut(Vec<(String, String, String)>, Arc<Job>) -> Fut,
    Fut: std::future::Future<Output = Result<HashMap<String, rules::RuleCheck>, String>>,
{
    let run = {
        let mut db = database.lock().map_err(|e| e.to_string())?;
        if !db::settings(&db)?.acknowledged {
            return Err("请先确认首次使用说明。".into());
        }
        execution::begin_personal(&mut db, &plan, db::now())?
    };
    on_begin(&run);
    job.bind_execution(&run)?;
    let began = Instant::now();
    let result: Result<(), String> = async {
        for item in &plan.items {
            if job.cancelled.load(Ordering::SeqCst) {
                break;
            }
            if began.elapsed() > Duration::from_secs(120) {
                return Err("个人文件处理超时，请在历史中核对已完成项目。".into());
            }
            if plan.movement.as_ref().is_some_and(|m| {
                m.targets
                    .iter()
                    .find(|t| t.id == item.id)
                    .is_none_or(|t| t.destination.is_none())
            }) {
                execution::skip(
                    &mut *database.lock().map_err(|e| e.to_string())?,
                    &run,
                    &item.id,
                    "目标已有同名项目或文件已在目标目录；按确认计划保留源文件。",
                    db::now(),
                )?;
                continue;
            }
            let settings = db::settings(&*database.lock().map_err(|e| e.to_string())?)?;
            let preliminary =
                validate(item, &plan, &settings, db::now()).and_then(|_| policy_stamp(&home));
            let mut stamp = match preliminary {
                Ok(stamp) => stamp,
                Err(reason) => {
                    execution::skip(
                        &mut *database.lock().map_err(|e| e.to_string())?,
                        &run,
                        &item.id,
                        &bounded(&reason),
                        db::now(),
                    )?;
                    continue;
                }
            };
            // Only path-level Mole protection is applicable to manual personal
            // selection. Cache module/activity eligibility is never fabricated.
            let mut requested = vec![(item.id.clone(), item.path.clone(), "unsupported".into())];
            if let Some(target) = plan
                .movement
                .as_ref()
                .and_then(|m| m.targets.iter().find(|t| t.id == item.id))
                .and_then(|t| t.destination.as_ref())
            {
                requested.push((
                    format!("{}-destination", item.id),
                    target.clone(),
                    "unsupported".into(),
                ));
            }
            let expected_ids: Vec<_> = requested.iter().map(|r| r.0.clone()).collect();
            let mut checks = check(requested.clone(), job.clone()).await;
            if job.cancelled.load(Ordering::SeqCst) {
                break;
            }
            // Mole may initialize its own log/configuration directories on first
            // use. Re-run read-only protection once against the new stamp;
            // no file operation or old positive result is retried/reused.
            match policy_stamp(&home) {
                Ok(current) if current != stamp => {
                    stamp = current;
                    checks = check(requested, job.clone()).await;
                }
                Err(error) => checks = Err(error),
                _ => {}
            }
            if job.cancelled.load(Ordering::SeqCst) {
                break;
            }
            let rule = checks.and_then(|mut values| {
                let mut result = None;
                for id in &expected_ids {
                    let rule = values.remove(id).ok_or("缺少源或目标路径保护结果。")?;
                    if rule.code != "passed" {
                        return Ok(rule);
                    }
                    result = Some(rule);
                }
                result.ok_or("缺少本项路径保护结果。".into())
            });
            let rule = match rule {
                Ok(rule) if rule.code == "passed" => rule,
                Ok(rule) => {
                    execution::skip(
                        &mut *database.lock().map_err(|e| e.to_string())?,
                        &run,
                        &item.id,
                        &bounded(&rule.reason),
                        db::now(),
                    )?;
                    continue;
                }
                Err(reason) => {
                    execution::skip(
                        &mut *database.lock().map_err(|e| e.to_string())?,
                        &run,
                        &item.id,
                        &bounded(&reason),
                        db::now(),
                    )?;
                    continue;
                }
            };
            let _path_rule = rule;
            let db = database.clone();
            let own_home = home.clone();
            let one = item.clone();
            let saved = plan.clone();
            let bound_run = run.clone();
            let control = job.clone();
            let unknown = tauri::async_runtime::spawn_blocking(move || -> Result<bool, String> {
                let prepare = || -> Result<PreparedMutation, String> {
                    if began.elapsed() > Duration::from_secs(120) {
                        return Err("本批个人文件处理已超时，本项保留。".into());
                    }
                    let settings = db::settings(&*db.lock().map_err(|e| e.to_string())?)?;
                    validate(&one, &saved, &settings, db::now())?;
                    if policy_stamp(&own_home)? != stamp {
                        return Err("Mole 保护配置已变化，请重新检查。".into());
                    }
                    match &saved.movement {
                        Some(m) => {
                            let target = m
                                .targets
                                .iter()
                                .find(|t| t.id == one.id)
                                .and_then(|t| t.destination.as_ref())
                                .ok_or("移动目标缺失。")?;
                            Ok(PreparedMutation::Move(movement::Prepared::new(
                                &one, m, target, &settings,
                            )?))
                        }
                        None => Ok(PreparedMutation::Trash(
                            crate::cleanup_target::PreparedFile::prepare(&one.target())?,
                        )),
                    }
                };
                if control.cancelled.load(Ordering::SeqCst) {
                    return Ok(false);
                }
                let target = match prepare() {
                    Ok(target) => target,
                    Err(reason) => {
                        execution::skip(
                            &mut *db.lock().map_err(|e| e.to_string())?,
                            &bound_run,
                            &one.id,
                            &bounded(&reason),
                            db::now(),
                        )?;
                        return Ok(false);
                    }
                };
                if control.cancelled.load(Ordering::SeqCst) {
                    return Ok(false);
                }
                execution::intent(
                    &mut *db.lock().map_err(|e| e.to_string())?,
                    &bound_run,
                    &one.id,
                    db::now(),
                )?;
                let attempt = target.perform(|| {
                    if control.cancelled.load(Ordering::SeqCst) {
                        return Err("系统处理开始前已取消。".into());
                    }
                    let settings = db::settings(&*db.lock().map_err(|e| e.to_string())?)?;
                    validate(&one, &saved, &settings, db::now())?;
                    if policy_stamp(&own_home)? != stamp {
                        return Err("保护配置在最终处理前已变化，保留。".into());
                    }
                    Ok(())
                });
                let (outcome, reason) = match attempt {
                    Ok(value) => value,
                    Err(reason) => (execution::Outcome::Failed, reason),
                };
                let unknown = matches!(outcome, execution::Outcome::Uncertain(_));
                execution::complete_item(
                    &mut *db.lock().map_err(|e| e.to_string())?,
                    &bound_run,
                    &one.id,
                    outcome,
                    &bounded(&reason),
                    db::now(),
                )?;
                Ok(unknown)
            })
            .await
            .map_err(|e| e.to_string())??;
            if unknown {
                return Ok(());
            }
        }
        execution::finish(
            &mut *database.lock().map_err(|e| e.to_string())?,
            &run,
            job.cancelled.load(Ordering::SeqCst),
            db::now(),
        )?;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        let mut db = database.lock().map_err(|e| e.to_string())?;
        let _ = execution::stop(&mut db, &run, db::now());
        return Err(format!("{error} 请在历史核对批次 {run}，不要重复处理。"));
    }
    execution::detail(&*database.lock().map_err(|e| e.to_string())?, &run)
}
fn bounded(value: &str) -> String {
    let mut end = value.len().min(1000);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let text = value[..end].replace('\0', " ");
    if text.trim().is_empty() {
        "本项未能完成，保留并重新检查。".into()
    } else {
        text
    }
}

#[cfg(test)]
mod tests;
