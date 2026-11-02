use super::*;
fn item() -> serde_json::Value {
    serde_json::json!({"token":"owned-app","full_token":"author/tap/owned-app","name":["Owned"],"installed":"1.0","pinned":false,"artifacts":[{"app":["Owned.app"],"target":"/Applications/Owned.app"},{"zap":[{"trash":["~/owned-data"]}]}]})
}
#[test]
fn manager_metadata_binds_a_unique_installed_single_app_and_keeps_unsupported_packages_visible_to_verification(
) {
    let raw = serde_json::json!({"casks":[item()]});
    let c = packages(&serde_json::to_vec(&raw).unwrap()).unwrap();
    assert_eq!(c.packages.len(), 1);
    assert!(c.tokens.contains("author/tap/owned-app"));
    for (key, value) in [
        ("pinned", serde_json::json!(true)),
        (
            "artifacts",
            serde_json::json!([{"uninstall":[{"script":"must-not-run"}]}]),
        ),
        (
            "artifacts",
            serde_json::json!([{"app":["Owned.app"],"target":"/elsewhere/Other.app"}]),
        ),
        (
            "artifacts",
            serde_json::json!([{"app":["Owned.app"],"target":"/Applications/../Owned.app"}]),
        ),
        (
            "artifacts",
            serde_json::json!([{"app":["Owned.app"],"target":"/Applications/Owned.app"},{"app":["Other.app"],"target":"/Applications/Other.app"}]),
        ),
    ] {
        let mut changed = item();
        changed[key] = value;
        let c = packages(&serde_json::to_vec(&serde_json::json!({"casks":[changed]})).unwrap())
            .unwrap();
        assert!(c.packages.is_empty());
        assert!(c.tokens.contains("author/tap/owned-app"));
    }
    assert!(
        packages(&serde_json::to_vec(&serde_json::json!({"casks":[item(),item()]})).unwrap())
            .is_err()
    );
    assert!(packages(b"{\"casks\": [{\"installed\":\"1\"}]}").is_err());
}
#[test]
fn command_protocol_cannot_add_flags_or_enable_global_package_cleanup() {
    for token in [
        "--force",
        "--zap",
        "a/b",
        "../bad",
        "/path",
        "https://site/file",
        "foo\nbar",
        "foo bar",
        "a/b/../c",
        "a/b/C",
    ] {
        assert!(!valid_token(token), "{token}")
    }
    let tool=Tool{path:PathBuf::from("/opt/homebrew/bin/brew"),witness:serde_json::from_value(serde_json::json!({"version":crate::observation::VERSION,"nodes":[],"size":0,"mtime":0,"mtime_ns":0,"ctime":0,"ctime_ns":0})).unwrap(),sha256:"a".repeat(64)};
    let c = tool
        .uninstall_command(Path::new("/Users/example"), "author/tap/owned-app")
        .unwrap();
    let s = c.as_std();
    assert_eq!(
        s.get_args().collect::<Vec<_>>(),
        ["uninstall", "--cask", "author/tap/owned-app"]
    );
    let env = s
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    for key in [
        "HOMEBREW_NO_AUTOREMOVE",
        "HOMEBREW_NO_AUTO_UPDATE",
        "HOMEBREW_NO_ANALYTICS",
        "HOMEBREW_NO_INSTALL_CLEANUP",
    ] {
        assert_eq!(env[key].as_deref(), Some("1"))
    }
    assert!(!env.contains_key("HOMEBREW_NO_INSTALL_FROM_API"));
}
fn record() -> Record {
    let now = db::now();
    let id = uuid::Uuid::new_v4().to_string();
    Record {
        id: id.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        draft: Draft {
            id,
            task_id: uuid::Uuid::new_v4().to_string(),
            index: 0,
            path: "/Applications/Owned.app".into(),
            bundle_id: "org.example.owned".into(),
            token: "author/tap/owned-app".into(),
            name: "Owned".into(),
            installed_version: "1".into(),
            manager_path: "/opt/homebrew/bin/brew".into(),
            metadata_sha256: "a".repeat(64),
            created_at: now,
            expires_at: now + 120000,
        },
        state: "running".into(),
        phase: "checking".into(),
        created_at: now,
        finished_at: None,
        reason: "owned".into(),
        reviewed_at: None,
    }
}
#[test]
fn known_background_privileged_and_store_app_payloads_require_the_original_manager() {
    let t = tempfile::tempdir().unwrap();
    let app = t.path().canonicalize().unwrap().join("Owned.app");
    std::fs::create_dir_all(app.join("Contents")).unwrap();
    let plist = app.join("Contents/Info.plist");
    let plain = "<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>org.example.owned</string></dict></plist>";
    std::fs::write(&plist, plain).unwrap();
    simple_app(&app).unwrap();
    for relative in [
        "Contents/Library/LoginItems",
        "Contents/Library/PrivilegedHelperTools",
        "Contents/_MASReceipt",
    ] {
        std::fs::create_dir_all(app.join(relative)).unwrap();
        assert!(simple_app(&app).is_err());
        std::fs::remove_dir_all(app.join(relative)).unwrap();
    }
    std::fs::write(
        &plist,
        plain.replace(
            "</dict>",
            "<key>SMPrivilegedExecutables</key><dict/></dict>",
        ),
    )
    .unwrap();
    assert!(simple_app(&app).is_err());
    std::fs::write(&plist, b"not a plist").unwrap();
    assert!(simple_app(&app).is_err());
}
#[test]
fn durable_intent_interruptions_block_new_mutations_until_review_and_never_replay() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().canonicalize().unwrap().join("state.sqlite");
    let mut c = db::open_owned(&path).unwrap();
    let mut r = record();
    journal::begin(&mut c, &r).unwrap();
    r.phase = "intent".into();
    journal::save(&c, &r).unwrap();
    drop(c);
    let mut c = db::open_owned(&path).unwrap();
    assert_eq!(
        journal::reconcile(&c, &[], &[]).unwrap()[0].state,
        "running"
    );
    let interrupted = journal::reconcile(&c, &[], &[r.id.clone()])
        .unwrap()
        .remove(0);
    assert_eq!(interrupted.state, "unknown");
    assert_eq!(interrupted.phase, "intent");
    assert!(journal::begin(&mut c, &record()).is_err());
    let mut reviewed = interrupted;
    reviewed.reviewed_at = Some(db::now());
    journal::save(&c, &reviewed).unwrap();
    journal::begin(&mut c, &record()).unwrap();
}

