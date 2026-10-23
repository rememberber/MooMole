use super::*;
use crate::{
    clean_preview::{Candidate, Preview, PROTOCOL},
    provenance::{Origin, Provenance},
};
use objc2::{rc::autoreleasepool, AnyThread};
use objc2_foundation::{NSAppleScript, NSString};
use std::{
    fs,
    os::unix::fs::{symlink, MetadataExt},
    path::PathBuf,
    process::{Command, Stdio},
};

fn fixture_plan(home: &Path, names: &[&str]) -> review::Plan {
    let root = home.join("Library/Logs");
    fs::create_dir_all(&root).unwrap();
    let items = names.iter().map(|name| {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "owned temporary log; never trash").unwrap();
        let m = fs::metadata(&path).unwrap();
        let path = path.to_str().unwrap().to_string();
        Candidate { id: String::new(), upstream_identity: format!("inode:{}:{}", m.dev(), m.ino()),
            path: path.clone(), section: "fixture logs".into(), size: Some(m.len()), item_count: 1,
            is_directory: false, resolved_path: None, covered_by: None, impact: None,
            provenance: Some(Provenance { version: 1, observations: vec![Origin::from_fields(&path,
                "record_dry_run_cleanup_target,_safe_clean_impl,safe_clean_guarded,_clean_user_log_directory,clean_user_essentials",
                "_user_log_delete_guard", "_user_log_delete_guard").unwrap()] }),
        }
    }).collect();
    let scan = review::capture(
        uuid::Uuid::new_v4().to_string(),
        Preview {
            output: String::new(),
            dry_run: true,
            protocol: PROTOCOL,
            revision: crate::engine::REVISION.into(),
            system_included: false,
            items,
        },
        crate::db::now(),
    )
    .unwrap();
    review::create(
        &scan,
        &scan
            .items
            .iter()
            .map(|i| i.candidate.id.clone())
            .collect::<Vec<_>>(),
        &[],
        crate::db::now(),
    )
    .unwrap()
}

fn setup(names: &[&str]) -> (tempfile::TempDir, PathBuf, review::Plan) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let plan = fixture_plan(&home, names);
    (temp, home, plan)
}

fn report(request: &Request) -> serde_json::Value {
    serde_json::json!({"version":1,"nonce":request.payload.nonce,"digest":request.payload.digest,
        "uid":0,"visibility":"observed","complete":"MooMole.logs.complete.v1",
        "items":request.payload.items.iter().map(|i| serde_json::json!({"id":i.id,"code":"noOpenHandles","checked_ms":monotonic_ms().unwrap()})).collect::<Vec<_>>()})
}

fn frame(value: &serde_json::Value) -> Vec<u8> {
    format!("ok\n{value}\n{END}\n").into_bytes()
}

