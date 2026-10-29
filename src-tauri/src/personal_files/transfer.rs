//! Explicit cross-volume copy, verification and source Trash. No copy/delete
//! fallback inside the same-volume mover; every phase has independent evidence.
use super::*;
use crate::{
    db,
    engine::Job,
    execution::{Outcome, Receipt},
    observation::AnchoredPath,
    rules,
};
use std::{
    os::unix::fs::MetadataExt,
    path::PathBuf,
    sync::{atomic::Ordering, Arc, Mutex},
};
pub(crate) mod commands;
pub(crate) mod journal;
mod native;
const BUDGET: Duration = Duration::from_secs(15 * 60);
const REVISION: &str = "MooMole.cross-volume-move.v1";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Plan {
    protocol: u32,
    revision: String,
    source: super::Plan,
    movement: movement::Movement,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Draft {
    id: String,
    scan_id: String,
    expires_at: i64,
    directory: String,
    conflict: movement::Conflict,
    items: Vec<FileRow>,
    targets: Vec<movement::Target>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Entry {
    id: String,
    path: String,
    target: Option<String>,
    temporary_path: Option<String>,
    state: String,
    reason: String,
    copy_receipt: Option<Receipt>,
    source_trash_receipt: Option<Receipt>,
    sha256: Option<String>,
    metadata_sha256: Option<String>,
    copy_witness: Option<Witness>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Record {
    protocol: u32,
    revision: String,
    id: String,
    request_id: String,
    created_at: i64,
    finished_at: Option<i64>,
    state: String,
    items: Vec<Entry>,
    reviewed_at: Option<i64>,
    reason: String,
}
#[derive(Default)]
pub(crate) struct Store {
    plans: HashMap<String, (Plan, Instant)>,
}
struct Context {
    database: Arc<Mutex<db::Database>>,
    home: PathBuf,
    engine_root: PathBuf,
}
fn prepare(
    source: super::Plan,
    destination: &str,
    conflict: movement::Conflict,
    settings: &db::Settings,
) -> Result<Plan, String> {
    source.validate_record()?;
    if source.origin() != MODULE
        || source.movement.is_some()
        || source.items.iter().map(|i| i.size).sum::<u64>() > 16 * 1024 * 1024 * 1024
    {
        return Err("本轮跨磁盘移动只支持所选普通文件，每批最多 20 项、16 GiB。".into());
    }
    let movement = movement::prepare_cross_volume(&source.items, destination, conflict, settings)?;
    for item in &source.items {
        validate_item(item, &source, &movement, settings)?;
    }
    Ok(Plan {
        protocol: 1,
        revision: REVISION.into(),
        source,
        movement,
    })
}
fn validate_item(
    item: &Item,
    source: &super::Plan,
    movement: &movement::Movement,
    settings: &db::Settings,
) -> Result<(), String> {
    if !settings.acknowledged
        || !ordinary(Path::new(&item.path))
        || !Path::new(&item.path).starts_with(&source.root)
        || protected(Path::new(&item.path), &settings.protected_paths)?
    {
        return Err("源文件范围或保护状态变化。".into());
    }
    let held = AnchoredPath::open(Path::new(&item.path))?;
    let m = held.target().metadata().map_err(|e| e.to_string())?;
    let directory = movement.validate_directory(settings)?;
    if held.witness != item.witness
        || !held.local_materialized()
        || !m.is_file()
        || m.nlink() != 1
        || m.uid() != unsafe { libc::geteuid() }
        || m.dev()
            == directory
                .target()
                .metadata()
                .map_err(|e| e.to_string())?
                .dev()
    {
        return Err("源文件身份、本地状态或磁盘关系变化，原件保留。".into());
    }
    if let Some(p) = movement
        .targets
        .iter()
        .find(|t| t.id == item.id)
        .and_then(|t| t.destination.as_ref())
    {
        if protected(Path::new(p), &settings.protected_paths)? {
            return Err("移动目标已受保护。".into());
        }
    }
    let access = crate::permissions::observe(&held, &crate::permissions::Budget::new())?;
    if access.code != crate::permissions::Code::NoRestrictionObserved {
        return Err(access.refusal_reason());
    }
    Ok(())
}
fn stopped(job: &Job, began: Instant) -> Result<(), String> {
    if job.cancelled.load(Ordering::SeqCst) {
        Err("已停止跨磁盘移动，已生成的副本不会自动删除。".into())
    } else if began.elapsed() > BUDGET {
        Err("跨磁盘移动超过本轮十五分钟预算。".into())
    } else {
        Ok(())
    }
}
type Stamp = (PathBuf, Witness, bool);
async fn policy(
    state: &Context,
    item: &Item,
    target: &str,
    job: Arc<Job>,
) -> Result<Stamp, String> {
    let mut stamp = writer::policy_stamp(&state.home)?;
    for attempt in 0..2 {
        let requested = vec![
            (item.id.clone(), item.path.clone(), "unsupported".into()),
            (
                format!("{}-destination", item.id),
                target.into(),
                "unsupported".into(),
            ),
        ];
        let mut result =
            rules::run(&state.engine_root, &state.home, &requested, job.clone()).await?;
        let after = writer::policy_stamp(&state.home)?;
        if after != stamp {
            if attempt == 0 {
                stamp = after;
                continue;
            }
            return Err("Mole 保护配置变化，本项保留。".into());
        }
        for (id, _, _) in requested {
            let r = result.remove(&id).ok_or("缺少源或目标的保护结果。")?;
            if r.code != "passed" {
                return Err(r.reason);
            }
        }
        return Ok(stamp);
    }
    Err("路径保护检查未完成。".into())
}
fn save(database: &Arc<Mutex<db::Database>>, record: &Record) -> Result<(), String> {
    journal::save(&*database.lock().map_err(|e| e.to_string())?, record)
}
async fn run(
    state: Context,
    plan: Plan,
    request_id: String,
    job: Arc<Job>,
) -> Result<Record, String> {
    run_with(state, plan, request_id, job, |_, _| Ok(())).await
}
async fn run_with(
    state: Context,
    plan: Plan,
    request_id: String,
    job: Arc<Job>,
    mut after_copy: impl FnMut(&Record, &Job) -> Result<(), String>,
) -> Result<Record, String> {
    let began = Instant::now();
    let database = state.database.clone();
    let now = db::now();
    if now < plan.source.created_at || now >= plan.source.expires_at {
        return Err("跨磁盘移动确认已过期。".into());
    }
    let mut record = Record {
        protocol: 1,
        revision: REVISION.into(),
        id: plan.source.id.clone(),
        request_id,
        created_at: now,
        finished_at: None,
        state: "running".into(),
        reviewed_at: None,
        reason: String::new(),
        items: plan
            .source
            .items
            .iter()
            .map(|item| Entry {
                id: item.id.clone(),
                path: item.path.clone(),
                target: plan
                    .movement
                    .targets
                    .iter()
                    .find(|t| t.id == item.id)
                    .and_then(|t| t.destination.clone()),
                temporary_path: None,
                state: "pending".into(),
                reason: String::new(),
                copy_receipt: None,
                source_trash_receipt: None,
                sha256: None,
                metadata_sha256: None,
                copy_witness: None,
            })
            .collect(),
    };
    journal::insert(
        &mut *database.lock().map_err(|e| e.to_string())?,
        &plan,
        &record,
    )?;
    for (index, item) in plan.source.items.iter().enumerate() {
        if stopped(&job, began).is_err() {
            break;
        }
        let Some(target) = record.items[index].target.clone() else {
            record.items[index].state = "skipped".into();
            record.items[index].reason = "按确认计划跳过重名目标，原件保留。".into();
            save(&database, &record)?;
            continue;
        };
        let settings = db::settings(&*database.lock().map_err(|e| e.to_string())?)?;
        if let Err(e) = validate_item(item, &plan.source, &plan.movement, &settings) {
            record.items[index].state = "skipped".into();
            record.items[index].reason = e;
            save(&database, &record)?;
            continue;
        }
        let stamp = match policy(&state, item, &target, job.clone()).await {
            Ok(s) => s,
            Err(e) => {
                record.items[index].state = "skipped".into();
                record.items[index].reason = e;
                save(&database, &record)?;
                continue;
            }
        };
        let temp = Path::new(&plan.movement.directory)
            .join(format!(".MooMole-transfer-{}-{}.partial", record.id, index));
        record.items[index].temporary_path = Some(temp.to_string_lossy().into_owned());
        record.items[index].state = "copying".into();
        save(&database, &record)?;
        let native_plan = plan.clone();
        let one = item.clone();
        let expected_stamp = stamp.clone();
        let home = state.home.clone();
        let db = database.clone();
        let control = job.clone();
        let target_path = target.clone();
        let copied = tauri::async_runtime::spawn_blocking(move || {
            native::copy(
                &one,
                &native_plan.movement,
                &target_path,
                &temp,
                &control,
                began,
                || {
                    stopped(&control, began)?;
                    let settings = db::settings(&*db.lock().map_err(|e| e.to_string())?)?;
                    validate_item(&one, &native_plan.source, &native_plan.movement, &settings)?;
                    if writer::policy_stamp(&home)? != expected_stamp {
                        return Err("Mole 保护配置在复制期间变化。".into());
                    }
                    Ok(())
                },
            )
        })
        .await
        .map_err(|_| "复制工作线程中断，请检查目标和临时位置。".to_string());
        let copied = match copied {
            Ok(Ok(c)) => c,
            other => {
                record.items[index].state = "uncertain".into();
                record.items[index].reason = match other {
                    Ok(Err(e)) => e,
                    Err(e) => e,
                    _ => unreachable!(),
                };
                save(&database, &record)?;
                break;
            }
        };
        record.items[index].copy_receipt = Some(copied.receipt.clone());
        record.items[index].copy_witness = Some(copied.witness.clone());
        record.items[index].sha256 = Some(copied.sha256.clone());
        record.items[index].metadata_sha256 = Some(copied.metadata_sha256.clone());
        record.items[index].state = "copyVerified".into();
        save(&database, &record)?;
        after_copy(&record, &job)?;
        if let Err(e) = stopped(&job, began) {
            record.items[index].state = "copyOnly".into();
            record.items[index].reason = e;
            save(&database, &record)?;
            break;
        }
        let fresh_stamp = match policy(&state, item, &target, job.clone()).await {
            Ok(s) => s,
            Err(e) => {
                record.items[index].state = "copyOnly".into();
                record.items[index].reason = e;
                save(&database, &record)?;
                break;
            }
        };
        record.items[index].state = "trashIntent".into();
        save(&database, &record)?;
        let db = database.clone();
        let home = state.home.clone();
        let p = plan.clone();
        let one = item.clone();
        let control = job.clone();
        let trashed = tauri::async_runtime::spawn_blocking(move || -> Result<_, String> {
            let native = crate::cleanup_target::PreparedFile::prepare(&one.target())?;
            native.move_to_trash_checked(|| {
                stopped(&control, began)?;
                let settings = db::settings(&*db.lock().map_err(|e| e.to_string())?)?;
                validate_item(&one, &p.source, &p.movement, &settings)?;
                if writer::policy_stamp(&home)? != fresh_stamp {
                    return Err("Mole 保护配置在原件处理前变化，原件保留。".into());
                }
                native::verify_copy(&copied, &control, began)?;
                Ok(())
            })
        })
        .await
        .map_err(|_| "原件处理线程中断，不能推断处理完成。".to_string());
        match trashed {
            Ok(Ok(result)) => match result.outcome {
                Outcome::Moved(receipt) => {
                    record.items[index].state = "moved".into();
                    record.items[index].source_trash_receipt = Some(receipt);
                    record.items[index].reason =
                        "目标副本与元数据已核对，原件已移入废纸篓；两端收据均已保存。".into();
                }
                Outcome::Failed => {
                    record.items[index].state = "copyOnly".into();
                    record.items[index].reason = result.reason;
                }
                Outcome::Uncertain(receipt) => {
                    record.items[index].state = "uncertain".into();
                    record.items[index].source_trash_receipt = receipt;
                    record.items[index].reason = result.reason;
                }
            },
            other => {
                record.items[index].state = "uncertain".into();
                record.items[index].reason = match other {
                    Ok(Err(e)) => e,
                    Err(e) => e,
                    _ => unreachable!(),
                };
            }
        }
        save(&database, &record)?;
        if record.items[index].state != "moved" {
            break;
        }
    }
    for entry in &mut record.items {
        if entry.state == "pending" {
            entry.state = if job.cancelled.load(Ordering::SeqCst) {
                "cancelled"
            } else {
                "notStarted"
            }
            .into();
            entry.reason = "本项未继续，未自动重试。".into();
        }
    }
    let moved = record.items.iter().filter(|e| e.state == "moved").count();
    let changed = record
        .items
        .iter()
        .any(|e| e.copy_receipt.is_some() || e.state == "uncertain");
    record.state = if record.items.iter().any(|e| e.state == "uncertain") {
        "uncertain"
    } else if moved == record.items.len() {
        "completed"
    } else if changed {
        "partial"
    } else if job.cancelled.load(Ordering::SeqCst) {
        "cancelled"
    } else {
        "noChanges"
    }
    .into();
    record.finished_at = Some(db::now());
    record.reason =
        "请分别核对目标副本与原件记录。原件在废纸篓时仍占用原磁盘；不自动清空、删除副本或重试。"
            .into();
    save(&database, &record)?;
    Ok(record)
}
#[cfg(test)]
mod tests;
