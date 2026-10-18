//! Short-lived result snapshots bridge dropped UI events and WebView reloads.
//! File contents and result payloads are never persisted by this cache.
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::VecDeque,
    io::Write,
    time::{Duration, Instant},
};

const MAX_RESULTS: usize = 10;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_ITEM_BYTES: usize = 8 * 1024 * 1024;
const TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub result_unavailable: Option<String>,
}

struct Entry {
    snapshot: Snapshot,
    bytes: usize,
    finished: Instant,
}

#[derive(Default)]
pub struct Results {
    entries: VecDeque<Entry>,
}

struct Counter(usize);
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_ITEM_BYTES.saturating_sub(self.0) {
            return Err(std::io::Error::other("snapshot too large"));
        }
        self.0 += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Results {
    pub fn remember(&mut self, snapshot: &Snapshot) {
        self.prune(Instant::now());
        self.entries
            .retain(|entry| entry.snapshot.id != snapshot.id);
        let mut bytes = Counter(0);
        let retained = serde_json::to_writer(&mut bytes, snapshot).is_ok();
        let stored = Snapshot {
            id: snapshot.id.clone(),
            kind: snapshot.kind.clone(),
            state: snapshot.state.clone(),
            result: if retained {
                snapshot.result.clone()
            } else {
                None
            },
            error: snapshot.error.clone(),
            result_unavailable: if retained {
                snapshot.result_unavailable.clone()
            } else {
                Some("结果超过临时保留上限，请重新检查以获取完整结果。".into())
            },
        };
        if !retained {
            bytes.0 = 0;
        }
        while self.entries.len() >= MAX_RESULTS
            || self.entries.iter().map(|e| e.bytes).sum::<usize>() + bytes.0 > MAX_BYTES
        {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            snapshot: stored,
            bytes: bytes.0,
            finished: Instant::now(),
        });
    }

    fn prune(&mut self, now: Instant) {
        self.entries
            .retain(|entry| now.saturating_duration_since(entry.finished) < TTL);
    }

    // Resolve an index only against a retained, successful Mole application list.
    // The icon IPC never accepts a path from the WebView or clones the full list.
    pub fn app_path(&mut self, id: &str, index: usize) -> Option<String> {
        self.prune(Instant::now());
        let entry = self.entries.iter().find(|e| e.snapshot.id == id)?;
        let snapshot = &entry.snapshot;
        if snapshot.kind != "apps" || snapshot.state != "succeeded" {
            return None;
        }
        snapshot
            .result
            .as_ref()?
            .as_array()?
            .get(index)?
            .get("path")?
            .as_str()
            .filter(|path| path.len() <= 4096)
            .map(str::to_owned)
    }

    pub fn app_target(
        &mut self,
        id: &str,
        index: usize,
    ) -> Option<(String, String, Option<String>)> {
        let path = self.app_path(id, index)?;
        let row = self
            .entries
            .iter()
            .find(|e| e.snapshot.id == id)?
            .snapshot
            .result
            .as_ref()?
            .as_array()?
            .get(index)?;
        let bundle_id = row.get("bundle_id")?.as_str()?;
        let name = row
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|name| {
                !name.is_empty() && name.len() <= 1024 && !name.chars().any(char::is_control)
            })
            .map(str::to_owned);
        (bundle_id.len() <= 255).then(|| (path, bundle_id.to_owned(), name))
    }

    pub fn get(&mut self, id: &str) -> Option<Snapshot> {
        self.prune(Instant::now());
        self.entries
            .iter()
            .find(|e| e.snapshot.id == id)
            .map(|e| e.snapshot.clone())
    }
}