fn direct(source: &str, payload: &Payload) -> std::process::Output {
    let hex = serde_json::to_vec(payload)
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    Command::new("/usr/bin/perl")
        .args(["-e", source, &hex])
        .env_clear()
        .env("LC_ALL", "C")
        .current_dir("/")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

// ONLY nonprivileged test source: replace account/root gates for the temporary
// home and optionally fixed child output. No runtime argument/env bypass exists.
fn fixture_source(visibility: Option<&str>, exact: Option<&str>) -> String {
    let mut source = PROGRAM.replace("$> == 0", "$> == $uid").replace(
        "@account && $account[7] eq encode('UTF-8', $home) or reject();",
        "1 or reject();",
    );
    for (command, fixture) in [
        ("exec { '/usr/sbin/lsof' } '/usr/sbin/lsof', '-F', 'pu', '-p', '1';", visibility),
        ("exec { '/usr/sbin/lsof' } '/usr/sbin/lsof', '-F', 'pfn', '--', encode('UTF-8', $path);", exact),
    ] {
        if let Some(fixture) = fixture {
            assert_eq!(source.matches(command).count(), 1);
            source = source.replace(command, fixture);
        }
    }
    source
}

const VISIBLE: &str = "print qq(p1\nu0\nfcwd\nf0\n); exit 0;";
const IDLE: &str = "exit 1;";

#[test]
fn bound_report_is_one_shot_and_rejects_other_plan_nonce_user_or_framing() {
    let (_temp, home, plan) = setup(&["first.log", "DiagnosticReports/second.log"]);
    let request = new_request(&plan, &home, crate::db::now()).unwrap();
    let bytes = frame(&report(&request));
    let codes = request
        .finish(&plan, &home, &bytes, crate::db::now())
        .unwrap()
        .consume(&plan, &home, crate::db::now())
        .unwrap();
    assert!(codes.iter().all(|(_, c)| *c == Code::NoOpenHandles));
    for field in [
        "nonce",
        "digest",
        "uid",
        "version",
        "complete",
        "visibility",
        "extra",
        "items",
    ] {
        let request = new_request(&plan, &home, crate::db::now()).unwrap();
        let mut value = report(&request);
        value[field] = match field {
            "uid" => 501.into(),
            "version" => 2.into(),
            "items" => serde_json::json!([]),
            _ => "wrong".into(),
        };
        assert!(
            request
                .finish(&plan, &home, &frame(&value), crate::db::now())
                .is_err(),
            "{field}"
        );
    }
    for invalid in [
        b"ok\n{}".to_vec(),
        bytes
            .iter()
            .copied()
            .chain(b"warning".iter().copied())
            .collect(),
        vec![b'x'; OUTPUT_LIMIT + 1],
    ] {
        let request = new_request(&plan, &home, crate::db::now()).unwrap();
        assert!(request
            .finish(&plan, &home, &invalid, crate::db::now())
            .is_err());
    }
}

#[test]
fn freshness_identity_plan_and_visibility_are_revalidated_at_consumption() {
    let (_temp, home, plan) = setup(&["first.log"]);
    for mode in ["future", "stale", "unknown", "order", "extra"] {
        let request = new_request(&plan, &home, crate::db::now()).unwrap();
        let mut value = report(&request);
        match mode {
            "future" => {
                value["items"][0]["checked_ms"] = (monotonic_ms().unwrap() + 100_000).into()
            }
            "stale" => {
                value["items"][0]["checked_ms"] =
                    monotonic_ms().unwrap().saturating_sub(FRESH_MS).into()
            }
            "unknown" => value["visibility"] = "unknown".into(),
            "order" => value["items"][0]["id"] = "another id".into(),
            _ => value["items"][0]["extra"] = true.into(),
        }
        assert!(
            request
                .finish(&plan, &home, &frame(&value), crate::db::now())
                .is_err(),
            "{mode}"
        );
    }
    for mode in ["plan", "expiry", "content", "ancestor"] {
        let (_temp, home, plan) = setup(&["first.log"]);
        let request = new_request(&plan, &home, crate::db::now()).unwrap();
        let evidence = request.finish(
            &plan,
            &home,
            &frame(&report_for(&plan, &home)),
            crate::db::now(),
        );
        // A separately generated request's response cannot be replayed.
        assert!(evidence.is_err());
        let request = new_request(&plan, &home, crate::db::now()).unwrap();
        let bytes = frame(&report(&request));
        let evidence = request
            .finish(&plan, &home, &bytes, crate::db::now())
            .unwrap();
        let mut source = plan.clone();
        let mut now = crate::db::now();
        match mode {
            "plan" => source.items[0].candidate.section.push_str("changed"),
            "expiry" => now = source.expires_at,
            "content" => {
                fs::write(
                    &source.items[0].candidate.path,
                    "modified after successful report",
                )
                .unwrap();
            }
            _ => {
                let logs = home.join("Library/Logs");
                fs::rename(&logs, home.join("retained-logs")).unwrap();
                symlink(home.join("retained-logs"), &logs).unwrap();
            }
        }
        assert!(evidence.consume(&source, &home, now,).is_err(), "{mode}");
    }
}

fn report_for(plan: &review::Plan, home: &Path) -> serde_json::Value {
    report(&new_request(plan, home, crate::db::now()).unwrap())
}

#[test]
fn request_refuses_wrong_source_hidden_nested_duplicate_directory_and_hardlink() {
    for mode in [
        "hidden",
        "nested",
        "source",
        "duplicate",
        "directory",
        "hardlink",
        "expired",
        "limit",
        "symlink",
    ] {
        let name = match mode {
            "hidden" => ".hidden",
            "nested" => "nested/log",
            _ => "ordinary.log",
        };
        let (_temp, home, mut plan) = setup(&[name]);
        match mode {
            "source" => plan.items[0].candidate.provenance = None,
            "duplicate" => plan.items.push(plan.items[0].clone()),
            "directory" => plan.items[0].candidate.is_directory = true,
            "hardlink" => {
                fs::hard_link(&plan.items[0].candidate.path, home.join("second-link")).unwrap()
            }
            "expired" => plan.expires_at = crate::db::now(),
            "limit" => plan.items = (0..21).map(|_| plan.items[0].clone()).collect(),
            "symlink" => {
                let path = PathBuf::from(&plan.items[0].candidate.path);
                fs::rename(&path, home.join("moved")).unwrap();
                symlink(home.join("moved"), &path).unwrap();
            }
            _ => {}
        }
        assert!(
            new_request(&plan, &home, crate::db::now()).is_err(),
            "{mode}"
        );
    }
}

#[test]
fn actual_program_requires_root_and_never_launches_authorization_in_tests() {
    let (_temp, home, plan) = setup(&["ordinary.log"]);
    let request = new_request(&plan, &home, crate::db::now()).unwrap();
    let output = direct(PROGRAM, &request.payload);
    assert_eq!(output.status.code(), Some(126));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    // Even when ONLY the root gate is replaced in test source, a fabricated
    // HOME cannot pass the real account-directory lookup.
    let output = direct(&PROGRAM.replace("$> == 0", "$> == $uid"), &request.payload);
    assert_eq!(output.status.code(), Some(126));
    assert!(output.stdout.is_empty());
    autoreleasepool(|_| {
        let source = request.source().unwrap();
        let script =
            NSAppleScript::initWithSource(NSAppleScript::alloc(), &NSString::from_str(&source))
                .unwrap();
        let mut error = None;
        assert!(
            unsafe { script.compileAndReturnError(Some(&mut error)) },
            "{error:?}"
        ); // compile only; never execute
        assert!(!source.contains("tell application"));
        assert!(!source.contains("password "));
        assert!(source.contains("with administrator privileges"));
    });
}

#[test]
fn temporary_fixture_queries_only_exact_literal_paths_and_checks_all_ancestors() {
    let (_temp, home, plan) = setup(&["空 格'\"$()`\n.log", "DiagnosticReports/report.log"]);
    let request = new_request(&plan, &home, crate::db::now()).unwrap();
    let source = fixture_source(Some(VISIBLE), Some(IDLE));
    let output = direct(&source, &request.payload);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["visibility"], "observed");
    assert!(value["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["code"] == "noOpenHandles"));
    // The root-side account check is deliberately replaced only in this fixture.
    // Structured uid is the real ordinary user; this is not authorization proof.
    assert_eq!(value["uid"], unsafe { libc::geteuid() });
    // Exercise the actual fixed clean-environment shell transport as well.
    // Paths are data encoded as hex, including Unicode, quotes and newlines.
    let command = request
        .command()
        .unwrap()
        .replace(&shell_literal(PROGRAM), &shell_literal(&source));
    let output = Command::new("/bin/sh")
        .args(["-c", &command])
        .env_clear()
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["items"][0]["code"], "noOpenHandles");
    fs::rename(home.join("Library/Logs"), home.join("retained-logs")).unwrap();
    symlink(home.join("retained-logs"), home.join("Library/Logs")).unwrap();
    let output = direct(&source, &request.payload);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["code"] == "changed"));
}