#[tokio::test]
#[ignore = "installs only an owned transient Cask/App via real Homebrew; production plan + executor, DB reopen; exact tap and fixture cleanup; no real user cask"]
async fn real_owned_homebrew_cask_uses_production_plan_and_durable_manager_execution() {
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move||objc2::rc::autoreleasepool(|_|{
        let mut temp=tempfile::Builder::new().prefix("MooMole-managed-native-").tempdir().unwrap();let home=temp.path().canonicalize().unwrap();let suffix=uuid::Uuid::new_v4().simple().to_string();let token=format!("moomole-owned-{}",&suffix[..12]);let full=format!("{token}/check/{token}");let prefix=if cfg!(target_arch="aarch64"){PathBuf::from("/opt/homebrew")}else{PathBuf::from("/usr/local")};let taps=prefix.join("Library/Taps");let taps_existed=taps.exists();let caskroom=prefix.join("Caskroom");let room_existed=caskroom.exists();let namespace=taps.join(&token);let tap=namespace.join("homebrew-check");let owned_tap=home.join("tap");
        temp.disable_cleanup(true);std::fs::write("../.cache/managed-app-native-rescue.json",serde_json::to_vec(&serde_json::json!({"ownedRoot":home,"token":token,"fullToken":full,"tap":tap,"tapsExisted":taps_existed,"caskroomExisted":room_existed})).unwrap()).unwrap();
        let app=home.join("Source/MooMoleOwned.app");std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();std::fs::copy("/usr/bin/true",app.join("Contents/MacOS/owned")).unwrap();let bundle_id=format!("org.example.moomoleowned.{}",&suffix[..12]);
        std::fs::write(app.join("Contents/Info.plist"),format!("<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>{bundle_id}</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleName</key><string>MooMoleOwned</string><key>CFBundleExecutable</key><string>owned</string><key>CFBundleVersion</key><string>1</string><key>CFBundleShortVersionString</key><string>1.0</string><key>LSBackgroundOnly</key><false/></dict></plist>")).unwrap();let source_bytes=std::fs::read(app.join("Contents/MacOS/owned")).unwrap();std::fs::write(home.join("unselected.txt"),b"unselected keep").unwrap();
        let zip=home.join("owned.zip");assert!(std::process::Command::new("/usr/bin/ditto").args(["-c","-k","--keepParent"]).arg(&app).arg(&zip).status().unwrap().success());let digest=format!("{:x}",Sha256::digest(std::fs::read(&zip).unwrap()));
        std::fs::create_dir_all(owned_tap.join("Casks")).unwrap();std::fs::write(owned_tap.join("Casks").join(format!("{token}.rb")),format!("cask \"{token}\" do\n version \"1.0\"\n sha256 \"{digest}\"\n url \"{}\"\n name \"MooMoleOwned\"\n homepage \"https://example.org/\"\n app \"MooMoleOwned.app\"\nend\n",reqwest::Url::from_file_path(&zip).unwrap())).unwrap();
        if !taps_existed{std::fs::create_dir(&taps).unwrap()}std::fs::create_dir(&namespace).unwrap();std::os::unix::fs::symlink(&owned_tap,&tap).unwrap();
        let tool=Tool::detect().unwrap();let mut install=tool.command(&home);install.args(["install","--cask"]).arg(format!("--appdir={}",home.join("Applications").display())).arg(&full);
        runtime.block_on(crate::process::output_bounded(install,Arc::new(Job::new(crate::engine::TaskKind::AppControl)),Duration::from_secs(60),CAP as u64)).expect("owned installation failed; exact rescue retained");
        let path=home.join("Applications/MooMoleOwned.app");let source=Source::from_row(&uuid::Uuid::new_v4().to_string(),0,&serde_json::json!({"name":"MooMoleOwned","path":path,"bundle_id":bundle_id,"source":"Homebrew"})).unwrap();
        let ctx=Context{home:home.clone(),engine_root:PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),current_app:None,current_id:"org.example.test-client".into(),protected_paths:vec![]};let control=Arc::new(Job::new(crate::engine::TaskKind::AppControl));let plan=runtime.block_on(Plan::prepare(source,ctx.clone(),control.clone())).expect("production preparation refused; exact own sample rescue retained");let draft=plan.draft.clone();let database=db::open_owned(&home.join("state.sqlite")).unwrap();let database=Arc::new(Mutex::new(database));let request=uuid::Uuid::new_v4().to_string();
        let result=commands::execute(plan,&database,&request,control,&runtime,|p,j|{p.validate(&ctx,j)?;Ok(ctx.clone())}).unwrap();
        std::fs::write("../.cache/managed-app-native-rescue.json",serde_json::to_vec(&serde_json::json!({"ownedRoot":home,"token":token,"fullToken":full,"tap":tap,"result":result,"tapsExisted":taps_existed,"caskroomExisted":room_existed})).unwrap()).unwrap();
        assert_eq!(result.state,"removed","{}",serde_json::to_string(&result).unwrap());assert!(!path.exists());assert!(!caskroom.join(&token).exists());assert_eq!(std::fs::read(app.join("Contents/MacOS/owned")).unwrap(),source_bytes);assert_eq!(std::fs::read(home.join("unselected.txt")).unwrap(),b"unselected keep");drop(database);let reopened=db::open_owned(&home.join("state.sqlite")).unwrap();assert_eq!(serde_json::to_value(&journal::list(&reopened).unwrap()[0]).unwrap(),serde_json::to_value(&result).unwrap());drop(reopened);
        assert_eq!(std::fs::read_link(&tap).unwrap(),owned_tap);std::fs::remove_file(&tap).unwrap();std::fs::remove_dir(&namespace).unwrap();if !taps_existed{std::fs::remove_dir(&taps).expect("manager created another tap; inspect exact rescue before cleaning")};if !room_existed{std::fs::remove_dir(&caskroom).unwrap()};
        temp.disable_cleanup(false);let removed=temp.path().to_path_buf();drop(temp);assert!(!removed.exists());std::fs::remove_file("../.cache/managed-app-native-rescue.json").unwrap();std::fs::write("../.cache/managed-app-native.json",serde_json::to_vec(&serde_json::json!({"scope":"real owned Homebrew Cask + production core, not packaged App window or real third-party compatibility","draft":draft,"result":result,"sourceAndUnselectedPreserved":true,"databaseReopenVerified":true,"ownedTemporaryRootAndTapRemoved":true,"noAutoremoveForceZapOrDependencyBypass":true})).unwrap()).unwrap();
    })).await.unwrap();
}
