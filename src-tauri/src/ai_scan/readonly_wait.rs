//! Bound foreground waits for metadata syscalls that macOS may hold for consent.
//! Only pure local observations belong here; never put model calls, persistence
//! or mutation inside the detached worker. Late values cannot resume analysis.
use crate::{
    engine::{Job, TaskKind},
    update_install::Gate,
};
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::sync::Semaphore;

struct Stop(Arc<Job>);
impl Drop for Stop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub(super) async fn run<T: Send + 'static>(
    gate: Arc<Gate>,
    slots: Arc<Semaphore>,
    job: Arc<Job>,
    limit: Duration,
    observe: impl FnOnce(Arc<Job>) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    if job.cancelled.load(Ordering::SeqCst) {
        return Err("本地读取已取消，本次元数据未交给模型。".into());
    }
    let permit = slots
        .try_acquire_owned()
        .map_err(|_| "仍有本地读取尚未返回，请处理系统访问提示或稍后重试。")?;
    let operation = gate.enter()?;
    let worker_job = Arc::new(Job::new(TaskKind::AiScan));
    let stop = Stop(worker_job.clone());
    let worker = tauri::async_runtime::spawn_blocking(move || {
        // These outlive the foreground timeout/cancel, until the syscall really
        // returns. They prevent unbounded retries and premature self-install.
        let _resources = (permit, operation);
        if worker_job.cancelled.load(Ordering::SeqCst) {
            return Err("本地读取已取消，本次元数据未交给模型。".into());
        }
        observe(worker_job)
    });
    let cancelled = async {
        while !job.cancelled.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    let result = tokio::select! {
        result = worker => result.map_err(|_| "本地只读检查未完成。".to_string())?,
        _ = cancelled => Err("已停止等待本地读取；系统中的只读检查可能仍在返回，本次元数据未交给模型。".into()),
        _ = tokio::time::sleep(limit) => Err("本地目录读取超过等待时限，请检查系统文件访问提示，或选择已授权的目录后重试。本次元数据未交给模型。".into()),
    };
    drop(stop);
    if job.cancelled.load(Ordering::SeqCst) {
        return Err(
            "已停止等待本地读取；系统中的只读检查可能仍在返回，本次元数据未交给模型。".into(),
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicUsize, mpsc};

    #[tokio::test]
    async fn held_read_can_timeout_or_cancel_without_late_continuation_or_releasing_its_guards() {
        for cancel in [false, true] {
            let gate = Arc::new(Gate::default());
            let slots = Arc::new(Semaphore::new(1));
            let job = Arc::new(Job::new(TaskKind::AiScan));
            let (release, held) = mpsc::channel();
            let (started, ready) = tokio::sync::oneshot::channel();
            let calls = Arc::new(AtomicUsize::new(0));
            let after = calls.clone();
            let (g, s, j) = (gate.clone(), slots.clone(), job.clone());
            let task = tokio::spawn(async move {
                let result = run(g, s, j, Duration::from_millis(150), move |token| {
                    started.send(()).unwrap();
                    held.recv().unwrap();
                    assert!(token.cancelled.load(Ordering::SeqCst));
                    Ok(42)
                })
                .await?;
                after.fetch_add(1, Ordering::SeqCst);
                Ok::<_, String>(result)
            });
            tokio::time::timeout(Duration::from_secs(3), ready)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(slots.available_permits(), 0);
            assert!(gate.begin().is_err());
            if cancel {
                job.cancel();
            }
            let error = tokio::time::timeout(Duration::from_secs(3), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err();
            assert!(error.contains(if cancel {
                "停止等待"
            } else {
                "等待时限"
            }));
            assert_eq!(slots.available_permits(), 0);
            assert!(gate.begin().is_err());
            assert!(run(
                gate.clone(),
                slots.clone(),
                Arc::new(Job::new(TaskKind::AiScan)),
                Duration::from_secs(1),
                |_| -> Result<(), String> { panic!("a held read must not admit another worker") }
            )
            .await
            .is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            release.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                while slots.available_permits() != 1 || gate.begin().is_err() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn completed_and_pre_cancelled_reads_keep_the_model_boundary() {
        let gate = Arc::new(Gate::default());
        let slots = Arc::new(Semaphore::new(1));
        let job = Arc::new(Job::new(TaskKind::AiScan));
        assert_eq!(
            run(
                gate.clone(),
                slots.clone(),
                job.clone(),
                Duration::from_secs(1),
                |_| Ok(7)
            )
            .await
            .unwrap(),
            7
        );
        assert_eq!(slots.available_permits(), 1);
        job.cancel();
        assert!(run(
            gate.clone(),
            slots.clone(),
            job,
            Duration::from_secs(1),
            |_| -> Result<(), String> { panic!("pre-cancelled read ran") }
        )
        .await
        .is_err());
        assert!(gate.begin().is_ok());
    }
}
