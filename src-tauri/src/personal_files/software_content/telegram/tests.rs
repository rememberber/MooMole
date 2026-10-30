use super::*;
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let t = tempfile::Builder::new()
        .prefix("MooMole-telegram-export-owned-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let folder = home.join("Downloads/ChatExport");
    std::fs::create_dir_all(folder.join("files")).unwrap();
    std::fs::write(folder.join("files/owned document.txt"), "owned document").unwrap();
    std::fs::write(folder.join("files/unlisted.txt"), "unlisted keep").unwrap();
    let manifest = folder.join("result.json");
    (t, home, manifest)
}
fn message(id: i64, path: &str) -> serde_json::Value {
    serde_json::json!({"id":id,"type":"message","file":path,"text":"owned secret body never returned"})
}
fn write(path: &Path, messages: Vec<serde_json::Value>) {
    std::fs::write(
        path,
        serde_json::to_vec(&serde_json::json!({"id":1,"type":"personal_chat","messages":messages}))
            .unwrap(),
    )
    .unwrap();
}
fn job() -> Arc<Job> {
    Arc::new(Job::new(crate::engine::TaskKind::PersonalScan))
}
#[test]
fn only_manifest_attachments_inside_the_export_scope_are_offered_and_changed_manifests_invalidate()
{
    let (_t, home, manifest) = fixture();
    let folder = manifest.parent().unwrap();
    std::fs::write(home.join("outside.txt"), "outside keep").unwrap();
    std::fs::write(folder.join("messages.html"), "owned chat transcript keep").unwrap();
    std::os::unix::fs::symlink(
        folder.join("files/owned document.txt"),
        folder.join("files/alias.txt"),
    )
    .unwrap();
    write(
        &manifest,
        vec![
            message(1, "files/owned document.txt"),
            message(2, "files/owned document.txt"),
            message(3, "../outside.txt"),
            message(4, "https://example.invalid/never"),
            message(5, "files/missing.txt"),
            message(6, "files/alias.txt"),
            message(
                7,
                "(File not included. Change data exporting settings to download.)",
            ),
            message(8, "result.json"),
            message(9, "messages.html"),
        ],
    );
    let original = std::fs::read(&manifest).unwrap();
    let scan = collect(&home, &manifest, &[], job()).unwrap();
    assert_eq!(scan.items.len(), 1);
    assert_eq!(scan.report.skipped, 8);
    assert!(!scan.report.complete);
    assert!(!serde_json::to_string(&scan.report)
        .unwrap()
        .contains("owned secret body"));
    let id = scan.items[0].id.clone();
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offer = store.offer(&[id], db::now()).unwrap();
    let plan = store.take(&offer.id, db::now()).unwrap();
    plan.validate_record().unwrap();
    let binding = plan.software_content.as_ref().unwrap();
    binding.validate(&plan.items[0], &home, db::now()).unwrap();
    assert!(!binding.allows(&home.join("outside.txt")));
    assert!(!binding.allows(&manifest));
    assert!(!binding.allows(&folder.join("messages.html")));
    assert!(!binding.allows(&folder.join("messages2.html")));
    assert_eq!(
        std::fs::read_to_string(folder.join("messages.html")).unwrap(),
        "owned chat transcript keep"
    );
    std::fs::write(&manifest, b"{}").unwrap();
    assert!(binding.validate(&plan.items[0], &home, db::now()).is_err());
    assert_eq!(
        std::fs::read_to_string(home.join("outside.txt")).unwrap(),
        "outside keep"
    );
    assert_eq!(
        std::fs::read_to_string(folder.join("files/unlisted.txt")).unwrap(),
        "unlisted keep"
    );
    std::fs::write(&manifest, &original).unwrap();
    let protected = collect(
        &home,
        &manifest,
        &[folder
            .join("files/owned document.txt")
            .to_str()
            .unwrap()
            .into()],
        job(),
    )
    .unwrap();
    assert!(protected.items.is_empty());
}
#[test]
fn account_and_left_chat_exports_are_scoped_without_treating_rich_inline_content_as_supported() {
    let (_t, home, manifest) = fixture();
    let data = serde_json::json!({"about":"owned about ignored","personal_information":{"phone_number":"private ignored"},"chats":{"list":[{"id":1,"type":"personal_chat","messages":[message(1,"files/owned document.txt")]}]},"left_chats":{"list":[{"id":2,"type":"public_channel","messages":[{"id":2,"type":"message","rich_message":{"text":"private rich ignored"}},{"id":3,"type":"message","photo_skip_reason":"file_size"}]}]}});
    std::fs::write(&manifest, serde_json::to_vec(&data).unwrap()).unwrap();
    let scan = collect(&home, &manifest, &[], job()).unwrap();
    assert_eq!(scan.items.len(), 1);
    assert_eq!(scan.report.visited, 3);
    assert_eq!(scan.report.skipped, 2);
    assert!(!scan.report.complete);
    let mut confused = data;
    confused["messages"] = serde_json::json!([]);
    std::fs::write(&manifest, serde_json::to_vec(&confused).unwrap()).unwrap();
    assert!(collect(&home, &manifest, &[], job()).is_err());
    let large = serde_json::json!({"id":1,"type":"personal_chat","messages":(0..=MESSAGES).map(|n|message(n as i64,"files/owned document.txt")).collect::<Vec<_>>()});
    assert!(parse(&serde_json::to_vec(&large).unwrap()).is_err());
}
#[test]
fn cancellation_protected_manifests_and_unknown_formats_do_not_infer_attachments() {
    let (_t, home, manifest) = fixture();
    write(&manifest, vec![message(1, "files/owned document.txt")]);
    let stopped = job();
    stopped.cancel();
    assert!(collect(&home, &manifest, &[], stopped).is_err());
    assert!(collect(
        &home,
        &manifest,
        &[manifest.to_str().unwrap().into()],
        job()
    )
    .is_err());
    for bytes in [
        b"<html>owned export</html>".as_slice(),
        b"{\"messages\":[]}",
        b"{\"id\":1,\"type\":\"future_chat\",\"messages\":[]}",
    ] {
        std::fs::write(&manifest, bytes).unwrap();
        assert!(collect(&home, &manifest, &[], job()).is_err());
    }
    assert_eq!(
        std::fs::read_to_string(manifest.parent().unwrap().join("files/owned document.txt"))
            .unwrap(),
        "owned document"
    );
}
