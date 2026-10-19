//! Pinned uninstall-mode path subset. No whitelist, full alias resolution,
//! cleanup data policy or execution eligibility is inferred from a non-match.
use super::*;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    version: u32,
    revision: &'static str,
    source: &'static str,
    pub code: &'static str,
    reason: Option<&'static str>,
    issue: Option<&'static str>,
}
impl Observation {
    pub fn unknown(issue: &'static str) -> Self {
        Self::result("unknown", None, Some(issue))
    }
    fn result(
        code: &'static str,
        reason: Option<&'static str>,
        issue: Option<&'static str>,
    ) -> Self {
        Self {
            version: 2,
            revision: crate::engine::REVISION,
            source: "moleUninstallPathSubset",
            code,
            reason,
            issue,
        }
    }
}
#[derive(Debug)]
enum Atom {
    Literal(u8),
    Home,
    Any,
    Star,
    Class(Vec<(u8, u8)>),
}
#[derive(Debug)]
struct Pattern(Vec<Atom>);
impl Pattern {
    fn parse(raw: &str) -> Result<Self, String> {
        let bytes = raw.trim().as_bytes();
        let mut tokens = vec![];
        let mut i = 0;
        let mut quoted = false;
        while i < bytes.len() {
            let b = bytes[i];
            match b {
                b'"' => {
                    quoted = !quoted;
                    i += 1;
                }
                b'$' if quoted
                    && bytes[i..].starts_with(b"$HOME")
                    && bytes.get(i + 5).is_none_or(|b| matches!(b, b'/' | b'"')) =>
                {
                    tokens.push(Atom::Home);
                    i += 5;
                }
                b'$' | b'`' | b'\'' => return Err("不支持的路径规则插值。".into()),
                b'\\' if !quoted => {
                    i += 1;
                    tokens.push(Atom::Literal(*bytes.get(i).ok_or("路径规则转义未结束。")?));
                    i += 1;
                }
                b'*' if !quoted => {
                    tokens.push(Atom::Star);
                    i += 1;
                }
                b'?' if !quoted => {
                    tokens.push(Atom::Any);
                    i += 1;
                }
                b'[' if !quoted => {
                    let end = bytes[i + 1..]
                        .iter()
                        .position(|b| *b == b']')
                        .ok_or("路径字符类未结束。")?
                        + i
                        + 1;
                    let content = &bytes[i + 1..end];
                    let mut ranges = vec![];
                    let mut n = 0;
                    while n < content.len() {
                        let lo = content[n];
                        if !lo.is_ascii_alphanumeric() {
                            return Err("路径字符类暂不支持。".into());
                        }
                        if content.get(n + 1) == Some(&b'-') {
                            let hi = *content.get(n + 2).ok_or("路径范围未结束。")?;
                            if !hi.is_ascii_alphanumeric() || hi < lo {
                                return Err("路径范围暂不支持。".into());
                            }
                            ranges.push((lo, hi));
                            n += 3;
                        } else {
                            ranges.push((lo, lo));
                            n += 1;
                        }
                    }
                    if ranges.is_empty() {
                        return Err("路径字符类为空。".into());
                    }
                    tokens.push(Atom::Class(ranges));
                    i = end + 1;
                }
                b if !quoted && b.is_ascii_whitespace() => {
                    return Err("路径规则存在未转义空白。".into())
                }
                b if b < 32 || b == 127 => return Err("路径规则存在控制字符。".into()),
                b => {
                    tokens.push(Atom::Literal(b));
                    i += 1;
                }
            }
        }
        if quoted || tokens.is_empty() || tokens.len() > 512 {
            return Err("路径规则未结束或超限。".into());
        }
        Ok(Self(tokens))
    }
    // Bash [[ ]] with LC_ALL=C: wildcards may consume '/' and dot bytes.
    fn matches(&self, input: &[u8], home: &[u8]) -> bool {
        let (mut i, mut j) = (0, 0);
        let mut star = None;
        loop {
            if let Some(Atom::Star) = self.0.get(j) {
                j += 1;
                star = Some((j, i));
                continue;
            }
            let consumed = match self.0.get(j) {
                Some(Atom::Home) if input[i..].starts_with(home) => Some(home.len()),
                Some(Atom::Literal(b)) if input.get(i) == Some(b) => Some(1),
                Some(Atom::Any) if i < input.len() => Some(1),
                Some(Atom::Class(ranges))
                    if input
                        .get(i)
                        .is_some_and(|b| ranges.iter().any(|(lo, hi)| lo <= b && b <= hi)) =>
                {
                    Some(1)
                }
                _ => None,
            };
            if let Some(n) = consumed {
                i += n;
                j += 1;
                continue;
            }
            if i == input.len() && j == self.0.len() {
                return true;
            }
            if let Some((next, retry)) = star {
                if retry < input.len() {
                    i = retry + 1;
                    j = next;
                    star = Some((next, i));
                    continue;
                }
            }
            return false;
        }
    }
}
struct Case(Vec<(Vec<Pattern>, bool)>);
impl Case {
    fn parse(raw: &str) -> Result<Self, String> {
        let cleaned = raw
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n")
            .replace("\\\n", "");
        let mut rules = vec![];
        for clause in cleaned.split(";;") {
            let clause = clause.trim();
            if clause.is_empty() {
                continue;
            }
            let (patterns, action) = clause.split_once(')').ok_or("路径分支未结束。")?;
            let action = match action.trim() {
                "return 0" | "matched=true" => true,
                "" => false,
                _ => return Err("路径分支动作暂不支持。".into()),
            };
            let patterns = patterns
                .split('|')
                .map(Pattern::parse)
                .collect::<Result<Vec<_>, _>>()?;
            if patterns.is_empty() || patterns.len() > 128 {
                return Err("路径分支数量不受支持。".into());
            }
            rules.push((patterns, action));
        }
        if rules.is_empty() || rules.len() > 64 {
            return Err("路径规则为空或超限。".into());
        }
        Ok(Self(rules))
    }
    fn ascii_lowercase(&mut self) {
        for (patterns, _) in &mut self.0 {
            for pattern in patterns {
                for atom in &mut pattern.0 {
                    if let Atom::Literal(byte) = atom {
                        *byte = byte.to_ascii_lowercase();
                    }
                }
            }
        }
    }
    fn protected(&self, path: &[u8], home: &[u8]) -> bool {
        self.0
            .iter()
            .find_map(|(patterns, protect)| {
                patterns
                    .iter()
                    .any(|p| p.matches(path, home))
                    .then_some(*protect)
            })
            .unwrap_or(false)
    }
}
fn function<'a>(source: &'a str, name: &str) -> Result<&'a str, String> {
    let declaration = format!("\n{name}() {{");
    if source.matches(&declaration).count() != 1 {
        return Err("路径函数声明无法确认。".into());
    }
    source
        .split_once(&declaration)
        .and_then(|(_, body)| body.split_once("\n}"))
        .map(|(body, _)| body)
        .ok_or("路径函数未结束。".into())
}
fn cases(source: &str) -> Vec<&str> {
    source
        .split("case \"$path\" in")
        .skip(1)
        .filter_map(|block| block.split_once("esac").map(|(body, _)| body))
        .collect()
}
pub(super) struct Policy {
    shared: Case,
    runtime: Case,
    system: Vec<Case>,
    critical_data: Case,
    endpoints: Vec<String>,
}
impl Policy {
    pub(super) fn parse(logic: &str, data: &str) -> Result<Self, String> {
        let shared = cases(function(logic, "_mole_is_shared_home_state_root")?);
        let runtime = cases(function(logic, "is_orbstack_runtime_path")?);
        let path = cases(function(logic, "should_protect_path")?);
        if shared.len() != 1 || runtime.len() != 1 || path.len() != 6 {
            return Err("路径规则结构不受支持。".into());
        }
        let endpoints = patterns(data, "ENDPOINT_SECURITY_BUNDLE_PREFIXES")?;
        if endpoints.len() > 32 || endpoints.iter().any(|p| p.contains('*')) {
            return Err("终端安全规则暂不支持。".into());
        }
        let mut runtime = Case::parse(runtime[0])?;
        runtime.ascii_lowercase();
        Ok(Self {
            shared: Case::parse(shared[0])?,
            runtime,
            system: path[2..5]
                .iter()
                .map(|body| Case::parse(body))
                .collect::<Result<_, _>>()?,
            critical_data: Case::parse(path[5])?,
            endpoints,
        })
    }
    fn protected(&self, path: &str, home: &str) -> Option<&'static str> {
        if self.shared.protected(
            path.strip_suffix('/').unwrap_or(path).as_bytes(),
            home.as_bytes(),
        ) {
            return Some("sharedRoot");
        }
        if self.runtime.protected(
            path.to_ascii_lowercase().as_bytes(),
            home.to_ascii_lowercase().as_bytes(),
        ) {
            return Some("containerRuntime");
        }
        if self
            .system
            .iter()
            .any(|rule| rule.protected(path.as_bytes(), home.as_bytes()))
        {
            return Some("systemComponent");
        }
        if (path.starts_with("/private/var/folders/") || path.starts_with("/var/folders/"))
            && self
                .endpoints
                .iter()
                .any(|prefix| path.to_ascii_lowercase().contains(prefix))
        {
            return Some("endpointSecurity");
        }
        if self
            .critical_data
            .protected(path.as_bytes(), home.as_bytes())
        {
            return Some("criticalData");
        }
        // In this pinned policy, SYSTEM_CRITICAL_BUNDLES and its Apple exceptions
        // contain only ID prefixes, never a leading '/' or '*'. They cannot
        // match our absolute path input. Cleanup-only branches stay excluded.
        None
    }
}
pub(super) fn observe(
    policy: Option<&Policy>,
    home: &Path,
    path: &Path,
    location: &str,
) -> Observation {
    if home == Path::new("/") {
        return Observation::unknown("pathUnsupported");
    }
    let valid = |p: &Path| {
        p.to_str()
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 4096
                    && !s.chars().any(char::is_control)
                    && p.is_absolute()
                    && !s.contains("//")
                    && p.components().all(|c| {
                        matches!(
                            c,
                            std::path::Component::RootDir | std::path::Component::Normal(_)
                        )
                    })
                    && !s.split('/').any(|part| matches!(part, "." | ".."))
            })
            .map(str::to_owned)
    };
    let (Some(home), Some(path)) = (valid(home), valid(path)) else {
        return Observation::unknown("pathUnsupported");
    };
    let Some(policy) = policy else {
        return Observation::unknown("rulesUnavailable");
    };
    if let Some(reason) = policy.protected(&path, &home) {
        return Observation::result("protected", Some(reason), None);
    }
    if !matches!(location, "present" | "notFound") {
        return Observation::unknown("locationUnverified");
    }
    Observation::result("notMatched", None, None)
}

