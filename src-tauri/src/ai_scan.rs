//! AI is a metadata advisor. Only original Mole scan IDs can enter review::Plan.
mod agents;
mod commands;
mod inventory;
use crate::{db, review};
pub(crate) use commands::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub(crate) const SKILL: &str = include_str!("../../skills/moomole-deep-scan/SKILL.md");
const SCHEMA: &str = include_str!("../../skills/moomole-deep-scan/output-schema.json");

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Evidence {
    id: String,
    source: String,
    path: String,
    size: Option<u64>,
    modified_at: Option<i64>,
    is_directory: bool,
    category: String,
    candidate_id: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Finding {
    evidence_id: String,
    recommendation: Recommendation,
    reason: String,
    impact: String,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Recommendation {
    Review,
    Keep,
    Investigate,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Report {
    summary: String,
    findings: Vec<Finding>,
    limitations: Vec<String>,
}
fn valid_text(text: &str, max: usize) -> bool {
    !text.trim().is_empty()
        && text.len() <= max
        && !text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
}
fn parse_report(bytes: &[u8], evidence: &[Evidence]) -> Result<Report, String> {
    let report: Report = serde_json::from_slice(bytes)
        .map_err(|_| "Agent 未返回约定的结构化建议；未生成清理选择。".to_string())?;
    if !valid_text(&report.summary, 4000)
        || report.findings.len() > 100
        || report.limitations.len() > 20
        || report.limitations.iter().any(|s| !valid_text(s, 2000))
    {
        return Err("AI 报告超过边界或包含无效文本，未生成清理选择。".into());
    }
    let mut seen = HashSet::new();
    for finding in &report.findings {
        let source = evidence
            .iter()
            .find(|e| e.id == finding.evidence_id)
            .ok_or("AI 引用了本次清单以外的对象，未生成清理选择。")?;
        if !seen.insert(&finding.evidence_id)
            || !valid_text(&finding.reason, 2000)
            || !valid_text(&finding.impact, 2000)
            || (finding.recommendation == Recommendation::Review && source.candidate_id.is_none())
        {
            return Err("AI 建议重复、无效或超出 Mole 审阅范围，未生成清理选择。".into());
        }
    }
    Ok(report)
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    id: String,
    agent_id: String,
    root: String,
    state: String,
    stage: String,
    started_at: i64,
    finished_at: Option<i64>,
    scan_id: Option<String>,
    expires_at: Option<i64>,
    evidence: Vec<Evidence>,
    limitations: Vec<String>,
    report: Option<Report>,
    error: Option<String>,
}
#[derive(Default)]
pub(crate) struct Store {
    snapshot: Option<Snapshot>,
}
impl Store {
    fn begin(&mut self, id: String, agent_id: String, root: String) -> Result<Snapshot, String> {
        if self.snapshot.as_ref().is_some_and(|s| s.state == "running") {
            return Err("已有 AI 深度扫描，请等待完成或停止。".into());
        }
        let snapshot = Snapshot {
            id,
            agent_id,
            root,
            state: "running".into(),
            stage: "正在检查 Agent…".into(),
            started_at: db::now(),
            finished_at: None,
            scan_id: None,
            expires_at: None,
            evidence: vec![],
            limitations: vec![],
            report: None,
            error: None,
        };
        self.snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }
    fn current(&mut self) -> Option<Snapshot> {
        if self.snapshot.as_ref().is_some_and(|s| {
            s.state != "running"
                && db::now() - s.finished_at.unwrap_or(s.started_at) > review::LIFETIME_MS
        }) {
            self.snapshot = None;
        }
        self.snapshot.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn evidence() -> Vec<Evidence> {
        vec![Evidence {
            id: "e0".into(),
            source: "metadata".into(),
            path: "/test/private.txt".into(),
            size: None,
            modified_at: None,
            is_directory: false,
            category: "file".into(),
            candidate_id: None,
        }]
    }
    #[test]
    fn active_snapshot_cannot_be_replaced_and_finished_reports_expire() {
        let mut store = Store::default();
        store
            .begin("first".into(), "codex".into(), "/test".into())
            .unwrap();
        assert!(store
            .begin("second".into(), "codex".into(), "/test".into())
            .is_err());
        assert_eq!(store.current().unwrap().id, "first");
        let snapshot = store.snapshot.as_mut().unwrap();
        snapshot.state = "cancelled".into();
        snapshot.finished_at = Some(db::now() - review::LIFETIME_MS - 1);
        assert!(store.current().is_none());
        assert!(store
            .begin("second".into(), "codex".into(), "/test".into())
            .is_ok());
    }
    #[test]
    fn rejects_invented_ids_commands_duplicates_and_metadata_execution() {
        for findings in [
            serde_json::json!([{"evidenceId":"invented","recommendation":"keep","reason":"保留","impact":"未知"}]),
            serde_json::json!([{"evidenceId":"e0","recommendation":"review","reason":"清理","impact":"未知"}]),
            serde_json::json!([{"evidenceId":"e0","recommendation":"keep","reason":"保留","impact":"未知","command":"rm"}]),
            serde_json::json!([{"evidenceId":"e0","recommendation":"keep","reason":"保留","impact":"未知"},
                {"evidenceId":"e0","recommendation":"keep","reason":"保留","impact":"未知"}]),
        ] {
            assert!(parse_report(
                serde_json::json!({"summary":"摘要","findings":findings,"limitations":[]})
                    .to_string()
                    .as_bytes(),
                &evidence()
            )
            .is_err());
        }
    }
    #[test]
    fn accepts_reasoning_without_granting_metadata_execution() {
        let bytes = br#"{"summary":"summary","findings":[{"evidenceId":"e0","recommendation":"investigate","reason":"unknown ownership","impact":"personal data may be lost"}],"limitations":[]}"#;
        assert_eq!(parse_report(bytes, &evidence()).unwrap().findings.len(), 1);
    }
}
