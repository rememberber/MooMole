use super::*;
use quick_xml::{events::Event, name::ResolveResult, NsReader};
pub(super) struct Node {
    pub ns: String,
    pub name: String,
    pub attrs: HashMap<(String, String), String>,
    pub text: String,
    pub children: Vec<Node>,
}
fn matches(ns: &str, kind: &str) -> bool {
    match kind {
        "w" => [
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
            "http://purl.oclc.org/ooxml/wordprocessingml/main",
        ]
        .contains(&ns),
        "s" => [
            "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
            "http://purl.oclc.org/ooxml/spreadsheetml/main",
        ]
        .contains(&ns),
        "p" => [
            "http://schemas.openxmlformats.org/presentationml/2006/main",
            "http://purl.oclc.org/ooxml/presentationml/main",
        ]
        .contains(&ns),
        "a" => [
            "http://schemas.openxmlformats.org/drawingml/2006/main",
            "http://purl.oclc.org/ooxml/drawingml/main",
        ]
        .contains(&ns),
        "r" => [
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
            "http://purl.oclc.org/ooxml/officeDocument/relationships",
        ]
        .contains(&ns),
        "rel" => ns == "http://schemas.openxmlformats.org/package/2006/relationships",
        "ct" => ns == "http://schemas.openxmlformats.org/package/2006/content-types",
        "" => ns.is_empty(),
        _ => false,
    }
}
impl Node {
    pub fn is(&self, kind: &str, name: &str) -> bool {
        self.name == name && matches(&self.ns, kind)
    }
    pub fn require(&self, kind: &str, name: &str) -> Result<(), String> {
        if self.is(kind, name) {
            Ok(())
        } else {
            Err(FAIL.into())
        }
    }
    pub fn attr(&self, kind: &str, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|((ns, key), _)| key == name && matches(ns, kind))
            .map(|(_, v)| v.as_str())
    }
    pub fn children_named<'a>(
        &'a self,
        kind: &'a str,
        name: &'a str,
    ) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |n| n.is(kind, name))
    }
}
fn namespace(ns: ResolveResult<'_>) -> Result<String, String> {
    match ns {
        ResolveResult::Unbound => Ok(String::new()),
        ResolveResult::Bound(n) => Ok(n.as_ref().into()),
        _ => Err(FAIL.into()),
    }
}
pub(super) fn parse(bytes: &[u8], start: Instant) -> Result<Node, String> {
    let input = std::str::from_utf8(bytes).map_err(|_| FAIL)?;
    if input
        .chars()
        .any(|c| c.is_control() && !['\n', '\r', '\t'].contains(&c))
    {
        return Err(FAIL.into());
    }
    let mut r = NsReader::from_reader(input.as_bytes());
    r.config_mut().expand_empty_elements = true;
    let mut stack: Vec<Node> = vec![];
    let mut root = None;
    let mut count = 0;
    loop {
        count += 1;
        if count > 100000 {
            return Err(FAIL.into());
        }
        budget(start)?;
        let (ns, event) = r.read_resolved_event().map_err(|_| FAIL)?;
        match event {
            Event::Start(e) => {
                if stack.len() >= 64 || root.is_some() {
                    return Err(FAIL.into());
                }
                let ns = namespace(ns)?;
                let name = e.local_name().as_ref().to_string();
                if ns.len() > 256 || name.len() > 128 {
                    return Err(FAIL.into());
                }
                let mut attrs = HashMap::new();
                for a in e.attributes() {
                    if attrs.len() >= 32 {
                        return Err(FAIL.into());
                    }
                    let a = a.map_err(|_| FAIL)?;
                    let (ns, name) = r.resolver().resolve_attribute(a.key);
                    let ns = namespace(ns)?;
                    let name = name.as_ref().to_string();
                    let value = a
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|_| FAIL)?
                        .into_owned();
                    if value.len() > 4096
                        || ns.len() > 256
                        || name.len() > 128
                        || attrs.insert((ns, name), value).is_some()
                    {
                        return Err(FAIL.into());
                    }
                }
                stack.push(Node {
                    ns,
                    name,
                    attrs,
                    text: String::new(),
                    children: vec![],
                });
            }
            Event::End(_) => {
                let node = stack.pop().ok_or(FAIL)?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node)
                } else if root.replace(node).is_some() {
                    return Err(FAIL.into());
                }
            }
            Event::Text(e) => {
                let s = quick_xml::escape::unescape(e.as_ref()).map_err(|_| FAIL)?;
                append(&mut stack, &s)?;
            }
            Event::CData(e) => append(&mut stack, e.as_ref())?,
            Event::GeneralRef(e) => {
                let raw = format!("&{};", e.as_ref());
                let s = quick_xml::escape::unescape(&raw).map_err(|_| FAIL)?;
                append(&mut stack, &s)?;
            }
            Event::DocType(_) => return Err(FAIL.into()),
            Event::Decl(d) => {
                if !stack.is_empty()
                    || root.is_some()
                    || d.version().map_or(true, |v| v != "1.0")
                    || d.encoding()
                        .is_some_and(|v| v.map_or(true, |v| !v.eq_ignore_ascii_case("utf-8")))
                {
                    return Err(FAIL.into());
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(FAIL.into());
    }
    root.ok_or_else(|| FAIL.into())
}
fn append(stack: &mut [Node], text: &str) -> Result<(), String> {
    if let Some(n) = stack.last_mut() {
        n.text.push_str(text);
        if n.text.len() > PART_LIMIT {
            return Err(FAIL.into());
        }
    } else if !text.trim().is_empty() {
        return Err(FAIL.into());
    }
    Ok(())
}
