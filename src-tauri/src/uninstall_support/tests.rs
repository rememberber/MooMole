use super::*;
use crate::uninstall_runtime::Fixture;
use std::{fs, os::unix::fs::symlink};

fn scope(f: &Fixture) -> Scope {
    let root = f.home.join("support-scope");
    let s = Scope {
        launch: vec![
            root.join("UserAgents"),
            root.join("Agents"),
            root.join("Daemons"),
        ],
        rooms: vec![root.join("ArmCaskroom"), root.join("IntelCaskroom")],
        helpers: root.join("PrivilegedHelperTools"),
    };
    for p in s
        .launch
        .iter()
        .chain(s.rooms.iter())
        .chain(std::iter::once(&s.helpers))
    {
        fs::create_dir_all(p).unwrap();
    }
    s
}
fn xml(body: &str) -> Vec<u8> {
    format!("<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>{body}</dict></plist>")
        .into_bytes()
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn binary(bytes: &[u8]) -> Vec<u8> {
    use objc2_foundation::*;
    objc2::rc::autoreleasepool(|_| unsafe {
        let object = NSPropertyListSerialization::propertyListWithData_options_format_error(
            &NSData::with_bytes(bytes),
            NSPropertyListReadOptions::Immutable,
            std::ptr::null_mut(),
        )
        .unwrap();
        NSPropertyListSerialization::dataWithPropertyList_format_options_error(
            &object,
            NSPropertyListFormat::BinaryFormat_v1_0,
            0,
        )
        .unwrap()
        .to_vec()
    })
}
#[test]
fn ordinary_app_support_observation_is_bound_read_only_and_does_not_grant_execution() {
    let f = Fixture::new(true);
    let scope = scope(&f);
    fs::write(scope.launch[0].join("other.plist"),xml("<key>Label</key><string>org.other.agent</string><key>ProgramArguments</key><array><string>/usr/bin/true</string><string>--url=https://example.invalid/a</string></array>")).unwrap();
    let o = observe_with(&f.plan, &f.ctx, &scope).unwrap();
    assert_eq!(o.report.background.code, Code::NotDetected);
    assert_eq!(o.report.management.code, Code::NotDetected);
    assert!(!o.report.can_execute);
    o.permits_confirmation(&f.plan, &f.ctx, crate::db::now())
        .unwrap();
    let serialized = serde_json::to_string(o.report()).unwrap();
    assert!(!serialized.contains("other.plist"));
    assert!(!serialized.contains("example.invalid"));
    assert!(f.app.is_dir());
    assert!(f.home.join("app.sqlite").symlink_metadata().is_err());
}
#[test]
fn embedded_background_or_privileged_declarations_are_retained_even_when_not_running() {
    for location in [
        "Contents/Library/LoginItems",
        "Contents/Library/LaunchAgents",
        "Contents/Library/LaunchDaemons",
        "Contents/Library/HelperTools",
        "Contents/Library/SystemExtensions",
        "privileged",
    ] {
        let mut f = Fixture::new(true);
        let scope = scope(&f);
        if location == "privileged" {
            let p = f.app.join("Contents/Info.plist");
            let old = fs::read_to_string(&p).unwrap();
            fs::write(
                &p,
                old.replace(
                    "</dict>",
                    "<key>SMPrivilegedExecutables</key><dict/></dict>",
                ),
            )
            .unwrap();
        } else {
            fs::create_dir_all(f.app.join(location)).unwrap();
        }
        f.plan = Fixture::plan(&f.app, &f.ctx);
        let o = observe_with(&f.plan, &f.ctx, &scope).unwrap();
        assert_eq!(o.report.background.code, Code::Detected, "{location}");
        assert!(o
            .permits_confirmation(&f.plan, &f.ctx, crate::db::now())
            .is_err());
    }
}
#[test]
fn launch_plists_keep_exact_ids_helpers_program_arguments_and_binary_references() {
    let f = Fixture::new(true);
    let scope = scope(&f);
    let id = f.plan.archive().bundle_id().to_owned();
    let program = escape(&format!("{}/Contents/MacOS/fixture-main", f.app.display()));
    let bodies=[
        format!("<key>Label</key><string>{id}.helper</string>"),
        format!("<key>AssociatedBundleIdentifiers</key><array><string>{id}</string></array>"),
        format!("<key>Program</key><string>{program}</string>"),
        format!("<key>ProgramArguments</key><array><string>/usr/bin/open</string><string>{program}</string></array>"),
        format!("<key>WatchPaths</key><array><string>{program}</string></array>"),
        format!("<key>ProgramArguments</key><array><string>/bin/sh</string><string>-c</string><string>open '{program}'</string></array>"),
    ];
    for root in &scope.launch {
        for body in &bodies {
            let mut body = body.clone();
            if !body.contains("<key>Label</key>") {
                body.insert_str(0, "<key>Label</key><string>org.other</string>");
            }
            if !body.contains("<key>Program</key>") && !body.contains("<key>ProgramArguments</key>")
            {
                body.push_str("<key>Program</key><string>/usr/bin/true</string>");
            }
            for bytes in [xml(&body), binary(&xml(&body))] {
                let p = root.join("unrelated-label.plist");
                fs::write(&p, bytes).unwrap();
                let o = observe_with(&f.plan, &f.ctx, &scope).unwrap();
                assert_eq!(o.report.background.code, Code::Detected, "{body}");
                fs::remove_file(&p).unwrap();
            }
        }
    }
    // Matching filename alone retains an unreadable/malformed definition. It
    // is deliberately NOT used to unload or delete that file.
    fs::write(scope.launch[0].join(format!("{id}.plist")), "not a plist").unwrap();
    assert_eq!(
        observe_with(&f.plan, &f.ctx, &scope)
            .unwrap()
            .report
            .background
            .code,
        Code::Detected
    );
}
#[test]
fn unrelated_prefixes_stay_distinct_and_unknown_plists_never_become_clearance() {
    let f = Fixture::new(true);
    let scope = scope(&f);
    let id = f.plan.archive().bundle_id().to_owned();
    for body in [
        format!("<key>Label</key><string>{id}2</string><key>Program</key><string>/usr/bin/true</string>"),
        "<key>Label</key><string>org.other</string><key>Program</key><string>/Applications/Another.app/Contents/MacOS/main</string>".into(),
    ] {
        fs::write(scope.launch[0].join("other.plist"), xml(&body)).unwrap();
        assert_eq!(
            observe_with(&f.plan, &f.ctx, &scope)
                .unwrap()
                .report
                .background
                .code,
            Code::NotDetected
        );
    }
    for bytes in [
        b"malformed".to_vec(),
        xml("<key>Label</key><string>org.other</string><key>Program</key><array/>"),
        xml("<key>Label</key><string>org.other</string><key>Program</key><string>relative/program</string>"),
        xml("<key>Label</key><string>org.other</string><key>ProgramArguments</key><integer>2</integer>"),
        xml("<key>Program</key><string>/usr/bin/true</string>"),
    ] {
        fs::write(scope.launch[0].join("other.plist"), bytes).unwrap();
        let o = observe_with(&f.plan, &f.ctx, &scope).unwrap();
        assert_eq!(o.report.background.code, Code::Unknown);
        assert!(o
            .permits_confirmation(&f.plan, &f.ctx, crate::db::now())
            .is_err());
    }
}
#[test]
fn homebrew_name_token_and_bundle_links_retain_without_executing_brew_or_reading_hooks() {
    let f = Fixture::new(true);
    let scope = scope(&f);
    let name = f.app.file_name().unwrap();
    for root in &scope.rooms {
        let version = root.join("different-token/1.0");
        fs::create_dir_all(&version).unwrap();
        let link = version.join(name);
        symlink(&f.app, &link).unwrap();
        let o = observe_with(&f.plan, &f.ctx, &scope).unwrap();
        assert_eq!(o.report.management.code, Code::Detected);
        assert!(o
            .permits_confirmation(&f.plan, &f.ctx, crate::db::now())
            .is_err());
        fs::remove_file(link).unwrap();
        // Even a stale, unrelated same-name copy is a conservative retention
        // clue. No name-based ownership or delete target is inferred.
        fs::create_dir(version.join(name)).unwrap();
        assert_eq!(
            observe_with(&f.plan, &f.ctx, &scope)
                .unwrap()
                .report
                .management
                .code,
            Code::Detected
        );
        fs::remove_dir_all(root.join("different-token")).unwrap();
    }
    let stem = f.app.file_stem().unwrap().to_str().unwrap().to_uppercase();
    fs::create_dir(scope.rooms[0].join(stem)).unwrap();
    assert_eq!(
        observe_with(&f.plan, &f.ctx, &scope)
            .unwrap()
            .report
            .management
            .reason,
        Some("homebrewToken")
    );
}
#[test]
fn missing_roots_new_configs_file_changes_root_replacement_and_aliases_revoke_clearance() {
    for mode in ["missing", "newFile", "fileChanged", "rootSwap", "alias"] {
        let f = Fixture::new(true);
        let scope = scope(&f);
        let target = scope.launch[0].join("other.plist");
        if mode == "missing" {
            fs::remove_dir(&scope.launch[0]).unwrap();
        }
        if mode == "fileChanged" {
            fs::write(&target, xml("<key>Label</key><string>org.other</string><key>Program</key><string>/usr/bin/true</string>")).unwrap();
        }
        let o = observe_with(&f.plan, &f.ctx, &scope).unwrap();
        o.permits_confirmation(&f.plan, &f.ctx, crate::db::now())
            .unwrap();
        match mode {
            "missing" => fs::create_dir(&scope.launch[0]).unwrap(),
            "newFile" | "fileChanged" => fs::write(
                &target,
                xml("<key>Label</key><string>io.moomole.runtime.fixture</string>"),
            )
            .unwrap(),
            "rootSwap" => {
                fs::rename(&scope.rooms[0], f.home.join("saved-room")).unwrap();
                fs::create_dir(&scope.rooms[0]).unwrap();
            }
            "alias" => {
                fs::rename(&scope.launch[0], f.home.join("saved-agents")).unwrap();
                symlink(f.home.join("saved-agents"), &scope.launch[0]).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            o.permits_confirmation(&f.plan, &f.ctx, crate::db::now())
                .is_err(),
            "{mode}"
        );
    }
}
#[test]
fn scanner_refuses_links_special_files_oversized_plists_and_exhausted_budget() {
    let f = Fixture::new(true);
    let scope = scope(&f);
    let target = scope.launch[0].join("other.plist");
    let outside = f.home.join("outside.plist");
    fs::write(&outside, xml("<key>Label</key><string>org.other</string><key>Program</key><string>/usr/bin/true</string>")).unwrap();
    symlink(&outside, &target).unwrap();
    assert_eq!(
        observe_with(&f.plan, &f.ctx, &scope)
            .unwrap()
            .report
            .background
            .code,
        Code::Unknown
    );
    fs::remove_file(&target).unwrap();
    fs::write(&target, vec![0u8; 512 * 1024 + 1]).unwrap();
    assert_eq!(
        observe_with(&f.plan, &f.ctx, &scope)
            .unwrap()
            .report
            .background
            .code,
        Code::Unknown
    );
    fs::remove_file(&target).unwrap();
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(target.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    assert_eq!(
        observe_with(&f.plan, &f.ctx, &scope)
            .unwrap()
            .report
            .background
            .code,
        Code::Unknown
    );
    assert_eq!(
        fs::read(&outside).unwrap(),
        xml("<key>Label</key><string>org.other</string><key>Program</key><string>/usr/bin/true</string>")
    );
    let mut scan = scan::Scan::new();
    for _ in 0..4097 {
        let _ = scan.exists(&f.app);
    }
    assert!(scan.finish().is_err());
}
#[test]
fn support_freshness_and_plan_home_binding_are_original_not_renewed() {
    let f = Fixture::new(true);
    let scope = scope(&f);
    let mut o = observe_with(&f.plan, &f.ctx, &scope).unwrap();
    assert!(o
        .permits_confirmation(&f.plan, &f.ctx, o.report.checked_at - 1)
        .is_err());
    assert!(o
        .permits_confirmation(&f.plan, &f.ctx, o.report.expires_at)
        .is_err());
    let other = Fixture::plan(&f.app, &f.ctx);
    assert!(o
        .permits_confirmation(&other, &f.ctx, crate::db::now())
        .is_err());
    let mut changed = f.ctx.clone();
    changed.home = f.home.join("another-home");
    assert!(o
        .permits_confirmation(&f.plan, &changed, crate::db::now())
        .is_err());
    o.started = Instant::now() - Duration::from_secs(6);
    assert!(o
        .permits_confirmation(&f.plan, &f.ctx, crate::db::now())
        .is_err());
}
#[test]
fn confirmations_require_both_fresh_runtime_and_unchanged_support_before_fixed_selection() {
    let f = Fixture::new(true);
    let scope = scope(&f);
    let mut store = crate::uninstall_confirmation::Confirmations::default();
    let support = observe_with(&f.plan, &f.ctx, &scope).unwrap();
    let runtime = crate::uninstall_runtime::fixture_observation(
        &f.plan,
        crate::uninstall_runtime::Code::NotObserved,
    );
    let g = store.begin_review(f.plan.id(), crate::db::now()).unwrap();
    let draft = store
        .publish(g, &f.plan, &f.ctx, &runtime, &support, crate::db::now())
        .unwrap()
        .unwrap();
    assert!(store
        .take(&draft.id, &f.plan, &f.ctx, crate::db::now())
        .is_ok());
    let g = store.begin_review(f.plan.id(), crate::db::now()).unwrap();
    fs::write(
        scope.launch[0].join("added.plist"),
        xml("<key>Label</key><string>io.moomole.runtime.fixture</string>"),
    )
    .unwrap();
    assert!(store
        .publish(g, &f.plan, &f.ctx, &runtime, &support, crate::db::now())
        .is_err());
    let support = observe_with(&f.plan, &f.ctx, &scope).unwrap();
    let g = store.begin_review(f.plan.id(), crate::db::now()).unwrap();
    assert!(store
        .publish(g, &f.plan, &f.ctx, &runtime, &support, crate::db::now())
        .unwrap()
        .is_none());
}
#[test]
#[ignore = "explicit read-only installed MooTool and fixed launch/Caskroom metadata; no processes or services started/stopped"]
fn installed_mootool_support_scope_is_observed_without_mutation() {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let ctx = Context {
        home: home.clone(),
        engine_root: Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
        current_app: None,
        current_id: "io.github.rememberber.moomole".into(),
        protected_paths: vec![],
    };
    let source=crate::uninstall_plan::Source::from_row(&uuid::Uuid::new_v4().to_string(),0,&serde_json::json!({"name":"MooTool","path":"/Applications/MooTool Next Electron.app","bundle_id":"com.rememberber.mootool.next.electron","source":"App"})).unwrap();
    let plan = Plan::prepare(source, &ctx).unwrap();
    let o = observe(&plan, &ctx).unwrap();
    eprintln!("support scope background={:?} reason={:?} entries={} management={:?} reason={:?} entries={} canPrepare={} canExecute={}",o.report.background.code,o.report.background.reason,o.report.background.checked_entries,o.report.management.code,o.report.management.reason,o.report.management.checked_entries,o.report.can_prepare_confirmation,o.report.can_execute);
    assert!(!o.report.can_execute);
}
