use super::*;
fn item(path: &Path) -> Item {
    let witness = crate::observation::witness(path).unwrap();
    Item {
        id: uuid::Uuid::new_v4().to_string(),
        path: path.to_str().unwrap().into(),
        size: witness.size,
        modified_at: 0,
        witness,
    }
}
#[test]
fn bounded_text_is_literal_and_changed_or_protected_sources_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let path = root.join("sample.html");
    std::fs::write(&path, "<script>literal text</script>\n中文").unwrap();
    let i = item(&path);
    assert!(
        matches!(render(&i,&[],1).unwrap(),Content::Text {text,truncated:false} if text.contains("<script>"))
    );
    assert!(render(&i, &[i.path.clone()], 1).is_err());
    std::fs::write(&path, "changed").unwrap();
    assert!(render(&i, &[], 1).is_err());
    std::fs::write(&path, "字".repeat(30_000)).unwrap();
    let i = item(&path);
    assert!(
        matches!(render(&i,&[],1).unwrap(),Content::Text {text,truncated:true} if text.len()<=TEXT_LIMIT as usize)
    );
    std::fs::write(&path, [0, 1, 2, 3]).unwrap();
    assert!(render(&item(&path), &[], 1).is_err());
}
fn pdf() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 240 300] /Contents 5 0 R >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 240 300] /Rotate 90 /Contents 5 0 R >>",
        "<< /Length 27 >>\nstream\n1 0 0 rg 10 10 80 100 re f\nendstream",
    ];
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend(format!("{} 0 obj\n{}\nendobj\n", i + 1, obj).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in offsets.iter().skip(1) {
        bytes.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    bytes
}
#[test]
fn native_raster_and_pdf_pages_are_real_bounded_png_without_changing_originals() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let png = media::fixture_png(128, 96);
    let path = root.join("image.png");
    std::fs::write(&path, &png).unwrap();
    let content = objc2::rc::autoreleasepool(|_| render(&item(&path), &[], 1)).unwrap();
    assert!(
        matches!(&content,Content::Image {width:128,height:96,data_url,..} if data_url.starts_with("data:image/png;base64,iVBOR"))
    );
    let source = pdf();
    let path = root.join("document.pdf");
    std::fs::write(&path, &source).unwrap();
    let first = render(&item(&path), &[], 1).unwrap();
    let second = render(&item(&path), &[], 2).unwrap();
    assert!(matches!(
        first,
        Content::Pdf {
            page: 1,
            pages: 2,
            ..
        }
    ));
    assert!(matches!(
        second,
        Content::Pdf {
            page: 2,
            pages: 2,
            ..
        }
    ));
    assert!(render(&item(&path), &[], 3).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), source);
    let report = serde_json::json!({"image":content,"pdfPage1":first,"pdfPage2":second,"originalPreserved":true});
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write(
        "../.cache/file-preview-native.json",
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
