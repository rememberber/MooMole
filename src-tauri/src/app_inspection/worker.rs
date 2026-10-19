//! Per-request progress and cooperative stopping; never interrupts an OS read.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

#[derive(Clone, Copy)]
pub(super) enum Phase {
    Queued,
    Rules,
    Identity,
    Background,
    Embedded,
    Associations,
    ExtensionData,
    Roots,
    Volumes,
    Receipts,
    Copies,
    Running,
    VerifyBackground,
    VerifyExtensionData,
    VerifyEmbedded,
    VerifyAssociations,
    VerifyCopies,
    VerifyRoots,
    VerifyVolumes,
    VerifyReceipts,
    VerifyIdentity,
    PathProtection,
    ComponentRules,
    Done,
}
impl Phase {
    fn description(self) -> &'static str {
        match self {
            Self::Queued => "等待检查开始",
            Self::Rules | Self::PathProtection | Self::ComponentRules => "核对保护规则",
            Self::Identity | Self::VerifyIdentity => "核对应用身份",
            Self::Background | Self::VerifyBackground => "核对后台应用条件",
            Self::Embedded | Self::VerifyEmbedded => "核对嵌入组件",
            Self::Associations | Self::VerifyAssociations => "核对关联位置",
            Self::ExtensionData | Self::VerifyExtensionData => "核对扩展数据",
            Self::Roots | Self::VerifyRoots => "核对常用安装目录",
            Self::Volumes | Self::VerifyVolumes => "核对本地卷",
            Self::Receipts | Self::VerifyReceipts => "核对安装收据",
            Self::Copies | Self::VerifyCopies => "核对安装副本",
            Self::Running => "核对主应用运行状态",
            Self::Done => "等待检查结果",
        }
    }
}
const STOPPED: &str = "应用状态检查已停止，结果无法确认；请稍后重试。";
#[derive(Default)]
pub(super) struct Progress {
    phase: AtomicU8,
    stopped: AtomicBool,
}
impl Progress {
    pub(super) fn enter(&self, phase: Phase) -> Result<(), String> {
        if self.stopped.load(Ordering::SeqCst) {
            return Err(STOPPED.into());
        }
        self.phase.store(phase as u8, Ordering::SeqCst);
        // Check again after publishing the phase, before the next operation.
        self.check()
    }
    fn check(&self) -> Result<(), String> {
        if self.stopped.load(Ordering::SeqCst) {
            Err(STOPPED.into())
        } else {
            Ok(())
        }
    }
    fn stop(&self) -> String {
        self.stopped.store(true, Ordering::SeqCst);
        const PHASES: [Phase; 24] = [
            Phase::Queued,
            Phase::Rules,
            Phase::Identity,
            Phase::Background,
            Phase::Embedded,
            Phase::Associations,
            Phase::ExtensionData,
            Phase::Roots,
            Phase::Volumes,
            Phase::Receipts,
            Phase::Copies,
            Phase::Running,
            Phase::VerifyBackground,
            Phase::VerifyExtensionData,
            Phase::VerifyEmbedded,
            Phase::VerifyAssociations,
            Phase::VerifyCopies,
            Phase::VerifyRoots,
            Phase::VerifyVolumes,
            Phase::VerifyReceipts,
            Phase::VerifyIdentity,
            Phase::PathProtection,
            Phase::ComponentRules,
            Phase::Done,
        ];
        let phase = PHASES
            .get(self.phase.load(Ordering::SeqCst) as usize)
            .copied()
            .unwrap_or(Phase::Queued);
        format!(
            "应用状态读取超时（{}），结果无法确认；请稍后重试。",
            phase.description()
        )
    }
}