pub fn from_record(record: crate::db::TaskRecord, active: bool) -> Snapshot {
    if active {
        return Snapshot {
            id: record.id,
            kind: record.kind,
            state: "running".into(),
            result: None,
            error: None,
            result_unavailable: None,
        };
    }
    let running = record.state == "running";
    let succeeded = record.state == "succeeded";
    Snapshot {
        id: record.id,
        kind: record.kind,
        state: if running && !active {
            "interrupted".into()
        } else {
            record.state
        },
        result: None,
        error: if running && !active {
            Some("任务已没有运行句柄，请重新检查；不会自动重启。".into())
        } else if !running && !succeeded {
            record.message
        } else {
            None
        },
        result_unavailable: succeeded.then(|| "本次结果已不在临时缓存中，请重新检查。".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(id: usize, result: Value) -> Snapshot {
        Snapshot {
            id: id.to_string(),
            kind: "status".into(),
            state: "succeeded".into(),
            result: Some(result),
            error: None,
            result_unavailable: None,
        }
    }
    #[test]
    fn snapshots_bound_count_bytes_and_age_and_keep_final_states() {
        let mut cache = Results::default();
        for id in 0..12 {
            cache.remember(&snapshot(id, serde_json::json!({"id":id})));
        }
        assert!(cache.get("0").is_none());
        assert!(cache.get("11").unwrap().result.is_some());
        cache.entries.front_mut().unwrap().finished = Instant::now() - TTL;
        assert!(cache.get("2").is_none());
        for id in 12..16 {
            cache.remember(&snapshot(id, Value::String("x".repeat(5 * 1024 * 1024))));
        }
        assert!(cache.entries.iter().map(|e| e.bytes).sum::<usize>() <= MAX_BYTES);
        cache.remember(&snapshot(99, Value::String("x".repeat(MAX_ITEM_BYTES))));
        let large = cache.get("99").unwrap();
        assert_eq!(large.state, "succeeded");
        assert!(large.result.is_none());
        assert!(large.result_unavailable.is_some());
    }
    #[test]
    fn icons_resolve_indices_only_from_retained_successful_application_results() {
        let mut cache = Results::default();
        let mut apps = snapshot(
            1,
            serde_json::json!([
                {"path":"/Applications/一.app"}, {"path":"/Applications/二.app"}, {"path":17}, {"path":"x".repeat(4097)}
            ]),
        );
        apps.kind = "apps".into();
        cache.remember(&apps);
        assert_eq!(
            cache.app_path("1", 1).as_deref(),
            Some("/Applications/二.app")
        );
        assert_eq!(
            cache.app_path("1", 0).as_deref(),
            Some("/Applications/一.app")
        );
        for index in [2, 3, usize::MAX] {
            assert!(cache.app_path("1", index).is_none());
        }
        assert!(cache.app_path("unknown", 0).is_none());
        apps.state = "failed".into();
        cache.remember(&apps);
        assert!(cache.app_path("1", 0).is_none());
        apps.state = "succeeded".into();
        apps.kind = "analyze".into();
        cache.remember(&apps);
        assert!(cache.app_path("1", 0).is_none());
        apps.kind = "apps".into();
        cache.remember(&apps);
        cache.entries.back_mut().unwrap().finished = Instant::now() - TTL;
        assert!(cache.app_path("1", 0).is_none());
        apps.result = None;
        cache.remember(&apps);
        assert!(cache.app_path("1", 0).is_none());
    }

    #[test]
    fn inspection_targets_bind_the_mole_identifier_without_accepting_other_task_results() {
        let mut cache = Results::default();
        let mut apps = snapshot(
            1,
            serde_json::json!([
                {"path":"/Applications/一.app","bundle_id":"org.example.first","name":"中文 应用"},
                {"path":"/Applications/二.app","bundle_id":""},
                {"path":"/Applications/三.app","bundle_id":17},
                {"path":"/Applications/四.app","bundle_id":"x".repeat(256)},
                {"path":"/Applications/五.app","bundle_id":"valid.id","name":17},
                {"path":"/Applications/六.app","bundle_id":"valid.id","name":"bad\nname"},
                {"path":"/Applications/七.app","bundle_id":"valid.id","name":"x".repeat(1025)}
            ]),
        );
        apps.kind = "apps".into();
        cache.remember(&apps);
        assert_eq!(
            cache.app_target("1", 0),
            Some((
                "/Applications/一.app".into(),
                "org.example.first".into(),
                Some("中文 应用".into())
            ))
        );
        assert_eq!(
            cache.app_target("1", 1),
            Some(("/Applications/二.app".into(), "".into(), None))
        );
        for index in [2, 3, usize::MAX] {
            assert!(cache.app_target("1", index).is_none());
        }
        for index in [4, 5, 6] {
            assert!(cache.app_target("1", index).unwrap().2.is_none());
        }
        apps.kind = "analyze".into();
        cache.remember(&apps);
        assert!(cache.app_target("1", 0).is_none());
        apps.kind = "apps".into();
        cache.remember(&apps);
        cache.entries.back_mut().unwrap().finished = Instant::now() - TTL;
        assert!(cache.app_target("1", 0).is_none());
    }

    #[test]
    fn persisted_status_does_not_invent_a_result_or_restart_lost_jobs() {
        let record = crate::db::TaskRecord {
            id: "id".into(),
            kind: "clean".into(),
            path: None,
            state: "running".into(),
            created_at: 1000,
            finished_at: None,
            message: None,
        };
        assert_eq!(from_record(record.clone(), true).state, "running");
        assert_eq!(from_record(record.clone(), false).state, "interrupted");
        let finishing = from_record(
            crate::db::TaskRecord {
                state: "succeeded".into(),
                ..record.clone()
            },
            true,
        );
        assert_eq!(finishing.state, "running");
        assert!(finishing.result_unavailable.is_none());
        let done = from_record(
            crate::db::TaskRecord {
                state: "succeeded".into(),
                ..record
            },
            false,
        );
        assert!(done.result.is_none());
        assert!(done.result_unavailable.is_some());
    }
}
