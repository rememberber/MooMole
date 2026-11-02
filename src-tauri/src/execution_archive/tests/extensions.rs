use super::*;
use serde_json::json;
fn managed(f: &Fixture) -> Value {
    let id = uuid::Uuid::new_v4().to_string();
    json!({"id":id,"requestId":uuid::Uuid::new_v4().to_string(),"draft":{
        "id":id,"taskId":uuid::Uuid::new_v4().to_string(),"index":0,
        "path":"/absent/Owned.app","bundleId":"org.moomole.owned","token":"owned",
        "name":"Owned","installedVersion":"1.0","managerPath":"/opt/homebrew/bin/brew",
        "metadataSha256":"a".repeat(64),"createdAt":f.at,"expiresAt":f.at+120000},
        "state":"removed","phase":"finished","createdAt":f.at,"finishedAt":f.at+1,
        "reason":"owned evidence, not an actual command","reviewedAt":null})
}
fn extraction(f: &Fixture) -> Value {
    let id = uuid::Uuid::new_v4().to_string();
    json!({"id":id,"requestId":uuid::Uuid::new_v4().to_string(),"draft":{
        "format":"tarGzip","id":id,"root":"/absent/source.tar.gz","outputPath":"/absent/output",
        "files":1,"directories":1,"hidden":0,"bytes":12,"metadataBytes":0,
        "sourceSha256":"b".repeat(64),"samples":[{"name":"owned.txt","directory":false}],
        "createdAt":f.at,"expiresAt":f.at+120000},
        "temporaryPath":format!("/absent/.MooMole-zip-extract-{id}.partial"),
        "state":"completed","phase":"finished","createdAt":f.at,"finishedAt":f.at+1,
        "artifact":{"device":"1","inode":"2","bytes":12,"sha256":"c".repeat(64)},
        "reason":"owned extraction evidence","reviewedAt":null})
}
fn save(f: &Fixture, table: &str, value: &Value) {
    super::super::extensions::init(&f.db).unwrap();
    f.db.execute(
        &format!("INSERT INTO {table} VALUES(?1,?2,?3)"),
        rusqlite::params![
            value["id"].as_str().unwrap(),
            value["requestId"].as_str().unwrap(),
            value.to_string()
        ],
    )
    .unwrap();
}
fn mole(f: &mut Fixture, event_count: usize) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let plan = serde_json::from_value(json!({
        "version":1,"id":uuid::Uuid::new_v4().to_string(),"revision":"0".repeat(40),"engineVersion":"1.64.0",
        "home":"/absent/historical-account","system":false,"method":"molePermanent","protectedPaths":["/absent/protected"],
        "createdAt":f.at,"expiresAt":f.at+120000,"scope":["system","userEssentials","appCaches","browsers","cloudOffice","developerTools","appsUtilities","virtualization","applicationSupport","appLeftovers","appleSilicon","firmware","timeMachine","largeFiles","projectArtifacts"]})).unwrap();
    crate::mole_clean::journal::create(&mut f.db, &id, &plan, Some(1024)).unwrap();
    // Fixed INSERT loop avoids an O(n^2) quota check in this data-volume test.
    // Read/finish/archive use production APIs; no file deletion or command runs.
    let tx = f.db.transaction().unwrap();
    for seq in 1..=event_count {
        tx.execute(
            "INSERT INTO mole_clean_events VALUES(?1,?2,'upstream','','owned event','')",
            rusqlite::params![id, seq as i64],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    crate::mole_clean::journal::diagnostics(&f.db, &id, "owned diagnostics\n").unwrap();
    crate::mole_clean::journal::finish(
        &f.db,
        &id,
        "failed",
        Some(2048),
        Some("partial result; keep evidence"),
    )
    .unwrap();
    id
}
#[test]
fn v4_mixed_archive_roundtrips_all_six_kinds_and_complete_mole_events() {
    let mut f = Fixture::new();
    f.cleanup();
    f.uninstall(false);
    f.maintenance(false);
    let m = mole(&mut f, 2101);
    let z = extraction(&f);
    let b = managed(&f);
    save(&f, "zip_extractions", &z);
    save(&f, "managed_app_runs", &b);
    // Unrelated records must not be removed along with the shared-budget journals.
    f.db.execute_batch("CREATE TABLE unrelated_recovery_proof(id TEXT PRIMARY KEY, body TEXT); INSERT INTO unrelated_recovery_proof VALUES('owned','preserve');").unwrap();
    let before = collect(&f.db).unwrap();
    assert_eq!(before.version, 4);
    assert_eq!(before.counts.total(), 6);
    assert!(
        crate::mole_clean::journal::read(&f.db, &m)
            .unwrap()
            .unwrap()
            .events_truncated
    );
    let original = super::super::extensions::mole(&f.db, &m).unwrap();
    let p = prepare(&mut f.db).unwrap();
    let root = f.archives();
    let h = archive(&mut f.db, &root, &p.fingerprint, f.at + 5).unwrap();
    assert_eq!(h.version, 4);
    assert_eq!(h.counts.mole_clean, 1);
    assert_eq!(h.counts.extraction, 1);
    assert_eq!(h.counts.managed_app, 1);
    for (table, _) in super::super::extensions::TABLES {
        assert_eq!(
            f.db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(
        f.db.query_row("SELECT body FROM unrelated_recovery_proof", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "preserve"
    );
    assert_eq!(detail(&root, &h.id, Kind::MoleClean, &m).unwrap(), original);
    assert_eq!(
        detail(&root, &h.id, Kind::Extraction, z["id"].as_str().unwrap()).unwrap(),
        z
    );
    assert_eq!(
        detail(&root, &h.id, Kind::ManagedApp, b["id"].as_str().unwrap()).unwrap(),
        b
    );
    let saved = load(&filename(&root, &h.id).unwrap(), true).unwrap();
    assert_eq!(collect(&saved.database).unwrap().body, before.body);
    let exported = f.root.path().join("exported.moomole-history");
    export(&root, &h.id, &exported).unwrap();
    let other = f.root.path().join("imported");
    assert_eq!(import(&other, &exported).unwrap(), h);
    assert_eq!(
        detail(&other, &h.id, Kind::MoleClean, &m).unwrap()["events"]
            .as_array()
            .unwrap()
            .len(),
        2101
    );
    let evidence = json!({"header":h,"view":view(&other,&h.id).unwrap(),"details":[
        {"kind":"moleClean","record":original},{"kind":"extraction","record":z},{"kind":"managedApp","record":b}]});
    if std::env::var_os("MOOMOLE_ARCHIVE_V4_EVIDENCE").is_some() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.cache");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("execution-archive-v4-native.json"),
            serde_json::to_vec(&evidence).unwrap(),
        )
        .unwrap();
    }
    drop(f.db);
    let reopened = db::open_fixture(&f.root.path().join("live.sqlite")).unwrap();
    assert!(crate::managed_apps::journal::list(&reopened)
        .unwrap()
        .is_empty());
    assert_eq!(view(&other, &h.id).unwrap().entries.len(), 6);
}
#[test]
fn v4_archive_releases_full_capacity_without_live_target_access() {
    let mut f = Fixture::new();
    for _ in 0..64 {
        save(&f, "managed_app_runs", &managed(&f));
    }
    let tx = f.db.transaction().unwrap();
    assert!(crate::execution_admission::reserve(&tx, 1).is_err());
    tx.rollback().unwrap();
    let p = prepare(&mut f.db).unwrap();
    assert_eq!(p.reserved_bytes, 64 * 32768);
    let root = f.archives();
    let h = archive(&mut f.db, &root, &p.fingerprint, f.at + 2).unwrap();
    assert_eq!(h.counts.managed_app, 64);
    let tx = f.db.transaction().unwrap();
    crate::execution_admission::reserve(&tx, 1).unwrap();
    tx.rollback().unwrap();
    assert_eq!(view(&root, &h.id).unwrap().entries.len(), 64);
}
#[test]
fn v4_pending_events_and_unknowns_require_review_and_preserve_uncertainty() {
    let mut f = Fixture::new();
    let id = mole(&mut f, 0);
    let token = uuid::Uuid::new_v4().to_string();
    f.db.execute(
        "INSERT INTO mole_clean_events VALUES(?1,1,'intent','/absent/owned','pending',?2)",
        rusqlite::params![id, token],
    )
    .unwrap();
    assert!(prepare(&mut f.db).is_err());
    f.db.execute(
        "UPDATE mole_clean_runs SET reviewed_at=created_at WHERE id=?1",
        [&id],
    )
    .unwrap();
    let p = prepare(&mut f.db).unwrap();
    let root = f.archives();
    let h = archive(&mut f.db, &root, &p.fingerprint, f.at + 2).unwrap();
    let v = view(&root, &h.id).unwrap();
    assert_eq!(v.entries[0].summary["resultUncertain"], true);
    assert_eq!(v.entries[0].summary["state"], "failed");
    for kind in [Kind::Extraction, Kind::ManagedApp] {
        let mut f = Fixture::new();
        let mut record = if kind == Kind::Extraction {
            extraction(&f)
        } else {
            managed(&f)
        };
        let table = if kind == Kind::Extraction {
            "zip_extractions"
        } else {
            "managed_app_runs"
        };
        record["state"] = json!("unknown");
        record["phase"] = json!(if kind == Kind::Extraction {
            "publishing"
        } else {
            "intent"
        });
        save(&f, table, &record);
        assert!(prepare(&mut f.db).is_err());
        record["reviewedAt"] = json!(f.at + 2);
        f.db.execute(&format!("UPDATE {table} SET body=?1"), [record.to_string()])
            .unwrap();
        assert!(prepare(&mut f.db).is_ok());
        record["state"] = json!("running");
        record["finishedAt"] = Value::Null;
        record["reviewedAt"] = Value::Null;
        f.db.execute(&format!("UPDATE {table} SET body=?1"), [record.to_string()])
            .unwrap();
        assert!(prepare(&mut f.db).is_err());
    }
}
#[test]
fn v4_rejects_changed_confirmations_or_invalid_events_and_external_cascades() {
    let mut f = Fixture::new();
    let m = mole(&mut f, 1);
    let p = prepare(&mut f.db).unwrap();
    f.db.execute("UPDATE mole_clean_events SET detail='changed'", [])
        .unwrap();
    let root = f.archives();
    assert!(archive(&mut f.db, &root, &p.fingerprint, f.at + 3).is_err());
    assert!(!root.exists());
    for sql in [
        "UPDATE mole_clean_events SET seq=2",
        "UPDATE mole_clean_events SET kind='removed',token=''",
        "UPDATE mole_clean_events SET path='/absent/../escape'",
    ] {
        let tx = f.db.transaction().unwrap();
        tx.execute_batch(sql).unwrap();
        assert!(collect(&tx).is_err());
        tx.rollback().unwrap();
    }
    f.db.execute_batch(
        "CREATE TABLE foreign_recovery(id TEXT REFERENCES mole_clean_runs(id) ON DELETE CASCADE);",
    )
    .unwrap();
    f.db.execute("INSERT INTO foreign_recovery VALUES(?1)", [&m])
        .unwrap();
    let p = prepare(&mut f.db).unwrap();
    assert!(archive(&mut f.db, &root, &p.fingerprint, f.at + 4).is_err());
    assert_eq!(
        f.db.query_row("SELECT count(*) FROM foreign_recovery", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn v4_published_archive_survives_failed_retirement_and_rejects_coherent_corruption() {
    let mut f = Fixture::new();
    let m = mole(&mut f, 2);
    save(&f, "managed_app_runs", &managed(&f));
    let before = collect(&f.db).unwrap();
    let p = prepare(&mut f.db).unwrap();
    let root = f.archives();
    let mut saved = None;
    assert!(
        archive_with(&mut f.db, &root, &p.fingerprint, f.at + 3, |_, h| {
            saved = Some(h.clone());
            Err("owned injected interruption".into())
        })
        .is_err()
    );
    assert_eq!(collect(&f.db).unwrap().body, before.body);
    let h = saved.unwrap();
    let path = filename(&root, &h.id).unwrap();
    assert_eq!(
        detail(&root, &h.id, Kind::MoleClean, &m).unwrap()["events"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let tables: Tables = serde_json::from_slice(&before.body).unwrap();
    for case in 0..6 {
        let mut changed = tables.clone();
        let mut head = h.clone();
        match case {
            0 => head.version = 3,
            1 => {
                changed.pop();
            }
            2 => {
                changed[13][0][1] = json!(3);
            }
            3 => {
                changed[12][0].pop();
            }
            4 => changed[13][0][0] = json!(uuid::Uuid::new_v4().to_string()),
            _ => {
                head.counts.managed_app = 0;
            }
        }
        let body = serde_json::to_vec(&changed).unwrap();
        head.payload_bytes = body.len();
        head.payload_sha256 = hash(&body);
        let mut bytes = serde_json::to_vec(&head).unwrap();
        bytes.push(b'\n');
        bytes.extend(body);
        fs::write(&path, bytes).unwrap();
        assert!(view(&root, &h.id).is_err(), "case {case}");
    }
    assert_eq!(collect(&f.db).unwrap().body, before.body);
}
#[test]
fn v4_mole_events_are_complete_beyond_the_legacy_row_budget() {
    let mut f = Fixture::new();
    for _ in 0..2 {
        mole(&mut f, 20000);
    }
    let before = collect(&f.db).unwrap();
    let rows: Tables = serde_json::from_slice(&before.body).unwrap();
    assert!(rows.iter().map(Vec::len).sum::<usize>() > MAX_ROWS);
    let p = prepare(&mut f.db).unwrap();
    let root = f.archives();
    let h = archive(&mut f.db, &root, &p.fingerprint, f.at + 4).unwrap();
    let loaded = load(&filename(&root, &h.id).unwrap(), true).unwrap();
    assert_eq!(collect(&loaded.database).unwrap().body, before.body);
}
