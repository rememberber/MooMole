use super::*;
fn fixture() -> (tempfile::TempDir, Context, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let path = home.join("Documents");
    std::fs::create_dir(&path).unwrap();
    (
        temp,
        Context {
            home,
            protected: vec![],
        },
        path,
    )
}
#[test]
fn search_scope_protection_identity_expiry_and_cancel() {
    let (_temp, mut ctx, path) = fixture();
    assert!(Plan::prepare(&ctx.home, &ctx).is_err());
    let library = ctx.home.join("Library");
    std::fs::create_dir(&library).unwrap();
    assert!(Plan::prepare(&library, &ctx).is_err());
    let plan = Plan::prepare(&path, &ctx).unwrap();
    let job = Job::new(TaskKind::SearchRepair);
    job.cancel();
    assert_eq!(plan.run(&ctx, &job).unwrap().outcome, Outcome::NotStarted);
    ctx.protected
        .push(path.join("kept").to_str().unwrap().into());
    assert!(plan.validate(&ctx).is_err());
    ctx.protected.clear();
    std::fs::rename(&path, ctx.home.join("old")).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(plan.validate(&ctx).is_err());
    let mut plan = Plan::prepare(&path, &ctx).unwrap();
    plan.draft.expires_at = db::now() - 1;
    assert!(plan.validate(&ctx).is_err());
}
#[test]
#[ignore = "requests real Spotlight import only for a disposable personal-folder sample"]
fn native_owned_folder_import_preserves_files() {
    let (_temp, ctx, path) = fixture();
    let file = path.join("MooMole-search-sample.txt");
    std::fs::write(&file, "MooMole owned indexing sample").unwrap();
    let before = std::fs::read(&file).unwrap();
    let plan = Plan::prepare(&path, &ctx).unwrap();
    let attempt = plan.run(&ctx, &Job::new(TaskKind::SearchRepair)).unwrap();
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/search-repair-native.json", serde_json::to_vec_pretty(&serde_json::json!({"attempt":attempt,"sourcePreserved":before == std::fs::read(&file).unwrap(),"verification":"command completion only; search visibility is asynchronous"})).unwrap()).unwrap();
    assert_eq!(attempt.outcome, Outcome::CommandSucceeded);
    assert_eq!(before, std::fs::read(&file).unwrap());
}
