use super::*;
fn fixture() -> (tempfile::TempDir, Source, Context) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let path = identity::fixture(&home, "Control.app", "dev.moomole.control");
    let source=Source::from_row(&uuid::Uuid::new_v4().to_string(),0,&serde_json::json!({"path":path,"bundle_id":"dev.moomole.control","name":"Control","source":"App"})).unwrap();
    let ctx = Context {
        home,
        engine_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
        current_app: None,
        current_id: "dev.moomole.client".into(),
        protected_paths: vec![],
    };
    (temp, source, ctx)
}
#[test]
fn current_identity_protection_and_cancel_are_checked_before_system_requests() {
    let (_temp, source, mut ctx) = fixture();
    let plan = Plan::prepare(source.clone(), Action::Quit, &ctx).unwrap();
    let job = Job::new(TaskKind::AppControl);
    job.cancel();
    let result =
        autoreleasepool(|_| perform(&plan, || Ok(ctx.clone()), &job, "cancel-owned")).unwrap();
    assert_eq!(result.outcome, "cancelled");
    assert!(!result.requested);
    ctx.protected_paths.push(source.path().into());
    assert!(plan.validate(&ctx).is_err());
    ctx.protected_paths.clear();
    std::fs::write(
        Path::new(source.path()).join("Contents/MacOS/fixture-main"),
        "changed",
    )
    .unwrap();
    assert!(plan.validate(&ctx).is_err());
}

#[test]
#[ignore = "launches and gracefully quits only a disposable, locally compiled sample App"]
fn owned_native_app_open_and_quit() {
    let (temp, source, ctx) = fixture();
    let code = temp.path().join("sample.m");
    std::fs::write(&code, "#import <AppKit/AppKit.h>\nint main() { @autoreleasepool { [NSApplication sharedApplication]; [NSApp run]; } return 0; }\n").unwrap();
    let executable = Path::new(source.path()).join("Contents/MacOS/fixture-main");
    assert!(std::process::Command::new("/usr/bin/xcrun")
        .args(["clang", "-fobjc-arc", "-framework", "AppKit"])
        .arg(&code)
        .arg("-o")
        .arg(&executable)
        .status()
        .unwrap()
        .success());
    assert!(std::process::Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-"])
        .arg(source.path())
        .status()
        .unwrap()
        .success());
    let job = Job::new(TaskKind::AppControl);
    let open = Plan::prepare(source.clone(), Action::Open, &ctx).unwrap();
    let opened =
        autoreleasepool(|_| perform(&open, || Ok(ctx.clone()), &job, "owned-open")).unwrap();
    let quit = Plan::prepare(source.clone(), Action::Quit, &ctx).unwrap();
    let stopped =
        autoreleasepool(|_| perform(&quit, || Ok(ctx.clone()), &job, "owned-quit")).unwrap();
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write(
        "../.cache/app-control-native.json",
        serde_json::to_vec_pretty(
            &serde_json::json!({"open":opened,"quit":stopped,"appPreserved":executable.exists()}),
        )
        .unwrap(),
    )
    .unwrap();
    // Keep a live sample's bundle available for manual inspection if quit was refused.
    if stopped.outcome != "quit" {
        let _ = temp.keep();
    }
    assert_eq!(opened.outcome, "opened");
    assert_eq!(stopped.outcome, "quit");
    assert!(executable.exists());
}
