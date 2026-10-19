//! Fixed-revision read-only component / official-uninstaller observations.
//! No Shell execution in production; no data ownership or uninstall eligibility.
mod identity;
mod path;
pub use path::IdentityBudget as PathBudget;
pub use path::Observation as PathObservation;
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
const UNINSTALL: &str = "1ffcd3f39735013210346e106c7ade367078faaefa3a6e072e20c1234ca37a31";
const BATCH: &str = "f44ed6a61ec5074f8efe89e65dd3d9fbc48dd78533b4d529d8220dd9660bcc54";
const RECEIPTS: &str = "5a75ca76477880016e035e76a688371653f354a57a8668f85906351609977894";
const BASE: &str = "58627b0717c02c6d27dbd28d33ff038affad9d6e8fec4770905c1bb3f1fda6f3";
const FILE_OPS: &str = "d483e6c196806205248bf25c94d9c1dea0ff8d621ed13806109c452675673005";
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
    official: Vec<OfficialRule>,
    paths: Option<path::Policy>,
}

struct OfficialRule {
    vendor: String,
    prefixes: Vec<String>,
    fragments: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialObservation {
    version: u32,
    revision: &'static str,
    code: &'static str,
    vendor: Option<String>,
    issue: Option<&'static str>,
}
impl OfficialObservation {
    pub fn unknown(issue: &'static str) -> Self {
        Self {
            version: 1,
            revision: crate::engine::REVISION,
            code: "unknown",
            vendor: None,
            issue: Some(issue),
        }
    }
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

// Read quoted literal rows only; the two field grammars below reject unsupported
// syntax. This never interpolates or evaluates the data as Shell code.
fn literal_rows(source: &str, name: &str) -> Result<Vec<String>, String> {
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
        if value.is_empty()
            || value.len() > 1024
            || result.len() >= 256
            || value.chars().any(char::is_control)
            || !(tail.trim().is_empty() || tail.trim().starts_with('#'))
        {
            return Err("规则语法不受支持。".into());
        }
        result.push(value.into());
    }
    Err("规则数组未结束。".into())
}
fn patterns(source: &str, name: &str) -> Result<Vec<String>, String> {
    let rows = literal_rows(source, name)?;
    for value in &rows {
        let literal = value.strip_suffix('*').unwrap_or(value);
        if literal.is_empty()
            || value.len() > 255
            || !literal
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return Err("规则语法不受支持。".into());
        }
    }
    Ok(rows)
}
impl OfficialRule {
    fn parse(row: &str) -> Result<Self, String> {
        let fields: Vec<_> = row.split('|').collect();
        if fields.len() != 3
            || fields[0].is_empty()
            || fields[0].len() > 64
            || !fields[0]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b' ')
        {
            return Err("官方卸载器规则无法识别。".into());
        }
        let list = |raw: &str, fragment: bool| -> Result<Vec<String>, String> {
            if raw.is_empty() {
                return Ok(vec![]);
            }
            let values: Vec<_> = raw.split(',').collect();
            if values.len() > 32
                || values.iter().any(|v| {
                    v.is_empty()
                        || v.len() > 255
                        || !v.bytes().all(|b| {
                            b.is_ascii_lowercase()
                                || b.is_ascii_digit()
                                || matches!(b, b'.' | b'_' | b'-')
                                || (fragment && b == b' ')
                        })
                })
            {
                return Err("官方卸载器匹配语法不受支持。".into());
            }
            Ok(values.into_iter().map(str::to_owned).collect())
        };
        let prefixes = list(fields[1], false)?;
        let fragments = list(fields[2], true)?;
        if prefixes.is_empty() && fragments.is_empty() {
            return Err("官方卸载器规则为空。".into());
        }
        Ok(Self {
            vendor: fields[0].into(),
            prefixes,
            fragments,
        })
    }
}
fn matches(id: &str, pattern: &str) -> bool {
    pattern
        .strip_suffix('*')
        .map_or(id == pattern, |prefix| id.starts_with(prefix))
}
impl Policy {
    fn parse(source: &str) -> Result<Self, String> {
        Ok(Self {
            paths: None,
            critical: patterns(source, "SYSTEM_CRITICAL_BUNDLES")?,
            allowed: patterns(source, "APPLE_UNINSTALLABLE_APPS")?,
            official: literal_rows(source, "OFFICIAL_UNINSTALLER_RULES")?
                .iter()
                .map(|row| OfficialRule::parse(row))
                .collect::<Result<_, _>>()?,
        })
    }
    fn protected(&self, id: &str) -> bool {
        // The upstream exception is checked first, even if both arrays match.
        !self.allowed.iter().any(|p| matches(id, p)) && self.critical.iter().any(|p| matches(id, p))
    }
    fn official_vendor(&self, id: &str, name: &str, path: &str) -> Option<&str> {
        // Upstream LC_ALL=C tr lowercases ASCII only; Unicode remains unchanged.
        let id = id.to_ascii_lowercase();
        let name = name.to_ascii_lowercase();
        let basename = path
            .rsplit('/')
            .find(|part| !part.is_empty())
            .unwrap_or("")
            .to_ascii_lowercase();
        let basename = basename.strip_suffix(".app").unwrap_or(&basename);
        self.official
            .iter()
            .find(|rule| {
                rule.prefixes.iter().any(|prefix| id.starts_with(prefix))
                    || rule
                        .fragments
                        .iter()
                        .any(|fragment| name.contains(fragment) || basename.contains(fragment))
            })
            .map(|rule| rule.vendor.as_str())
    }
}
pub fn load(root: &Path) -> Result<Policy, String> {
    let info = crate::engine::info(root);
    if !info.available {
        return Err(info.reason.unwrap_or("引擎不可用。".into()));
    }
    // Bind both semantics and data to the audited revision, beyond the mutable
    // prepared manifest. Upstream upgrades need explicit adaptation and tests.
    let logic = pinned_text(&root.join("lib/core/app_protection.sh"), LOGIC)?;
    pinned_text(&root.join("bin/uninstall.sh"), UNINSTALL)?;
    pinned_text(&root.join("lib/uninstall/batch.sh"), BATCH)?;
    pinned_text(&root.join("lib/core/pkg_receipts.sh"), RECEIPTS)?;
    pinned_text(&root.join("lib/core/base.sh"), BASE)?;
    pinned_text(&root.join("lib/core/file_ops.sh"), FILE_OPS)?;
    let data = pinned_text(&root.join("lib/core/app_protection_data.sh"), DATA)?;
    let mut policy = Policy::parse(&data)?;
    policy.paths = Some(path::Policy::parse(&logic, &data)?);
    Ok(policy)
}
pub fn observe_path_with_identity(
    policy: Option<&Policy>,
    home: &Path,
    path: &Path,
    location: &str,
    budget: &mut PathBudget,
) -> PathObservation {
    path::observe_with_identity(
        policy.and_then(|p| p.paths.as_ref()),
        home,
        path,
        location,
        budget,
    )
}