#[test]
fn actual_exact_lsof_sees_external_handle_but_request_holds_no_target_handle() {
    let (_temp, home, plan) = setup(&["occupied ' 空格.log"]);
    let request = new_request(&plan, &home, crate::db::now()).unwrap();
    // Visibility is synthetic ONLY here; exact-file lsof is the real fixed
    // system binary as an ordinary user against our own temporary file.
    let source = fixture_source(Some(VISIBLE), None);
    let output = direct(&source, &request.payload);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["items"][0]["code"], "noOpenHandles");
    let held = fs::File::open(&plan.items[0].candidate.path).unwrap();
    let output = direct(&source, &request.payload);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["items"][0]["code"], "busy");
    drop(held);
}

#[test]
fn missing_visibility_warning_errors_overflow_and_budget_never_report_idle() {
    let (_temp, home, plan) = setup(&["ordinary.log"]);
    for (visibility, exact, expected) in [
        (
            "print qq(p1\nu0\nlsof: warning\n); exit 0;",
            IDLE,
            "unknown",
        ),
        ("print qq(p1\nu0\nf4294967296\n); exit 0;", IDLE, "unknown"),
        (VISIBLE, "print qq(lsof: warning\n); exit 1;", "unknown"),
        (VISIBLE, "exit 2;", "unknown"),
        (VISIBLE, "print 'x' x 5000; exit 1;", "unknown"),
        (
            VISIBLE,
            "select undef,undef,undef,1; exit 1;",
            "budgetExhausted",
        ),
        (
            "select undef,undef,undef,1; exit 0;",
            IDLE,
            "budgetExhausted",
        ),
    ] {
        let request = new_request(&plan, &home, crate::db::now()).unwrap();
        let source =
            fixture_source(Some(visibility), Some(exact)).replace("mono() + 5000", "mono() + 200");
        let output = direct(&source, &request.payload);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["items"][0]["code"], expected);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("lsof: warning"));
    }
}

