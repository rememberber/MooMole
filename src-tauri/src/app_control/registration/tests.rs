use super::*;
fn fixture() -> (tempfile::TempDir, Source, Context) {
    let t = tempfile::Builder::new()
        .prefix("MooMole-registration-owned-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let id = format!("dev.moomole.registration.{}", uuid::Uuid::new_v4());
    let path = identity::fixture(&home, "OwnedRegistration.app", &id);
    let source = Source::from_row(
        &uuid::Uuid::new_v4().to_string(),
        0,
        &serde_json::json!({"path":path,"bundle_id":id,"name":"OwnedRegistration","source":"App"}),
    )
    .unwrap();
    let ctx = Context {
        home,
        engine_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
        current_app: None,
        current_id: "dev.moomole.client".into(),
        protected_paths: vec![],
    };
    (t, source, ctx)
}
fn report(plan: &Plan) -> Report {
    let (path, id, _, _) = plan.source.control_target();
    Report {
        request_id: uuid::Uuid::new_v4().to_string(),
        action: Action::Register,
        path: path.into(),
        bundle_id: id.into(),
        outcome: "unknown",
        requested: false,
        pids: vec![],
        reason: String::new(),
        registration_status: None,
        registered_path_observed: None,
    }
}
#[test]
fn protected_identity_and_cancel_refuse_registration_before_the_system_call() {
    let (_t, source, mut ctx) = fixture();
    let plan = Plan::prepare(source.clone(), Action::Register, &ctx).unwrap();
    let job = Job::new(TaskKind::AppControl);
    job.cancel();
    let called = std::cell::Cell::new(0);
    let result = perform_with(
        &plan,
        || Ok(ctx.clone()),
        &job,
        report(&plan),
        |_| {
            called.set(1);
            Ok(0)
        },
        |_, _| Ok(true),
    )
    .unwrap();
    assert_eq!(called.get(), 0);
    assert!(!result.requested);
    assert_eq!(result.outcome, "cancelled");
    ctx.protected_paths.push(source.path().into());
    assert!(Plan::prepare(source.clone(), Action::Register, &ctx).is_err());
    ctx.protected_paths.clear();
    ctx.current_id = source.control_target().1.into();
    assert!(Plan::prepare(source, Action::Register, &ctx).is_err());
}
#[test]
fn registration_requires_zero_status_and_the_exact_selected_copy_without_repeating_the_call() {
    let (_t, source, ctx) = fixture();
    let plan = Plan::prepare(source, Action::Register, &ctx).unwrap();
    let job = Job::new(TaskKind::AppControl);
    let calls = std::cell::Cell::new(0);
    let result = perform_with(
        &plan,
        || Ok(ctx.clone()),
        &job,
        report(&plan),
        |_| {
            calls.set(calls.get() + 1);
            Ok(0)
        },
        |path, id| {
            assert_eq!(path, Path::new(plan.source.path()));
            assert_eq!(id, plan.source.control_target().1);
            Ok(true)
        },
    )
    .unwrap();
    assert_eq!(result.outcome, "registered");
    assert_eq!(result.registration_status, Some(0));
    assert_eq!(result.registered_path_observed, Some(true));
    assert_eq!(calls.get(), 1);
    let result = perform_with(
        &plan,
        || Ok(ctx.clone()),
        &job,
        report(&plan),
        |_| Ok(-10811),
        |_, _| panic!("failed call cannot become a verified registration"),
    )
    .unwrap();
    assert_eq!(result.outcome, "unknown");
    assert!(result.requested);
    let result = perform_with(
        &plan,
        || Ok(ctx.clone()),
        &job,
        report(&plan),
        |_| Ok(0),
        |_, _| {
            job.cancel();
            Ok(false)
        },
    )
    .unwrap();
    assert_eq!(result.outcome, "cancelled");
    assert_eq!(result.registered_path_observed, Some(false));
    assert!(result.requested);
}
#[test]
fn changing_a_bundle_after_the_registration_call_preserves_an_unknown_result() {
    let (_t, source, ctx) = fixture();
    let plan = Plan::prepare(source, Action::Register, &ctx).unwrap();
    let job = Job::new(TaskKind::AppControl);
    let result = perform_with(
        &plan,
        || Ok(ctx.clone()),
        &job,
        report(&plan),
        |path| {
            std::fs::write(path.join("Contents/MacOS/fixture-main"), b"owned changed").unwrap();
            Ok(0)
        },
        |_, _| panic!("changed source cannot be verified"),
    )
    .unwrap();
    assert_eq!(result.outcome, "unknown");
    assert_eq!(result.registration_status, Some(0));
    assert!(result.requested);
}
#[test]
#[ignore = "registers only one unique self-created App, then unregisters that exact fixture; never launches or quits any App"]
fn real_owned_application_registration_is_observed_and_preserves_bundle_bytes_and_history() {
    let (_t, temporary_source, ctx) = fixture();
    // Exercise an actual installed-App location. Temporary-directory Apps may
    // be accepted by LSRegisterURL without becoming NSWorkspace candidates.
    let applications = PathBuf::from(std::env::var_os("HOME").unwrap()).join("Applications");
    let owned = tempfile::Builder::new()
        .prefix("MooMole-registration-owned-")
        .tempdir_in(&applications)
        .unwrap();
    let owned_home = owned.path().canonicalize().unwrap();
    let bundle = identity::fixture(
        &owned_home,
        "OwnedRegistration.app",
        temporary_source.control_target().1,
    );
    let source=Source::from_row(&uuid::Uuid::new_v4().to_string(),0,&serde_json::json!({"path":bundle,"bundle_id":temporary_source.control_target().1,"name":"OwnedRegistration","source":"App"})).unwrap();
    let executable = Path::new(source.path()).join("Contents/MacOS/fixture-main");
    // A real, locally compiled ordinary Cocoa App, never launched by this test.
    let code = ctx.home.join("registration-sample.m");
    std::fs::write(&code,"#import <AppKit/AppKit.h>\nint main() { @autoreleasepool { [NSApplication sharedApplication]; [NSApp run]; } return 0; }\n").unwrap();
    assert!(std::process::Command::new("/usr/bin/xcrun")
        .args(["clang", "-fobjc-arc", "-framework", "AppKit"])
        .arg(&code)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap()
        .status
        .success());
    let info_path = Path::new(source.path()).join("Contents/Info.plist");
    let info = std::fs::read_to_string(&info_path).unwrap();
    let suffix = format!("moomole_owned_{}", uuid::Uuid::new_v4().simple());
    std::fs::write(&info_path,info.replace("</dict></plist>",&format!("<key>CFBundleName</key><string>OwnedRegistration</string><key>CFBundleInfoDictionaryVersion</key><string>6.0</string><key>CFBundleDocumentTypes</key><array><dict><key>CFBundleTypeName</key><string>MooMoleOwned</string><key>CFBundleTypeRole</key><string>Viewer</string><key>CFBundleTypeExtensions</key><array><string>{suffix}</string></array></dict></array></dict></plist>"))).unwrap();
    assert!(std::process::Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-"])
        .arg(source.path())
        .output()
        .unwrap()
        .status
        .success());
    let plan = Plan::prepare(source.clone(), Action::Register, &ctx).unwrap();
    let request = uuid::Uuid::new_v4().to_string();
    let database = ctx.home.join("app.sqlite");
    let db = db::open_owned(&database).unwrap();
    db::start(&db, &request, "appregistration", Some(source.path())).unwrap();
    let before =
        autoreleasepool(|_| observed(Path::new(source.path()), source.control_target().1)).unwrap();
    // macOS can discover Apps in ~/Applications before this explicit request.
    // The product repairs an existing registration too; still require the real
    // update call to return zero and observe this exact copy afterwards.
    let info = std::fs::read(Path::new(source.path()).join("Contents/Info.plist")).unwrap();
    let original = std::fs::read(&executable).unwrap();
    let result = autoreleasepool(|_| {
        super::super::perform(
            &plan,
            || Ok(ctx.clone()),
            &Job::new(TaskKind::AppControl),
            &request,
        )
    })
    .unwrap();
    // Cleanup is deliberately test-only and bound to this exact unique App.
    let cleanup=std::process::Command::new("/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister").arg("-u").arg(source.path()).output().unwrap();
    assert!(cleanup.status.success());
    assert_eq!(
        result.outcome,
        "registered",
        "{}",
        serde_json::to_string_pretty(&result).unwrap()
    );
    assert_eq!(result.registration_status, Some(0));
    assert_eq!(result.registered_path_observed, Some(true));
    assert!(result.pids.is_empty());
    assert_eq!(std::fs::read(&executable).unwrap(), original);
    assert_eq!(
        std::fs::read(Path::new(source.path()).join("Contents/Info.plist")).unwrap(),
        info
    );
    db::finish(&db, &request, "succeeded", &result.reason).unwrap();
    drop(db);
    let reopened = db::open_owned(&database).unwrap();
    let history = db::history(&reopened).unwrap();
    assert!(history
        .iter()
        .any(|r| r.id == request && r.state == "succeeded" && r.kind == "appregistration"));
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/app-registration-native.json",serde_json::to_vec_pretty(&serde_json::json!({"beforeRegistered":before,"result":result,"appBytesPreserved":true,"historyReopened":true,"ownedRegistrationUnregisterExit":cleanup.status.code(),"noApplicationsLaunchedOrQuit":true})).unwrap()).unwrap();
}
