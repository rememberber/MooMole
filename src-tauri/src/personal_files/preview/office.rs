//! Bounded OOXML text excerpts from an in-memory package, never Office execution.
use super::*;
use std::{collections::HashMap, io::Cursor};
mod extract;
mod package;
#[cfg(test)]
mod tests;
mod xml;
const FAIL: &str = "办公文件结构、编码或容量暂不支持，请在对应应用中查看；原文件保持不变。";
const PART_LIMIT: usize = 2 * 1024 * 1024;
const READ_LIMIT: usize = 8 * 1024 * 1024;
const SECTIONS: usize = 512;
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Section {
    title: String,
    hidden: bool,
}
struct Excerpt {
    text: String,
    truncated: bool,
}
impl Excerpt {
    fn new() -> Self {
        Self {
            text: String::new(),
            truncated: false,
        }
    }
    fn add(&mut self, s: &str) {
        let remaining = TEXT_LIMIT as usize - self.text.len();
        let mut n = remaining.min(s.len());
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        self.text.push_str(&s[..n]);
        self.truncated |= n < s.len();
    }
}
fn budget(start: Instant) -> Result<(), String> {
    if start.elapsed() > Duration::from_secs(4) {
        Err("办公文件预览达到本轮处理时限，请在对应应用中查看。".into())
    } else {
        Ok(())
    }
}
pub(super) fn render(bytes: &[u8], format: &str, part: u32) -> Result<Content, String> {
    if part == 0 {
        return Err(FAIL.into());
    }
    let start = Instant::now();
    let mut pkg = package::Package::open(bytes, start)?;
    let types = pkg.xml("[Content_Types].xml")?;
    let types = package::Types::parse(&types, start)?;
    let root = pkg.rels("")?;
    let main = package::unique(&root, "officeDocument")?.target("", "officeDocument")?;
    types.require_type(
        &main,
        match format {
            "docx" => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
            }
            "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
            "pptx" => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"
            }
            _ => return Err(FAIL.into()),
        },
    )?;
    let doc = pkg.xml(&main)?;
    let (sections, excerpt) = match format {
        "docx" => {
            if part != 1 {
                return Err(FAIL.into());
            }
            doc.require("w", "document")?;
            let bodies = doc
                .children
                .iter()
                .filter(|n| n.is("w", "body"))
                .collect::<Vec<_>>();
            if bodies.len() != 1 {
                return Err(FAIL.into());
            }
            let mut out = Excerpt::new();
            extract::text(bodies[0], "w", &mut out, false);
            (
                vec![Section {
                    title: String::new(),
                    hidden: false,
                }],
                out,
            )
        }
        "pptx" | "xlsx" => {
            let excel = format == "xlsx";
            let ns = if excel { "s" } else { "p" };
            doc.require(ns, if excel { "workbook" } else { "presentation" })?;
            let lists = doc
                .children
                .iter()
                .filter(|n| n.is(ns, if excel { "sheets" } else { "sldIdLst" }))
                .collect::<Vec<_>>();
            if lists.len() != 1 {
                return Err(FAIL.into());
            }
            let rels = pkg.rels(&main)?;
            let mut links = vec![];
            let mut sections = vec![];
            let mut ids = std::collections::HashSet::new();
            for n in &lists[0].children {
                if !n.is(ns, if excel { "sheet" } else { "sldId" }) {
                    return Err(FAIL.into());
                }
                if sections.len() >= SECTIONS {
                    return Err(FAIL.into());
                }
                let id = n.attr("r", "id").ok_or(FAIL)?;
                if !ids.insert(id.to_owned()) {
                    return Err(FAIL.into());
                }
                let kind = if excel { "worksheet" } else { "slide" };
                let target = rels.get(id).ok_or(FAIL)?.target(&main, kind)?;
                types.require_type(
                    &target,
                    if excel {
                        "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"
                    } else {
                        "application/vnd.openxmlformats-officedocument.presentationml.slide+xml"
                    },
                )?;
                let title = if excel {
                    n.attr("", "name")
                        .filter(|v| {
                            !v.is_empty()
                                && v.len() <= 128
                                && !v.chars().any(|c| {
                                    c.is_control()
                                        || ('\u{202a}'..='\u{202e}').contains(&c)
                                        || ('\u{2066}'..='\u{2069}').contains(&c)
                                })
                        })
                        .ok_or(FAIL)?
                        .to_string()
                } else {
                    String::new()
                };
                let hidden = if excel {
                    match n.attr("", "state") {
                        None | Some("visible") => false,
                        Some("hidden" | "veryHidden") => true,
                        _ => return Err(FAIL.into()),
                    }
                } else {
                    false
                };
                sections.push(Section { title, hidden });
                links.push(target);
            }
            let target = links.get(part as usize - 1).ok_or(FAIL)?;
            let body = pkg.xml(target)?;
            let excerpt = if excel {
                body.require("s", "worksheet")?;
                let shared = match rels
                    .values()
                    .filter(|r| r.kind("sharedStrings"))
                    .collect::<Vec<_>>()
                    .as_slice()
                {
                    [] => vec![],
                    [rel] => {
                        let target = rel.target(&main, "sharedStrings")?;
                        types.require_type(&target,"application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml")?;
                        extract::shared(&pkg.xml(&target)?)?
                    }
                    _ => return Err(FAIL.into()),
                };
                extract::sheet(&body, &shared)?
            } else {
                body.require("p", "sld")?;
                let mut out = Excerpt::new();
                extract::text(&body, "a", &mut out, false);
                out
            };
            (sections, excerpt)
        }
        _ => unreachable!(),
    };
    budget(start)?;
    Ok(Content::Document {
        format: format.into(),
        part,
        sections,
        text: excerpt.text,
        truncated: excerpt.truncated,
    })
}
