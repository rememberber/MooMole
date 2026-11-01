use super::{journal, plan::Authorized, runner, terminal};
use crate::{
    db,
    engine::{Job, TaskKind},
    AppState,
};
use serde::Serialize;
use std::sync::{atomic::Ordering, Arc};
#[tauri::command]
pub(crate) async fn prepare_mole_cleanup(
    system: bool,
    state: tauri::State<'_, AppState>,
) -> Result<super::plan::Plan, String> {
    if state
        .jobs
        .lock()
        .map_err(|_| "无法核对任务。")?
        .values()
        .any(|j| j.kind.exclusive_operation())
    {
        return Err("请先完成当前修改或权限检查。".into());
    }
    let operation = state.updates.installation.enter()?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        {
            let db = state.db.lock().map_err(|_| "无法核对清理准入。")?;
            let t = db.unchecked_transaction().map_err(|e| e.to_string())?;
            crate::execution_admission::reserve(&t, journal::RESERVED_BYTES)?;
            t.rollback().map_err(|e| e.to_string())?;
        }
        let a = Authorized::prepare(&state, system)?;
        let mut plan = state
            .mole_clean
            .plan
            .lock()
            .map_err(|_| "无法保存完整清理确认。")?;
        let view = a.view.clone();
        *plan = Some(a);
        Ok(view)
    })
    .await
    .map_err(|_| "完整清理准备中断，未执行。".to_string())?
}
#[tauri::command]
pub(crate) fn discard_mole_cleanup(
    plan_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut p = state
        .mole_clean
        .plan
        .lock()
        .map_err(|_| "无法撤销完整清理确认。")?;
    if p.as_ref().is_some_and(|a| a.view.id == plan_id) {
        *p = None;
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn execute_mole_cleanup(
    plan_id: String,
    request_id: String,
    permanent_confirmed: bool,
    trash_confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    uuid::Uuid::parse_str(&request_id).map_err(|_| "完整清理请求无效。")?;
    if !permanent_confirmed || !trash_confirmed {
        return Err("请明确确认永久删除和废纸篓处理范围。".into());
    }
    let operation = state.updates.installation.enter()?;
    let job = Arc::new(Job::new(TaskKind::MoleClean));
    let a = {
        let mut plan = state
            .mole_clean
            .plan
            .lock()
            .map_err(|_| "无法读取完整清理确认。")?;
        let a = plan.take().ok_or("完整清理确认已撤销或使用。")?;
        if a.view.id != plan_id {
            return Err("完整清理确认不匹配，未执行。".into());
        }
        a.validate(&state)?;
        // Match start_task's database -> jobs lock order.
        let mut db = state.db.lock().map_err(|_| "无法保存完整清理意图。")?;
        let mut jobs = state.jobs.lock().map_err(|_| "无法登记完整清理任务。")?;
        if state.shutdown.closing() || !jobs.is_empty() {
            return Err("请等待当前任务结束后再开始完整清理。".into());
        }
        if journal::read(&db, &request_id)?.is_some() {
            return Err("此清理请求已有记录，请查看原结果，不要重复执行。".into());
        }
        journal::create(&mut db, &request_id, &a.view, None)?;
        jobs.insert(request_id.clone(), job.clone());
        a
    };
    let state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let _operation = operation;
        let work =
            tauri::async_runtime::spawn(runner::run(state.clone(), request_id.clone(), a, job));
        let result = work
            .await
            .map_err(|_| "完整清理执行中断，请核对已保存的意图与结果，不要自动重试。".to_string())
            .and_then(|r| r);
        if let Err(e) = result {
            if let Ok(db) = state.db.lock() {
                let _ = journal::finish(&db, &request_id, "uncertain", None, Some(&e));
            }
        }
        state
            .mole_clean
            .authorization
            .store(false, Ordering::SeqCst);
        *state
            .mole_clean
            .input
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        state
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&request_id);
    });
    Ok(())
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    version: u32,
    active_request: Option<String>,
    authorization_required: bool,
    records: Vec<journal::Summary>,
    current: Option<journal::Record>,
}
#[tauri::command]
pub(crate) fn get_mole_cleanup(
    request_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Snapshot, String> {
    if let Some(id) = &request_id {
        uuid::Uuid::parse_str(id).map_err(|_| "清理记录标识无效。")?;
    }
    let db = state.db.lock().map_err(|_| "无法读取完整清理记录。")?;
    let jobs = state.jobs.lock().map_err(|_| "无法读取清理进度。")?;
    let active_request = jobs
        .iter()
        .find(|(_, j)| j.kind == TaskKind::MoleClean)
        .map(|(id, _)| id.clone());
    let records = journal::list(&db)?;
    let selected = active_request
        .as_ref()
        .or(request_id.as_ref())
        .or_else(|| records.first().map(|r| &r.id));
    let current = selected
        .map(|id| journal::read(&db, id))
        .transpose()?
        .flatten();
    Ok(Snapshot {
        version: 1,
        active_request,
        authorization_required: state.mole_clean.authorization.load(Ordering::SeqCst),
        records,
        current,
    })
}
#[tauri::command]
pub(crate) fn cancel_mole_cleanup(
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let jobs = state.jobs.lock().map_err(|_| "无法读取完整清理任务。")?;
    if let Some(j) = jobs
        .get(&request_id)
        .filter(|j| j.kind == TaskKind::MoleClean)
    {
        j.cancel();
    }
    Ok(())
}
#[tauri::command]
pub(crate) fn authorize_mole_cleanup(
    request_id: String,
    password: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if !terminal::valid(&password) {
        return Err("授权输入无效。".into());
    }
    if !state.mole_clean.authorization.load(Ordering::SeqCst) {
        return Err("当前没有等待中的清理授权。".into());
    }
    let jobs = state.jobs.lock().map_err(|_| "无法核对授权任务。")?;
    if !jobs
        .get(&request_id)
        .is_some_and(|j| j.kind == TaskKind::MoleClean && !j.cancelled.load(Ordering::SeqCst))
    {
        return Err("授权任务已结束。".into());
    }
    let mut input = state
        .mole_clean
        .input
        .lock()
        .map_err(|_| "授权输入已结束。")?;
    let (_, file) = input
        .as_mut()
        .filter(|(id, _)| id == &request_id)
        .ok_or("授权输入与当前任务不匹配。")?;
    state
        .mole_clean
        .authorization
        .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| "此授权输入已提交，请等待下次提示。")?;
    let result = terminal::submit(file, password);
    if result.is_err() {
        if let Some(j) = jobs.get(&request_id) {
            j.cancel();
        }
    }
    result
}
#[tauri::command]
pub(crate) fn acknowledge_mole_cleanup(
    request_id: String,
    last_seq: i64,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|_| "无法保存清理核查。")?;
    journal::acknowledge(&db, &request_id, last_seq, db::now())
}
#[tauri::command]
pub(crate) fn export_mole_cleanup(
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let db = state.db.lock().map_err(|_| "无法导出清理记录。")?;
    let mut record = journal::read(&db, &request_id)?.ok_or("清理记录不存在。")?;
    if record.state == "running" {
        return Err("请等待本次清理结束后再导出完整记录。".into());
    }
    let mut s = db
        .prepare(
            "SELECT seq,kind,path,detail,token FROM mole_clean_events WHERE run_id=?1 ORDER BY seq",
        )
        .map_err(|e| e.to_string())?;
    record.events = s
        .query_map([&request_id], |r| {
            Ok(journal::Event {
                seq: r.get(0)?,
                kind: r.get(1)?,
                path: r.get(2)?,
                detail: r.get(3)?,
                token: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    record.events_truncated = false;
    serde_json::to_string_pretty(&record).map_err(|e| e.to_string())
}
#[tauri::command]
pub(crate) fn clear_mole_cleanup_record(
    request_id: String,
    last_seq: i64,
    clear_confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if !clear_confirmed {
        return Err("请明确确认清除已核查的清理记录。".into());
    }
    let mut db = state.db.lock().map_err(|_| "无法清除清理记录。")?;
    journal::remove_reviewed(&mut db, &request_id, last_seq)
}
#[tauri::command]
pub(crate) async fn save_mole_cleanup_export(
    request_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    use std::io::Write;
    use tauri_plugin_dialog::DialogExt;
    let operation = state.updates.installation.enter()?;
    let report = export_mole_cleanup(request_id.clone(), state)?;
    let (send, receive) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("保存完整清理记录 / Save cleanup record")
        .set_file_name(format!("MooMole-clean-{request_id}.json"))
        .add_filter("JSON", &["json"])
        .save_file(move |p| {
            let _ = send.send(p);
        });
    let Some(path) = receive
        .await
        .map_err(|_| "保存窗口未返回，原记录继续保留。")?
    else {
        return Ok(false);
    };
    let path = path.into_path().map_err(|_| "请选择本机文件。")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        if !path.is_absolute()
            || !path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("json"))
        {
            return Err("请选择绝对路径下的 JSON 文件。".to_string());
        }
        let mut file = tempfile::NamedTempFile::new_in(path.parent().ok_or("保存目录无效。")?)
            .map_err(|e| e.to_string())?;
        file.write_all(report.as_bytes())
            .and_then(|_| file.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        file.persist_noclobber(&path)
            .map_err(|_| "保存位置已存在或无法写入，未覆盖；请另选文件名。".to_string())?;
        Ok(true)
    })
    .await
    .map_err(|_| "记录保存中断，请核对文件；原历史继续保留。".to_string())?
}
