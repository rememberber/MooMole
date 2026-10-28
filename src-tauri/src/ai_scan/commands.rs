use super::{agents, inventory, Snapshot, SKILL};
use crate::{
    db,
    engine::{self, Job, TaskKind},
    review, AppState,
};
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
};

#[tauri::command]
pub(crate) async fn detect_ai_agents(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<agents::Agent>, String> {
    let home = state.home.clone();
    tauri::async_runtime::spawn_blocking(move || agents::detect(&home))
        .await
        .map_err(|_| "Agent 检测未完成。".into())
}
#[tauri::command]
pub(crate) fn ai_scan_skill() -> &'static str {
    SKILL
}
#[tauri::command]
pub(crate) fn get_ai_scan(state: tauri::State<'_, AppState>) -> Result<Option<Snapshot>, String> {
    Ok(state
        .ai_scan
        .lock()
        .map_err(|_| "无法读取 AI 扫描状态。")?
        .current())
}
#[tauri::command]
pub(crate) fn cancel_ai_scan(
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if let Some(job) = state
        .jobs
        .lock()
        .map_err(|_| "无法读取扫描任务。")?
        .get(&request_id)
        .filter(|j| j.kind == TaskKind::AiScan)
    {
        job.cancel();
    }
    Ok(())
}
#[tauri::command]
pub(crate) fn start_ai_scan(
    agent_id: String,
    root: String,
    metadata_consent: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Snapshot, String> {
    if !metadata_consent {
        return Err("请先确认将本次文件元数据发送给所选 Agent 的模型服务；可能使用其额度。".into());
    }
    if !["codex", "claude"].contains(&agent_id.as_str()) {
        return Err("该 Agent 尚未适配受限扫描。".into());
    }
    let operation = state.updates.installation.enter()?;
    // Follow the task registry lock order: database -> jobs -> module store.
    let database = state.db.lock().map_err(|_| "无法读取保护设置。")?;
    let settings = db::settings(&database)?;
    if !settings.acknowledged {
        return Err("请先阅读并确认首次使用说明。".into());
    }
    // Path syntax now; descriptor-relative physical scope validation happens in
    // the owned worker, away from Tauri's UI thread.
    if root.len() > 4096 || !root.starts_with('/') || root.chars().any(char::is_control) {
        return Err("请选择有效的绝对目录。".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let job = Arc::new(Job::new(TaskKind::AiScan));
    let mut jobs = state.jobs.lock().map_err(|_| "无法登记扫描任务。")?;
    if state.shutdown.closing() || !jobs.is_empty() {
        return Err("请先等待当前任务完成，再开始 AI 扫描。".into());
    }
    let mut store = state.ai_scan.lock().map_err(|_| "无法开始 AI 扫描。")?;
    let snapshot = store.begin(id.clone(), agent_id.clone(), root.clone())?;
    if let Err(error) = db::start(&database, &id, "aiscan", Some(&root)) {
        store.snapshot = None;
        return Err(error);
    }
    jobs.insert(id.clone(), job.clone());
    drop(store);
    drop(jobs);
    drop(database);
    let state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let _operation = operation;
        let outcome = run(
            &state,
            &id,
            &agent_id,
            PathBuf::from(root),
            job.clone(),
            settings.protected_paths,
            settings.language,
        )
        .await;
        let cancelled = job.cancelled.load(Ordering::SeqCst);
        let (status, error) = if cancelled {
            (
                "cancelled",
                Some("扫描已停止；没有执行清理。已发送给模型的信息无法撤回。".into()),
            )
        } else {
            match &outcome {
                Ok(_) => ("succeeded", None),
                Err(e) => ("failed", Some(e.clone())),
            }
        };
        let persisted = state
            .db
            .lock()
            .map_err(|_| "无法保存扫描历史。".to_string())
            .and_then(|db| {
                db::finish(
                    &db,
                    &id,
                    status,
                    error
                        .as_deref()
                        .unwrap_or("AI 建议已生成，尚未执行清理。报告只在当前 App 会话短期保留。"),
                )
            });
        if let Ok(mut store) = state.ai_scan.lock() {
            if let Some(s) = store.snapshot.as_mut().filter(|s| s.id == id) {
                s.state = status.into();
                s.finished_at = Some(db::now());
                s.stage = if cancelled {
                    "扫描已停止"
                } else if outcome.is_ok() {
                    "分析完成，等待审阅"
                } else {
                    "扫描未完成"
                }
                .into();
                s.error = error.or_else(|| persisted.err());
                if !cancelled {
                    s.report = outcome.ok();
                }
            }
        }
        if let Ok(mut jobs) = state.jobs.lock() {
            jobs.remove(&id);
        }
    });
    Ok(snapshot)
}
fn stage(state: &AppState, id: &str, text: &str) {
    if let Ok(mut store) = state.ai_scan.lock() {
        if let Some(s) = store.snapshot.as_mut().filter(|s| s.id == id) {
            s.stage = text.into();
        }
    }
}
async fn run(
    state: &AppState,
    id: &str,
    agent_id: &str,
    root: PathBuf,
    job: Arc<Job>,
    protected: Vec<String>,
    language: String,
) -> Result<super::Report, String> {
    let work_root = root.clone();
    let rules = protected.clone();
    tauri::async_runtime::spawn_blocking(move || {
        inventory::validate_root(work_root.to_str().ok_or("目录名称无法编码。")?, &rules)
    })
    .await
    .map_err(|_| "目录检查未完成。")??;
    let scratch = tempfile::Builder::new()
        .prefix("moomole-ai-")
        .tempdir()
        .map_err(|_| "无法准备私有扫描目录。")?;
    let executable = agents::prepare(agent_id, &state.home, scratch.path(), job.clone()).await?;
    stage(state, id, "正在读取 Mole 清理候选…");
    let result =
        engine::run_clean_preview(&state.engine_root, &state.home, job.clone(), &protected).await;
    if job.cancelled.load(Ordering::SeqCst) {
        return Err("扫描已取消。".into());
    }
    let mut limitations = vec![];
    let scan = match result {
        Ok(value) => {
            let preview = serde_json::from_value(value).map_err(|_| "Mole 清单格式无效。")?;
            let scan_id = uuid::Uuid::new_v4().to_string();
            let scan = tauri::async_runtime::spawn_blocking(move || {
                review::capture(scan_id, preview, db::now())
            })
            .await
            .map_err(|_| "Mole 身份检查未完成。")??;
            db::save_scan(
                &*state.db.lock().map_err(|_| "无法保留 Mole 清单。")?,
                &scan,
            )?;
            Some(scan)
        }
        Err(error) => {
            limitations.push(format!(
                "Mole 扫描未完成，当前仅有扩展元数据线索，不能提交清理审阅。{error}"
            ));
            None
        }
    };
    stage(state, id, "正在扫描所选目录的本地线索…");
    let snapshot_scan = scan.clone();
    let work_job = job.clone();
    let work_rules = protected.clone();
    let (evidence, mut notes) = tauri::async_runtime::spawn_blocking(move || {
        inventory::collect(snapshot_scan.as_ref(), &root, &work_rules, work_job)
    })
    .await
    .map_err(|_| "本地线索扫描未完成。")??;
    limitations.append(&mut notes);
    // Do not send a stale protection snapshot after a settings change.
    if db::settings(&*state.db.lock().map_err(|_| "无法复查保护设置。")?)?.protected_paths
        != protected
    {
        return Err("保护设置在扫描期间发生变化，请重新扫描；本次元数据未交给模型。".into());
    }
    if evidence.is_empty() {
        return Err("本次范围没有可供 AI 分析的线索，请选择其他普通目录。".into());
    }
    {
        let mut store = state.ai_scan.lock().map_err(|_| "无法保留扫描结果。")?;
        if let Some(s) = store.snapshot.as_mut().filter(|s| s.id == id) {
            s.scan_id = scan.as_ref().map(|s| s.id.clone());
            s.expires_at = scan.as_ref().map(|s| s.expires_at);
            s.evidence = evidence.clone();
            s.limitations = limitations.clone();
        }
    }
    stage(state, id, "Agent 正在结合本地线索与模型知识分析…");
    agents::analyze(
        agent_id,
        &executable,
        scratch.path(),
        &evidence,
        &limitations,
        &language,
        job,
    )
    .await
}
