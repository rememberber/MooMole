use super::*;
pub(super) fn text(node: &xml::Node, ns: &str, out: &mut Excerpt, phonetic: bool) {
    if out.truncated
        || phonetic
        || node.is("w", "del")
        || node.is("w", "moveFrom")
        || node.is("s", "rPh")
        || (node.ns == "http://schemas.openxmlformats.org/markup-compatibility/2006"
            && node.name == "AlternateContent")
    {
        return;
    }
    if node.is(ns, "t") {
        out.add(&node.text);
        return;
    }
    if node.is(ns, "tab") {
        out.add("\t");
        return;
    }
    if node.is(ns, "br") || node.is(ns, "cr") {
        out.add("\n");
        return;
    }
    for child in &node.children {
        text(child, ns, out, false);
        if out.truncated {
            break;
        }
    }
    if node.is(ns, "p") {
        out.add("\n")
    }
}
pub(super) fn shared(node: &xml::Node) -> Result<Vec<String>, String> {
    node.require("s", "sst")?;
    let mut strings = vec![];
    for n in &node.children {
        if n.is("s", "si") {
            if strings.len() >= 50000 {
                return Err(FAIL.into());
            }
            let mut out = Excerpt::new();
            text(n, "s", &mut out, false);
            if out.truncated {
                return Err(FAIL.into());
            }
            strings.push(out.text);
        }
    }
    Ok(strings)
}
fn cell_ref(raw: &str) -> Result<(u32, u32), String> {
    let i = raw.bytes().position(|b| b.is_ascii_digit()).ok_or(FAIL)?;
    let (col, row) = raw.split_at(i);
    if col.is_empty()
        || col.len() > 3
        || !col.bytes().all(|b| b.is_ascii_uppercase())
        || row.starts_with('0')
        || !row.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(FAIL.into());
    }
    let col = col
        .bytes()
        .fold(0u32, |n, b| n * 26 + (b - b'A' + 1) as u32);
    let row = row.parse::<u32>().map_err(|_| FAIL)?;
    if col == 0 || col > 16384 || row == 0 || row > 1048576 {
        return Err(FAIL.into());
    }
    Ok((col, row))
}
fn reference(mut col: u32, row: u32) -> String {
    let mut chars = vec![];
    while col > 0 {
        col -= 1;
        chars.push((b'A' + (col % 26) as u8) as char);
        col /= 26
    }
    chars.reverse();
    format!("{}{row}", chars.iter().collect::<String>())
}
pub(super) fn sheet(node: &xml::Node, strings: &[String]) -> Result<Excerpt, String> {
    let data = node.children_named("s", "sheetData").collect::<Vec<_>>();
    if data.len() != 1 {
        return Err(FAIL.into());
    }
    let mut out = Excerpt::new();
    let mut previous_row = 0;
    let mut cells = 0;
    for row in data[0].children_named("s", "row") {
        let number = row
            .attr("", "r")
            .map(|s| s.parse::<u32>().map_err(|_| FAIL))
            .transpose()?
            .unwrap_or(previous_row + 1);
        if number <= previous_row || number > 1048576 {
            return Err(FAIL.into());
        }
        previous_row = number;
        let mut previous_col = 0;
        for cell in row.children_named("s", "c") {
            cells += 1;
            if cells > 5000 {
                out.truncated = true;
                break;
            }
            let (col, r) = cell
                .attr("", "r")
                .map(cell_ref)
                .transpose()?
                .unwrap_or((previous_col + 1, number));
            if r != number || col <= previous_col || col > 16384 {
                return Err(FAIL.into());
            }
            previous_col = col;
            let vals = cell.children_named("s", "v").collect::<Vec<_>>();
            let formulas = cell.children_named("s", "f").collect::<Vec<_>>();
            if vals.len() > 1 || formulas.len() > 1 {
                return Err(FAIL.into());
            }
            let raw = vals.first().map(|v| v.text.as_str()).unwrap_or("");
            let value = match cell.attr("", "t").unwrap_or("n") {
                "s" => strings
                    .get(raw.parse::<usize>().map_err(|_| FAIL)?)
                    .ok_or(FAIL)?
                    .clone(),
                "inlineStr" => {
                    let mut s = Excerpt::new();
                    for n in cell.children_named("s", "is") {
                        text(n, "s", &mut s, false)
                    }
                    if s.truncated {
                        return Err(FAIL.into());
                    }
                    s.text
                }
                "b" => match raw {
                    "0" => "FALSE".into(),
                    "1" => "TRUE".into(),
                    "" => String::new(),
                    _ => return Err(FAIL.into()),
                },
                "n" | "str" | "e" | "d" => raw.into(),
                _ => return Err(FAIL.into()),
            };
            out.add(&reference(col, number));
            out.add("\t");
            if let Some(f) = formulas.first() {
                out.add("=");
                out.add(if f.text.is_empty() { "…" } else { &f.text });
                out.add(" → ");
            }
            out.add(&value);
            out.add("\n");
            if out.truncated {
                break;
            }
        }
        if out.truncated {
            break;
        }
    }
    Ok(out)
}
