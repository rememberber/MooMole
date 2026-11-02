use super::*;
fn job() -> Job {
    Job::new(TaskKind::AppControl)
}
fn xml(items: &str) -> Vec<u8> {
    format!(r#"<?xml version="1.0" encoding="UTF-8"?><rss version="2.0" xmlns:s="http://www.andymatuschak.org/xml-namespaces/sparkle"><channel>{items}</channel></rss>"#).into_bytes()
}
fn read(items: &str) -> (Vec<Release>, usize, usize, bool) {
    feed::parse(&xml(items), "1", "15.0", "arm64", &job()).unwrap()
}
#[test]
fn appcasts_bind_namespaces_stable_builds_full_packages_and_declared_requirements() {
    let items = r#"<item><s:version>3</s:version><s:shortVersionString>2.1</s:shortVersionString><s:minimumSystemVersion>12.0</s:minimumSystemVersion><s:minimumAutoupdateVersion>2</s:minimumAutoupdateVersion><enclosure url="https://updates.example.org/app.zip?a=1&amp;b=2" length="123"/></item>
 <item><enclosure url="https://updates.example.org/old.zip" s:version="1"/></item>
 <item><s:channel>beta</s:channel><s:version>99</s:version><enclosure url="https://updates.example.org/beta.zip"/></item>
 <item><s:version>8</s:version><s:minimumSystemVersion>99.0</s:minimumSystemVersion><enclosure url="https://updates.example.org/future.zip"/></item>
 <item><s:version>7</s:version><s:hardwareRequirements>x86_64</s:hardwareRequirements><enclosure url="https://updates.example.org/intel.zip"/></item>
 <item><s:version>6</s:version><s:minimumUpdateVersion>2</s:minimumUpdateVersion><enclosure url="https://updates.example.org/requires.zip"/></item>"#;
    let (rows, inspected, skipped, complete) = read(items);
    assert!(complete);
    assert_eq!((rows.len(), inspected, skipped), (1, 6, 5));
    assert_eq!(rows[0].build, "3");
    assert_eq!(rows[0].version, "2.1");
    assert!(rows[0].requires_review);
    assert!(rows[0].url.ends_with("a=1&b=2"));
    let (rows, _, _, ok) = read(
        r#"<item><enclosure url="https://updates.example.org/legacy.dmg" s:version="2" s:shortVersionString="1.1"/></item><item><s:version>4</s:version><link>https://updates.example.org/upgrade</link></item>"#,
    );
    assert!(ok);
    assert_eq!(rows[0].kind, "page");
    assert_eq!(rows[1].kind, "package");
    let (rows, _, _, ok) = read(
        r#"<item><enclosure url="https://updates.example.org/app.zip" s:version="2" length="0"/></item>"#,
    );
    assert!(ok);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].bytes, None);
}
#[test]
fn ambiguous_unknown_or_unsafe_feed_items_never_turn_into_up_to_date_claims() {
    for item in [
        r#"<item><s:version>2</s:version><s:version>3</s:version><enclosure url="https://updates.example.org/a.zip"/></item>"#,
        r#"<item><s:version>2</s:version><enclosure url="https://localhost/a.zip"/></item>"#,
        r#"<item><s:version>2</s:version><s:futureRequirement>yes</s:futureRequirement><enclosure url="https://updates.example.org/a.zip"/></item>"#,
        r#"<item><s:version><b>2</b></s:version><enclosure url="https://updates.example.org/a.zip"/></item>"#,
        r#"<item><s:version>2-rc1</s:version><enclosure url="https://updates.example.org/a.zip"/></item>"#,
        r#"<item xmlns:s="https://wrong.invalid"><s:version>2</s:version><enclosure url="https://updates.example.org/a.zip"/></item>"#,
        r#"<item xmlns:t="http://www.andymatuschak.org/xml-namespaces/sparkle"><enclosure url="https://updates.example.org/a.zip" s:version="2" t:version="3"/></item>"#,
        r#"<item><s:version>2</s:version><enclosure url="https://updates.example.org/a.zip" s:futureRequirement="yes"/></item>"#,
    ] {
        let (r, _, _, complete) = read(item);
        assert!(r.is_empty() && !complete, "{item}");
    }
    assert!(!read("").3);
    let doctype = b"<!DOCTYPE rss [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><rss><channel/></rss>";
    assert!(feed::parse(doctype, "1", "15.0", "arm64", &job()).is_err());
    let j = job();
    j.cancel();
    assert!(feed::parse(&xml(""), "1", "15.0", "arm64", &j).is_err());
}
#[test]
fn update_urls_and_resolved_addresses_never_grant_access_to_local_or_reserved_networks() {
    for raw in [
        "http://updates.example.org/feed",
        "https://127.0.0.1/feed",
        "https://localhost/feed",
        "https://example.org:444/feed",
        "https://user:secret@example.org/feed",
        "https://example.org/feed#fragment",
        "https://host.local/feed",
        "https://example.org\n/feed",
    ] {
        assert!(network::url(raw).is_err(), "{raw}");
    }
    for raw in [
        "127.0.0.1",
        "10.1.2.3",
        "169.254.169.254",
        "192.168.1.1",
        "100.64.1.1",
        "192.0.2.1",
        "224.0.0.1",
        "::1",
        "fc00::1",
        "fe80::1",
        "::ffff:127.0.0.1",
        "2001:db8::1",
        "2002:7f00::1",
        "3fff::1",
    ] {
        assert!(!network::public(raw.parse().unwrap()), "{raw}");
    }
    assert!(network::public("1.1.1.1".parse().unwrap()));
    assert!(network::public("2606:4700:4700::1111".parse().unwrap()));
}
fn fixture() -> (tempfile::TempDir, Source, Context) {
    let temp = tempfile::Builder::new()
        .prefix("MooMole-appcast-owned-")
        .tempdir()
        .unwrap();
    let home = temp.path().canonicalize().unwrap();
    let path = identity::fixture(&home, "Owned.app", "org.moomole.feed.fixture");
    let info = path.join("Contents/Info.plist");
    let text=std::fs::read_to_string(&info).unwrap().replace("</dict>","<key>SUFeedURL</key><string>https://raw.githubusercontent.com/p0deje/Maccy/master/appcast.xml</string></dict>");
    std::fs::write(info, text).unwrap();
    let source=Source::from_row(&uuid::Uuid::new_v4().to_string(),0,&serde_json::json!({"name":"Owned","path":path,"bundle_id":"org.moomole.feed.fixture","source":"App"})).unwrap();
    let ctx = Context {
        home,
        engine_root: Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
        current_app: None,
        current_id: "io.github.rememberber.moomole".into(),
        protected_paths: vec![],
    };
    (temp, source, ctx)
}
#[test]
fn app_declared_feed_is_read_only_bound_to_the_exact_copy_and_expires() {
    let (_temp, source, ctx) = fixture();
    let mut p = Plan::prepare(source, ctx.clone(), &job()).unwrap();
    assert_eq!(p.probe.state, "ready");
    p.validate(&ctx, &job()).unwrap();
    let mut protected = ctx.clone();
    protected.protected_paths.push(p.probe.path.clone());
    assert!(p.validate(&protected, &job()).is_err());
    p.issued = Instant::now() - Duration::from_secs(301);
    assert!(p.validate(&ctx, &job()).is_err());
    let path = Path::new(&p.probe.path).join("Contents/Info.plist");
    let changed = std::fs::read_to_string(&path)
        .unwrap()
        .replace("raw.githubusercontent.com", "localhost");
    std::fs::write(path, changed).unwrap();
    assert!(p.validate(&ctx, &job()).is_err());
    let p = Plan::prepare(p.source.clone(), ctx, &job()).unwrap();
    assert_eq!(p.probe.state, "unsupported");
    assert!(p.probe.feed_url.is_none());
}
#[test]
fn download_key_is_bound_to_installed_identity_and_signed_feed_policy_stays_with_original_updater()
{
    use base64::Engine;
    let (_temp, source, ctx) = fixture();
    let path = Path::new(source.control_target().0).join("Contents/Info.plist");
    let before = std::fs::read_to_string(&path).unwrap();
    let k = base64::engine::general_purpose::STANDARD.encode(
        ed25519_dalek::SigningKey::from_bytes(&[31; 32])
            .verifying_key()
            .to_bytes(),
    );
    std::fs::write(
        &path,
        before.replace(
            "</dict>",
            &format!("<key>SUPublicEDKey</key><string>{k}</string></dict>"),
        ),
    )
    .unwrap();
    let plan = Plan::prepare(source.clone(), ctx.clone(), &job()).unwrap();
    assert!(plan.probe.verified_download);
    let with_policy = std::fs::read_to_string(&path)
        .unwrap()
        .replace("</dict>", "<key>SURequireSignedFeed</key><true/></dict>");
    std::fs::write(&path, with_policy).unwrap();
    assert!(plan.validate(&ctx, &job()).is_err());
    assert!(
        !Plan::prepare(source, ctx, &job())
            .unwrap()
            .probe
            .verified_download
    );
}
#[test]
#[ignore = "explicit HTTPS GET of public Maccy appcast from an owned temporary App declaration; no browser opening, package download or app changes"]
fn real_public_appcast_discovery_preserves_owned_application_and_returns_structured_versions() {
    let (_temp, source, ctx) = fixture();
    let job = Arc::new(job());
    let p = Plan::prepare(source, ctx.clone(), &job).unwrap();
    let original = std::fs::read(Path::new(&p.probe.path).join("Contents/Info.plist")).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let bytes = runtime
        .block_on(network::fetch(
            p.probe.feed_url.as_deref().unwrap(),
            job.clone(),
        ))
        .unwrap();
    let arch = if std::env::consts::ARCH == "aarch64" {
        "arm64"
    } else {
        std::env::consts::ARCH
    };
    let (releases, inspected, skipped, complete) = feed::parse(
        &bytes,
        p.probe.current_build.as_deref().unwrap(),
        &system_version().unwrap(),
        arch,
        &job,
    )
    .unwrap();
    assert!(!releases.is_empty());
    p.validate(&ctx, &job).unwrap();
    assert_eq!(
        std::fs::read(Path::new(&p.probe.path).join("Contents/Info.plist")).unwrap(),
        original
    );
    let now = db::now();
    let report = Report {
        probe_id: p.probe.id.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        checked_at: now,
        expires_at: p.probe.expires_at,
        state: "newerReported",
        complete,
        inspected,
        skipped,
        releases,
    };
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/appcast-native.json",serde_json::to_vec_pretty(&serde_json::json!({"probe":p.probe,"report":report,"ownedApplicationUnchanged":true,"packagesNotDownloaded":true,"browserNotOpened":true,"appWindowVerified":false})).unwrap()).unwrap();
}
