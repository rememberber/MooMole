use super::*;
#[test]
fn numeric_upgrade_requires_checksum_supported_source_and_no_other_package_dependencies() {
    let mut row = item();
    row["version"] = serde_json::json!("2.0");
    row["url"] = serde_json::json!("https://example.org/owned.zip");
    row["sha256"] = serde_json::json!("a".repeat(64));
    row["outdated"] = serde_json::json!(true);
    row["auto_updates"] = serde_json::json!(false);
    row["disabled"] = serde_json::json!(false);
    row["depends_on"] = serde_json::json!({});
    assert!(super::super::upgrade::offer(&row).is_ok());
    for (key, value) in [
        ("version", serde_json::json!("latest")),
        ("version", serde_json::json!("0.9")),
        ("sha256", serde_json::json!("no_check")),
        ("url", serde_json::json!("http://example.org/owned.zip")),
        ("auto_updates", serde_json::json!(true)),
        ("depends_on", serde_json::json!({"formula":["other"]})),
        ("disabled", serde_json::json!(true)),
    ] {
        let mut bad = row.clone();
        bad[key] = value;
        assert!(super::super::upgrade::offer(&bad).is_err(), "{key}");
    }
}
#[test]
fn upgrading_command_requires_sha_preserves_quarantine_and_never_quits_or_bypasses_dependencies() {
    let tool=Tool{path:PathBuf::from("/opt/homebrew/bin/brew"),witness:serde_json::from_value(serde_json::json!({"version":crate::observation::VERSION,"nodes":[],"size":0,"mtime":0,"mtime_ns":0,"ctime":0,"ctime_ns":0})).unwrap(),sha256:"a".repeat(64)};
    let c = super::super::upgrade::command(&tool, Path::new("/owned"), "author/tap/owned").unwrap();
    let args = c
        .as_std()
        .get_args()
        .map(|v| v.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        args,
        [
            "upgrade",
            "--cask",
            "--require-sha",
            "--no-quit",
            "author/tap/owned"
        ]
    );
    for token in [
        "--force",
        "--greedy",
        "../bad",
        "https://example.org/script",
    ] {
        assert!(super::super::upgrade::command(&tool, Path::new("/owned"), token).is_err());
    }
    let fetch =
        super::super::upgrade::fetch(&tool, Path::new("/owned"), "author/tap/owned").unwrap();
    assert_eq!(
        fetch.as_std().get_args().collect::<Vec<_>>(),
        ["fetch", "--cask", "author/tap/owned"]
    );
}
#[test]
fn upgraded_evidence_cannot_be_invented_or_mislabelled_as_removal_and_v5_roundtrips() {
    let root = tempfile::tempdir().unwrap();
    let mut c = db::open_fixture(&root.path().join("state.sqlite")).unwrap();
    let mut r = record();
    r.draft.installed_version = "1.0".into();
    r.draft.upgrade = Some(super::super::upgrade::Offer {
        version: "2.0".into(),
        url: "https://example.org/owned.zip".into(),
        sha256: "a".repeat(64),
        publisher: "adHoc".into(),
    });
    journal::begin(&mut c, &r).unwrap();
    r.phase = "finished".into();
    r.finished_at = Some(r.created_at);
    r.state = "upgraded".into();
    assert!(journal::save(&c, &r).is_err());
    r.upgrade_result = Some(super::super::upgrade::ResultProof {
        version: "2.0".into(),
        publisher: "adHoc".into(),
        bundle_sha256: "b".repeat(64),
        receipt_sha256: "c".repeat(64),
    });
    journal::save(&c, &r).unwrap();
    let p = crate::execution_archive::prepare(&mut c).unwrap();
    let archives = root.path().canonicalize().unwrap().join("archives");
    let h =
        crate::execution_archive::archive(&mut c, &archives, &p.fingerprint, db::now()).unwrap();
    assert_eq!(serde_json::to_value(&h).unwrap()["version"], 5);
    let detail = crate::execution_archive::detail(
        &archives,
        &h.id,
        crate::execution_archive::Kind::ManagedApp,
        &r.id,
    )
    .unwrap();
    assert_eq!(detail, serde_json::to_value(&r).unwrap());
    assert!(journal::list(&c).unwrap().is_empty());
}
#[tokio::test]
#[ignore = "real owned Homebrew install + upgrade + uninstall; temporary source/tap only, exact rescue on failure"]
async fn real_owned_cask_upgrades_through_production_confirmation_and_preserves_source_and_history()
{
    let runtime = tokio::runtime::Handle::current();
    tauri::async_runtime::spawn_blocking(move||objc2::rc::autoreleasepool(|_|{
  let mut temp=tempfile::Builder::new().prefix("moomole-upgrade-owned-").tempdir().unwrap();let home=temp.path().canonicalize().unwrap();let suffix=uuid::Uuid::new_v4().simple().to_string();let token=format!("moomole-upgrade-{}",&suffix[..12]);let full=format!("{token}/check/{token}");let prefix=if cfg!(target_arch="aarch64"){PathBuf::from("/opt/homebrew")}else{PathBuf::from("/usr/local")};let taps=prefix.join("Library/Taps");let taps_existed=taps.exists();let room=prefix.join("Caskroom");let room_existed=room.exists();let namespace=taps.join(&token);let tap=namespace.join("homebrew-check");let owned_tap=home.join("tap");let bundle_id=format!("org.example.moomoleupgrade.{}",&suffix[..12]);
  let mut zips=vec![];for v in ["1.0","2.0"]{let app=home.join(v).join("MooMoleOwned.app");std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();std::fs::copy("/usr/bin/true",app.join("Contents/MacOS/owned")).unwrap();std::fs::write(app.join("Contents/Info.plist"),format!("<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>{bundle_id}</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleName</key><string>MooMoleOwned</string><key>CFBundleExecutable</key><string>owned</string><key>CFBundleVersion</key><string>{v}</string><key>CFBundleShortVersionString</key><string>{v}</string><key>LSBackgroundOnly</key><false/></dict></plist>")).unwrap();assert!(std::process::Command::new("/usr/bin/codesign").args(["--force","--deep","--sign","-","--identifier",&bundle_id]).arg(&app).output().unwrap().status.success());assert!(std::process::Command::new("/usr/bin/xattr").args(["-w","com.apple.quarantine","0083;65555555;MooMoleOwnedTest;"]).arg(&app).status().unwrap().success());let zip=home.join(format!("owned-{v}.zip"));assert!(std::process::Command::new("/usr/bin/ditto").args(["-c","-k","--keepParent"]).arg(&app).arg(&zip).status().unwrap().success());let bytes=std::fs::read(&zip).unwrap();zips.push((v,zip,format!("{:x}",Sha256::digest(&bytes)),bytes));}
  std::fs::create_dir_all(owned_tap.join("Casks")).unwrap();let recipe=owned_tap.join("Casks").join(format!("{token}.rb"));let write_recipe=|i:usize|{let(v,zip,digest,_)=&zips[i];std::fs::write(&recipe,format!("cask \"{token}\" do\n version \"{v}\"\n sha256 \"{digest}\"\n url \"{}\"\n name \"MooMoleOwned\"\n homepage \"https://example.org/\"\n app \"MooMoleOwned.app\"\nend\n",reqwest::Url::from_file_path(zip).unwrap())).unwrap();};write_recipe(0);
  temp.disable_cleanup(true);let rescue="../.cache/managed-upgrade-native-rescue.json";std::fs::create_dir_all("../.cache").unwrap();std::fs::write(rescue,serde_json::to_vec(&serde_json::json!({"ownedRoot":home,"token":token,"fullToken":full,"tap":tap,"tapsExisted":taps_existed,"caskroomExisted":room_existed})).unwrap()).unwrap();
  if !taps_existed{std::fs::create_dir(&taps).unwrap();}std::fs::create_dir(&namespace).unwrap();std::os::unix::fs::symlink(&owned_tap,&tap).unwrap();
  let tool=Tool::detect().unwrap();let mut install=tool.command(&home);install.args(["install","--cask"]).arg(format!("--appdir={}",home.join("Applications").display())).arg(&full);runtime.block_on(crate::process::output_bounded(install,Arc::new(Job::new(crate::engine::TaskKind::AppControl)),Duration::from_secs(90),CAP as u64)).expect("owned install failed; exact rescue retained");
  write_recipe(1);std::fs::write(home.join("unselected.txt"),b"keep").unwrap();let path=home.join("Applications/MooMoleOwned.app");let source=Source::from_row(&uuid::Uuid::new_v4().to_string(),0,&serde_json::json!({"name":"MooMoleOwned","path":path,"bundle_id":bundle_id,"source":"Homebrew"})).unwrap();let ctx=Context{home:home.clone(),engine_root:PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),current_app:None,current_id:"org.example.client".into(),protected_paths:vec![]};let control=Arc::new(Job::new(crate::engine::TaskKind::AppControl));let raw=runtime.block_on(tool.installed(&home,control.clone())).unwrap();std::fs::write(home.join("catalog-summary.json"),serde_json::to_vec(&serde_json::json!({"packages":raw.packages.iter().map(|p|serde_json::json!({"token":p.token,"version":p.version,"upgrade":p.upgrade})).collect::<Vec<_>>()})).unwrap()).unwrap();
  let plan=runtime.block_on(Plan::prepare_for(source,ctx.clone(),control.clone(),true)).expect("production upgrade preparation refused; exact rescue retained");let draft=plan.draft.clone();let database=Arc::new(Mutex::new(db::open_owned(&home.join("state.sqlite")).unwrap()));let request=uuid::Uuid::new_v4().to_string();let result=commands::execute(plan,&database,&request,control,&runtime,|p,j|{p.validate(&ctx,j)?;Ok(ctx.clone())}).unwrap();std::fs::write(home.join("result.json"),serde_json::to_vec(&result).unwrap()).unwrap();assert_eq!(result.state,"upgraded","{}",serde_json::to_string(&result).unwrap());assert_eq!(identity::capture(&path,&bundle_id).unwrap().app_version.as_deref(),Some("2.0"));assert!(std::process::Command::new("/usr/bin/xattr").args(["-p","com.apple.quarantine"]).arg(&path).output().unwrap().status.success());
  for (_,zip,_,bytes) in &zips{assert_eq!(std::fs::read(zip).unwrap(),*bytes);}assert_eq!(std::fs::read(home.join("unselected.txt")).unwrap(),b"keep");drop(database);let mut reopened=db::open_owned(&home.join("state.sqlite")).unwrap();assert_eq!(serde_json::to_value(&journal::list(&reopened).unwrap()[0]).unwrap(),serde_json::to_value(&result).unwrap());let preview=crate::execution_archive::prepare(&mut reopened).unwrap();let archives=home.join("archives");let header=crate::execution_archive::archive(&mut reopened,&archives,&preview.fingerprint,db::now()).unwrap();let detail=crate::execution_archive::detail(&archives,&header.id,crate::execution_archive::Kind::ManagedApp,&result.id).unwrap();assert_eq!(detail,serde_json::to_value(&result).unwrap());drop(reopened);
  let command=tool.uninstall_command(&home,&full).unwrap();runtime.block_on(crate::process::output_bounded(command,Arc::new(Job::new(crate::engine::TaskKind::AppControl)),Duration::from_secs(90),CAP as u64)).expect("owned cleanup failed; exact rescue retained");assert!(!path.exists());assert!(!room.join(&token).exists());assert_eq!(std::fs::read_link(&tap).unwrap(),owned_tap);std::fs::remove_file(&tap).unwrap();std::fs::remove_dir(&namespace).unwrap();if !taps_existed{std::fs::remove_dir(&taps).unwrap();}if !room_existed{std::fs::remove_dir(&room).unwrap();}temp.disable_cleanup(false);let removed=temp.path().to_path_buf();drop(temp);assert!(!removed.exists());std::fs::remove_file(rescue).unwrap();std::fs::write("../.cache/managed-upgrade-native.json",serde_json::to_vec(&serde_json::json!({"draft":draft,"result":result,"archiveHeader":header,"sourceAndUnselectedPreserved":true,"databaseReopenAndV5ArchiveVerified":true,"ownedTemporaryRootAndTapRemoved":true,"actualThirdPartyOrAppWindow":false})).unwrap()).unwrap();
 })).await.unwrap();
}
