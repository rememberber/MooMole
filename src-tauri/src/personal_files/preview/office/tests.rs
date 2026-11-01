use super::*;
use std::io::Write;
const REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const OR: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
fn zip(entries: &[(&str, String)]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(vec![]));
    for (name, body) in entries {
        z.start_file(
            *name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        z.write_all(body.as_bytes()).unwrap();
    }
    z.finish().unwrap().into_inner()
}
fn types(entries: &[(&str, &str)]) -> String {
    format!(r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">{}</Types>"#,entries.iter().map(|(p,t)|format!(r#"<Override PartName="/{p}" ContentType="application/vnd.openxmlformats-officedocument.{t}+xml"/>"#)).collect::<String>())
}
fn rels(entries: &[(&str, &str, &str)]) -> String {
    format!(
        r#"<Relationships xmlns="{REL}">{}</Relationships>"#,
        entries
            .iter()
            .map(|(id, t, target)| format!(
                r#"<Relationship Id="{id}" Type="{OR}/{t}" Target="{target}"/>"#
            ))
            .collect::<String>()
    )
}
fn word(text: &str) -> Vec<u8> {
    zip(&[
        (
            "[Content_Types].xml",
            types(&[("word/main.xml", "wordprocessingml.document.main")]),
        ),
        (
            "_rels/.rels",
            rels(&[("r1", "officeDocument", "word/main.xml")]),
        ),
        (
            "word/main.xml",
            format!(
                r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>{text}</w:t></w:r><w:r><w:tab/><w:t>中文</w:t><w:br/><w:t>next</w:t></w:r></w:p><w:del><w:r><w:delText>deleted</w:delText></w:r></w:del><w:p><w:r><w:instrText>NEVER RUN</w:instrText><w:t>cached result</w:t></w:r></w:p></w:body></w:document>"#
            ),
        ),
    ])
}
fn excel() -> Vec<u8> {
    zip(&[
        (
            "[Content_Types].xml",
            types(&[
                ("xl/main.xml", "spreadsheetml.sheet.main"),
                ("xl/one.xml", "spreadsheetml.worksheet"),
                ("xl/two.xml", "spreadsheetml.worksheet"),
                ("xl/strings.xml", "spreadsheetml.sharedStrings"),
            ]),
        ),
        (
            "_rels/.rels",
            rels(&[("r1", "officeDocument", "/xl/main.xml")]),
        ),
        (
            "xl/_rels/main.xml.rels",
            rels(&[
                ("r1", "worksheet", "one.xml"),
                ("r2", "worksheet", "./two.xml"),
                ("r3", "sharedStrings", "strings.xml"),
            ]),
        ),
        (
            "xl/main.xml",
            format!(
                r#"<workbook xmlns="{S}" xmlns:r="{OR}"><sheets><sheet name="预算 中文" sheetId="2" r:id="r2"/><sheet name="Archive" sheetId="1" r:id="r1" state="veryHidden"/></sheets></workbook>"#
            ),
        ),
        (
            "xl/strings.xml",
            format!(
                r#"<sst xmlns="{S}"><si><r><t>shared &amp; </t></r><r><t>中文</t></r><rPh><t>not phonetics</t></rPh></si></sst>"#
            ),
        ),
        (
            "xl/two.xml",
            format!(
                r#"<worksheet xmlns="{S}"><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="inlineStr"><is><t>&lt;script&gt;literal&lt;/script&gt;</t></is></c><c r="D1" t="b"><v>1</v></c><c r="E1"><v>45678</v></c><c r="F1"><f>SUM(D1:E1)</f><v>45679</v></c></row><row r="3" hidden="1"><c><v>42</v></c></row></sheetData></worksheet>"#
            ),
        ),
        (
            "xl/one.xml",
            format!(
                r#"<worksheet xmlns="{S}"><sheetData><row><c t="str"><v>hidden sheet value</v></c></row></sheetData></worksheet>"#
            ),
        ),
    ])
}
fn slides() -> Vec<u8> {
    zip(&[
        (
            "[Content_Types].xml",
            types(&[
                ("ppt/presentation.xml", "presentationml.presentation.main"),
                ("ppt/slides/one.xml", "presentationml.slide"),
                ("ppt/slides/two.xml", "presentationml.slide"),
            ]),
        ),
        (
            "_rels/.rels",
            rels(&[("r1", "officeDocument", "ppt/presentation.xml")]),
        ),
        (
            "ppt/_rels/presentation.xml.rels",
            rels(&[
                ("r1", "slide", "slides/one.xml"),
                ("r2", "slide", "slides/two.xml"),
            ]),
        ),
        (
            "ppt/presentation.xml",
            format!(
                r#"<p:presentation xmlns:p="{P}" xmlns:r="{OR}"><p:sldIdLst><p:sldId id="256" r:id="r2"/><p:sldId id="257" r:id="r1"/></p:sldIdLst></p:presentation>"#
            ),
        ),
        (
            "ppt/slides/one.xml",
            format!(
                r#"<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><a:p><a:r><a:t>second</a:t></a:r></a:p></p:cSld></p:sld>"#
            ),
        ),
        (
            "ppt/slides/two.xml",
            format!(
                r#"<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><a:p><a:r><a:t>first 中文</a:t></a:r></a:p></p:cSld></p:sld>"#
            ),
        ),
    ])
}
#[test]
fn word_text_is_literal_namespaced_and_bounded_without_field_execution() {
    let content = render(
        &word("hello &amp; &lt;script&gt;literal&lt;/script&gt;"),
        "docx",
        1,
    )
    .unwrap();
    assert!(
        matches!(content,Content::Document{format,part:1,sections,text,truncated:false} if format=="docx" && sections.len()==1 && text=="hello & <script>literal</script>\t中文\nnext\ncached result\n")
    );
    let long = render(&word(&"字".repeat(30000)), "docx", 1).unwrap();
    assert!(
        matches!(long,Content::Document{text,truncated:true,..}if text.len()<=TEXT_LIMIT as usize && text.is_char_boundary(text.len()))
    );
    assert!(render(&word("body"), "docx", 2).is_err());
}
#[test]
fn sheets_resolve_declared_order_rich_shared_inline_raw_and_cached_values() {
    let bytes = excel();
    let first = render(&bytes, "xlsx", 1).unwrap();
    assert!(
        matches!(first,Content::Document{sections,text,part:1,truncated:false,..}if sections.len()==2 && sections[0].title=="预算 中文" && sections[1].hidden && text=="A1\tshared & 中文\nB1\t<script>literal</script>\nD1\tTRUE\nE1\t45678\nF1\t=SUM(D1:E1) → 45679\nA3\t42\n")
    );
    assert!(
        matches!(render(&bytes,"xlsx",2).unwrap(),Content::Document{part:2,text,..} if text.contains("hidden sheet value"))
    );
    assert!(render(&bytes, "xlsx", 3).is_err());
    assert!(render(&bytes, "docx", 1).is_err());
}
#[test]
fn slides_use_relationship_order_and_support_strict_namespaces() {
    let bytes = slides();
    assert!(
        matches!(render(&bytes,"pptx",1).unwrap(),Content::Document{text,sections,..} if text=="first 中文\n" && sections.len()==2)
    );
    assert!(
        matches!(render(&bytes,"pptx",2).unwrap(),Content::Document{text,..} if text=="second\n")
    );
    let strict=zip(&[("[Content_Types].xml",types(&[("body.xml","wordprocessingml.document.main")])),("_rels/.rels",rels(&[("r1","officeDocument","body.xml")]).replace(OR,"http://purl.oclc.org/ooxml/officeDocument/relationships")),("body.xml",r#"<x:document xmlns:x="http://purl.oclc.org/ooxml/wordprocessingml/main"><x:body><x:p><x:r><x:t>strict</x:t></x:r></x:p></x:body></x:document>"#.into())]);
    assert!(
        matches!(render(&strict,"docx",1).unwrap(),Content::Document{text,..} if text=="strict\n")
    );
}
#[test]
fn unknown_dangerous_ambiguous_and_over_budget_packages_remain_unpreviewed() {
    for text in [
        "<!DOCTYPE w:document [<!ENTITY x SYSTEM 'file:///etc/passwd'>]>",
        "<x>",
        "<x/><x/>",
        "<x>&custom;</x>",
        "<x>\0</x>",
    ] {
        assert!(
            xml::parse(text.as_bytes(), Instant::now()).is_err(),
            "{text}"
        )
    }
    assert!(xml::parse(
        &format!("{}{}", "<x>".repeat(65), "</x>".repeat(65)).into_bytes(),
        Instant::now()
    )
    .is_err());
    assert!(xml::parse(b"<x/>", Instant::now() - Duration::from_secs(5)).is_err());
    for target in [
        "../../outside.xml",
        "https://example.org/a",
        "file:///etc/passwd",
        "//host/a",
        "a%2fb.xml",
        "%2e%2e/outside.xml",
        "a.xml?x=1",
    ] {
        let bytes = zip(&[
            ("_rels/.rels", rels(&[("r1", "officeDocument", target)])),
            (
                "[Content_Types].xml",
                types(&[("word/main.xml", "wordprocessingml.document.main")]),
            ),
        ]);
        assert!(render(&bytes, "docx", 1).is_err(), "{target}");
    }
    let external = zip(&[
        (
            "_rels/.rels",
            rels(&[("r1", "officeDocument", "main.xml")])
                .replace(" Target=", " TargetMode=\"External\" Target="),
        ),
        (
            "[Content_Types].xml",
            types(&[("main.xml", "wordprocessingml.document.main")]),
        ),
    ]);
    assert!(render(&external, "docx", 1).is_err());
    let mut zip64 = zip(&[("data", "body".into())]);
    let end = zip64.len();
    zip64[end - 2..].copy_from_slice(&4u16.to_le_bytes());
    zip64.extend_from_slice(b"PK\x06\x06");
    assert!(package::Package::open(&zip64, Instant::now()).is_err());
    let duplicate = zip(&[("body.xml", "one".into()), ("BODY.xml", "two".into())]);
    assert!(package::Package::open(&duplicate, Instant::now()).is_err());
    let large = zip(&[("_rels/.rels", "x".repeat(PART_LIMIT + 1))]);
    assert!(package::Package::open(&large, Instant::now())
        .unwrap()
        .xml("_rels/.rels")
        .is_err());
}
#[test]
fn real_owned_files_preview_all_formats_read_only_and_keep_scan_identity_checks() {
    let temp = tempfile::Builder::new()
        .prefix("MooMole-office-preview-")
        .tempdir()
        .unwrap();
    let root = temp.path().canonicalize().unwrap();
    let mut reports = vec![];
    for (format, source) in [
        ("docx", word("owned Word")),
        ("xlsx", excel()),
        ("pptx", slides()),
    ] {
        let path = root.join(format!("owned.{format}"));
        std::fs::write(&path, &source).unwrap();
        let item = super::super::tests::item(&path);
        let content = super::super::render(&item, &[], 1).unwrap();
        assert!(super::super::render(&item, &[item.path.clone()], 1).is_err());
        reports.push(serde_json::json!({"scanId":uuid::Uuid::new_v4().to_string(),"itemId":item.id,"path":item.path,"content":content}));
        if format != "docx" {
            reports.push(serde_json::json!({"scanId":uuid::Uuid::new_v4().to_string(),"itemId":item.id,"path":item.path,"content":super::super::render(&item,&[],2).unwrap()}))
        }
        assert_eq!(std::fs::read(&path).unwrap(), source);
        std::fs::write(&path, b"changed").unwrap();
        assert!(super::super::render(&item, &[], 1).is_err());
    }
    // Apple's own exporter supplies an independent real DOCX package.
    let input = root.join("export.txt");
    std::fs::write(&input, "Independent Apple exporter 中文").unwrap();
    let output = root.join("export.docx");
    let result = std::process::Command::new("/usr/bin/textutil")
        .args(["-convert", "docx", "-output"])
        .arg(&output)
        .arg(&input)
        .output()
        .unwrap();
    assert!(result.status.success(), "{:?}", result.stderr);
    let bytes = std::fs::read(&output).unwrap();
    let item = super::super::tests::item(&output);
    let content = super::super::render(&item, &[], 1).unwrap();
    assert!(
        matches!(&content,Content::Document{text,..}if text.contains("Independent Apple exporter 中文"))
    );
    assert_eq!(std::fs::read(&output).unwrap(), bytes);
    reports.push(serde_json::json!({"scanId":uuid::Uuid::new_v4().to_string(),"itemId":item.id,"path":item.path,"content":content}));
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/office-preview-native.json",serde_json::to_vec(&serde_json::json!({"reports":reports,"originalsPreserved":true,"independentAppleDocx":true,"appWindowVerified":false})).unwrap()).unwrap();
}
