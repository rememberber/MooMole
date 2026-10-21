//! Ephemeral, read-only review of a native observation. Never execution authority.
use super::{Report, Value, IDS};
use serde::Serialize;

pub const LIFETIME_MS: i64 = 60_000;
#[derive(Default)]
pub struct Store {
    generation: u64,
    latest: Option<(Report, Vec<String>)>,
}
impl Store {
    pub fn begin(&mut self) -> Result<u64, String> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("请重新打开应用后再检查。")?;
        // A refresh attempt invalidates old native authority even if its UI
        // continues showing an earlier result after an error.
        self.latest = None;
        Ok(self.generation)
    }
    pub fn publish(
        &mut self,
        generation: u64,
        report: Report,
        client: Vec<String>,
    ) -> Result<(), String> {
        if generation != self.generation
            || report.protocol != 4
            || report.revision != crate::engine::REVISION
        {
            return Err("维护检查已经被更新，请重新检查。".into());
        }
        self.latest = Some((report, client));
        Ok(())
    }
    pub fn prepare(
        &self,
        observation_id: &str,
        selection: &[String],
        client: &[String],
        now: i64,
    ) -> Result<Review, String> {
        uuid::Uuid::parse_str(observation_id).map_err(|_| "维护检查标识无效，请重新检查。")?;
        if selection.is_empty()
            || selection.len() > 2
            || selection
                .iter()
                .enumerate()
                .any(|(index, id)| !IDS.contains(&id.as_str()) || selection[..index].contains(id))
        {
            return Err("请选择本次已检查的维护项目，不能重复或扩大范围。".into());
        }
        let (report, captured_client) = self
            .latest
            .as_ref()
            .ok_or("没有可用的本机维护检查，请先检查。")?;
        if report.observation_id != observation_id || captured_client != client {
            return Err("本机检查或保护设置已变化，请重新检查。".into());
        }
        let expires_at = report
            .checked_at
            .checked_add(LIFETIME_MS)
            .ok_or("检查时间无效，请重新检查。")?;
        if now < report.checked_at || now >= expires_at {
            return Err("维护检查已过期或时间变化，请重新检查。".into());
        }
        let items = selection
            .iter()
            .map(|id| {
                let item = report
                    .items
                    .iter()
                    .find(|item| item.id == id)
                    .ok_or("项目不属于本次检查。")?;
                let keys = item
                    .findings
                    .iter()
                    .map(|finding| {
                        let outcome = if item.code == "protected" {
                            "preserveProtected"
                        } else if item.code == "restricted" {
                            "preserveRestricted"
                        } else if finding.value == Value::Unknown
                            || finding
                                .original_evidence
                                .iter()
                                .any(|v| v.state == "changed")
                            || item.task_protection == "unknown"
                            || item.paths.iter().any(|path| {
                                path.state != "present"
                                    || path.access != "noRestrictionObserved"
                                    || path.client_protection == "unknown"
                                    || path.mole_protection == "unknown"
                            })
                        {
                            "preserveUnknown"
                        } else if (item.id == IDS[0] && finding.value == Value::Enabled)
                            || (item.id == IDS[1] && finding.value == Value::Disabled)
                        {
                            "unchanged"
                        } else {
                            "needsEvidence"
                        };
                        Key {
                            key: finding.key,
                            label: finding.label,
                            current: finding.value,
                            original_evidence: finding.original_evidence.clone(),
                            intent: if item.id == IDS[0] {
                                "enableSetting"
                            } else {
                                "removeOverride"
                            },
                            outcome,
                        }
                    })
                    .collect();
                Ok(Item { id: item.id, keys })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Review {
            protocol: 2,
            revision: crate::engine::REVISION,
            observation_id: report.observation_id.clone(),
            checked_at: report.checked_at,
            prepared_at: now,
            expires_at,
            mode: "reviewOnly",
            can_execute: false,
            originals_captured: false,
            concurrency_resolved: false,
            items,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    protocol: u32,
    revision: &'static str,
    observation_id: String,
    checked_at: i64,
    prepared_at: i64,
    expires_at: i64,
    mode: &'static str,
    can_execute: bool,
    originals_captured: bool,
    concurrency_resolved: bool,
    items: Vec<Item>,
}
#[derive(Debug, Serialize)]
struct Item {
    id: &'static str,
    keys: Vec<Key>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Key {
    key: &'static str,
    label: &'static str,
    current: Value,
    original_evidence: Vec<super::originals::Evidence>,
    intent: &'static str,
    outcome: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maintenance::{paths, Finding};
    fn report() -> Report {
        Report {
            protocol: 4,
            observation_id: uuid::Uuid::new_v4().to_string(),
            revision: crate::engine::REVISION,
            checked_at: 1000,
            rule_source: "defaults",
            items: IDS
                .iter()
                .map(|id| super::super::Item {
                    id,
                    code: "observed",
                    task_protection: "notMatched",
                    findings: vec![Finding {
                        domain: "fixture",
                        key: "fixture",
                        label: "fixture",
                        value: if *id == IDS[0] {
                            Value::Disabled
                        } else {
                            Value::Enabled
                        },
                        managed: false,
                        original_evidence: super::super::originals::unavailable("unconfirmed"),
                    }],
                    paths: vec![paths::Observation {
                        location: "fixture",
                        state: "present",
                        client_protection: "notMatched",
                        mole_protection: "notMatched",
                        access: "noRestrictionObserved",
                    }],
                })
                .collect(),
        }
    }
    #[test]
    fn selection_is_exact_and_reviews_do_not_extend_evidence_lifetime() {
        let mut store = Store::default();
        let report = report();
        let id = report.observation_id.clone();
        let generation = store.begin().unwrap();
        store.publish(generation, report, vec![]).unwrap();
        let selected = vec![IDS[1].into(), IDS[0].into()];
        for now in [1000, 60_999] {
            let review = store.prepare(&id, &selected, &[], now).unwrap();
            assert_eq!(
                review.items.iter().map(|item| item.id).collect::<Vec<_>>(),
                [IDS[1], IDS[0]]
            );
            assert_eq!(review.expires_at, 61_000);
            assert!(!review.can_execute);
            assert!(!review.originals_captured);
            assert!(!review.concurrency_resolved);
            assert_eq!(review.items[0].keys[0].outcome, "needsEvidence");
            let wire = serde_json::to_value(&review).unwrap();
            assert_eq!(wire["protocol"], 2);
            let key = &wire["items"][0]["keys"][0];
            assert_eq!(key["originalEvidence"][0]["scope"], "anyHost");
            assert_eq!(key["originalEvidence"][1]["state"], "unconfirmed");
            assert!(key.get("original_evidence").is_none());
        }
        for now in [999, 61_000, 90_000] {
            assert!(store.prepare(&id, &selected, &[], now).is_err())
        }
        for selected in [
            vec![],
            vec![IDS[0].into(), IDS[0].into()],
            vec!["disk_verify".into()],
            vec!["__proto__".into()],
            vec![IDS[0].into(), IDS[1].into(), IDS[0].into()],
        ] {
            assert!(store.prepare(&id, &selected, &[], 1000).is_err())
        }
        assert!(store
            .prepare(
                &uuid::Uuid::new_v4().to_string(),
                &[IDS[0].into()],
                &[],
                1000
            )
            .is_err());
        assert!(store
            .prepare(&id, &[IDS[0].into()], &["/new/protection".into()], 1000)
            .is_err());
    }
    #[test]
    fn refresh_invalidates_old_observations_and_late_publish_cannot_replace_newer_one() {
        let mut store = Store::default();
        let original = report();
        let id = original.observation_id.clone();
        let old = store.begin().unwrap();
        store.publish(old, original.clone(), vec![]).unwrap();
        let current = store.begin().unwrap();
        assert!(store.prepare(&id, &[IDS[0].into()], &[], 1000).is_err());
        assert!(store.publish(old, original, vec![]).is_err());
        let next = report();
        let next_id = next.observation_id.clone();
        store.publish(current, next, vec![]).unwrap();
        assert!(store.prepare(&id, &[IDS[0].into()], &[], 1000).is_err());
        assert!(store.prepare(&next_id, &[IDS[0].into()], &[], 1000).is_ok());
        assert!(Store::default()
            .prepare(&next_id, &[IDS[0].into()], &[], 1000)
            .is_err());
    }
    #[test]
    fn protection_unknown_and_unchanged_outcomes_remain_distinct() {
        for (code, value, path_state, expected) in [
            ("protected", Value::Enabled, "present", "preserveProtected"),
            (
                "restricted",
                Value::Enabled,
                "present",
                "preserveRestricted",
            ),
            ("unknown", Value::Unknown, "present", "preserveUnknown"),
            ("unknown", Value::Enabled, "missing", "preserveUnknown"),
            ("observed", Value::Enabled, "present", "unchanged"),
        ] {
            let mut report = report();
            let id = report.observation_id.clone();
            report.items[0].code = code;
            report.items[0].findings[0].value = value;
            report.items[0].paths[0].state = path_state;
            let mut store = Store::default();
            let generation = store.begin().unwrap();
            store.publish(generation, report, vec![]).unwrap();
            assert_eq!(
                store
                    .prepare(&id, &[IDS[0].into()], &[], 1000)
                    .unwrap()
                    .items[0]
                    .keys[0]
                    .outcome,
                expected
            );
        }
    }
}
