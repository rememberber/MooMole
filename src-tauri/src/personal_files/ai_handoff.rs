use super::*;
use crate::{ai_scan::personal_handoff::Source, engine::Job, observation::AnchoredPath};
use std::sync::Arc;
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Focus {
    snapshot_id: String,
    agent_id: String,
    analyzed_at: i64,
    items: Vec<FocusItem>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FocusItem {
    evidence_id: String,
    item_id: String,
    path: String,
    recommendation: String,
    reason: String,
    impact: String,
}
pub(super) fn collect(source: Source, rules: &[String], job: Arc<Job>) -> Result<Scan, String> {
    source.current()?;
    for choice in &source.choices {
        if job.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("AI 关注项的重新扫描已取消，未处理文件。".into());
        }
        if protected(Path::new(&choice.path), rules)?
            || AnchoredPath::open(Path::new(&choice.path))?.witness != choice.witness
        {
            return Err(
                "AI 关注项在分析后已变化、受保护或无法读取，请重新分析；未自动匹配替代文件。"
                    .into(),
            );
        }
    }
    let mut scan = super::scan::collect(&source.root, rules, job)?;
    if scan.report.cancelled {
        return Err("AI 关注项的重新扫描已取消，未处理文件。".into());
    }
    source.current()?;
    let mut items = vec![];
    for choice in source.choices {
        let current = scan
            .items
            .iter()
            .find(|i| i.path == choice.path && i.witness == choice.witness)
            .ok_or("新扫描未能核对全部 AI 关注项，可能已变化或超出扫描范围；未自动选择文件。")?;
        items.push(FocusItem {
            evidence_id: choice.evidence_id,
            item_id: current.id.clone(),
            path: current.path.clone(),
            recommendation: choice.recommendation,
            reason: choice.reason,
            impact: choice.impact,
        });
    }
    scan.report.ai_focus = Some(Focus {
        snapshot_id: source.snapshot_id,
        agent_id: source.agent_id,
        analyzed_at: source.analyzed_at,
        items,
    });
    Ok(scan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::{
        ai_scan::personal_handoff::{live_owned_source, owned_fixture_source},
        engine::{Job, TaskKind},
    };
    fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let t = tempfile::Builder::new()
            .prefix("MooMole-ai-handoff-owned-")
            .tempdir()
            .unwrap();
        let home = t.path().canonicalize().unwrap();
        let root = home.join("Downloads");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(
            root.join("owned-archive.zip"),
            "synthetic owned archive-shaped file, not a real archive\n",
        )
        .unwrap();
        std::fs::write(
            root.join("keep-notes.txt"),
            "owned notes, must preserve contents\n",
        )
        .unwrap();
        (t, root)
    }
    #[test]
    fn fresh_scan_maps_to_new_ids_without_an_execution_plan_and_changed_files_are_not_substituted()
    {
        let (_t, root) = fixture();
        let source = owned_fixture_source(&root);
        let scan = collect(source, &[], Arc::new(Job::new(TaskKind::PersonalScan))).unwrap();
        let focus = scan.report.ai_focus.as_ref().unwrap();
        assert_eq!(focus.items.len(), 2);
        assert!(focus.items.iter().all(|f| f.item_id != f.evidence_id
            && scan
                .items
                .iter()
                .any(|i| i.id == f.item_id && i.path == f.path)));
        let source = owned_fixture_source(&root);
        std::fs::write(root.join("keep-notes.txt"), "changed after model").unwrap();
        assert!(collect(source, &[], Arc::new(Job::new(TaskKind::PersonalScan))).is_err());
        let source = owned_fixture_source(&root);
        let rules = vec![root.join("keep-notes.txt").to_str().unwrap().into()];
        assert!(collect(source, &rules, Arc::new(Job::new(TaskKind::PersonalScan))).is_err());
        let cancelled = Arc::new(Job::new(TaskKind::PersonalScan));
        cancelled.cancel();
        match collect(owned_fixture_source(&root), &[], cancelled) {
            Err(e) => assert!(e.contains("取消")),
            Ok(_) => panic!("cancelled handoff must not publish a scan"),
        }
    }
    #[tokio::test]
    #[ignore = "one real Codex request with only unique self-created fixture metadata; then explicit production Mole-guarded native move, receipt and database reopen"]
    async fn live_codex_personal_advice_to_fresh_scan_and_native_move() {
        let (t, root) = fixture();
        let home = t.path().canonicalize().unwrap();
        let actual_home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap();
        let (source, model) = live_owned_source(&root, &actual_home).await.unwrap();
        let source_id = source.snapshot_id.clone();
        let source_path = source.choices[0].path.clone();
        let before = std::fs::read(&source_path).unwrap();
        let scan = collect(source, &[], Arc::new(Job::new(TaskKind::PersonalScan))).unwrap();
        let fresh = serde_json::to_value(&scan.report).unwrap();
        let selected = scan.report.ai_focus.as_ref().unwrap().items[0]
            .item_id
            .clone();
        let mut store = Store {
            scan: Some(scan),
            ..Default::default()
        };
        let target = home.join("Organized");
        std::fs::create_dir(&target).unwrap();
        let settings = db::Settings {
            acknowledged: true,
            ..Default::default()
        };
        // This is the user's second explicit selection and confirmation in the
        // native boundary test; the AI IDs never become execution IDs.
        let offer = store
            .offer_move(
                &[selected],
                target.to_str().unwrap(),
                movement::Conflict::Skip,
                &settings,
                db::now(),
            )
            .unwrap();
        let plan = store.take(&offer.id, db::now()).unwrap();
        let encoded = serde_json::to_value(&plan).unwrap();
        assert!(encoded.get("aiFocus").is_none());
        let database = db::open_owned(&home.join("owned.sqlite")).unwrap();
        db::save_settings(&database, &settings).unwrap();
        let database = Arc::new(std::sync::Mutex::new(database));
        let engine = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        let rule_home = home.clone();
        let result = writer::run_with(
            database.clone(),
            home.clone(),
            plan,
            Arc::new(Job::new(TaskKind::PersonalMove)),
            move |requested, job| {
                let h = rule_home.clone();
                let e = engine.clone();
                async move { crate::rules::run(&e, &h, &requested, job).await }
            },
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(
            result.summary.moved_count,
            1,
            "{}",
            serde_json::to_string_pretty(&result).unwrap()
        );
        let receipt = result.items[0].receipt.as_ref().unwrap();
        assert_eq!(std::fs::read(&receipt.destination).unwrap(), before);
        assert!(!Path::new(&source_path).exists());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        let run = result.summary.id.clone();
        let expected = serde_json::to_value(&result).unwrap();
        drop(database);
        let reopened = db::open_owned(&home.join("owned.sqlite")).unwrap();
        assert_eq!(
            serde_json::to_value(crate::execution::detail(&reopened, &run).unwrap()).unwrap(),
            expected
        );
        std::fs::write(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.cache/ai-handoff-native.json"),serde_json::to_vec_pretty(&serde_json::json!({"snapshotId":source_id,"model":model,"freshScan":fresh,"execution":result,"reopenEqual":true})).unwrap()).unwrap();
    }
}
