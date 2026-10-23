use super::Observation;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    version: u8,
    read_only: bool,
    pub request_id: Option<String>,
    pub state: &'static str,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    observation: Option<Observation>,
    diagnostic: Option<String>,
}
pub(crate) struct Store(Snapshot);
impl Default for Store {
    fn default() -> Self {
        Self(Snapshot {
            version: 1,
            read_only: true,
            request_id: None,
            state: "idle",
            started_at: None,
            finished_at: None,
            observation: None,
            diagnostic: None,
        })
    }
}
impl Store {
    pub fn snapshot(&self) -> Snapshot {
        self.0.clone()
    }
    pub fn begin(&mut self, id: String, now: i64) -> Result<Snapshot, String> {
        if matches!(self.0.state, "running" | "cancelling") {
            return Err("上次权限检查尚未结束，请稍后重试。".into());
        }
        self.0 = Snapshot {
            version: 1,
            read_only: true,
            request_id: Some(id),
            state: "running",
            started_at: Some(now),
            finished_at: None,
            observation: None,
            diagnostic: None,
        };
        Ok(self.snapshot())
    }
    pub fn cancel(&mut self, id: &str) -> Result<Snapshot, String> {
        if self.0.request_id.as_deref() != Some(id)
            || !matches!(self.0.state, "running" | "cancelling")
        {
            return Err("这次检查已结束，请刷新查看结果。".into());
        }
        self.0.state = "cancelling";
        Ok(self.snapshot())
    }
    pub fn finish(&mut self, id: &str, observation: Observation, now: i64) {
        if self.0.request_id.as_deref() != Some(id)
            || !matches!(self.0.state, "running" | "cancelling")
        {
            return;
        }
        self.0.observation = Some(if self.0.state == "cancelling" {
            Observation::Cancelled
        } else {
            observation
        });
        self.0.finished_at = Some(now);
        self.0.state = "finished";
    }
    pub fn finish_with_diagnostic(
        &mut self,
        id: &str,
        observation: Observation,
        now: i64,
        diagnostic: String,
    ) {
        let active = self.0.request_id.as_deref() == Some(id)
            && matches!(self.0.state, "running" | "cancelling");
        self.finish(id, observation, now);
        if active {
            self.0.diagnostic = Some(diagnostic);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_admission_stale_cancellation_and_late_results_are_bound() {
        let mut store = Store::default();
        assert_eq!(store.snapshot().state, "idle");
        store.begin("first".into(), 1).unwrap();
        assert!(store.begin("second".into(), 2).is_err());
        assert!(store.cancel("other").is_err());
        store.cancel("first").unwrap();
        assert!(store.begin("second".into(), 2).is_err());
        store.finish("first", Observation::CompleteViewObserved, 3);
        assert_eq!(store.snapshot().observation, Some(Observation::Cancelled));
        store.begin("second".into(), 4).unwrap();
        assert!(store.cancel("first").is_err());
        store.finish("first", Observation::CompleteViewObserved, 5);
        assert_eq!(store.snapshot().state, "running");
        store.finish("second", Observation::IncompleteOrFailed, 6);
        store.finish("second", Observation::CompleteViewObserved, 7);
        assert_eq!(
            store.snapshot().observation,
            Some(Observation::IncompleteOrFailed)
        );
    }
}
