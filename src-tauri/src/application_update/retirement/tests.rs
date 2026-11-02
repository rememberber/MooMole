use super::*;
use std::{
    cell::RefCell,
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    process::Command,
};
fn control() -> Job {
    Job::new(TaskKind::AppControl)
}
fn row(root: &Path) -> Record {
    let source_id = uuid::Uuid::new_v4().to_string();
    let id = uuid::Uuid::new_v4().to_string();
    let now = db::now();
    let stage = root.join(format!(".moomole-app-update-{source_id}"));
    Record {
        id: id.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        draft: Draft {
            id,
            source_id,
            name: "Owned".into(),
            installed: root.join("Owned.app").to_string_lossy().into(),
            retained: stage.join("Owned.app").to_string_lossy().into(),
            stage: stage.to_string_lossy().into(),
            installed_version: "2.0".into(),
            retained_version: "1.0".into(),
            after_restore: false,
            publisher: "adHoc".into(),
            bytes: 12,
            created_at: now,
            expires_at: now + 120000,
        },
        state: "running".into(),
        phase: "checking".into(),
        reason: "owned test".into(),
        created_at: now,
        finished_at: None,
        receipt: None,
        stage_removed: false,
        reviewed_at: None,
    }
}
#[test]
fn interrupted_retirement_is_unknown_without_replay_and_review_never_revives_source() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let path = root.join("state.sqlite");
    let mut r = row(&root);
    {
        let mut c = db::open_fixture(&path).unwrap();
        journal::begin(&mut c, &r).unwrap();
        assert!(!available(&journal::list(&c).unwrap(), &r.draft.source_id));
        assert_eq!(
            journal::reconcile(&c, &[r.request_id.clone()]).unwrap()[0].state,
            "running"
        );
        r.phase = "intent".into();
        journal::save(&c, &r).unwrap();
    }
    let c = db::open_fixture(&path).unwrap();
    let mut r = journal::list(&c).unwrap().remove(0);
    assert_eq!(r.state, "unknown");
    assert!(super::super::journal::admission(&c).is_err());
    assert!(!Path::new(&r.draft.installed).exists());
    r.reviewed_at = Some(db::now().max(r.created_at));
    journal::save(&c, &r).unwrap();
    super::super::journal::admission(&c).unwrap();
    assert!(block_source(&c, &r.draft.source_id).is_err());
    assert!(recovery::retirement_mapping(&c, &r.draft.source_id)
        .unwrap()
        .is_none());
    assert!(!available(&journal::list(&c).unwrap(), &r.draft.source_id));
}
#[test]
fn retirement_journal_rejects_redirected_stages_false_success_and_weaker_durability() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let c = db::open_fixture(&root.join("state.sqlite")).unwrap();
    let r = row(&root);
    journal::save(&c, &r).unwrap();
    for mode in [
        "stage",
        "installed",
        "ttl",
        "finished",
        "success",
        "unknownField",
    ] {
        let mut bad = r.clone();
        match mode {
            "stage" => bad.draft.stage = root.to_string_lossy().into(),
            "installed" => bad.draft.installed = root.join("Other.app").to_string_lossy().into(),
            "ttl" => bad.draft.expires_at = i64::MIN,
            "finished" => bad.finished_at = Some(db::now()),
            "success" => {
                bad.state = "moved".into();
                bad.phase = "finished".into();
                bad.finished_at = Some(db::now());
            }
            _ => {
                let mut j = serde_json::to_value(&r).unwrap();
                j["unknownField"] = true.into();
                assert!(serde_json::from_value::<Record>(j).is_err());
                continue;
            }
        }
        assert!(journal::save(&c, &bad).is_err(), "{mode}");
    }
    c.pragma_update(None, "synchronous", "NORMAL").unwrap();
    assert!(journal::save(&c, &r).is_err());
    assert_eq!(journal::list(&c).unwrap()[0].id, r.id);
}
#[test]
fn retired_copy_remains_unavailable_after_review_and_duplicate_confirmation_is_denied() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let mut c = db::open_fixture(&root.join("state.sqlite")).unwrap();
    let mut r = row(&root);
    journal::begin(&mut c, &r).unwrap();
    r.state = "moved".into();
    r.phase = "finished".into();
    r.finished_at = Some(db::now());
    r.receipt = Some(crate::execution::Receipt {
        destination: root.join("actual.app").to_string_lossy().into(),
        device: 1,
        inode: 2,
    });
    r.stage_removed = true;
    journal::save(&c, &r).unwrap();
    assert!(block_source(&c, &r.draft.source_id).is_err());
    assert!(journal::begin(&mut c, &r).is_err());
    assert!(source(&c, &r.draft.source_id).is_err()); // No receipt, never adopt by a matching name.
    let mut refused = row(&root);
    refused.state = "retained".into();
    refused.phase = "finished".into();
    refused.finished_at = Some(db::now());
    journal::save(&c, &refused).unwrap();
    assert!(available(
        &journal::list(&c).unwrap(),
        &refused.draft.source_id
    ));
}
fn fixture() -> (
    tempfile::TempDir,
    Mutex<db::Database>,
    Context,
    super::super::Record,
    PathBuf,
    PathBuf,
) {
    let t = tempfile::Builder::new()
        .prefix("MooMole-retained-app-owned-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let downloads = home.join("Downloads");
    fs::create_dir(&downloads).unwrap();
    let id = format!("dev.moomole.retained.{}", uuid::Uuid::new_v4().simple());
    let old = identity::fixture(&home, "Owned.app", &id);
    let new = identity::fixture(&downloads, "New.app", &id);
    let code = home.join("owned.c");
    fs::write(&code, b"int main(void){return 0;}\n").unwrap();
    for (path, version) in [(&old, "1.0"), (&new, "2.0")] {
        let plist = path.join("Contents/Info.plist");
        fs::write(
            &plist,
            fs::read_to_string(&plist).unwrap().replace(
                "<string>1.0</string>",
                &format!("<string>{version}</string>"),
            ),
        )
        .unwrap();
        assert!(Command::new("/usr/bin/clang")
            .args(["-mmacosx-version-min=12.0", "-o"])
            .arg(path.join("Contents/MacOS/fixture-main"))
            .arg(&code)
            .status()
            .unwrap()
            .success());
        fs::set_permissions(
            path.join("Contents/MacOS/fixture-main"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        fs::create_dir(path.join("Contents/Resources")).unwrap();
        fs::write(path.join("Contents/Resources/version"), version).unwrap();
        symlink("Resources/version", path.join("Contents/version-link")).unwrap();
        assert!(Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(path)
            .status()
            .unwrap()
            .success());
    }
    let data = home.join("user-data");
    fs::write(&data, b"package external user data must stay").unwrap();
    let ctx = Context {
        home: home.clone(),
        engine_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
        current_app: None,
        current_id: "dev.moomole.client".into(),
        protected_paths: vec![],
    };
    let src = Source::from_row(
        &uuid::Uuid::new_v4().to_string(),
        0,
        &serde_json::json!({"path":old,"bundle_id":id,"name":"Owned","source":"App"}),
    )
    .unwrap();
    let job = control();
    let plan = super::super::Plan::prepare(src, new.clone(), ctx.clone(), &job).unwrap();
    let mut c = db::open_owned(&home.join("state.sqlite")).unwrap();
    let mut r = super::super::Record {
        id: plan.draft.id.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        draft: plan.draft.clone(),
        state: "running".into(),
        phase: "checking".into(),
        reason: "owned".into(),
        created_at: db::now(),
        finished_at: None,
        backup: old
            .parent()
            .unwrap()
            .join(format!(".moomole-app-update-{}/Owned.app", plan.draft.id))
            .to_string_lossy()
            .into(),
        reviewed_at: None,
        recovery_receipt: None,
    };
    super::super::journal::begin(&mut c, &r).unwrap();
    super::super::perform(
        &plan,
        &job,
        || Ok(ctx.clone()),
        &mut r,
        |r| super::super::journal::save(&c, r),
    )
    .unwrap();
    assert_eq!(r.state, "updated");
    r.phase = "finished".into();
    r.finished_at = Some(db::now());
    super::super::journal::save(&c, &r).unwrap();
    (t, Mutex::new(c), ctx, r, new, data)
}
fn native_case(
    after_restore: bool,
    lost: bool,
    stop_after_move: bool,
    reports: &mut Vec<serde_json::Value>,
) {
    let (mut t, c, ctx, original, new, data) = fixture();
    let job = control();
    let original_json = serde_json::to_value(&original).unwrap();
    let restored = if after_restore {
        Some(recovery::tests::restore_owned(&c, original.clone(), ctx.clone(), &job).unwrap())
    } else {
        None
    };
    let make = || {
        Plan::prepare(
            source(&c.lock().unwrap(), &original.id).unwrap(),
            ctx.clone(),
            &job,
        )
        .unwrap()
    };
    let before_installed = native::inspect(Path::new(&original.draft.path), &job, true).unwrap();
    let before_retained = native::inspect(Path::new(&original.backup), &job, true).unwrap();
    let retained_bytes = byte_inventory(Path::new(&original.backup));
    let before_download = native::inspect(&new, &job, false).unwrap();
    let mut expired = make();
    expired.issued = Instant::now() - Duration::from_secs(121);
    assert!(expired.check(&c.lock().unwrap(), &ctx, &job).is_err());
    let mut rollback = make();
    rollback.draft.created_at = db::now() + 10000;
    assert!(rollback.check(&c.lock().unwrap(), &ctx, &job).is_err());
    let extra = Path::new(&original.backup)
        .parent()
        .unwrap()
        .join("unknown.txt");
    fs::write(&extra, b"must keep").unwrap();
    assert!(Plan::prepare(
        source(&c.lock().unwrap(), &original.id).unwrap(),
        ctx.clone(),
        &job
    )
    .is_err());
    assert_eq!(fs::read(&extra).unwrap(), b"must keep");
    fs::remove_file(&extra).unwrap();
    for mode in ["cancelled", "protected", "intentFailure"] {
        let plan = make();
        let j = control();
        let mut changed = ctx.clone();
        if mode == "cancelled" {
            j.cancel()
        } else if mode == "protected" {
            changed.protected_paths.push(original.backup.clone())
        }
        if mode == "intentFailure" {
            c.lock().unwrap().execute_batch("CREATE TRIGGER retained_intent_fail BEFORE UPDATE ON application_retained_retirements WHEN json_extract(NEW.body,'$.phase')='intent' BEGIN SELECT RAISE(FAIL,'owned intent failure');END;").unwrap();
        }
        let r = run(
            plan,
            uuid::Uuid::new_v4().to_string(),
            &j,
            &c,
            || Ok(changed.clone()),
            |_| panic!("native operation must not start"),
        )
        .unwrap();
        assert_eq!(
            r.state,
            if mode == "cancelled" {
                "cancelled"
            } else {
                "retained"
            }
        );
        assert!(r.receipt.is_none());
        if mode == "intentFailure" {
            c.lock()
                .unwrap()
                .execute_batch("DROP TRIGGER retained_intent_fail;")
                .unwrap();
        }
        assert_eq!(
            native::inspect(Path::new(&original.backup), &job, true).unwrap(),
            before_retained
        );
    }
    let plan = make();
    assert_eq!(plan.draft.after_restore, after_restore);
    assert_eq!(
        plan.draft.retained_version,
        if after_restore { "2.0" } else { "1.0" }
    );
    if lost {
        c.lock().unwrap().execute_batch("CREATE TRIGGER retained_completion_fail BEFORE UPDATE ON application_retained_retirements WHEN json_extract(NEW.body,'$.phase')='finished' BEGIN SELECT RAISE(FAIL,'owned completion failure');END;").unwrap();
    }
    let draft = plan.draft.clone();
    let actual = RefCell::new(None);
    t.disable_cleanup(true);
    eprintln!("Owned retained App rescue root: {}", t.path().display());
    let result = run(
        plan,
        uuid::Uuid::new_v4().to_string(),
        &job,
        &c,
        || {
            // Production context reads this same DB. A guard held by the
            // executor must not leak into that call (or the App would deadlock).
            let guard = c
                .try_lock()
                .map_err(|_| "context called under journal lock")?;
            db::settings(&guard)?;
            Ok(ctx.clone())
        },
        |r| {
            if stop_after_move {
                job.cancel();
            }
            *actual.borrow_mut() = Some(r.clone());
            fs::create_dir_all("../.cache").unwrap();
            fs::write(
                "../.cache/retained-app-native-rescue.json",
                serde_json::to_vec(&serde_json::json!({"ownedRoot":t.path(),"record":r})).unwrap(),
            )
            .unwrap();
        },
    );
    let native = actual
        .into_inner()
        .expect("explicit native acceptance must return its receipt");
    let receipt = native
        .receipt
        .as_ref()
        .expect("keep root and rescue record on unknown native result");
    let meta = fs::symlink_metadata(&receipt.destination).unwrap();
    assert!(meta.is_dir());
    assert_eq!((meta.dev(), meta.ino()), (receipt.device, receipt.inode));
    let stage_removed = !Path::new(&draft.stage).exists();
    // Return only the exact owned package after identity and content checks;
    // its test root is retained on failure, never purge a shared Trash directory.
    if stage_removed {
        fs::create_dir(&draft.stage).unwrap();
    }
    assert!(!Path::new(&draft.retained).exists());
    fs::rename(&receipt.destination, &draft.retained).unwrap();
    assert_eq!(byte_inventory(Path::new(&draft.retained)), retained_bytes);
    assert!(Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&draft.retained)
        .status()
        .unwrap()
        .success());
    t.disable_cleanup(false);
    fs::remove_file("../.cache/retained-app-native-rescue.json").unwrap();
    assert_eq!(native.state, "moved");
    assert_eq!(stage_removed, !stop_after_move);
    if lost {
        assert!(result.is_err());
        c.lock()
            .unwrap()
            .execute_batch("DROP TRIGGER retained_completion_fail;")
            .unwrap();
    } else {
        let r = result.unwrap();
        assert_eq!(r.state, "moved");
        assert_eq!(r.stage_removed, !stop_after_move);
    }
    let verification = control();
    assert_eq!(
        native::inspect(Path::new(&original.draft.path), &verification, true).unwrap(),
        before_installed
    );
    assert_eq!(
        native::inspect(&new, &verification, false).unwrap(),
        before_download
    );
    assert_eq!(
        fs::read(&data).unwrap(),
        b"package external user data must stay"
    );
    drop(c);
    let reopened = db::open_fixture(&ctx.home.join("state.sqlite")).unwrap();
    let rows = journal::list(&reopened).unwrap();
    let r = rows.iter().find(|r| r.draft.id == draft.id).unwrap();
    assert_eq!(r.state, if lost { "unknown" } else { "moved" });
    assert!(block_source(&reopened, &original.id).is_err());
    assert!(!recovery::tests::source_available(&reopened, &original.id));
    assert_eq!(
        serde_json::to_value(super::super::journal::list(&reopened).unwrap().remove(0)).unwrap(),
        original_json
    );
    if let Some(expected) = restored {
        assert_eq!(recovery::tests::owned_records(&reopened)[0], expected);
    }
    reports.push(serde_json::json!({"record":r,"afterRestore":after_restore,"lostCompletion":lost,"stoppedAfterMove":stop_after_move,"installedAndDownloadedPackagesUnchanged":true,"externalUserDataPreserved":true,"originalUpdateAndRestoreRecordsUnchanged":true,"ownedNativeReceiptReturned":true,"emptyStageRemoved":stage_removed,"reopenedWithoutReplay":true}));
}
#[test]
#[ignore = "real native Trash of self-generated signed complete Apps before/after restore, exact receipt return and auto-cleaned temp roots; never launch programs"]
fn real_retained_app_cleanup_before_after_restore_and_lost_completion() {
    let mut reports = vec![];
    objc2::rc::autoreleasepool(|_| {
        for after_restore in [false, true] {
            for lost in [false, true] {
                native_case(after_restore, lost, false, &mut reports)
            }
        }
        native_case(false, false, true, &mut reports);
    });
    fs::write(
        "../.cache/retained-app-native.json",
        serde_json::to_vec(&serde_json::json!({"appWindowVerified":false,"reports":reports}))
            .unwrap(),
    )
    .unwrap();
}

fn byte_inventory(root: &Path) -> std::collections::BTreeMap<PathBuf, String> {
    fn walk(root: &Path, p: &Path, out: &mut std::collections::BTreeMap<PathBuf, String>) {
        let m = fs::symlink_metadata(p).unwrap();
        let value = if m.is_symlink() {
            format!("link:{}", fs::read_link(p).unwrap().display())
        } else if m.is_dir() {
            "directory".into()
        } else {
            format!("file:{:x}", Sha256::digest(fs::read(p).unwrap()))
        };
        out.insert(p.strip_prefix(root).unwrap().into(), value);
        if m.is_dir() {
            for e in fs::read_dir(p).unwrap() {
                walk(root, &e.unwrap().path(), out)
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(root, root, &mut out);
    out
}
