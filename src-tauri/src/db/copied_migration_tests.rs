//! Explicit local acceptance on a closed, owned snapshot; never opens live data.
use super::*;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, os::unix::fs::MetadataExt};

const TABLES: [&str; 10] = [
    "settings",
    "execution_runs",
    "execution_items",
    "execution_events",
    "uninstall_runs",
    "uninstall_events",
    "maintenance_runs",
    "maintenance_steps",
    "maintenance_events",
    "maintenance_reviews",
];
fn hashes(db: &Connection) -> BTreeMap<&'static str, String> {
    TABLES
        .into_iter()
        .map(|table| {
            let mut query = db
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                .unwrap();
            let columns = query.column_count();
            let mut rows = query.query([]).unwrap();
            let mut digest = Sha256::new();
            while let Some(row) = rows.next().unwrap() {
                digest.update([0xff]);
                for index in 0..columns {
                    use rusqlite::types::ValueRef;
                    let (kind, value): (u8, Vec<u8>) = match row.get_ref(index).unwrap() {
                        ValueRef::Null => (0, vec![]),
                        ValueRef::Integer(n) => (1, n.to_le_bytes().to_vec()),
                        ValueRef::Real(n) => (2, n.to_bits().to_le_bytes().to_vec()),
                        ValueRef::Text(b) => (3, b.to_vec()),
                        ValueRef::Blob(b) => (4, b.to_vec()),
                    };
                    digest.update([kind]);
                    digest.update((value.len() as u64).to_le_bytes());
                    digest.update(value);
                }
            }
            (table, format!("{:x}", digest.finalize()))
        })
        .collect()
}

#[test]
#[ignore = "explicit closed owned v6 snapshot in local-artifacts: copies to a private temporary database, migrates only the copy, checks ten execution/settings tables and original-schema backup"]
fn copied_v6_snapshot_preserves_all_execution_facts_and_original_backup() {
    let source = std::path::PathBuf::from(
        std::env::var("MOOMOLE_OWNED_V6_SNAPSHOT")
            .expect("Provide an explicit closed owned snapshot"),
    );
    let metadata = std::fs::symlink_metadata(&source).unwrap();
    assert!(metadata.is_file() && !metadata.is_symlink() && metadata.nlink() == 1);
    assert!(metadata.len() > 0 && metadata.len() < 128 * 1024 * 1024);
    let source = source.canonicalize().unwrap();
    let allowed = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("local-artifacts")
        .canonicalize()
        .unwrap();
    assert!(
        source.starts_with(allowed),
        "Snapshot must be in ignored owned evidence, never live application data"
    );
    let mut wal = source.as_os_str().to_os_string();
    wal.push("-wal");
    let wal = std::path::PathBuf::from(wal);
    assert!(
        std::fs::symlink_metadata(wal)
            .map_or(true, |m| m.is_file() && !m.is_symlink() && m.len() == 0),
        "Use a closed complete snapshot, not a main file with committed WAL"
    );
    let bytes = std::fs::read(&source).unwrap();
    let after = source.metadata().unwrap();
    assert_eq!(
        (
            metadata.dev(),
            metadata.ino(),
            metadata.len(),
            metadata.mtime(),
            metadata.mtime_nsec()
        ),
        (
            after.dev(),
            after.ino(),
            after.len(),
            after.mtime(),
            after.mtime_nsec()
        )
    );
    let scratch = tempfile::tempdir().unwrap();
    let target = scratch.path().join("app.sqlite");
    std::fs::write(&target, &bytes).unwrap();
    let before = Connection::open(&target).unwrap();
    assert_eq!(
        before
            .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        6
    );
    let expected = hashes(&before);
    drop(before);
    let migrated = open(&target).unwrap();
    assert_eq!(
        migrated
            .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        7
    );
    assert_eq!(hashes(&migrated), expected);
    assert_eq!(migrated.query_row::<i64,_,_>("SELECT (SELECT count(*) FROM execution_reviews)+(SELECT count(*) FROM uninstall_reviews)",[],|r|r.get(0)).unwrap(),0);
    drop(migrated);
    let backups = std::fs::read_dir(scratch.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("moomole-before-v7-")
        })
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    let backup = Connection::open(&backups[0]).unwrap();
    assert_eq!(
        backup
            .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        6
    );
    assert_eq!(hashes(&backup), expected);
    assert_eq!(hashes(&open(&target).unwrap()), expected);
    assert_eq!(
        Sha256::digest(std::fs::read(&source).unwrap()),
        Sha256::digest(&bytes)
    );
    println!("Owned snapshot SHA-256 {:x}; v6 -> v7 and restart preserve ten table hashes; backup stays v6; source unchanged",Sha256::digest(bytes));
}
