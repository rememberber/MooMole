use super::*;
use std::{fs, os::unix::fs::PermissionsExt, process::Command, sync::Mutex};
fn draft(root: &Path) -> Draft {
    let source_id = uuid::Uuid::new_v4().to_string();
    let now = db::now();
    Draft {
        id: uuid::Uuid::new_v4().to_string(),
        source_id: source_id.clone(),
        name: "Owned".into(),
        path: root.join("Owned.app").to_string_lossy().into_owned(),
        retained: root
            .join(format!(".moomole-app-update-{source_id}/Owned.app"))
            .to_string_lossy()
            .into_owned(),
        from_version: "2.0".into(),
        to_version: "1.0".into(),
        publisher: "adHoc".into(),
        bytes: 12,
        created_at: now,
        expires_at: now + 120_000,
    }
}
fn record(draft: Draft) -> Record {
    Record {
        id: draft.id.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        created_at: db::now(),
        finished_at: None,
        state: "running".into(),
        phase: "checking".into(),
        reason: "owned fixture".into(),
        receipt: None,
        reviewed_at: None,
        draft,
    }
}
#[test]
fn restoration_journal_reopens_unknown_and_blocks_updates_without_replaying() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let path = root.join("state.sqlite");
    let mut r = record(draft(&root));
    {
        let mut c = db::open_fixture(&path).unwrap();
        journal::begin(&mut c, &r).unwrap();
        r.phase = "intent".into();
        journal::save(&c, &r).unwrap();
        assert_eq!(
            journal::reconcile(&c, &[r.request_id.clone()]).unwrap()[0].state,
            "running"
        );
    }
    let c = db::open_fixture(&path).unwrap();
    let mut row = journal::reconcile(&c, &[]).unwrap().remove(0);
    assert_eq!(row.state, "unknown");
    assert!(super::super::journal::admission(&c).is_err());
    assert!(!Path::new(&row.draft.path).exists());
    row.reviewed_at = Some(db::now());
    journal::save(&c, &row).unwrap();
    assert!(super::super::journal::admission(&c).is_ok());
    row.state = "restored".into();
    row.phase = "finished".into();
    assert!(journal::save(&c, &row).is_err());
}
#[test]
fn old_updates_without_complete_receipts_cannot_be_adopted_as_recovery_authority() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let c = db::open_fixture(&root.join("db.sqlite")).unwrap();
    assert!(source(&c, &uuid::Uuid::new_v4().to_string()).is_err());
    let d = draft(&root);
    let old = super::super::Draft {
        id: d.source_id.clone(),
        task_id: uuid::Uuid::new_v4().to_string(),
        index: 0,
        name: d.name.clone(),
        path: d.path.clone(),
        replacement: root.join("Download.app").to_string_lossy().into_owned(),
        bundle_id: "dev.moomole.restore.owned".into(),
        old_version: "1.0".into(),
        new_version: "2.0".into(),
        publisher: "adHoc".into(),
        bytes: 12,
        created_at: d.created_at,
        expires_at: d.expires_at,
    };
    let row = super::super::Record {
        id: old.id.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        created_at: db::now(),
        finished_at: Some(db::now()),
        draft: old,
        state: "updated".into(),
        phase: "finished".into(),
        reason: "old record".into(),
        backup: d.retained.clone(),
        reviewed_at: None,
        recovery_receipt: None,
    };
    super::super::journal::save(&c, &row).unwrap();
    assert_eq!(row.view().unwrap()["recoveryAvailable"], false);
    assert!(source(&c, &row.id).is_err());
}
fn native_roundtrip(lose_completion: bool) {
    let t = tempfile::Builder::new()
        .prefix("MooMole-app-restore-owned-")
        .tempdir()
        .unwrap();
    let home = t.path().canonicalize().unwrap();
    let apps = home.join("Applications");
    let downloads = home.join("Downloads");
    fs::create_dir(&apps).unwrap();
    fs::create_dir(&downloads).unwrap();
    let id = "dev.moomole.restore.owned";
    let old = identity::fixture(&home, "Owned.app", id);
    let new = identity::fixture(&downloads, "New.app", id);
    for (path, version) in [(&old, "1.0"), (&new, "2.0")] {
        let plist = path.join("Contents/Info.plist");
        let text = fs::read_to_string(&plist).unwrap().replace(
            "<string>1.0</string>",
            &format!("<string>{version}</string>"),
        );
        fs::write(&plist, text).unwrap();
        let code = home.join(format!("main-{version}.c"));
        fs::write(&code, b"int main(void){return 0;}\n").unwrap();
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
        fs::create_dir_all(path.join("Contents/Resources")).unwrap();
        fs::write(path.join("Contents/Resources/version"), version).unwrap();
        assert!(Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(path)
            .status()
            .unwrap()
            .success());
    }
    let data = home.join("user-database");
    fs::write(&data, b"new-version user data must stay").unwrap();
    let ctx = Context {
        home: home.clone(),
        engine_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
        current_app: None,
        current_id: "dev.moomole.client".into(),
        protected_paths: vec![],
    };
    let control = Job::new(TaskKind::AppControl);
    let src = Source::from_row(
        &uuid::Uuid::new_v4().to_string(),
        0,
        &serde_json::json!({"path":old,"bundle_id":id,"name":"Owned","source":"App"}),
    )
    .unwrap();
    let plan = objc2::rc::autoreleasepool(|_| {
        super::super::Plan::prepare(src, new.clone(), ctx.clone(), &control)
    })
    .unwrap();
    let mut original = super::super::Record {
        id: plan.draft.id.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        created_at: db::now(),
        finished_at: None,
        draft: plan.draft.clone(),
        state: "running".into(),
        phase: "checking".into(),
        reason: "owned".into(),
        backup: apps
            .join(format!(".moomole-app-update-{}/Owned.app", plan.draft.id))
            .to_string_lossy()
            .into_owned(),
        reviewed_at: None,
        recovery_receipt: None,
    };
    let path = home.join("state.sqlite");
    let mut c = db::open_owned(&path).unwrap();
    super::super::journal::begin(&mut c, &original).unwrap();
    objc2::rc::autoreleasepool(|_| {
        super::super::perform(
            &plan,
            &control,
            || Ok(ctx.clone()),
            &mut original,
            |r| super::super::journal::save(&c, r),
        )
    })
    .unwrap();
    original.phase = "finished".into();
    original.finished_at = Some(db::now());
    super::super::journal::save(&c, &original).unwrap();
    assert_eq!(original.view().unwrap()["recoveryAvailable"], true);
    let original_json = serde_json::to_value(&original).unwrap();
    let replacement_before = native::inspect(&new, &control, false).unwrap();
    let prep = || {
        objc2::rc::autoreleasepool(|_| Plan::prepare(original.clone(), ctx.clone(), &control))
            .unwrap()
    };
    let mut expired = prep();
    expired.started = Instant::now() - Duration::from_secs(121);
    assert!(expired.time().is_err());
    for mode in ["cancelled", "protected", "intentNotDurable"] {
        let p = prep();
        let stopped_job = Job::new(TaskKind::AppControl);
        if mode == "cancelled" {
            stopped_job.cancel();
        }
        let mut changed = ctx.clone();
        if mode == "protected" {
            changed.protected_paths.push(original.backup.clone());
        }
        let mut r = record(p.draft.clone());
        let result = objc2::rc::autoreleasepool(|_| {
            perform(
                &p,
                &stopped_job,
                || Ok(changed.clone()),
                || Ok(()),
                &mut r,
                |r| {
                    if mode == "intentNotDurable" && r.phase == "intent" {
                        Err("owned durable failure".into())
                    } else {
                        Ok(())
                    }
                },
            )
        });
        assert!(result.is_err());
        assert_eq!(
            fs::read(old.join("Contents/Resources/version")).unwrap(),
            b"2.0"
        );
        assert_eq!(
            fs::read(Path::new(&original.backup).join("Contents/Resources/version")).unwrap(),
            b"1.0"
        );
    }
    let p = prep();
    let db = Mutex::new(c);
    if lose_completion {
        let c = db.lock().unwrap();
        c.execute_batch(journal::TABLE).unwrap();
        c.execute_batch("CREATE TRIGGER lose_restore_completion BEFORE UPDATE ON application_update_restorations WHEN json_extract(NEW.body,'$.state')='restored' BEGIN SELECT RAISE(ABORT,'owned lost completion'); END;").unwrap();
    }
    let result = commands::run(p, uuid::Uuid::new_v4().to_string(), &control, &db, || {
        Ok(ctx.clone())
    });
    if lose_completion {
        assert!(result.is_err());
    } else {
        assert_eq!(result.unwrap().state, "restored");
    }
    assert_eq!(
        fs::read(old.join("Contents/Resources/version")).unwrap(),
        b"1.0"
    );
    assert_eq!(
        fs::read(Path::new(&original.backup).join("Contents/Resources/version")).unwrap(),
        b"2.0"
    );
    assert_eq!(fs::read(&data).unwrap(), b"new-version user data must stay");
    assert_eq!(
        native::inspect(&new, &control, false).unwrap(),
        replacement_before
    );
    drop(db);
    let c = db::open_fixture(&path).unwrap();
    let records = journal::reconcile(&c, &[]).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].state,
        if lose_completion {
            "unknown"
        } else {
            "restored"
        }
    );
    assert_eq!(
        super::super::journal::admission(&c).is_err(),
        lose_completion
    );
    assert_eq!(
        serde_json::to_value(super::super::journal::list(&c).unwrap().remove(0)).unwrap(),
        original_json
    );
    assert!(objc2::rc::autoreleasepool(|_| Plan::prepare(original, ctx, &control)).is_err());
    fs::write(format!("../.cache/application-restoration-native-{}.json",if lose_completion {"lost-completion"}else{"completed"}),serde_json::to_vec_pretty(&serde_json::json!({"scope":"native core, not App window acceptance","records":records,"sourceUpdateUnchanged":true,"dataAndDownloadedPackagePreserved":true,"actualSwapToOlder":true,"completionLossReopenedUnknownWithoutReplay":lose_completion,"reliableCompletionVerified":!lose_completion,"programsWereNotRun":true})).unwrap()).unwrap();
}

#[test]
#[ignore = "actually updates and restores owned signed Mach-O App bundles, including completion loss; never runs them or touches user applications"]
fn real_signed_update_restore_preserves_new_bundle_data_and_reopens_unknown_without_replay() {
    native_roundtrip(false);
    native_roundtrip(true);
}
