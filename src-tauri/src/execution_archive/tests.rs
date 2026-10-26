use super::*;
use crate::{
    clean_preview::{Candidate, Preview as CleanPreview, PROTOCOL},
    db,
    maintenance::quicklook::{Attempt, Outcome, Step},
    review,
    uninstall_plan::{Context, Plan, Source},
};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
struct Fixture {
    root: tempfile::TempDir,
    db: Connection,
    at: i64,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("archive-owned-")
            .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
            .unwrap();
        let db = db::open_fixture(&root.path().join("live.sqlite")).unwrap();
        Self {
            root,
            db,
            at: db::now(),
        }
    }
    fn archives(&self) -> PathBuf {
        self.root.path().join("archives")
    }
    fn maintenance(&mut self, unknown: bool) -> String {
        let s = maintenance::Selection::new(
            &uuid::Uuid::new_v4().to_string(),
            &uuid::Uuid::new_v4().to_string(),
            vec![Step::Generators],
            &"a".repeat(64),
            self.at,
            self.at + 30_000,
        )
        .unwrap();
        let run = maintenance::begin(&mut self.db, &s, &uuid::Uuid::new_v4().to_string(), self.at)
            .unwrap();
        maintenance::intent(&mut self.db, &run, Step::Generators, self.at + 1).unwrap();
        if unknown {
            maintenance::stop_worker(&mut self.db, &run, self.at + 2).unwrap();
        } else {
            maintenance::record_attempt(
                &mut self.db,
                &run,
                &Attempt {
                    step: Step::Generators,
                    outcome: Outcome::CommandSucceeded,
                    exit_code: Some(0),
                    stop: None,
                    reason: "commandSucceeded",
                },
                self.at + 2,
            )
            .unwrap();
            maintenance::finish(&mut self.db, &run, false, self.at + 3).unwrap();
        }
        run.id().into()
    }
    fn cleanup(&mut self) -> String {
        let path = self.root.path().join("owned.log");
        fs::write(&path, "owned evidence fixture").unwrap();
        let m = path.metadata().unwrap();
        let scan = review::capture(
            uuid::Uuid::new_v4().to_string(),
            CleanPreview {
                output: String::new(),
                dry_run: true,
                protocol: PROTOCOL,
                revision: crate::engine::REVISION.into(),
                system_included: false,
                items: vec![Candidate {
                    id: String::new(),
                    upstream_identity: format!("inode:{}:{}", m.dev(), m.ino()),
                    path: path.to_str().unwrap().into(),
                    section: "owned fixture".into(),
                    size: Some(m.len()),
                    item_count: 1,
                    covered_by: None,
                    is_directory: false,
                    resolved_path: None,
                    impact: None,
                    provenance: None,
                }],
            },
            self.at,
        )
        .unwrap();
        let plan =
            review::create(&scan, &[scan.items[0].candidate.id.clone()], &[], self.at).unwrap();
        let item = &plan.items[0].candidate.id;
        let run = execution::begin(&mut self.db, &plan, self.at).unwrap();
        execution::intent(&mut self.db, &run, item, self.at + 1).unwrap();
        let destination = self.root.path().join("owned-recovery.log");
        fs::rename(path, &destination).unwrap();
        execution::complete_item(
            &mut self.db,
            &run,
            item,
            execution::Outcome::Moved(execution::Receipt {
                destination: destination.to_str().unwrap().into(),
                device: m.dev(),
                inode: m.ino(),
            }),
            "owned test move",
            self.at + 2,
        )
        .unwrap();
        execution::finish(&mut self.db, &run, false, self.at + 3).unwrap();
        run
    }
    fn uninstall(&mut self, unknown: bool) -> String {
        let home = self.root.path().canonicalize().unwrap();
        let id = "org.moomole.archive.fixture";
        let path = crate::app_inspection::identity::fixture(
            &home,
            &format!("{}.app", uuid::Uuid::new_v4()),
            id,
        );
        let ctx = Context {
            home: home.clone(),
            engine_root: Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
            current_app: None,
            current_id: "io.github.rememberber.moomole".into(),
            protected_paths: vec![],
        };
        let row = serde_json::json!({"path":path,"bundle_id":id,"name":"Owned archive fixture","source":"App"});
        let plan = Plan::prepare(
            Source::from_row(&uuid::Uuid::new_v4().to_string(), 0, &row).unwrap(),
            &ctx,
        )
        .unwrap();
        let run = uninstall::begin(&mut self.db, &plan, db::now()).unwrap();
        uninstall::intent(&mut self.db, &run, db::now()).unwrap();
        if unknown {
            uninstall::reconcile(&mut self.db, db::now()).unwrap();
        } else {
            let destination = home.join("owned-recovery.app");
            fs::rename(&path, &destination).unwrap();
            let after = crate::app_inspection::identity::capture(&destination, id).unwrap();
            let node = after.root.nodes.last().unwrap();
            uninstall::complete(
                &mut self.db,
                &run,
                crate::uninstall_target::Outcome::Moved(crate::uninstall_target::Receipt {
                    version: 1,
                    actual: execution::Receipt {
                        destination: destination.to_str().unwrap().into(),
                        device: node.dev,
                        inode: node.ino,
                    },
                    observed_inventory: Some(after.inventory),
                }),
                "owned test move",
                db::now(),
            )
            .unwrap();
        }
        run.id().into()
    }
}
#[test]
fn full_capacity_archive_preserves_all_three_modules_and_releases_admission_without_target_changes()
{
    let mut f = Fixture::new();
    let cleanup = f.cleanup();
    let uninstall = f.uninstall(false);
    let unknown = f.maintenance(true);
    let d = maintenance::detail(&f.db, &unknown).unwrap();
    maintenance::acknowledge(&mut f.db, &unknown, &d.snapshot_sha256, f.at + 4).unwrap();
    for _ in 3..64 {
        f.maintenance(false);
    }
    let before = collect(&f.db).unwrap();
    assert_eq!(before.counts.total(), 64);
    let originals = vec![
        (
            Kind::Cleanup,
            cleanup.clone(),
            serde_json::to_value(execution::detail(&f.db, &cleanup).unwrap()).unwrap(),
        ),
        (
            Kind::Uninstall,
            uninstall.clone(),
            serde_json::to_value(uninstall::detail(&f.db, &uninstall).unwrap()).unwrap(),
        ),
        (
            Kind::Maintenance,
            unknown.clone(),
            serde_json::to_value(maintenance::detail(&f.db, &unknown).unwrap()).unwrap(),
        ),
    ];
    let t = f.db.transaction().unwrap();
    assert!(crate::execution_admission::reserve(&t, 1).is_err());
    t.rollback().unwrap();
    let preview = prepare(&mut f.db).unwrap();
    let root = f.archives();
    let h = archive(&mut f.db, &root, &preview.fingerprint, f.at + 100).unwrap();
    for table in TABLES {
        assert_eq!(
            f.db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    let t = f.db.transaction().unwrap();
    crate::execution_admission::reserve(&t, 1).unwrap();
    t.rollback().unwrap();
    let loaded = load(&filename(&f.archives(), &h.id).unwrap(), true).unwrap();
    assert_eq!(collect(&loaded.database).unwrap().body, before.body);
    for (kind, id, expected) in originals {
        let value = detail(&f.archives(), &h.id, kind, &id).unwrap();
        assert_eq!(value, expected);
        assert!(!value["events"].as_array().unwrap().is_empty());
    }
    assert_eq!(
        fs::read_to_string(f.root.path().join("owned-recovery.log")).unwrap(),
        "owned evidence fixture"
    );
    assert!(f.root.path().join("owned-recovery.app").is_dir());
    assert_eq!(list(&f.archives()).unwrap().archives, vec![h.clone()]);
    assert_eq!(view(&f.archives(), &h.id).unwrap().entries.len(), 64);
    drop(f.db);
    let reopened = db::open_fixture(&f.root.path().join("live.sqlite")).unwrap();
    assert!(execution::list(&reopened).unwrap().is_empty());
    assert_eq!(view(&root, &h.id).unwrap().entries.len(), 64);
}
#[test]
fn unknown_running_changed_and_unsafe_storage_preserve_live_evidence() {
    let mut f = Fixture::new();
    let id = f.maintenance(true);
    assert!(prepare(&mut f.db).unwrap_err().contains("核查"));
    let d = maintenance::detail(&f.db, &id).unwrap();
    maintenance::acknowledge(&mut f.db, &id, &d.snapshot_sha256, f.at + 4).unwrap();
    let p = prepare(&mut f.db).unwrap();
    f.maintenance(false);
    assert!(archive(
        &mut f.db,
        &f.root.path().join("archives"),
        &p.fingerprint,
        f.at + 10
    )
    .unwrap_err()
    .contains("变化"));
    let before = collect(&f.db).unwrap().body;
    let root = f.archives();
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    let p = prepare(&mut f.db).unwrap();
    assert!(archive(&mut f.db, &root, &p.fingerprint, f.at + 10).is_err());
    assert_eq!(collect(&f.db).unwrap().body, before);
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    f.db.pragma_update(None, "synchronous", "OFF").unwrap();
    assert!(archive(&mut f.db, &root, &p.fingerprint, f.at + 10).is_err());
}
#[test]
fn export_import_is_read_only_non_overwriting_and_rejects_corruption_and_links() {
    let mut f = Fixture::new();
    let id = f.uninstall(true);
    let d = uninstall::detail(&f.db, &id).unwrap();
    crate::execution_review::acknowledge(
        &mut f.db,
        crate::execution_review::Kind::Uninstall,
        &id,
        &d.snapshot_sha256,
        db::now(),
    )
    .unwrap();
    let p = prepare(&mut f.db).unwrap();
    let root = f.archives();
    let h = archive(&mut f.db, &root, &p.fingerprint, db::now()).unwrap();
    let external = f.root.path().join("external.moomole-history");
    export(&root, &h.id, &external).unwrap();
    let bytes = fs::read(&external).unwrap();
    assert!(export(&root, &h.id, &external).is_err());
    assert_eq!(bytes, fs::read(&external).unwrap());
    let imported = f.root.path().join("imported");
    assert_eq!(import(&imported, &external).unwrap(), h);
    assert_eq!(import(&imported, &external).unwrap(), h);
    let value = detail(&imported, &h.id, Kind::Uninstall, &id).unwrap();
    assert_eq!(value["itemState"], "uncertain");
    assert!(value["reviewedAt"].is_number());
    assert_eq!(uninstall::list(&f.db).unwrap().len(), 0);
    let link = f.root.path().join("linked");
    std::os::unix::fs::symlink(&external, &link).unwrap();
    assert!(import(&imported, &link).is_err());
    let mut broken = bytes;
    *broken.last_mut().unwrap() ^= 1;
    fs::write(&external, broken).unwrap();
    assert!(import(&imported, &external).is_err());
    assert!(view(&imported, &h.id).is_ok());
}

#[test]
fn published_copy_survives_interruption_and_final_transaction_failure_without_losing_originals() {
    let mut f = Fixture::new();
    let run = f.maintenance(false);
    let before = collect(&f.db).unwrap().body;
    let preview = prepare(&mut f.db).unwrap();
    let root = f.archives();
    assert!(
        archive_with(&mut f.db, &root, &preview.fingerprint, f.at + 10, |_, _| {
            Err("owned interruption after durable publication".into())
        })
        .is_err()
    );
    assert_eq!(collect(&f.db).unwrap().body, before);
    let copies = list(&root).unwrap().archives;
    assert_eq!(copies.len(), 1);
    assert!(detail(&root, &copies[0].id, Kind::Maintenance, &run).is_ok());
    assert!(archive_with(&mut f.db,&root,&preview.fingerprint,f.at+11,|t,_|{
  t.execute_batch("CREATE TABLE fault_only_reference(id TEXT REFERENCES maintenance_runs(id) DEFERRABLE INITIALLY DEFERRED);").unwrap();t.execute("INSERT INTO fault_only_reference VALUES (?1)",[&run]).unwrap();Ok(())
 }).unwrap_err().contains("提交未确认"));
    assert_eq!(collect(&f.db).unwrap().body, before);
    assert_eq!(list(&root).unwrap().archives.len(), 2);
    assert_eq!(
        f.db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='fault_only_reference'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    let h = archive(&mut f.db, &root, &preview.fingerprint, f.at + 12).unwrap();
    assert!(maintenance::list(&f.db).unwrap().is_empty());
    assert!(detail(&root, &h.id, Kind::Maintenance, &run).is_ok());
    assert!(prepare(&mut f.db).is_err());
}

#[test]
fn running_records_and_disabled_foreign_keys_cannot_be_archived() {
    let mut f = Fixture::new();
    let s = maintenance::Selection::new(
        &uuid::Uuid::new_v4().to_string(),
        &uuid::Uuid::new_v4().to_string(),
        vec![Step::Generators],
        &"a".repeat(64),
        f.at,
        f.at + 30_000,
    )
    .unwrap();
    let run = maintenance::begin(&mut f.db, &s, &uuid::Uuid::new_v4().to_string(), f.at).unwrap();
    assert!(prepare(&mut f.db).unwrap_err().contains("结束任务"));
    assert!(!f.archives().exists());
    maintenance::finish(&mut f.db, &run, true, f.at + 2).unwrap();
    let preview = prepare(&mut f.db).unwrap();
    let before = collect(&f.db).unwrap().body;
    let root = f.archives();
    f.db.pragma_update(None, "foreign_keys", false).unwrap();
    assert!(archive(&mut f.db, &root, &preview.fingerprint, f.at + 4).is_err());
    assert_eq!(collect(&f.db).unwrap().body, before);
    assert!(!root.exists());
}

#[test]
fn coherent_outer_digest_cannot_hide_invalid_snapshots_reviews_or_orphan_rows() {
    let mut f = Fixture::new();
    let id = f.maintenance(true);
    let d = maintenance::detail(&f.db, &id).unwrap();
    maintenance::acknowledge(&mut f.db, &id, &d.snapshot_sha256, f.at + 4).unwrap();
    let p = prepare(&mut f.db).unwrap();
    let root = f.archives();
    let h = archive(&mut f.db, &root, &p.fingerprint, f.at + 10).unwrap();
    let path = filename(&root, &h.id).unwrap();
    let original = fs::read(&path).unwrap();
    let at = original.iter().position(|c| *c == b'\n').unwrap();
    let tables: Tables = serde_json::from_slice(&original[at + 1..]).unwrap();
    for case in 0..5 {
        let mut changed = tables.clone();
        match case {
            0 => changed[10][0][2] = Value::String("invalid reviewed time".into()),
            1 => changed[7][0][7] = Value::String("a".repeat(64)),
            2 => {
                let duplicate = changed[9][0].clone();
                changed[9].push(duplicate);
            }
            3 => {
                changed[7][0].pop();
            }
            _ => {
                let mut orphan = changed[10][0].clone();
                orphan[0] = Value::String(uuid::Uuid::new_v4().to_string());
                changed[10].push(orphan);
            }
        }
        let body = serde_json::to_vec(&changed).unwrap();
        let mut head = h.clone();
        head.payload_sha256 = hash(&body);
        head.payload_bytes = body.len();
        let mut bytes = serde_json::to_vec(&head).unwrap();
        bytes.push(b'\n');
        bytes.extend(body);
        fs::write(&path, bytes).unwrap();
        assert!(view(&root, &h.id).is_err(), "case {case}");
    }
    fs::write(&path, &original).unwrap();
    assert!(detail(&root, &h.id, Kind::Maintenance, &id).is_ok());
    let alias = f.root.path().join("shared-evidence");
    fs::hard_link(&path, &alias).unwrap();
    assert!(view(&root, &h.id).is_err());
    fs::remove_file(alias).unwrap();
    assert!(view(&root, &h.id).is_ok());
}

#[test]
fn independent_process_kill_after_publication_keeps_both_evidence_and_live_rows() {
    const KEY: &str = "MOOMOLE_ARCHIVE_CRASH_OWNED_ROOT";
    if let Some(path) = std::env::var_os(KEY) {
        let root = PathBuf::from(path);
        assert_eq!(root.canonicalize().unwrap(), root);
        assert!(root
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("archive-owned-"));
        assert_eq!(
            fs::read(root.join("owned-crash-marker")).unwrap(),
            b"MooMole private archive crash fixture"
        );
        let mut db = db::open_fixture(&root.join("live.sqlite")).unwrap();
        let p = prepare(&mut db).unwrap();
        let result = archive_with(
            &mut db,
            &root.join("archives"),
            &p.fingerprint,
            db::now(),
            |_, _| {
                fs::write(root.join("published-ready"), b"verified").unwrap();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err("bounded child watchdog".into())
            },
        );
        assert!(result.is_err());
        return;
    }
    let mut f = Fixture::new();
    let id = f.maintenance(false);
    let before = collect(&f.db).unwrap().body;
    let root = f.root.path().to_path_buf();
    fs::write(
        root.join("owned-crash-marker"),
        b"MooMole private archive crash fixture",
    )
    .unwrap();
    drop(f.db);
    let mut child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","execution_archive::tests::independent_process_kill_after_publication_keeps_both_evidence_and_live_rows","--nocapture"]).env(KEY,&root).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !root.join("published-ready").exists() {
        if child.try_wait().unwrap().is_some() {
            panic!("owned crash child exited before publication");
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("owned crash child did not publish in time");
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    let mut reopened = db::open_fixture(&root.join("live.sqlite")).unwrap();
    assert_eq!(collect(&reopened).unwrap().body, before);
    let archives = root.join("archives");
    let saved = list(&archives).unwrap().archives;
    assert_eq!(saved.len(), 1);
    assert!(detail(&archives, &saved[0].id, Kind::Maintenance, &id).is_ok());
    let p = prepare(&mut reopened).unwrap();
    archive(&mut reopened, &archives, &p.fingerprint, db::now()).unwrap();
    assert!(maintenance::list(&reopened).unwrap().is_empty());
    assert_eq!(list(&archives).unwrap().archives.len(), 2);
}

#[test]
fn changed_published_copy_and_unexpected_references_cannot_release_live_rows() {
    let mut f = Fixture::new();
    f.maintenance(false);
    let before = collect(&f.db).unwrap().body;
    let p = prepare(&mut f.db).unwrap();
    let root = f.archives();
    let rescue = f.root.path().join("moved-copy.moomole-history");
    assert!(
        archive_with(&mut f.db, &root, &p.fingerprint, f.at + 10, |_, h| {
            fs::rename(filename(&root, &h.id).unwrap(), &rescue).unwrap();
            Ok(())
        })
        .is_err()
    );
    assert_eq!(collect(&f.db).unwrap().body, before);
    assert!(load(&rescue, false).is_ok());
    f.db.execute_batch("CREATE TABLE unarchived_reference(id TEXT REFERENCES maintenance_runs(id) ON DELETE CASCADE)").unwrap();
    let run = maintenance::list(&f.db).unwrap()[0].id.clone();
    f.db.execute("INSERT INTO unarchived_reference VALUES (?1)", [run])
        .unwrap();
    assert!(archive(&mut f.db, &root, &p.fingerprint, f.at + 11)
        .unwrap_err()
        .contains("其他表引用"));
    assert_eq!(collect(&f.db).unwrap().body, before);
    assert_eq!(
        f.db.query_row("SELECT count(*) FROM unarchived_reference", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn corrupt_header_does_not_hide_other_archives_or_discard_files() {
    let mut f = Fixture::new();
    f.maintenance(false);
    let p = prepare(&mut f.db).unwrap();
    let root = f.archives();
    let h = archive(&mut f.db, &root, &p.fingerprint, f.at + 10).unwrap();
    let corrupt = filename(&root, &uuid::Uuid::new_v4().to_string()).unwrap();
    fs::write(&corrupt, "incomplete owned header").unwrap();
    fs::set_permissions(&corrupt, fs::Permissions::from_mode(0o600)).unwrap();
    let listing = list(&root).unwrap();
    assert_eq!(listing.archives, vec![h]);
    assert_eq!(listing.unreadable, 1);
    assert!(corrupt.exists());
}