fn observed_id<'a>(id: Option<&'a str>, identity: &str) -> Result<&'a str, &'static str> {
    if identity == "different" {
        return Err("identityChanged");
    }
    let Some(id) = id.filter(|id| {
        !id.is_empty()
            && id.len() <= 255
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    }) else {
        return Err("identityUnavailable");
    };
    Ok(id)
}
pub fn observe(policy: Option<&Policy>, id: Option<&str>, identity: &str) -> Observation {
    let id = match observed_id(id, identity) {
        Ok(id) => id,
        Err(issue) => return Observation::unknown(issue),
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

pub fn observe_official(
    policy: Option<&Policy>,
    id: Option<&str>,
    identity: &str,
    name: Option<&str>,
    path: &str,
) -> OfficialObservation {
    let id = match observed_id(id, identity) {
        Ok(id) => id,
        Err(issue) => return OfficialObservation::unknown(issue),
    };
    let Some(name) = name.filter(|name| {
        !name.is_empty() && name.len() <= 1024 && !name.chars().any(char::is_control)
    }) else {
        return OfficialObservation::unknown("nameUnavailable");
    };
    let Some(policy) = policy else {
        return OfficialObservation::unknown("rulesUnavailable");
    };
    let vendor = policy.official_vendor(id, name, path).map(str::to_owned);
    OfficialObservation {
        version: 1,
        revision: crate::engine::REVISION,
        code: if vendor.is_some() {
            "officialRequired"
        } else {
            "notMatched"
        },
        vendor,
        issue: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> Policy {
        Policy::parse("readonly SYSTEM_CRITICAL_BUNDLES=(\n \"com.apple.*\"\n \"security*\"\n)\nreadonly APPLE_UNINSTALLABLE_APPS=(\n \"com.apple.dt.*\" # exception\n \"com.apple.Motion\"\n)\nreadonly OFFICIAL_UNINSTALLER_RULES=(\n \"First|com.first.|managed agent\"\n \"Second|com.second.|falcon\"\n \"Third|com.third.|\"\n)\n").unwrap()
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
    fn official_rules_preserve_ascii_case_prefixes_name_fragments_and_vendor_order() {
        let p = policy();
        for (id, name, path, expected) in [
            (
                "COM.FIRST.Agent",
                "普通应用",
                "/Applications/Other.app",
                Some("First"),
            ),
            (
                "other.id",
                "中文 MANAGED AGENT 工具",
                "/Applications/Other.app",
                Some("First"),
            ),
            (
                "other.id",
                "ordinary",
                "/Applications/中文 FALCON.ApP",
                Some("Second"),
            ),
            (
                "com.second.agent",
                "managed agent",
                "/Applications/Other.app",
                Some("First"),
            ),
            (
                "com.third.agent",
                "ordinary",
                "/Applications/Other.app",
                Some("Third"),
            ),
            (
                "xcom.first.agent",
                "ordinary",
                "/Applications/Other.app",
                None,
            ),
            ("other.id", "falcön", "/Applications/ordinary.app", None),
        ] {
            assert_eq!(
                p.official_vendor(id, name, path),
                expected,
                "{id} {name} {path}"
            );
        }
    }
    #[test]
    fn official_rules_require_complete_bound_context_and_do_not_turn_unknown_into_no_match() {
        let p = policy();
        for (id, identity, name, issue) in [
            (
                Some("com.first.agent"),
                "different",
                Some("managed agent"),
                "identityChanged",
            ),
            (
                None,
                "unavailable",
                Some("managed agent"),
                "identityUnavailable",
            ),
            (Some("com.first.agent"), "matches", None, "nameUnavailable"),
            (
                Some("com.first.agent"),
                "matches",
                Some("bad\nname"),
                "nameUnavailable",
            ),
        ] {
            let value = observe_official(Some(&p), id, identity, name, "/Applications/Agent.app");
            assert_eq!(value.code, "unknown");
            assert_eq!(value.issue, Some(issue));
            assert!(value.vendor.is_none());
        }
        assert_eq!(
            observe_official(
                None,
                Some("com.first.agent"),
                "matches",
                Some("Agent"),
                "/Applications/A.app"
            )
            .issue,
            Some("rulesUnavailable")
        );
        assert_eq!(
            observe_official(
                Some(&p),
                Some("com.first.agent"),
                "unreported",
                Some("Agent"),
                "/Applications/A.app"
            )
            .code,
            "officialRequired"
        );
    }
    #[test]
    fn unsupported_vendor_fields_and_match_syntax_fail_closed() {
        for row in [
            "No fields",
            "Vendor||",
            "Vendor|prefix|fragment|extra",
            "Vendor|prefix,,other|",
            "Vendor|PREFIX|",
            "Vendor|prefix*|",
            "Vendor|$(anything)|",
            "Vendor||fragment?",
            "|prefix|",
        ] {
            assert!(OfficialRule::parse(row).is_err(), "{row}");
        }
        assert!(OfficialRule::parse(&format!("Vendor|{}|", "a".repeat(256))).is_err());
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
        let mut cases = vec![(
            "org.example.app".into(),
            "普通应用".into(),
            "/Applications/普通.app".into(),
        )];
        for rule in &policy.official {
            for prefix in &rule.prefixes {
                cases.extend([
                    (
                        format!("{prefix}agent"),
                        "ordinary".into(),
                        "/Applications/ordinary.app".into(),
                    ),
                    (
                        format!("{prefix}agent").to_uppercase(),
                        "ordinary".into(),
                        "/Applications/ordinary.app".into(),
                    ),
                    (
                        format!("x{prefix}agent"),
                        "ordinary".into(),
                        "/Applications/ordinary.app".into(),
                    ),
                ]);
            }
            for fragment in &rule.fragments {
                cases.extend([
                    (
                        "other.id".into(),
                        fragment.clone(),
                        "/Applications/ordinary.app".into(),
                    ),
                    (
                        "other.id".into(),
                        format!("中文 {} 工具", fragment.to_uppercase()),
                        "/Applications/ordinary.app".into(),
                    ),
                    (
                        "other.id".into(),
                        "ordinary".into(),
                        format!("/Applications/中文 {}.ApP", fragment.to_uppercase()),
                    ),
                ]);
            }
        }
        cases.push((
            "com.cisco.secureclient.agent".into(),
            "eset management agent".into(),
            "/Applications/Falcon.app".into(),
        ));
        let output = std::process::Command::new("/bin/bash").env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin").env("HOME", tmp.path()).env("TMPDIR", tmp.path())
            .arg("-c").arg("source \"$1/lib/core/app_protection.sh\"; shift; while [[ $# -gt 0 ]]; do vendor=''; if vendor=$(official_uninstaller_vendor \"$1\" \"$2\" \"$3\"); then printf '%s\\n' \"$vendor\"; else status=$?; [[ $status == 1 ]] || exit 2; printf '%s\\n' none; fi; shift 3; done")
            .arg("moomole-official-test").arg(&root).args(cases.iter().flat_map(|(id,name,path)| [id,name,path])).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual = String::from_utf8(output.stdout).unwrap();
        assert_eq!(actual.lines().count(), cases.len());
        for ((id, name, path), vendor) in cases.iter().zip(actual.lines()) {
            assert_eq!(
                policy.official_vendor(id, name, path).unwrap_or("none"),
                vendor,
                "{id} {name} {path}"
            );
        }
        println!(
            "Compared {} contexts against pinned official_uninstaller_vendor",
            cases.len()
        );
    }
}
