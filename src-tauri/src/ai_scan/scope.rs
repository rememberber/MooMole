//! Source admission before building any model inventory.
use super::{inventory, Evidence};
use crate::{engine::Job, review::Scan};
use std::{future::Future, path::Path, sync::Arc};
pub(super) async fn optional_mole<T, F, U>(include: bool, run: F) -> Option<T>
where
    F: FnOnce() -> U,
    U: Future<Output = T>,
{
    if include {
        Some(run().await)
    } else {
        None
    }
}
pub(super) fn collect(
    include: bool,
    scan: Option<&Scan>,
    root: &Path,
    rules: &[String],
    job: Arc<Job>,
) -> Result<(Vec<Evidence>, Vec<String>), String> {
    if !include && scan.is_some() {
        return Err("仅所选目录模式不能接收 Mole 全账户候选，未向模型发送。".into());
    }
    let (evidence, mut notes) = inventory::collect(scan, root, rules, job)?;
    if !include {
        if evidence.iter().any(|e| {
            e.source != "metadata"
                || e.candidate_id.is_some()
                || !Path::new(&e.path).starts_with(root)
                || Path::new(&e.path) == root
        }) {
            return Err("线索超出仅所选目录的分析范围，未向模型发送。".into());
        }
        notes.insert(
            0,
            "本次仅分析所选目录的有界文件元数据，未运行 Mole 常用清理位置扫描；不是全盘检查。"
                .into(),
        );
    }
    Ok((evidence, notes))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine::TaskKind, review};
    #[tokio::test]
    async fn folder_mode_never_invokes_the_mole_scan_and_combined_mode_invokes_it_once() {
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let none = optional_mole(false, || async {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            42
        })
        .await;
        assert_eq!(none, None);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let some = optional_mole(true, || async {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            42
        })
        .await;
        assert_eq!(some, Some(42));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    #[test]
    fn folder_mode_collects_only_owned_root_metadata_and_rejects_even_an_empty_mole_scan() {
        let t = tempfile::Builder::new()
            .prefix("MooMole-ai-scope-owned-")
            .tempdir()
            .unwrap();
        let home = t.path().canonicalize().unwrap();
        let root = home.join("Selected");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("owned.txt"), "owned").unwrap();
        std::fs::write(home.join("outside-private.txt"), "outside").unwrap();
        std::os::unix::fs::symlink(home.join("outside-private.txt"), root.join("alias")).unwrap();
        let job = || Arc::new(Job::new(TaskKind::AiScan));
        let (items, notes) = collect(false, None, &root, &[], job()).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].path.ends_with("Selected/owned.txt"));
        assert!(notes[0].contains("未运行 Mole"));
        let fake = review::Scan {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: 0,
            expires_at: 1,
            revision: crate::engine::REVISION.into(),
            protocol: crate::clean_preview::PROTOCOL,
            system_included: false,
            items: vec![],
        };
        assert!(collect(false, Some(&fake), &root, &[], job()).is_err());
    }
}
