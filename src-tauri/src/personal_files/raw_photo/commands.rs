use super::*;
use crate::AppState;
struct Registered {
    state: AppState,
    id: String,
}
impl Drop for Registered {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.state.jobs.lock() {
            jobs.remove(&self.id);
        }
    }
}
struct Stop(Arc<Job>);
impl Drop for Stop {
    fn drop(&mut self) {
        self.0.cancel()
    }
}
#[tauri::command]
pub(crate) async fn preview_raw_photo(
    domain: preview::Domain,
    scan_id: String,
    item_id: String,
    request_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<preview::Preview, String> {
    uuid::Uuid::parse_str(&request_id).map_err(|_| "RAW 预览请求标识无效。")?;
    let (item, _, _) = preview::selected_context(&state, domain, &scan_id, &item_id)?;
    let settings = db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?;
    if !settings.acknowledged {
        return Err("请先确认首次使用说明。".into());
    }
    let operation = state.updates.installation.enter()?;
    let slot = state
        .preview_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "上一项内容预览仍在进行，请稍后重试。")?;
    let job = Arc::new(Job::new(TaskKind::PersonalScan));
    {
        let mut jobs = state.jobs.lock().map_err(|e| e.to_string())?;
        if state.shutdown.closing()
            || jobs.len() >= 3
            || jobs.contains_key(&request_id)
            || jobs.values().any(|j| j.kind.exclusive_operation())
        {
            return Err("请先结束当前文件任务，再预览 RAW。".into());
        }
        jobs.insert(request_id.clone(), job.clone());
    }
    let registered = Registered {
        state: state.inner().clone(),
        id: request_id,
    };
    let stop = Stop(job.clone());
    let state = state.inner().clone();
    let mut worker = tauri::async_runtime::spawn_blocking(move || {
        let _resources = (operation, slot, registered);
        objc2::rc::autoreleasepool(|_| {
            let result = analyze(
                &item,
                &settings.protected_paths,
                job,
                Instant::now(),
                1024,
                &mut 0,
            )?;
            let current = preview::selected_context(&state, domain, &scan_id, &item_id)?.0;
            let rules =
                db::settings(&*state.db.lock().map_err(|e| e.to_string())?)?.protected_paths;
            if current.witness != item.witness
                || protected(Path::new(&item.path), &rules)?
                || AnchoredPath::open(Path::new(&item.path))?.witness != item.witness
            {
                return Err("RAW 预览期间来源或保护范围变化，请重新扫描。".into());
            }
            Ok(preview::Preview {
                scan_id,
                item_id,
                path: item.path,
                content: preview::Content::RawImage {
                    data_url: result.png,
                    width: result.width,
                    height: result.height,
                    type_identifier: result.type_identifier,
                },
            })
        })
    });
    let result = tokio::select! {r=&mut worker=>r.map_err(|_|"RAW 预览未取得完整结果。")?,_=tokio::time::sleep(Duration::from_secs(30))=>return Err("RAW 预览等待超时，已请求停止；不会自动重复读取。".into())};
    drop(stop);
    result
}
