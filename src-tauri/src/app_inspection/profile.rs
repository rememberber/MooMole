//! Explicit read-only diagnostics only; no application paths or IDs in reports.
use super::*;
use std::{cell::RefCell, time::Instant};

thread_local! {
    static CAPTURE: RefCell<Option<(Instant, Option<&'static str>, Vec<(&'static str, Duration)>)>> = const { RefCell::new(None) };
}
pub(super) fn checkpoint(phase: &'static str) {
    CAPTURE.with_borrow_mut(|capture| {
        if let Some((last, previous, phases)) = capture {
            let now = Instant::now();
            assert!(phases.len() < 32);
            if let Some(previous) = previous {
                phases.push((*previous, now.duration_since(*last)));
            }
            *previous = Some(phase);
            *last = now;
        }
    });
}
fn capture<T>(work: impl FnOnce() -> T) -> (T, Vec<(&'static str, Duration)>) {
    CAPTURE.with_borrow_mut(|value| {
        assert!(value.is_none());
        *value = Some((Instant::now(), None, vec![]));
    });
    let result = work();
    checkpoint("Finished");
    let phases = CAPTURE.with_borrow_mut(|value| value.take().unwrap().2);
    (result, phases)
}

#[cfg(target_os = "macos")]
async fn installed(path: &'static str, id: &'static str, name: &'static str) {
    let home = std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME required"));
    let engine = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
    assert!(
        std::path::Path::new(path).is_dir(),
        "explicit diagnostic target is absent"
    );
    let list = crate::engine::run(
        &engine,
        &home,
        Arc::new(crate::engine::Job::new(crate::engine::TaskKind::Apps)),
        None,
        &[],
    )
    .await
    .expect("pinned Mole application list required");
    let index = list
        .as_array()
        .unwrap()
        .iter()
        .position(|row| row["path"] == path)
        .expect("target must be present in Mole list");
    let mut cache = crate::tasks::Results::default();
    let task_id = uuid::Uuid::new_v4().to_string();
    cache.remember(&crate::tasks::Snapshot {
        id: task_id.clone(),
        kind: "apps".into(),
        state: "succeeded".into(),
        result: Some(list),
        error: None,
        result_unavailable: None,
    });
    let (bound_path, bound_id, bound_name) = cache.app_target(&task_id, index).unwrap();
    assert_eq!(bound_path, path);
    assert_eq!(bound_id, id);
    assert_eq!(bound_name.as_deref(), Some(name));
    let candidates = cache.app_candidates(&task_id).unwrap();
    eprintln!(
        "inspection profile source=moleList candidateCount={}",
        candidates.len()
    );
    for attempt in 1..=3 {
        let home = home.clone();
        let engine = engine.clone();
        let candidates = candidates.clone();
        let started = Instant::now();
        let (result, phases) = tokio::task::spawn_blocking(move || {
            capture(|| {
                inspect_worker(
                    path,
                    id,
                    Some(name),
                    Some(candidates),
                    &engine,
                    &home,
                    &Progress::default(),
                )
            })
        })
        .await
        .unwrap();
        let result = result.expect("read-only inspection failed");
        eprintln!(
            "inspection profile attempt={attempt} workerAndQueueMs={}",
            started.elapsed().as_millis()
        );
        for (phase, elapsed) in phases {
            eprintln!("  {phase}: {}us", elapsed.as_micros());
        }
        let value = serde_json::to_value(result).unwrap();
        eprintln!(
            "  scopes: associations={} embedded={} installations={} receipts={}",
            value["associations"]["code"],
            value["embedded"]["code"],
            value["installations"]["code"],
            value["installations"]["receipts"]["code"]
        );
    }
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "explicit real read-only profile: installed Code, HOME metadata and system discovery; no cache purge or permission changes"]
async fn installed_code_read_only_profile() {
    installed(
        "/Applications/Visual Studio Code.app",
        "com.microsoft.VSCode",
        "Code",
    )
    .await;
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "explicit real read-only profile: installed MooTool, HOME metadata and system discovery; no cache purge or permission changes"]
async fn installed_mootool_read_only_profile() {
    installed(
        "/Applications/MooTool Next Electron.app",
        "com.rememberber.mootool.next.electron",
        "MooTool",
    )
    .await;
}
