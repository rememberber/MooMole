//! Fixed-revision, read-only adaptation of should_protect_from_uninstall.
//! No Shell execution in production; no data ownership or uninstall eligibility.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

const LOGIC: &str = "75cb9db37d26656c01e6c2298d3907c0a7b3b0a2d73fec1494ae522447c37d37";
const DATA: &str = "ac2cdaf4e71d35756ae861ed1e1a9ca7efeeea4e8806e0365c20ebb839615f51";
const LIMIT: u64 = 256 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    version: u32,
    revision: &'static str,
    pub code: &'static str,
    issue: Option<&'static str>,
}
impl Observation {
    pub fn unknown(issue: &'static str) -> Self {
        Self {
            version: 1,
            revision: crate::engine::REVISION,
            code: "unknown",
            issue: Some(issue),
        }
    }
}

pub struct Policy {
    critical: Vec<String>,
    allowed: Vec<String>,
}

fn pinned_text(path: &Path, digest: &str) -> Result<String, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.nlink() != 1 || meta.len() > LIMIT {
        return Err("不支持的规则资源。".into());
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT || format!("{:x}", Sha256::digest(&bytes)) != digest {
        return Err("规则资源与固定版本不一致。".into());
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

// This grammar is intentionally narrower than Bash: only the fixed revision's
// quoted literal IDs / trailing-star prefixes. Reject syntax drift, never eval.
fn patterns(source: &str, name: &str) -> Result<Vec<String>, String> {
    let header = format!("readonly {name}=(");
    if source.lines().filter(|line| *line == header).count() != 1 {
        return Err("规则数组无法识别。".into());
    }
    let mut lines = source.lines().skip_while(|line| *line != header).skip(1);
    let mut result = Vec::new();
    for line in lines.by_ref() {
        let line = line.trim();
        if line == ")" {
            return if result.is_empty() {
                Err("规则数组为空。".into())
            } else {
                Ok(result)
            };
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (value, tail) = line
            .strip_prefix('"')
            .and_then(|line| line.split_once('"'))
            .ok_or("规则格式无法识别。")?;
        let literal = value.strip_suffix('*').unwrap_or(value);
        if literal.is_empty()
            || value.len() > 255
            || result.len() >= 256
            || !literal
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            || !(tail.trim().is_empty() || tail.trim().starts_with('#'))
        {
            return Err("规则语法不受支持。".into());
        }
        result.push(value.into());
    }
    Err("规则数组未结束。".into())
}
fn matches(id: &str, pattern: &str) -> bool {
    pattern
        .strip_suffix('*')
        .map_or(id == pattern, |prefix| id.starts_with(prefix))
}
impl Policy {
    fn parse(source: &str) -> Result<Self, String> {
        Ok(Self {
            critical: patterns(source, "SYSTEM_CRITICAL_BUNDLES")?,
            allowed: patterns(source, "APPLE_UNINSTALLABLE_APPS")?,
        })
    }
    fn protected(&self, id: &str) -> bool {
        // The upstream exception is checked first, even if both arrays match.
        !self.allowed.iter().any(|p| matches(id, p)) && self.critical.iter().any(|p| matches(id, p))
    }
}
pub fn load(root: &Path) -> Result<Policy, String> {
    let info = crate::engine::info(root);
    if !info.available {
        return Err(info.reason.unwrap_or("引擎不可用。".into()));
    }
    // Bind both semantics and data to the audited revision, beyond the mutable
    // prepared manifest. Upstream upgrades need explicit adaptation and tests.
    pinned_text(&root.join("lib/core/app_protection.sh"), LOGIC)?;
    Policy::parse(&pinned_text(
        &root.join("lib/core/app_protection_data.sh"),
        DATA,
    )?)
}
pub fn observe(policy: Option<&Policy>, id: Option<&str>, identity: &str) -> Observation {
    if identity == "different" {
        return Observation::unknown("identityChanged");
    }
    let Some(id) = id.filter(|id| {
        !id.is_empty()
            && id.len() <= 255
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    }) else {
        return Observation::unknown("identityUnavailable");
    };
    let Some(policy) = policy else {
        return Observation::unknown("rulesUnavailable");
    };
    Observation {
        version: 1,
        revision: crate::engine::REVISION,
        code: if policy.protected(id) {
            "protected"
        } else {
            "notMatched"
        },
        issue: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> Policy {
        Policy::parse("readonly SYSTEM_CRITICAL_BUNDLES=(\n \"com.apple.*\"\n \"security*\"\n)\nreadonly APPLE_UNINSTALLABLE_APPS=(\n \"com.apple.dt.*\" # exception\n \"com.apple.Motion\"\n)\n").unwrap()
    }
    #[test]
    fn upstream_exception_precedes_critical_and_matching_is_case_sensitive() {
        for id in [
            "com.apple.dt.Xcode",
            "com.apple.Motion",
            "COM.APPLE.finder",
            "org.example.app",
        ] {
            assert!(!policy().protected(id), "{id}");
        }
        for id in [
            "com.apple.finder",
            "com.apple.motion",
            "security",
            "securityd",
        ] {
            assert!(policy().protected(id));
        }
        assert!(!policy().protected("prefix.securityd"));
    }
    #[test]
    fn unsupported_array_syntax_and_resource_changes_never_produce_no_match() {
        for source in ["", "readonly SYSTEM_CRITICAL_BUNDLES=(\n)", "readonly SYSTEM_CRITICAL_BUNDLES=(\n \"x\"; touch anything\n)", "readonly SYSTEM_CRITICAL_BUNDLES=(\n \"a*b\"\n)", "readonly SYSTEM_CRITICAL_BUNDLES=(\n \"$(command)\"\n)", "readonly SYSTEM_CRITICAL_BUNDLES=(\n \"x\"\n", "readonly SYSTEM_CRITICAL_BUNDLES=(\n \"x\"\n)\nreadonly SYSTEM_CRITICAL_BUNDLES=(\n \"y\"\n)"] {
            assert!(patterns(source, "SYSTEM_CRITICAL_BUNDLES").is_err());
        }
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("rules");
        std::fs::write(&path, "fixture").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"fixture"));
        assert_eq!(pinned_text(&path, &digest).unwrap(), "fixture");
        std::fs::write(&path, "changed").unwrap();
        assert!(pinned_text(&path, &digest).is_err());
        std::fs::write(&path, vec![b'x'; LIMIT as usize + 1]).unwrap();
        assert!(pinned_text(&path, &digest).is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(pinned_text(&path, &digest).is_err());
        let outside = tmp.path().join("outside");
        std::fs::write(&outside, "fixture").unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        assert!(pinned_text(&path, &digest).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::hard_link(&outside, &path).unwrap();
        assert!(pinned_text(&path, &digest).is_err());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "fixture");
    }
    #[test]
    fn missing_changed_or_unavailable_identity_stays_unknown_even_when_rules_match() {
        assert_eq!(
            observe(None, Some("com.apple.finder"), "matches").issue,
            Some("rulesUnavailable")
        );
        for id in [None, Some(""), Some("bad/id")] {
            assert_eq!(
                observe(Some(&policy()), id, "unavailable").issue,
                Some("identityUnavailable")
            );
        }
        assert_eq!(
            observe(Some(&policy()), Some("com.apple.finder"), "different").issue,
            Some("identityChanged")
        );
        assert_eq!(
            observe(Some(&policy()), Some("com.apple.finder"), "unreported").code,
            "protected"
        );
        assert_eq!(
            observe(Some(&policy()), Some("com.apple.dt.Xcode"), "matches").code,
            "notMatched"
        );
    }
    #[test]
    #[ignore = "Explicit read-only differential check against the prepared pinned Mole engine"]
    fn prepared_rules_match_actual_mole_predicate() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        let policy = load(&root).unwrap();
        let mut ids = vec!["org.example.app".into(), "com.apple.dt.Xcode".into()];
        for p in policy.critical.iter().chain(&policy.allowed) {
            let literal = p.trim_end_matches('*');
            ids.extend([
                literal.into(),
                format!("{literal}.probe"),
                format!("x{literal}"),
                literal.to_uppercase(),
                literal.to_lowercase(),
            ]);
        }
        let tmp = tempfile::tempdir().unwrap();
        let output = std::process::Command::new("/bin/bash").env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin").env("HOME", tmp.path()).env("TMPDIR", tmp.path())
            .arg("-c").arg("source \"$1/lib/core/app_protection.sh\"; shift; for id do if should_protect_from_uninstall \"$id\"; then printf '1\\n'; else status=$?; [[ $status == 1 ]] || exit 2; printf '0\\n'; fi; done")
            .arg("moomole-rule-test").arg(&root).args(&ids).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual = String::from_utf8(output.stdout).unwrap();
        assert_eq!(actual.lines().count(), ids.len());
        for (id, code) in ids.iter().zip(actual.lines()) {
            assert_eq!(policy.protected(id), code == "1", "{id}");
        }
        println!(
            "Compared {} IDs against pinned should_protect_from_uninstall",
            ids.len()
        );
    }
}
