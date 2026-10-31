use super::*;
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    process::Command,
};
fn job() -> Job {
    Job::new(TaskKind::AppControl)
}

#[test]
fn declared_privileged_service_management_is_retained_even_without_embedded_helpers() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().canonicalize().unwrap();
    let app = identity::fixture(&home, "Managed.app", "dev.moomole.managed.fixture");
    assert!(managed(&app).is_ok());
    let file = app.join("Contents/Info.plist");
    let original = fs::read_to_string(&file).unwrap();
    for key in ["SMPrivilegedExecutables", "SMAuthorizedClients"] {
        fs::write(
            &file,
            original.replace("</dict>", &format!("<key>{key}</key><dict/></dict>")),
        )
        .unwrap();
        assert!(objc2::rc::autoreleasepool(|_| managed(&app)).is_err());
        assert!(app.join("Contents/MacOS/fixture-main").is_file());
    }
}
fn attr(path: &Path, key: &std::ffi::CStr, value: &[u8]) {
    let held = AnchoredPath::open(path).unwrap();
    assert_eq!(
        unsafe {
            libc::fsetxattr(
                held.target().as_raw_fd(),
                key.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        },
        0
    );
}
#[test]
fn bounded_copy_and_swap_preserve_contents_links_modes_and_security_attributes() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let source = root.join("source");
    let stage = root.join("stage");
    let target = root.join("target");
    for path in [&source, &stage, &target] {
        fs::create_dir(path).unwrap();
    }
    fs::create_dir(source.join("dir")).unwrap();
    fs::write(source.join("dir/data"), b"new bytes").unwrap();
    fs::set_permissions(source.join("dir/data"), fs::Permissions::from_mode(0o640)).unwrap();
    symlink("dir/data", source.join("link")).unwrap();
    attr(
        &source,
        c"com.apple.quarantine",
        b"0081;68000000;MooMoleOwnedTest;00000000-0000-4000-8000-000000000001",
    );
    attr(
        &source.join("dir/data"),
        c"com.moomole.fixture",
        b"must survive",
    );
    fs::write(target.join("old"), b"old bytes").unwrap();
    let old_inode = fs::metadata(&target).unwrap().ino();
    let from = AnchoredPath::open(&source).unwrap();
    let to = AnchoredPath::open(&stage).unwrap();
    let before = native::inspect(&source, &job(), true).unwrap();
    assert_eq!(
        native::tree(from.target(), Some(to.target()), &job(), true).unwrap(),
        before
    );
    assert_eq!(
        native::inspect(&stage, &job(), true).unwrap().content,
        before.content
    );
    assert_eq!(native::inspect(&source, &job(), true).unwrap(), before);
    let parent = File::open(&root).unwrap();
    native::exchange(&parent, c"stage", &parent, c"target").unwrap();
    assert_eq!(fs::read(target.join("dir/data")).unwrap(), b"new bytes");
    assert_eq!(fs::read(stage.join("old")).unwrap(), b"old bytes");
    assert_eq!(fs::metadata(&stage).unwrap().ino(), old_inode);
    assert_eq!(
        native::inspect(&target, &job(), true).unwrap().content,
        before.content
    );
}
#[test]
fn unsafe_links_hardlinks_special_files_and_cancellation_never_copy() {
    for mode in ["absolute", "escape", "hardlink", "fifo", "cancel"] {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().canonicalize().unwrap();
        let path = root.join("app");
        fs::create_dir(&path).unwrap();
        let control = job();
        match mode {
            "absolute" => symlink(&root, path.join("link")).unwrap(),
            "escape" => symlink("../outside", path.join("link")).unwrap(),
            "hardlink" => {
                fs::write(path.join("file"), "retained").unwrap();
                fs::hard_link(path.join("file"), root.join("alias")).unwrap();
            }
            "fifo" => {
                let n = std::ffi::CString::new(path.join("fifo").to_str().unwrap()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(n.as_ptr(), 0o600) }, 0);
            }
            _ => control.cancel(),
        }
        assert!(native::inspect(&path, &control, true).is_err(), "{mode}");
        assert!(path.exists());
    }
}
#[test]
fn portable_digest_distinguishes_directory_boundaries_and_detects_same_size_edits() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    fs::create_dir(root.join("a")).unwrap();
    fs::write(root.join("a/b"), "one").unwrap();
    let before = native::inspect(&root, &job(), true).unwrap();
    fs::rename(root.join("a/b"), root.join("b")).unwrap();
    let changed = native::inspect(&root, &job(), true).unwrap();
    assert_ne!(before.content, changed.content);
    fs::write(root.join("b"), "two").unwrap();
    assert_ne!(
        changed.content,
        native::inspect(&root, &job(), true).unwrap().content
    );
    assert!(version("2.0-rc1").is_err());
    assert_eq!(version("2.0").unwrap(), version("2.0.0").unwrap());
    assert!(version("18446744073709551616").is_err());
}
fn record(draft: Draft) -> Record {
    let backup = Path::new(&draft.path)
        .parent()
        .unwrap()
        .join(format!(".moomole-app-update-{}", draft.id))
        .join(Path::new(&draft.path).file_name().unwrap());
    Record {
        id: draft.id.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        created_at: db::now(),
        finished_at: None,
        draft,
        state: "running".into(),
        phase: "checking".into(),
        reason: "owned fixture".into(),
        backup: backup.to_string_lossy().into_owned(),
        reviewed_at: None,
        recovery_receipt: None,
    }
}
#[test]
fn journal_reopens_without_replay_blocks_unknown_and_retains_every_record() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let path = root.join("state.sqlite");
    let draft = Draft {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: uuid::Uuid::new_v4().to_string(),
        index: 0,
        name: "Owned".into(),
        path: root.join("Owned.app").to_string_lossy().into_owned(),
        replacement: root.join("New.app").to_string_lossy().into_owned(),
        bundle_id: "dev.moomole.fixture".into(),
        old_version: "1".into(),
        new_version: "2".into(),
        publisher: "adHoc".into(),
        bytes: 1,
        created_at: 1,
        expires_at: 120001,
    };
    let mut r = record(draft);
    {
        let mut c = db::open_fixture(&path).unwrap();
        journal::begin(&mut c, &r).unwrap();
        r.phase = "intent".into();
        journal::save(&c, &r).unwrap();
    }
    let c = db::open_fixture(&path).unwrap();
    let rows = journal::reconcile(&c, &[]).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, "unknown");
    assert!(journal::admission(&c).is_err());
    assert!(!Path::new(&r.draft.path).exists());
    let mut reviewed = rows[0].clone();
    reviewed.reviewed_at = Some(db::now());
    journal::save(&c, &reviewed).unwrap();
    assert!(journal::admission(&c).is_ok());
    let mut forged = reviewed.clone();
    forged.backup = root.join("elsewhere.app").to_string_lossy().into_owned();
    assert!(journal::save(&c, &forged).is_err());
    assert_eq!(journal::list(&c).unwrap()[0].backup, reviewed.backup);
}
#[test]
#[ignore = "compiles and updates only two disposable signed native App fixtures; never launches them"]
fn owned_signed_app_update_retains_old_bundle_and_reopens_receipt() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().canonicalize().unwrap();
    let id = format!(
        "dev.moomole.update.fixture.{}",
        uuid::Uuid::new_v4().simple()
    );
    let old = identity::fixture(&home, "Owned.app", &id);
    let new = identity::fixture(&home.join("downloads"), "New.app", &id);
    let code = home.join("sample.c");
    fs::write(&code, "int main(void){return 0;}\n").unwrap();
    for (path, version) in [(&old, "1.0"), (&new, "2.0")] {
        let info = path.join("Contents/Info.plist");
        let content = fs::read_to_string(&info).unwrap().replace(
            "<string>1.0</string>",
            &format!("<string>{version}</string>"),
        );
        fs::write(&info, content).unwrap();
        assert!(Command::new("/usr/bin/plutil")
            .args(["-convert", "binary1"])
            .arg(&info)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("/usr/bin/xcrun")
            .args(["clang", "-mmacosx-version-min=11.0"])
            .arg(&code)
            .arg("-o")
            .arg(path.join("Contents/MacOS/fixture-main"))
            .status()
            .unwrap()
            .success());
        fs::create_dir(path.join("Contents/Resources")).unwrap();
        fs::write(path.join("Contents/Resources/data"), version).unwrap();
        symlink("Resources/data", path.join("Contents/relative")).unwrap();
        assert!(Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(path)
            .status()
            .unwrap()
            .success());
    }
    attr(
        &new,
        c"com.apple.quarantine",
        b"0081;68000000;OwnedTest;00000000-0000-4000-8000-000000000001",
    );
    let source = Source::from_row(
        &uuid::Uuid::new_v4().to_string(),
        0,
        &serde_json::json!({"path":old,"bundle_id":id,"name":"Owned","source":"App"}),
    )
    .unwrap();
    let ctx = Context {
        home: home.clone(),
        engine_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
        current_app: None,
        current_id: "dev.moomole.client".into(),
        protected_paths: vec![],
    };
    let control = job();
    let plan = objc2::rc::autoreleasepool(|_| {
        Plan::prepare(source.clone(), new.clone(), ctx.clone(), &control)
    })
    .unwrap();
    assert_eq!(plan.draft.publisher, "adHoc");
    let new_before = native::inspect(&new, &control, false).unwrap();
    let old_inode = fs::metadata(&old).unwrap().ino();
    for mode in ["cancelled", "protectionChanged", "intentNotDurable"] {
        let control = job();
        if mode == "cancelled" {
            control.cancel();
        }
        let mut context = ctx.clone();
        if mode == "protectionChanged" {
            context.protected_paths.push(source.path().into());
        }
        let mut refused = record(plan.draft.clone());
        let result = objc2::rc::autoreleasepool(|_| {
            perform(
                &plan,
                &control,
                || Ok(context.clone()),
                &mut refused,
                |row| {
                    if mode == "intentNotDurable" && row.phase == "intent" {
                        Err("owned injected durable-write failure".into())
                    } else {
                        Ok(())
                    }
                },
            )
        });
        assert!(result.is_err(), "{mode}");
        assert_eq!(
            fs::read(old.join("Contents/Resources/data")).unwrap(),
            b"1.0"
        );
        assert_eq!(fs::metadata(&old).unwrap().ino(), old_inode);
        assert_eq!(native::inspect(&new, &job(), false).unwrap(), new_before);
        if mode == "intentNotDurable" {
            assert_eq!(refused.phase, "intent");
            // Only our failed-copy fixture; production never removes this.
            fs::remove_dir_all(Path::new(&refused.backup).parent().unwrap()).unwrap();
        } else {
            assert!(!Path::new(&refused.backup).parent().unwrap().exists());
        }
    }
    let mut c = db::open_fixture(&home.join("update.sqlite")).unwrap();
    let mut r = record(plan.draft.clone());
    journal::begin(&mut c, &r).unwrap();
    let result = objc2::rc::autoreleasepool(|_| {
        perform(
            &plan,
            &control,
            || Ok(ctx.clone()),
            &mut r,
            |row| journal::save(&c, row),
        )
    });
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(r.state, "updated");
    r.phase = "finished".into();
    r.finished_at = Some(db::now());
    journal::save(&c, &r).unwrap();
    assert_eq!(
        fs::read(old.join("Contents/Resources/data")).unwrap(),
        b"2.0"
    );
    assert_eq!(
        fs::read(Path::new(&r.backup).join("Contents/Resources/data")).unwrap(),
        b"1.0"
    );
    assert_eq!(fs::metadata(&r.backup).unwrap().ino(), old_inode);
    assert_eq!(native::inspect(&new, &control, false).unwrap(), new_before);
    drop(c);
    let c = db::open_fixture(&home.join("update.sqlite")).unwrap();
    assert_eq!(journal::reconcile(&c, &[]).unwrap()[0].state, "updated");
    fs::create_dir_all("../.cache").unwrap();
    fs::write("../.cache/application-update-native.json",serde_json::to_vec_pretty(&serde_json::json!({"record":r,"sourceUnchanged":true,"oldInodeRetained":true,"signatureAndQuarantineVerified":true,"appProgramsWereNotRun":true,"scope":"native core, not App window acceptance"})).unwrap()).unwrap();
    // A newer observation with changed protection or bytes is refused before
    // staging. An already-used command cannot replay its consumed plan.
    let mut protected = ctx.clone();
    protected.protected_paths.push(source.path().into());
    assert!(Plan::prepare(source, new, protected, &job()).is_err());
}
