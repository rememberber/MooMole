use super::*;
use quick_xml::{events::Event, name::ResolveResult, NsReader};
use std::collections::HashMap;
const SPARKLE: &str = "http://www.andymatuschak.org/xml-namespaces/sparkle";
#[derive(Default)]
struct Item {
    fields: HashMap<String, String>,
    attributes: HashMap<String, String>,
    enclosure: Option<String>,
    page: Option<String>,
    bytes: Option<u64>,
    signature: Option<String>,
    invalid: bool,
    foreign: bool,
}
struct Node {
    name: String,
    sparkle: bool,
    plain: bool,
    text: String,
}
fn field(s: &str) -> bool {
    [
        "version",
        "shortVersionString",
        "minimumSystemVersion",
        "maximumSystemVersion",
        "minimumUpdateVersion",
        "hardwareRequirements",
        "channel",
        "minimumAutoupdateVersion",
    ]
    .contains(&s)
}
fn clean(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && !s.chars().any(|c| {
            c.is_control()
                || ('\u{202a}'..='\u{202e}').contains(&c)
                || ('\u{2066}'..='\u{2069}').contains(&c)
        })
}
pub(super) fn parse(
    bytes: &[u8],
    build: &str,
    os: &str,
    arch: &str,
    job: &Job,
) -> Result<(Vec<Release>, usize, usize, bool), String> {
    if bytes.len() > 1024 * 1024 || std::str::from_utf8(bytes).is_err() {
        return Err("更新来源必须是 1 MiB 内的 UTF-8 XML。".into());
    }
    let current = crate::application_update::version(build)?;
    let os = crate::application_update::version(os)?;
    let mut reader = NsReader::from_reader(bytes);
    reader.config_mut().expand_empty_elements = true;
    let mut stack: Vec<Node> = vec![];
    let mut item: Option<Item> = None;
    let mut roots = 0;
    let mut channels = 0;
    let mut inspected = 0;
    let mut skipped = 0;
    let mut complete = true;
    let mut releases = vec![];
    let mut events = 0;
    loop {
        check(job)?;
        events += 1;
        if events > 30000 {
            return Err("更新来源 XML 事件超过本轮上限。".into());
        }
        let (ns, event) = reader
            .read_resolved_event()
            .map_err(|_| "更新来源 XML 结构无法核对。")?;
        let sparkle = matches!(ns,ResolveResult::Bound(v) if v.as_ref()==SPARKLE);
        let plain = matches!(ns, ResolveResult::Unbound);
        if matches!(ns, ResolveResult::Unknown(_)) {
            return Err("更新来源包含未绑定的 XML 命名空间。".into());
        }
        match event {
            Event::Start(e) => {
                if stack.len() >= 32 {
                    return Err("更新来源 XML 层级超过上限。".into());
                }
                let name = e.local_name().as_ref().to_string();
                if stack.is_empty() {
                    if !plain || name != "rss" || roots != 0 {
                        return Err("更新来源不是本轮支持的 RSS appcast。".into());
                    }
                    roots += 1;
                }
                if stack.len() == 1 && plain && name == "channel" {
                    channels += 1;
                }
                if stack.len() == 2
                    && plain
                    && name == "item"
                    && stack[1].plain
                    && stack[1].name == "channel"
                {
                    if inspected >= 128 {
                        return Err("更新来源条目超过本轮 128 项上限。".into());
                    }
                    item = Some(Item::default());
                }
                if let Some(ref mut i) = item {
                    if stack.last().is_some_and(|n| n.sparkle && field(&n.name)) {
                        i.invalid = true;
                    }
                    if stack.len() == 3
                        && sparkle
                        && !field(&name)
                        && ![
                            "releaseNotesLink",
                            "fullReleaseNotesLink",
                            "tags",
                            "deltas",
                            "criticalUpdate",
                            "phasedRolloutInterval",
                        ]
                        .contains(&name.as_str())
                    {
                        i.invalid = true;
                    }
                    if stack.len() == 3 && plain && name == "enclosure" {
                        let mut attrs = HashMap::<String, String>::new();
                        for attr in e.attributes() {
                            let a = attr.map_err(|_| "更新条目属性重复或损坏。")?;
                            let (ns, local) = reader.resolver().resolve_attribute(a.key);
                            let key = local.as_ref().to_string();
                            let prefix = match ns {
                                ResolveResult::Unbound => "",
                                ResolveResult::Bound(v) if v.as_ref() == SPARKLE => "sparkle:",
                                ResolveResult::Unknown(_) => {
                                    return Err("更新属性命名空间无法核对。".into())
                                }
                                _ => "other:",
                            };
                            let value = a
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .map_err(|_| "更新属性内容无法核对。")?
                                .into_owned();
                            if value.len() > 4096 {
                                return Err("更新属性内容超过上限。".into());
                            }
                            if attrs.insert(format!("{prefix}{key}"), value).is_some() {
                                i.invalid = true;
                            }
                            if prefix == "sparkle:"
                                && ![
                                    "os",
                                    "version",
                                    "shortVersionString",
                                    "edSignature",
                                    "dsaSignature",
                                    "deltaFrom",
                                    "installationType",
                                ]
                                .contains(&key.as_str())
                            {
                                i.invalid = true;
                            }
                        }
                        if attrs.get("sparkle:os").is_some_and(|v| v != "macos") {
                            i.foreign = true;
                        } else {
                            if i.enclosure.is_some() {
                                i.invalid = true;
                            }
                            i.enclosure = attrs.get("url").cloned();
                            i.signature = attrs.get("sparkle:edSignature").cloned();
                            if let Some(raw) = attrs.get("length") {
                                match raw.parse::<u64>() {
                                    // Some deployed feeds use zero for an unknown size.
                                    // This is discovery only, never download verification.
                                    Ok(0) => i.bytes = None,
                                    Ok(n) if n > 0 && n <= 16 * 1024 * 1024 * 1024 => {
                                        i.bytes = Some(n)
                                    }
                                    _ => i.invalid = true,
                                }
                            }
                            for key in ["version", "shortVersionString"] {
                                if let Some(v) = attrs.get(&format!("sparkle:{key}")) {
                                    i.attributes.insert(key.into(), v.clone());
                                }
                            }
                            if attrs.contains_key("sparkle:deltaFrom") {
                                i.invalid = true;
                            }
                        }
                    }
                }
                stack.push(Node {
                    name,
                    sparkle,
                    plain,
                    text: String::new(),
                });
            }
            Event::Text(e) => {
                let depth = stack.len();
                if let Some(n) = stack.last_mut() {
                    if depth == 4 && ((n.sparkle && field(&n.name)) || n.plain && n.name == "link")
                    {
                        let raw = e.as_ref();
                        n.text.push_str(
                            &quick_xml::escape::unescape(raw)
                                .map_err(|_| "更新文本实体无法核对。")?,
                        );
                        if n.text.len() > 4096 {
                            return Err("更新文本超过上限。".into());
                        }
                    }
                }
            }
            Event::CData(e) => {
                let depth = stack.len();
                if let Some(n) = stack.last_mut() {
                    if depth == 4 && ((n.sparkle && field(&n.name)) || n.plain && n.name == "link")
                    {
                        n.text.push_str(e.as_ref());
                        if n.text.len() > 4096 {
                            return Err("更新文本超过上限。".into());
                        }
                    }
                }
            }
            Event::GeneralRef(e) => {
                if stack.len() == 4 {
                    if let Some(n) = stack.last_mut() {
                        if n.sparkle && field(&n.name) || n.plain && n.name == "link" {
                            let token = format!("&{};", e.as_ref());
                            n.text.push_str(
                                &quick_xml::escape::unescape(&token)
                                    .map_err(|_| "更新文本实体无法核对。")?,
                            );
                        }
                    }
                }
            }
            Event::End(_) => {
                let n = stack.pop().ok_or("更新 XML 结束位置不匹配。")?;
                if stack.len() == 3 {
                    if let Some(i) = item.as_mut() {
                        let text = n.text.trim().to_string();
                        if n.sparkle && field(&n.name) {
                            if i.fields.insert(n.name.clone(), text).is_some() {
                                i.invalid = true;
                            }
                        } else if n.plain && n.name == "link" && i.page.replace(text).is_some() {
                            i.invalid = true;
                        }
                    }
                }
                if stack.len() == 2 && n.plain && n.name == "item" {
                    let i = item.take().ok_or("更新条目范围无法核对。")?;
                    inspected += 1;
                    match release(i, &current, &os, arch) {
                        Ok(Some(r)) => releases.push(r),
                        Ok(None) => skipped += 1,
                        Err(()) => {
                            skipped += 1;
                            complete = false;
                        }
                    }
                }
            }
            Event::DocType(_) => return Err("更新来源不支持 DTD 或自定义实体。".into()),
            Event::Decl(e) => {
                if e.version().map_or(true, |v| v != "1.0") {
                    return Err("更新来源 XML 版本尚未支持。".into());
                }
                if e.encoding()
                    .is_some_and(|v| v.is_err() || !v.unwrap().eq_ignore_ascii_case("utf-8"))
                {
                    return Err("更新来源 XML 编码尚未支持。".into());
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if roots != 1 || channels != 1 || !stack.is_empty() || item.is_some() {
        return Err("更新来源 RSS 结构不完整。".into());
    }
    releases.sort_by(|a, b| {
        crate::application_update::version(&b.build)
            .unwrap()
            .cmp(&crate::application_update::version(&a.build).unwrap())
    });
    for i in 0..releases.len() {
        for j in i + 1..releases.len() {
            if releases[i].build == releases[j].build
                && releases[i].url == releases[j].url
                && (releases[i].signature != releases[j].signature
                    || releases[i].bytes != releases[j].bytes)
            {
                releases[i].downloadable = false;
                releases[j].downloadable = false;
                complete = false;
            }
        }
    }
    releases.dedup_by(|a, b| a.build == b.build && a.url == b.url);
    if inspected == 0 {
        complete = false;
    }
    Ok((releases, inspected, skipped, complete))
}
fn release(
    mut i: Item,
    current: &Vec<u64>,
    os: &Vec<u64>,
    arch: &str,
) -> Result<Option<Release>, ()> {
    if i.invalid {
        return Err(());
    }
    if i.fields.get("channel").is_some_and(|v| !v.is_empty()) {
        return Ok(None);
    }
    if i.foreign && i.enclosure.is_none() {
        return Ok(None);
    }
    for (k, v) in i.attributes {
        if let Some(old) = i.fields.get(&k) {
            if old != &v {
                return Err(());
            }
        } else {
            i.fields.insert(k, v);
        }
    }
    let build = i.fields.get("version").ok_or(())?;
    let number = crate::application_update::version(build).map_err(|_| ())?;
    if number <= *current {
        return Ok(None);
    }
    for key in [
        "minimumSystemVersion",
        "maximumSystemVersion",
        "minimumUpdateVersion",
    ] {
        if let Some(v) = i.fields.get(key) {
            let v = crate::application_update::version(v).map_err(|_| ())?;
            if match key {
                "minimumSystemVersion" => v > *os,
                "maximumSystemVersion" => v < *os,
                _ => v > *current,
            } {
                return Ok(None);
            }
        }
    }
    if let Some(v) = i.fields.get("hardwareRequirements") {
        if !["arm64", "x86_64"].contains(&v.as_str()) {
            return Err(());
        }
        if v != arch {
            return Ok(None);
        }
    }
    let requires_review = if let Some(v) = i.fields.get("minimumAutoupdateVersion") {
        crate::application_update::version(v).map_err(|_| ())? > *current
    } else {
        false
    };
    let version = i.fields.get("shortVersionString").unwrap_or(build).clone();
    if !clean(&version, 128) {
        return Err(());
    }
    let (raw, kind) = if let Some(v) = i.enclosure {
        (v, "package")
    } else {
        (i.page.ok_or(())?, "page")
    };
    let url = network::url(&raw).map_err(|_| ())?.to_string();
    let downloadable = kind == "package"
        && i.bytes.is_some_and(|n| n <= download::MAX_BYTES)
        && i.signature
            .as_deref()
            .is_some_and(|s| download::signature(s).is_ok())
        && download::extension(&url).is_ok();
    Ok(Some(Release {
        id: uuid::Uuid::new_v4().to_string(),
        version,
        build: build.clone(),
        url,
        kind,
        bytes: i.bytes,
        requires_review,
        signature: i.signature,
        downloadable,
    }))
}