/// One shared checkpoint budget for both main-ID and extension locations.
pub struct IdentityBudget {
    pub(super) started: std::time::Instant,
    pub(super) attempts: usize,
}
impl Default for IdentityBudget {
    fn default() -> Self {
        Self {
            started: std::time::Instant::now(),
            attempts: 0,
        }
    }
}
impl IdentityBudget {
    pub(super) fn expired(&self) -> bool {
        self.started.elapsed() > std::time::Duration::from_millis(500)
    }
    pub(super) fn take(&mut self) -> bool {
        if self.attempts >= 64 || self.expired() {
            return false;
        }
        self.attempts += 1;
        true
    }
}
pub(super) fn observe_with_identity(
    policy: Option<&Policy>,
    home: &Path,
    path: &Path,
    location: &str,
    budget: &mut IdentityBudget,
) -> Observation {
    observe_with_root(
        policy,
        home,
        path,
        location,
        budget,
        Path::new("/private/var/folders"),
    )
}
fn observe_with_root(
    policy: Option<&Policy>,
    home: &Path,
    path: &Path,
    location: &str,
    budget: &mut IdentityBudget,
    root: &Path,
) -> Observation {
    let literal = observe(policy, home, path, location);
    // Only the endpoint-security rule has an existing-root callback upstream.
    // Other positive rules retain precedence. Unsupported inputs/resources never probe.
    if literal.code == "protected"
        || matches!(literal.issue, Some("rulesUnavailable" | "pathUnsupported"))
    {
        return literal;
    }
    let Some(policy) = policy else {
        return literal;
    };
    let lowered = path.to_str().unwrap().to_ascii_lowercase();
    if !policy
        .endpoints
        .iter()
        .any(|prefix| lowered.contains(prefix))
    {
        return literal;
    }
    match super::identity::within(path, root, budget) {
        Ok(true) => Observation::result("protected", Some("endpointSecurityIdentity"), None),
        Ok(false) => literal,
        Err(issue) => Observation::unknown(issue),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> Policy {
        let mut runtime=Case::parse(r#"*/Library/Group\ Containers/*dev.orbstack | */Library/Group\ Containers/*dev.orbstack/* | */.orbstack | */.orbstack/*) matched=true ;;"#).unwrap();
        runtime.ascii_lowercase();
        Policy {
            shared:Case::parse(r#""$HOME"/.[Cc][Oo][Nn][Ff][Ii][Gg]) return 0 ;;"#).unwrap(),
            runtime,
            system:vec![Case::parse(r#"*[Ss]ystem[Ss]ettings* | *com.apple.finder*) return 0 ;;"#).unwrap()],
            critical_data:Case::parse(r#"*/Codex/pending/*/*) return 0 ;; */Codex/pending/?*) ;; */Codex | */Codex/* | */Library/Preferences/plugin.[0-9][0-9].plist) return 0 ;;"#).unwrap(),
            endpoints:vec!["com.security.".into()],
        }
    }
    #[test]
    fn glob_order_quoted_home_case_and_ranges_follow_literal_uninstall_semantics() {
        let p = policy();
        let home = "/Users/name[*]?";
        for (path, reason) in [
            (format!("{home}/.Config/"), "sharedRoot"),
            (
                format!("{home}/Library/Group Containers/TEAM.DEV.ORBSTACK/data"),
                "containerRuntime",
            ),
            (format!("{home}/.OrbStack"), "containerRuntime"),
            (
                format!("{home}/Library/Caches/org.SystemSettings.app"),
                "systemComponent",
            ),
            (format!("{home}/Codex"), "criticalData"),
            (format!("{home}/Codex/pending/deeper/file"), "criticalData"),
            (
                format!("{home}/Library/Preferences/plugin.12.plist"),
                "criticalData",
            ),
            (
                "/private/var/folders/a/C/COM.SECURITY.agent".into(),
                "endpointSecurity",
            ),
        ] {
            assert_eq!(p.protected(&path, home), Some(reason), "{path}");
        }
        for path in [
            format!("{home}/.config/child"),
            format!("{home}/Codex/pending/a"),
            format!("{home}/Library/Caches/com.jetbrains.PyCharm"),
            format!("{home}/Library/Preferences/plugin.1.plist"),
            "/Users/nameX/.config".into(),
            "/other/private/var/folders/a/C/com.security.agent".into(),
        ] {
            assert!(p.protected(&path, home).is_none(), "{path}");
        }
        assert!(Pattern::parse(r#""$HOME/Cache/*""#)
            .unwrap()
            .matches(format!("{home}/Cache/*").as_bytes(), home.as_bytes()));
        assert!(!Pattern::parse(r#""$HOME/Cache/*""#)
            .unwrap()
            .matches(format!("{home}/Cache/data").as_bytes(), home.as_bytes()));
        assert!(Pattern::parse("*/a?b*")
            .unwrap()
            .matches(b"/x/azb/y", b"/unused"));
    }
    #[test]
    fn unknown_rules_locations_and_unsupported_paths_do_not_turn_into_no_match() {
        let p = policy();
        let home = Path::new("/Users/test");
        assert_eq!(
            observe(None, home, &home.join("Library/Caches/a"), "present").issue,
            Some("rulesUnavailable")
        );
        assert_eq!(
            observe(Some(&p), home, &home.join("Library/Caches/a"), "unknown").issue,
            Some("locationUnverified")
        );
        for state in ["present", "notFound"] {
            assert_eq!(
                observe(Some(&p), home, &home.join("Library/Caches/a"), state).code,
                "notMatched"
            );
        }
        for state in ["present", "notFound", "unknown"] {
            assert_eq!(
                observe(
                    Some(&p),
                    home,
                    &home.join("Library/Caches/com.apple.finder"),
                    state
                )
                .code,
                "protected"
            );
        }
        for path in ["relative", "/x/../y", "/x/./y", "/x//y", "/x\ny"] {
            assert_eq!(
                observe(Some(&p), home, Path::new(path), "present").issue,
                Some("pathUnsupported")
            );
        }
        assert_eq!(
            observe(Some(&p), Path::new("/"), Path::new("/a"), "present").issue,
            Some("pathUnsupported")
        );
    }
    #[test]
    #[cfg(target_os = "macos")]
    fn endpoint_identity_is_scoped_retains_missing_descendants_and_does_not_clear_other_guards() {
        let t = tempfile::tempdir_in("/private/tmp").unwrap();
        let root = t.path().join("root");
        let data = root.join("com.security.agent");
        std::fs::create_dir_all(&data).unwrap();
        let p = policy();
        for (path, status) in [
            (&data, "present"),
            (&data.join("missing"), "notFound"),
            (&data, "unknown"),
        ] {
            let result = observe_with_root(
                Some(&p),
                t.path(),
                path,
                status,
                &mut IdentityBudget::default(),
                &root,
            );
            assert_eq!(result.code, "protected");
            assert_eq!(result.reason, Some("endpointSecurityIdentity"));
        }
        let outside = t.path().join("com.security.outside");
        std::fs::create_dir(&outside).unwrap();
        assert_eq!(
            observe_with_root(
                Some(&p),
                t.path(),
                &outside,
                "present",
                &mut IdentityBudget::default(),
                &root
            )
            .code,
            "notMatched"
        );
        assert_eq!(
            observe_with_root(
                Some(&p),
                t.path(),
                &outside.join("missing"),
                "notFound",
                &mut IdentityBudget::default(),
                &root
            )
            .issue,
            Some("identityUnverified")
        );
        let mut expired = IdentityBudget {
            started: std::time::Instant::now(),
            attempts: 64,
        };
        assert_eq!(
            observe_with_root(Some(&p), t.path(), &data, "present", &mut expired, &root).issue,
            Some("identityLimitReached")
        );
        assert_eq!(
            observe_with_root(
                Some(&p),
                t.path(),
                &t.path().join("plain"),
                "notFound",
                &mut expired,
                &root
            )
            .code,
            "notMatched"
        );
        assert_eq!(
            observe_with_root(
                Some(&p),
                t.path(),
                &t.path().join("com.apple.finder.com.security."),
                "unknown",
                &mut expired,
                &root
            )
            .code,
            "protected"
        );
        assert_eq!(
            observe_with_root(None, t.path(), &data, "present", &mut expired, &root).issue,
            Some("rulesUnavailable")
        );
    }
    #[test]
    fn unsupported_shell_patterns_and_actions_are_not_evaluated() {
        for pattern in [
            "",
            "$HOME/x",
            "\"$OTHER/x\"",
            "\"$(anything)\"",
            "[!a]",
            "[z-a]",
            "[0-]",
            "[ab",
            "unquoted space",
            "'literal'",
        ] {
            assert!(Pattern::parse(pattern).is_err(), "{pattern}");
        }
        for body in [
            "",
            "x) echo anything ;;",
            "x) return 1 ;;",
            "x) return 0; touch anything ;;",
            "missing",
        ] {
            assert!(Case::parse(body).is_err(), "{body}");
        }
        assert!(Policy::parse("", "").is_err());
    }
    #[test]
    #[cfg(target_os = "macos")]
    fn retained_missing_locations_and_latest_client_guards_remain_independent() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let mut outer=super::super::Policy::parse("readonly SYSTEM_CRITICAL_BUNDLES=(\n \"com.apple.*\"\n)\nreadonly APPLE_UNINSTALLABLE_APPS=(\n \"com.apple.dt.*\"\n)\nreadonly OFFICIAL_UNINSTALLER_RULES=(\n \"Vendor|com.vendor.|\"\n)\n").unwrap();
        outer.paths = Some(policy());
        let mut locations = crate::app_associations::mac::Check::begin(
            &home,
            Some("com.apple.finder"),
            "matches",
            true,
        )
        .finish();
        locations.attach_mole_paths(Some(&outer), &home, &mut IdentityBudget::default());
        locations
            .attach_protection(
                &home,
                Some("com.apple.finder"),
                "org.client.app",
                &[home.join("Library/Caches").to_str().unwrap().into()],
            )
            .unwrap();
        let value = serde_json::to_value(&locations).unwrap();
        assert_eq!(value["entries"][1]["status"], "notFound");
        assert_eq!(
            value["entries"][1]["molePathProtection"]["code"],
            "protected"
        );
        assert_eq!(
            value["entries"][1]["protectionReasons"][0],
            "clientDirectory"
        );
        locations.attach_mole_paths(None, &home, &mut IdentityBudget::default());
        let value = serde_json::to_value(&locations).unwrap();
        assert_eq!(
            value["entries"][1]["molePathProtection"]["issue"],
            "rulesUnavailable"
        );
        assert_eq!(
            value["entries"][1]["protectionReasons"][0],
            "clientDirectory"
        );
        let mut data = crate::app_associations::mac::extensions::Check::begin(
            &home,
            vec!["com.apple.finder.share".into()],
            "observed",
            None,
        )
        .finish();
        data.attach_mole_paths(Some(&outer), &home, &mut IdentityBudget::default());
        let value = serde_json::to_value(data).unwrap();
        assert!(value["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["molePathProtection"]["code"] == "protected" && e["status"] == "notFound"));
    }
    #[test]
    #[ignore = "requires pinned Mole source; compares only uninstall literal rules, with filesystem alias callback absent"]
    fn prepared_literal_paths_match_original_uninstall_mode_without_cleanup_overreach() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole");
        let loaded = super::super::load(&root).unwrap();
        let policy = loaded.paths.as_ref().unwrap();
        let logic = std::fs::read_to_string(root.join("lib/core/app_protection.sh")).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap().join("literal[*]?");
        let home = home.to_str().unwrap();
        let mut paths = vec![];
        for case in std::iter::once(&policy.shared)
            .chain(std::iter::once(&policy.runtime))
            .chain(policy.system.iter())
            .chain(std::iter::once(&policy.critical_data))
        {
            for (patterns, _) in &case.0 {
                for pattern in patterns {
                    for fill in ["", "item", "中文"] {
                        let mut sample = vec![];
                        for atom in &pattern.0 {
                            match atom {
                                Atom::Literal(b) => sample.push(*b),
                                Atom::Home => sample.extend(home.as_bytes()),
                                Atom::Any => sample.push(b'a'),
                                Atom::Star => sample.extend(fill.as_bytes()),
                                Atom::Class(ranges) => sample.push(ranges[0].0),
                            }
                        }
                        let sample = String::from_utf8(sample).unwrap();
                        let path = if sample.starts_with('/') {
                            sample
                        } else {
                            format!("{home}/Library/Caches/{sample}")
                        };
                        if path.contains("//") {
                            continue;
                        }
                        paths.extend([
                            path.clone(),
                            format!("{path}/child"),
                            format!("{path}.extra"),
                            path.to_ascii_lowercase(),
                            path.to_ascii_uppercase(),
                        ]);
                    }
                }
            }
        }
        for prefix in &policy.endpoints {
            paths.extend([
                format!("/private/var/folders/a/C/{prefix}agent"),
                format!(
                    "/var/folders/a/X/{}.code_sign_clone",
                    prefix.to_ascii_uppercase()
                ),
                format!("/other/var/folders/a/C/{prefix}"),
            ]);
        }
        paths.extend([
            format!("{home}/Library/Caches/com.jetbrains.PyCharm"),
            format!("{home}/Library/Preferences/com.microsoft.app.plist"),
            format!("{home}/Library/Application Support/Codex/Crashpad/pending/file"),
            format!("{home}/Library/Application Support/Codex/Crashpad/pending/file/nested"),
            format!("{home}/Library/Group Containers/TEAM.DEV.ORBSTACK/runtime"),
        ]);
        paths.sort();
        paths.dedup();
        let mut script = "set -euo pipefail\nMOLE_UNINSTALL_MODE=1\n".to_owned();
        for (name, values) in [
            ("SYSTEM_CRITICAL_BUNDLES", &loaded.critical),
            ("APPLE_UNINSTALLABLE_APPS", &loaded.allowed),
            ("ENDPOINT_SECURITY_BUNDLE_PREFIXES", &policy.endpoints),
        ] {
            script.push_str(&format!("{name}=(\n"));
            for value in values {
                script.push_str(&format!("\"{value}\"\n"));
            }
            script.push_str(")\n");
        }
        for name in [
            "_mole_is_shared_home_state_root",
            "is_orbstack_runtime_path",
            "is_endpoint_security_cache_path",
            "should_protect_path",
        ] {
            // Avoid changing HOME: substitute the isolated fixture variable in
            // extracted test-only function text, preserving its original quotes.
            script.push_str(
                &crate::app_background::test_function(&logic, name)
                    .replace("$HOME", "$MOOMOLE_TEST_ROOT"),
            );
            script.push('\n');
        }
        script.push_str("for path do if should_protect_path \"$path\"; then printf '1'; else status=$?; [[ $status == 1 ]] || exit 2; printf '0'; fi; done");
        let output = std::process::Command::new("/bin/bash")
            .args(["-c", &script, "literal-paths"])
            .args(&paths)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .env("MOOMOLE_TEST_ROOT", home)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout.len(), paths.len());
        for (path, code) in paths.iter().zip(output.stdout) {
            assert_eq!(
                policy.protected(path, home).is_some(),
                code == b'1',
                "{path}"
            );
        }
        println!(
            "Compared {} paths against original uninstall-mode literal rules",
            paths.len()
        );
    }
}
