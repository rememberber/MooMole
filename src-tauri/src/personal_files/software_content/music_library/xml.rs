//! XML-only, bounded preflight before the system plist decoder. No fetching DTDs.
//! Reject duplicate keys instead of relying on Foundation's last-value choice.
use super::*;
use quick_xml::{events::Event, Reader};
struct Frame {
    tag: String,
    keys: HashSet<String>,
    text: String,
}
fn append(stack: &mut [Frame], text: &str) -> Result<(), String> {
    if let Some(frame) = stack.last_mut() {
        if frame.tag == "key" {
            if frame.text.len() + text.len() > 4096 {
                return Err(FAIL.into());
            }
            frame.text.push_str(text);
        }
    } else if !text.trim().is_empty() {
        return Err(FAIL.into());
    }
    Ok(())
}
pub(super) fn validate(bytes: &[u8], job: &Job, start: Instant) -> Result<(), String> {
    let s = std::str::from_utf8(bytes)
        .map_err(|_| FAIL)?
        .trim_start_matches('\u{feff}');
    if s.chars()
        .any(|c| c.is_control() && !['\n', '\r', '\t'].contains(&c))
    {
        return Err(FAIL.into());
    }
    let mut reader = Reader::from_reader(s.as_bytes());
    reader.config_mut().expand_empty_elements = true;
    let mut stack = Vec::<Frame>::new();
    let mut roots = 0;
    let mut count = 0;
    let mut doctype = false;
    let mut declaration = false;
    loop {
        count += 1;
        if count > 200_000
            || job.cancelled.load(Ordering::SeqCst)
            || start.elapsed() > Duration::from_secs(10)
        {
            return Err("Music XML 读取已停止或超过结构上限，未确认范围保留。".into());
        }
        match reader.read_event().map_err(|_| FAIL)? {
            Event::Start(e) => {
                if stack.len() >= 32 {
                    return Err(FAIL.into());
                }
                let tag = e.name().as_ref().to_string();
                if ![
                    "plist", "dict", "array", "key", "string", "integer", "real", "date", "data",
                    "true", "false",
                ]
                .contains(&tag.as_str())
                {
                    return Err(FAIL.into());
                }
                if stack.is_empty() {
                    roots += 1;
                    if roots != 1 || tag != "plist" {
                        return Err(FAIL.into());
                    }
                } else if tag == "plist" {
                    return Err(FAIL.into());
                }
                let mut version = false;
                for attribute in e.attributes() {
                    let a = attribute.map_err(|_| FAIL)?;
                    if tag != "plist"
                        || a.key.as_ref() != "version"
                        || a.value.as_ref() != "1.0"
                        || version
                    {
                        return Err(FAIL.into());
                    }
                    version = true;
                }
                if tag == "plist" && !version {
                    return Err(FAIL.into());
                }
                stack.push(Frame {
                    tag,
                    keys: HashSet::new(),
                    text: String::new(),
                });
            }
            Event::End(_) => {
                let frame = stack.pop().ok_or(FAIL)?;
                if frame.tag == "key" {
                    let parent = stack.last_mut().ok_or(FAIL)?;
                    if parent.tag != "dict" || !parent.keys.insert(frame.text) {
                        return Err(FAIL.into());
                    }
                }
            }
            Event::Text(e) => append(
                &mut stack,
                &quick_xml::escape::unescape(e.as_ref()).map_err(|_| FAIL)?,
            )?,
            Event::CData(e) => append(&mut stack, e.as_ref())?,
            Event::GeneralRef(e) => append(
                &mut stack,
                &quick_xml::escape::unescape(&format!("&{};", e.as_ref())).map_err(|_| FAIL)?,
            )?,
            Event::DocType(e) => {
                let d = e.as_ref().split_whitespace().collect::<Vec<_>>().join(" ");
                if doctype || roots>0 || !["plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"","plist PUBLIC \"-//Apple Computer//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\""].contains(&d.as_str()){return Err(FAIL.into());}
                doctype = true;
            }
            Event::Decl(d) => {
                if declaration
                    || roots > 0
                    || d.version().map_or(true, |v| v != "1.0")
                    || d.encoding()
                        .is_some_and(|v| v.map_or(true, |v| !v.eq_ignore_ascii_case("utf-8")))
                {
                    return Err(FAIL.into());
                }
                declaration = true;
            }
            Event::Comment(_) => {}
            Event::Eof => break,
            _ => return Err(FAIL.into()),
        }
    }
    if roots != 1 || !stack.is_empty() {
        return Err(FAIL.into());
    }
    Ok(())
}