#[test]
fn cancellation_and_expiration_withdraw_previously_valid_evidence() {
    let (_temp, home, plan) = setup(&["ordinary.log"]);
    for mode in ["cancelled", "deadline", "freshness"] {
        let request = new_request(&plan, &home, crate::db::now()).unwrap();
        let bytes = frame(&report(&request));
        let mut evidence = request
            .finish(&plan, &home, &bytes, crate::db::now())
            .unwrap();
        match mode {
            "cancelled" => evidence.request.job.cancelled.store(true, Ordering::SeqCst),
            "deadline" => evidence.request.payload.expires_ms = monotonic_ms().unwrap(),
            _ => {
                // Simulate elapsed monotonic time without sleeping or changing
                // the machine clock. The observation is after issuance, but old.
                evidence.request.issued_ms = monotonic_ms().unwrap() - FRESH_MS - 10;
                evidence.report.items[0].checked_ms = monotonic_ms().unwrap() - FRESH_MS;
            }
        }
        assert!(
            evidence.consume(&plan, &home, crate::db::now()).is_err(),
            "{mode}"
        );
    }
}

#[test]
fn native_program_rechecks_identity_after_the_exact_query() {
    let (_temp, home, plan) = setup(&["ordinary.log"]);
    let request = new_request(&plan, &home, crate::db::now()).unwrap();
    // Mutation belongs solely to this nonprivileged temporary-file fixture;
    // production code has no write operation, selectable program or such flag.
    let source = fixture_source(Some(VISIBLE), Some("open my $out, '>>', encode('UTF-8', $path) or exit 126; print $out 'fixture-change'; close $out; exit 1;"));
    let output = direct(&source, &request.payload);
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["items"][0]["code"], "changed");
    // Even a forged 'idle' response cannot defeat the parent's full witness.
    let bytes = frame(&report(&request));
    assert!(request
        .finish(&plan, &home, &bytes, crate::db::now())
        .is_err());
}

#[test]
fn root_side_rejects_expired_foreign_scope_and_changed_identity_before_query() {
    let (_temp, home, plan) = setup(&["ordinary.log"]);
    let source = fixture_source(Some(VISIBLE), Some(IDLE));
    for mode in ["expired", "scope", "links", "ancestors"] {
        let mut request = new_request(&plan, &home, crate::db::now()).unwrap();
        match mode {
            "expired" => request.payload.expires_ms = 0,
            "scope" => request.payload.items[0].path = "/etc/passwd".into(),
            "links" => request.payload.items[0].links = "2".into(),
            _ => request.payload.items[0].nodes[0][0] = "/another".into(),
        }
        let output = direct(&source, &request.payload);
        assert_eq!(output.status.code(), Some(126), "{mode}");
        assert!(output.stdout.is_empty());
    }
    let mut request = new_request(&plan, &home, crate::db::now()).unwrap();
    request.payload.items[0].nodes.last_mut().unwrap()[2] = "0".into();
    let output = direct(&source, &request.payload);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["items"][0]["code"], "changed");
}

fn new_request(plan: &review::Plan, home: &Path, now: i64) -> Result<Request, String> {
    Request::new(
        plan,
        home,
        now,
        Arc::new(Job::new(crate::engine::TaskKind::LogTrash)),
    )
}
