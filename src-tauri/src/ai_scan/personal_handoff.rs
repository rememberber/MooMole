//! Resolve advisor IDs into a read-only rescan source. No mutation authority.
use super::*;
use crate::observation::Witness;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
#[derive(Clone)]
pub(crate) struct Choice {
    pub evidence_id: String,
    pub path: String,
    pub witness: Witness,
    pub recommendation: String,
    pub reason: String,
    pub impact: String,
}
pub(crate) struct Source {
    pub snapshot_id: String,
    pub agent_id: String,
    pub root: PathBuf,
    pub analyzed_at: i64,
    pub choices: Vec<Choice>,
    expires_at: i64,
    issued: Instant,
}
impl Source {
    pub(crate) fn current(&self) -> Result<(), String> {
        let now = db::now();
        if now < self.analyzed_at
            || now < self.expires_at - review::LIFETIME_MS
            || now >= self.expires_at
            || self.issued.elapsed() >= Duration::from_millis(review::LIFETIME_MS as u64)
        {
            return Err("AI 个人文件线索已过期，请重新分析；不会据旧建议处理。".into());
        }
        Ok(())
    }
}
pub(super) fn resolve(
    s: &Snapshot,
    request: &str,
    ids: &[String],
    rules: &[String],
) -> Result<Source, String> {
    if s.id != request
        || s.state != "succeeded"
        || s.report.is_none()
        || ids.is_empty()
        || ids.len() > 20
        || ids.iter().collect::<HashSet<_>>().len() != ids.len()
    {
        return Err("请选择本次 AI 报告中的个人文件关注项，每次最多 20 项。".into());
    }
    let mut choices = vec![];
    for id in ids {
        let e = s
            .evidence
            .iter()
            .find(|e| &e.id == id)
            .ok_or("AI 关注项不属于当前报告。")?;
        let f = s
            .report
            .as_ref()
            .unwrap()
            .findings
            .iter()
            .find(|f| &f.evidence_id == id)
            .ok_or("本项没有本次模型说明，未带入文件整理。")?;
        let p = Path::new(&e.path);
        let folded = PathBuf::from(e.path.to_lowercase());
        if e.source != "metadata"
            || !e.personal_file
            || e.is_directory
            || e.candidate_id.is_some()
            || !crate::personal_files::ordinary(p)
            || !p.starts_with(&s.root)
            || p == Path::new(&s.root)
            || rules.iter().any(|r| {
                let r = PathBuf::from(r.to_lowercase());
                folded.starts_with(&r) || r.starts_with(&folded)
            })
        {
            return Err("关注项已受保护或不属于可重新检查的普通个人文件，未带入整理。".into());
        }
        let witness = e
            .witness
            .clone()
            .ok_or("个人文件线索缺少原始身份，请重新分析。")?;
        choices.push(Choice {
            evidence_id: id.clone(),
            path: e.path.clone(),
            witness,
            recommendation: match f.recommendation {
                Recommendation::Keep => "keep",
                Recommendation::Investigate => "investigate",
                Recommendation::Review => return Err("个人文件线索不能作为 Mole 清理建议。".into()),
            }
            .into(),
            reason: f.reason.clone(),
            impact: f.impact.clone(),
        });
    }
    let source = Source {
        snapshot_id: s.id.clone(),
        agent_id: s.agent_id.clone(),
        root: PathBuf::from(&s.root),
        analyzed_at: s.finished_at.ok_or("AI 报告尚未完成。")?,
        choices,
        expires_at: s
            .personal_expires_at
            .ok_or("这份报告没有可用的个人文件线索，请重新分析。")?,
        issued: s.personal_issued.ok_or("个人文件线索时间无法核对。")?,
    };
    source.current()?;
    Ok(source)
}
impl Store {
    pub(crate) fn personal_source(
        &mut self,
        request: &str,
        ids: &[String],
        rules: &[String],
    ) -> Result<Source, String> {
        let s = self.current().ok_or("AI 报告已撤下或失效，请重新分析。")?;
        resolve(&s, request, ids, rules)
    }
}
#[cfg(test)]
fn snapshot(
    root: &Path,
    evidence: Vec<Evidence>,
    report: Report,
    issued: Instant,
    created_at: i64,
) -> Snapshot {
    Snapshot {
        id: uuid::Uuid::new_v4().to_string(),
        agent_id: "codex".into(),
        include_mole: false,
        root: root.to_str().unwrap().into(),
        state: "succeeded".into(),
        stage: "test".into(),
        started_at: created_at,
        finished_at: Some(db::now()),
        scan_id: None,
        expires_at: None,
        personal_expires_at: Some(created_at + review::LIFETIME_MS),
        personal_issued: Some(issued),
        evidence,
        limitations: vec![],
        report: Some(report),
        error: None,
    }
}
#[cfg(test)]
pub(crate) fn owned_fixture_source(root: &Path) -> Source {
    let (evidence, _) = inventory::collect(
        None,
        root,
        &[],
        std::sync::Arc::new(crate::engine::Job::new(crate::engine::TaskKind::AiScan)),
    )
    .unwrap();
    let findings = evidence
        .iter()
        .filter(|e| e.personal_file)
        .map(|e| Finding {
            evidence_id: e.id.clone(),
            recommendation: Recommendation::Keep,
            reason: "Owned fixture; retain personal content".into(),
            impact: "Metadata cannot establish backup status".into(),
        })
        .collect::<Vec<_>>();
    let selected = findings
        .iter()
        .take(20)
        .map(|f| f.evidence_id.clone())
        .collect::<Vec<_>>();
    let s = snapshot(
        root,
        evidence,
        Report {
            summary: "Owned deterministic fixture; not a model response".into(),
            findings,
            limitations: vec![],
        },
        Instant::now(),
        db::now(),
    );
    resolve(&s, &s.id, &selected, &[]).unwrap()
}
#[cfg(test)]
pub(crate) async fn live_owned_source(
    root: &Path,
    home: &Path,
) -> Result<(Source, serde_json::Value), String> {
    if !root.to_string_lossy().contains("MooMole-ai-handoff-owned-") {
        return Err("Live acceptance requires the unique owned fixture".into());
    }
    let job = std::sync::Arc::new(crate::engine::Job::new(crate::engine::TaskKind::AiScan));
    let scratch = tempfile::Builder::new()
        .prefix("MooMole-ai-handoff-agent-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let executable = agents::prepare("codex", home, scratch.path(), job.clone()).await?;
    let (evidence, limits) = super::scope::collect(false, None, root, &[], job.clone())?;
    let issued = Instant::now();
    let created_at = db::now();
    let wire = serde_json::to_value(&evidence).map_err(|e| e.to_string())?;
    assert!(wire
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e.get("witness").is_none()));
    let report = agents::analyze(
        "codex",
        &executable,
        scratch.path(),
        &evidence,
        &limits,
        "en",
        job,
    )
    .await?;
    let s = snapshot(root, evidence, report, issued, created_at);
    let selected = s
        .report
        .as_ref()
        .unwrap()
        .findings
        .iter()
        .filter(|f| {
            s.evidence
                .iter()
                .any(|e| e.id == f.evidence_id && e.personal_file)
        })
        .take(1)
        .map(|f| f.evidence_id.clone())
        .collect::<Vec<_>>();
    let source = resolve(&s, &s.id, &selected, &[])?;
    Ok((
        source,
        serde_json::json!({"sentEvidence":wire,"report":s.report}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Job, TaskKind};
    use std::sync::Arc;
    #[test]
    fn only_retained_personal_evidence_with_current_report_identity_can_be_handed_off() {
        let dir = tempfile::Builder::new()
            .prefix("MooMole-ai-handoff-owned-")
            .tempdir()
            .unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("owned.txt"), "own").unwrap();
        let (evidence, _) =
            inventory::collect(None, &root, &[], Arc::new(Job::new(TaskKind::AiScan))).unwrap();
        let report = Report {
            summary: "Keep owned data".into(),
            findings: vec![Finding {
                evidence_id: evidence[0].id.clone(),
                recommendation: Recommendation::Keep,
                reason: "Personal".into(),
                impact: "No backup proof".into(),
            }],
            limitations: vec![],
        };
        let s = snapshot(&root, evidence, report, Instant::now(), db::now());
        let ids = vec![s.evidence[0].id.clone()];
        assert!(resolve(&s, &s.id, &ids, &[]).is_ok());
        for wrong in [
            vec![],
            vec!["invented".into()],
            vec![ids[0].clone(), ids[0].clone()],
        ] {
            assert!(resolve(&s, &s.id, &wrong, &[]).is_err());
        }
        assert!(resolve(&s, "wrong snapshot", &ids, &[]).is_err());
        assert!(resolve(
            &s,
            &s.id,
            &ids,
            &[root.join("owned.txt").to_str().unwrap().to_uppercase()]
        )
        .is_err());
        let mut bad = s.clone();
        bad.state = "cancelled".into();
        assert!(resolve(&bad, &bad.id, &ids, &[]).is_err());
        let mut bad = s.clone();
        bad.personal_issued = Some(Instant::now() - Duration::from_secs(901));
        assert!(resolve(&bad, &bad.id, &ids, &[]).is_err());
        let mut bad = s.clone();
        bad.personal_expires_at = Some(db::now() + review::LIFETIME_MS + 30_000);
        assert!(resolve(&bad, &bad.id, &ids, &[]).is_err());
        let mut bad = s.clone();
        bad.evidence[0].witness = None;
        assert!(resolve(&bad, &bad.id, &ids, &[]).is_err());
        assert!(serde_json::to_value(&s).unwrap()["evidence"][0]
            .get("witness")
            .is_none());
    }
}