pub(super) async fn run<T: Send + 'static>(
    slots: Arc<Semaphore>,
    wait: Duration,
    work: impl FnOnce(&Progress) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    run_until(slots, tokio::time::sleep(wait), work).await
}
async fn run_until<T: Send + 'static>(
    slots: Arc<Semaphore>,
    deadline: impl std::future::Future<Output = ()>,
    work: impl FnOnce(&Progress) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let slot = slots
        .try_acquire_owned()
        .map_err(|_| "已有应用状态检查正在进行，请稍后重试。")?;
    let progress = Arc::new(Progress::default());
    let active = progress.clone();
    let mut worker = tokio::task::spawn_blocking(move || {
        let _slot = slot;
        active.check()?;
        work(&active)
    });
    tokio::select! {
        biased;
        joined = &mut worker => joined.map_err(|_| "应用状态读取未完成，请重试。")?,
        _ = deadline => Err(progress.stop()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicUsize, mpsc};
    use tokio::sync::oneshot;

    async fn idle(slots: &Semaphore, capacity: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while slots.available_permits() != capacity {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn success_error_and_panic_release_the_actual_worker_slot() {
        let slots = Arc::new(Semaphore::new(1));
        assert_eq!(
            run(slots.clone(), Duration::from_secs(2), |progress| {
                progress.enter(Phase::Rules)?;
                Ok(42)
            })
            .await
            .unwrap(),
            42
        );
        idle(&slots, 1).await;
        assert_eq!(
            run::<()>(slots.clone(), Duration::from_secs(2), |_| Err(
                "unknown".into()
            ))
            .await
            .unwrap_err(),
            "unknown"
        );
        idle(&slots, 1).await;
        let error = run::<()>(slots.clone(), Duration::from_secs(2), |_| panic!("fixture"))
            .await
            .unwrap_err();
        assert_eq!(error, "应用状态读取未完成，请重试。");
        idle(&slots, 1).await;
    }
    #[tokio::test]
    async fn timeout_keeps_slot_until_os_work_ends_then_stops_remaining_phases() {
        let slots = Arc::new(Semaphore::new(1));
        let (started_tx, started_rx) = oneshot::channel();
        let (stop_tx, stop_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (done_tx, done_rx) = oneshot::channel();
        let task = tokio::spawn(run_until::<()>(
            slots.clone(),
            async {
                let _ = stop_rx.await;
            },
            move |progress| {
                progress.enter(Phase::Receipts)?;
                let _ = started_tx.send(());
                let _ = release_rx.recv();
                let next = progress.enter(Phase::Copies);
                let _ = done_tx.send(next.is_err());
                next?;
                panic!("later query must not start");
            },
        ));
        started_rx.await.unwrap();
        stop_tx.send(()).unwrap();
        let error = task.await.unwrap().unwrap_err();
        assert_eq!(
            error,
            "应用状态读取超时（核对安装收据），结果无法确认；请稍后重试。"
        );
        assert_eq!(slots.available_permits(), 0);
        let busy = run(slots.clone(), Duration::from_secs(2), |_| Ok(()))
            .await
            .unwrap_err();
        assert_eq!(busy, "已有应用状态检查正在进行，请稍后重试。");
        release_tx.send(()).unwrap();
        assert!(done_rx.await.unwrap());
        idle(&slots, 1).await;
        assert!(run(slots, Duration::from_secs(2), |_| Ok(())).await.is_ok());
    }
    #[test]
    fn queued_timeout_does_not_start_inspection_or_release_its_slot_early() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        runtime.block_on(async {
            let slots = Arc::new(Semaphore::new(1));
            let (ready_tx, ready_rx) = oneshot::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let blocker = tokio::task::spawn_blocking(move || {
                let _ = ready_tx.send(());
                let _ = release_rx.recv();
            });
            ready_rx.await.unwrap();
            let (stop_tx, stop_rx) = oneshot::channel();
            let calls = Arc::new(AtomicUsize::new(0));
            let observed = calls.clone();
            let task = tokio::spawn(run_until::<()>(
                slots.clone(),
                async {
                    let _ = stop_rx.await;
                },
                move |_| {
                    observed.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            ));
            idle(&slots, 0).await;
            stop_tx.send(()).unwrap();
            assert!(task.await.unwrap().unwrap_err().contains("等待检查开始"));
            assert_eq!(slots.available_permits(), 0);
            release_tx.send(()).unwrap();
            blocker.await.unwrap();
            idle(&slots, 1).await;
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        });
    }
    #[tokio::test]
    async fn concurrent_requests_keep_independent_progress_and_stop_flags() {
        let slots = Arc::new(Semaphore::new(2));
        let mut tasks = vec![];
        let mut releases = vec![];
        for phase in [Phase::Associations, Phase::Running] {
            let (ready_tx, ready_rx) = oneshot::channel();
            let (stop_tx, stop_rx) = oneshot::channel();
            let (release_tx, release_rx) = mpsc::channel();
            tasks.push(tokio::spawn(run_until::<()>(
                slots.clone(),
                async {
                    let _ = stop_rx.await;
                },
                move |progress| {
                    progress.enter(phase)?;
                    let _ = ready_tx.send(());
                    let _ = release_rx.recv();
                    progress.enter(Phase::Done)
                },
            )));
            ready_rx.await.unwrap();
            releases.push(release_tx);
            stop_tx.send(()).unwrap();
        }
        let first = tasks.remove(0).await.unwrap().unwrap_err();
        let second = tasks.remove(0).await.unwrap().unwrap_err();
        assert!(first.contains("核对关联位置"));
        assert!(second.contains("核对主应用运行状态"));
        assert_eq!(slots.available_permits(), 0);
        for release in releases {
            release.send(()).unwrap();
        }
        idle(&slots, 2).await;
    }
    #[test]
    fn phase_descriptions_are_static_and_all_checkpoints_reject_after_stop() {
        // Every declared discriminant must have its own matching description.
        for phase in [
            Phase::Queued,
            Phase::Rules,
            Phase::Identity,
            Phase::Background,
            Phase::Embedded,
            Phase::Associations,
            Phase::ExtensionData,
            Phase::Roots,
            Phase::Volumes,
            Phase::Receipts,
            Phase::Copies,
            Phase::Running,
            Phase::VerifyBackground,
            Phase::VerifyExtensionData,
            Phase::VerifyEmbedded,
            Phase::VerifyAssociations,
            Phase::VerifyCopies,
            Phase::VerifyRoots,
            Phase::VerifyVolumes,
            Phase::VerifyReceipts,
            Phase::VerifyIdentity,
            Phase::PathProtection,
            Phase::ComponentRules,
            Phase::Done,
        ] {
            let progress = Progress::default();
            progress.enter(phase).unwrap();
            assert_eq!(
                progress.stop(),
                format!(
                    "应用状态读取超时（{}），结果无法确认；请稍后重试。",
                    phase.description()
                )
            );
            assert_eq!(progress.enter(Phase::Done).unwrap_err(), STOPPED);
        }
    }
}
