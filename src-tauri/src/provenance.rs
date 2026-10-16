//! Observations of the pinned engine's actual ledger call chain. Neither a
//! section label nor a path prefix is treated as evidence of a module caller.
use serde::{Deserialize, Serialize};

pub const VERSION: u64 = 1;
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Origin {
    pub path: String,
    pub functions: Vec<String>,
    pub delete_guard: String,
    pub final_guard: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Provenance {
    pub version: u64,
    pub observations: Vec<Origin>,
}
fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit()))
}
impl Origin {
    pub fn from_fields(
        path: &str,
        functions: &str,
        delete_guard: &str,
        final_guard: &str,
    ) -> Result<Self, String> {
        let functions: Vec<String> = functions.split(',').map(String::from).collect();
        if functions.is_empty()
            || functions.len() > 32
            || functions.iter().any(|f| !name(f))
            || (!delete_guard.is_empty() && !name(delete_guard))
            || (!final_guard.is_empty() && !name(final_guard))
        {
            return Err("扫描调用来源无效或超过上限。".into());
        }
        Ok(Self {
            path: path.into(),
            functions,
            delete_guard: delete_guard.into(),
            final_guard: final_guard.into(),
        })
    }
    fn profile(&self) -> &'static str {
        let stack: Vec<_> = self.functions.iter().map(String::as_str).collect();
        if self.delete_guard == "_user_cache_deno_delete_guard"
            && self.final_guard == self.delete_guard
            && stack.starts_with(&[
                "record_dry_run_cleanup_target",
                "_safe_clean_impl",
                "safe_clean_guarded",
                "clean_user_essentials",
            ])
        {
            "userCache"
        } else if self.delete_guard == "_user_log_delete_guard"
            && self.final_guard == self.delete_guard
            && stack.starts_with(&[
                "record_dry_run_cleanup_target",
                "_safe_clean_impl",
                "safe_clean_guarded",
                "_clean_user_log_directory",
                "clean_user_essentials",
            ])
        {
            "userLogs"
        } else {
            "unsupported"
        }
    }
}
impl Provenance {
    pub fn binding(&self, path: &str) -> &'static str {
        if self.version != VERSION || self.observations.is_empty() || self.observations.len() > 16 {
            return "missing";
        }
        let first = self.observations[0].profile();
        if self
            .observations
            .iter()
            .any(|o| o.path != path || o.profile() != first)
        {
            "mixed"
        } else {
            first
        }
    }
}
pub fn binding(value: Option<&Provenance>, path: &str) -> &'static str {
    value.map_or("missing", |p| p.binding(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caller_and_both_guards_are_required_and_all_observations_survive() {
        let o=Origin::from_fields("/fixture/cache","record_dry_run_cleanup_target,_safe_clean_impl,safe_clean_guarded,clean_user_essentials,perform_cleanup","_user_cache_deno_delete_guard","_user_cache_deno_delete_guard").unwrap();
        let mut p = Provenance {
            version: VERSION,
            observations: vec![o.clone()],
        };
        assert_eq!(p.binding("/fixture/cache"), "userCache");
        let mut other = o.clone();
        other.functions[3] = "other_module".into();
        p.observations.push(other);
        assert_eq!(p.binding("/fixture/cache"), "mixed");
        p.observations = vec![o.clone()];
        p.observations[0].final_guard.clear();
        assert_eq!(p.binding("/fixture/cache"), "unsupported");
        p.observations = vec![o];
        assert_eq!(p.binding("/fixture/alias"), "mixed");
        assert!(Origin::from_fields("/fixture", "bad;function", "", "").is_err());
        assert_eq!(binding(None, "/fixture"), "missing");
        let log=Origin::from_fields("/fixture/log","record_dry_run_cleanup_target,_safe_clean_impl,safe_clean_guarded,_clean_user_log_directory,clean_user_essentials","_user_log_delete_guard","_user_log_delete_guard").unwrap();
        let logs = Provenance {
            version: VERSION,
            observations: vec![log],
        };
        assert_eq!(logs.binding("/fixture/log"), "userLogs");
    }
}
