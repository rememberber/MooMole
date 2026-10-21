//! Test-only durable fixture recovery. Not compiled into App; no Apple targets.
//! The persistent registry replaces the old in-memory Ticket, within a private
//! test directory. Digests detect corruption, not a malicious same-user editor.
use super::*;
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    os::unix::fs::PermissionsExt,
    process::{Child, Command, Stdio},
    time::Duration,
};

const DATABASE: &str = "preference-recovery.sqlite";
const CHILD_TEST: &str = "maintenance::preference_sandbox::recovery::fixture_subprocess";
const DIRECTORY_ENV: &str = "MOOMOLE_PREFERENCE_RECOVERY_DIRECTORY";
const MODE_ENV: &str = "MOOMOLE_PREFERENCE_RECOVERY_MODE";
const SCHEMA: &str = "CREATE TABLE registry (
    slot INTEGER PRIMARY KEY CHECK(slot=1), domain TEXT NOT NULL, owner TEXT NOT NULL,
    uid INTEGER NOT NULL, protocol INTEGER NOT NULL CHECK(protocol=1));
CREATE TABLE recovery (
    slot INTEGER PRIMARY KEY CHECK(slot=1), id TEXT NOT NULL UNIQUE,
    body TEXT NOT NULL CHECK(length(body)<=32768), digest TEXT NOT NULL CHECK(length(digest)=64),
    phase TEXT NOT NULL CHECK(phase IN ('captured','applyIntent','applied','restoreIntent','restored','uncertainApply','uncertainRestore','conflict')));
PRAGMA user_version=1;";

#[derive(Clone)]
struct Identity {
    domain: String,
    owner: String,
}
impl Identity {
    fn from_lab(lab: &Lab) -> Self {
        Self {
            domain: lab.domain.clone(),
            owner: lab.owner.clone(),
        }
    }
    fn validate(&self) -> Result<()> {
        let v4 = |text: &str| {
            uuid::Uuid::parse_str(text)
                .is_ok_and(|id| id.get_version_num() == 4 && id.to_string() == text)
        };
        // Canonical, newly minted fixture identifiers only. No fallback or CLI domain.
        if !self.domain.strip_prefix(PREFIX).is_some_and(v4) || !v4(&self.owner) {
            return Err("persistent fixture identity invalid".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Captured,
    ApplyIntent,
    Applied,
    RestoreIntent,
    Restored,
    UncertainApply,
    UncertainRestore,
    Conflict,
}
impl Phase {
    fn text(self) -> &'static str {
        match self {
            Self::Captured => "captured",
            Self::ApplyIntent => "applyIntent",
            Self::Applied => "applied",
            Self::RestoreIntent => "restoreIntent",
            Self::Restored => "restored",
            Self::UncertainApply => "uncertainApply",
            Self::UncertainRestore => "uncertainRestore",
            Self::Conflict => "conflict",
        }
    }
    fn parse(value: &str) -> Result<Self> {
        [
            Self::Captured,
            Self::ApplyIntent,
            Self::Applied,
            Self::RestoreIntent,
            Self::Restored,
            Self::UncertainApply,
            Self::UncertainRestore,
            Self::Conflict,
        ]
        .into_iter()
        .find(|p| p.text() == value)
        .ok_or("unknown recovery phase".into())
    }
    fn permits(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Captured, Self::ApplyIntent)
                | (Self::ApplyIntent, Self::Applied)
                | (Self::Applied | Self::UncertainApply, Self::RestoreIntent)
                | (Self::RestoreIntent, Self::Restored)
                | (Self::ApplyIntent | Self::RestoreIntent, Self::Conflict)
        )
    }
}
struct Record {
    id: String,
    snapshot: Snapshot,
    phase: Phase,
}
// Field order releases SQLite before its directory lock.
struct Store {
    db: Connection,
    identity: Identity,
    directory: PathBuf,
    _owner: crate::instance::Lease,
}
fn private_directory(directory: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(directory).map_err(|e| e.to_string())?;
    if !directory.is_absolute()
        || !meta.is_dir()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o777 != 0o700
    {
        return Err("recovery directory is not private/owned".into());
    }
    Ok(())
}
fn private_file(path: &Path) -> Result<File> {
    let held = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| e.to_string())?;
    let meta = held.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o777 != 0o600
        || meta.len() > 1024 * 1024
    {
        return Err("recovery database is not private/regular/bounded".into());
    }
    Ok(held)
}
impl Store {
    fn connect(directory: &Path) -> Result<(Connection, crate::instance::Lease)> {
        private_directory(directory)?;
        let owner = crate::instance::acquire(directory).map_err(|e| e.to_string())?;
        let path = directory.join(DATABASE);
        let held = private_file(&path)?;
        let db = Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| e.to_string())?;
        let named = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        let meta = held.metadata().map_err(|e| e.to_string())?;
        if meta.dev() != named.dev()
            || meta.ino() != named.ino()
            || meta.nlink() != 1
            || !named.is_file()
        {
            return Err("recovery database identity changed".into());
        }
        db.busy_timeout(Duration::from_millis(100))
            .map_err(|e| e.to_string())?;
        // One fixture DB, no attachments or App schema migration. DELETE mode
        // avoids pretending this proves integration with the App's WAL database.
        db.pragma_update(None, "journal_mode", "DELETE")
            .map_err(|e| e.to_string())?;
        db.pragma_update(None, "synchronous", "FULL")
            .map_err(|e| e.to_string())?;
        db.pragma_update(None, "fullfsync", "ON")
            .map_err(|e| e.to_string())?;
        db.pragma_update(None, "trusted_schema", "OFF")
            .map_err(|e| e.to_string())?;
        Ok((db, owner))
    }
    fn create(directory: &Path, identity: Identity) -> Result<Self> {
        private_directory(directory)?;
        identity.validate()?;
        // Creation, including the named DB, happens under the same directory lease.
        let owner = crate::instance::acquire(directory).map_err(|e| e.to_string())?;
        let path = directory.join(DATABASE);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        File::open(directory)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        let mut db = Connection::open(&path).map_err(|e| e.to_string())?;
        db.pragma_update(None, "journal_mode", "DELETE")
            .map_err(|e| e.to_string())?;
        db.pragma_update(None, "synchronous", "FULL")
            .map_err(|e| e.to_string())?;
        db.pragma_update(None, "fullfsync", "ON")
            .map_err(|e| e.to_string())?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        tx.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO registry VALUES (1,?1,?2,?3,1)",
            params![identity.domain, identity.owner, unsafe { libc::geteuid() }],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Self {
            db,
            identity,
            directory: directory.into(),
            _owner: owner,
        })
    }
    fn open(directory: &Path) -> Result<Self> {
        let (db, owner) = Self::connect(directory)?;
        let version: u32 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version != 1 {
            return Err("unsupported recovery schema".into());
        }
        // Bound registry text before materializing it. Local files remain untrusted.
        let row:(String,String,i64,i64)=db.query_row("SELECT substr(domain,1,256),substr(owner,1,64),uid,protocol FROM registry WHERE slot=1 AND length(domain)<=256 AND length(owner)<=64",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())?;
        if row.2 != unsafe { libc::geteuid() } as i64 || row.3 != 1 {
            return Err("recovery user/protocol mismatch".into());
        }
        let identity = Identity {
            domain: row.0,
            owner: row.1,
        };
        identity.validate()?;
        let mut store = Self {
            db,
            identity,
            directory: directory.into(),
            _owner: owner,
        };
        // Validate the body/digest/fixture binding before touching recovery state.
        if store.load_optional()?.is_some() {
            let tx = store.db.transaction().map_err(|e| e.to_string())?;
            tx.execute("UPDATE recovery SET phase=CASE phase WHEN 'applyIntent' THEN 'uncertainApply' WHEN 'restoreIntent' THEN 'uncertainRestore' ELSE phase END WHERE slot=1",[]).map_err(|e|e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        Ok(store)
    }
    fn insert(&mut self, snapshot: &Snapshot) -> Result<String> {
        snapshot.validate(&self.identity.domain, &self.identity.owner)?;
        // Captured absence is legal ONLY because this fixture owns a marker and
        // verified a key list; unconfirmed real-domain NULL is not representable.
        if snapshot
            .original
            .as_ref()
            .is_some_and(|old| old == &snapshot.expected || old.could_coalesce(&snapshot.expected))
        {
            return Err("equivalent original/expected representation refused".into());
        }
        let body = serde_json::to_string(snapshot).map_err(|e| e.to_string())?;
        if body.len() as u64 > LIMIT {
            return Err("recovery snapshot too large".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO recovery VALUES (1,?1,?2,?3,'captured')",
            params![id, body, digest],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }
    fn load_optional(&self) -> Result<Option<Record>> {
        let size: Option<i64> = self
            .db
            .query_row(
                "SELECT length(CAST(body AS BLOB)) FROM recovery WHERE slot=1",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(size) = size else {
            return Ok(None);
        };
        if !(1..=LIMIT as i64).contains(&size) {
            return Err("recovery body too large/empty".into());
        }
        let (id,body,digest,phase):(String,String,String,String)=self.db.query_row("SELECT substr(id,1,64),body,substr(digest,1,65),substr(phase,1,64) FROM recovery WHERE slot=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())?;
        if uuid::Uuid::parse_str(&id).is_err()
            || digest != format!("{:x}", Sha256::digest(body.as_bytes()))
        {
            return Err("recovery identity/digest mismatch".into());
        }
        let snapshot: Snapshot =
            serde_json::from_str(&body).map_err(|_| "recovery schema mismatch")?;
        snapshot.validate(&self.identity.domain, &self.identity.owner)?;
        if snapshot
            .original
            .as_ref()
            .is_some_and(|old| old == &snapshot.expected || old.could_coalesce(&snapshot.expected))
        {
            return Err("invalid no-change recovery intent".into());
        }
        Ok(Some(Record {
            id,
            snapshot,
            phase: Phase::parse(&phase)?,
        }))
    }
    fn load(&self) -> Result<Record> {
        self.load_optional()?
            .ok_or("missing recovery record".into())
    }
    fn transition(&mut self, id: &str, from: Phase, to: Phase) -> Result<()> {
        if !from.permits(to) {
            return Err("illegal recovery transition".into());
        }
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        if tx
            .execute(
                "UPDATE recovery SET phase=?1 WHERE slot=1 AND id=?2 AND phase=?3",
                params![to.text(), id, from.text()],
            )
            .map_err(|e| e.to_string())?
            != 1
        {
            return Err("recovery state changed; no operation".into());
        }
        tx.commit().map_err(|e| e.to_string())
    }
    fn lab(&self) -> Result<Lab> {
        self.identity.validate()?;
        let lab = Lab {
            domain: self.identity.domain.clone(),
            owner: self.identity.owner.clone(),
            journals: tempfile::tempdir_in(&self.directory).map_err(|e| e.to_string())?,
            cleanup_on_drop: false,
        };
        // Only a registered UUID fixture whose two live markers still match.
        // Resuming does not generate/mutate/repair those markers.
        for scope in [Scope::AnyHost, Scope::ThisHost] {
            lab.observe(scope)?;
        }
        Ok(lab)
    }
}

#[derive(Debug, PartialEq)]
enum View {
    NotStarted,
    ExpectedObserved,
    OriginalObserved,
    Conflict,
    Unknown,
    Finished,
    UnsupportedOriginal,
}
fn recoverable(snapshot: &Snapshot) -> bool {
    // Cross-process NUL truncation was observed despite same-process read-back
    // and JSON fidelity. Such records are archival only, never mutation input.
    [&snapshot.expected]
        .into_iter()
        .chain(snapshot.original.iter())
        .all(|atom| !matches!(atom, Atom::Text(units) if units.contains(&0)))
}
fn bound_fixture(store: &Store, lab: &Lab) -> Result<()> {
    if lab.domain != store.identity.domain || lab.owner != store.identity.owner {
        return Err("fixture does not match persistent registry; no operation".into());
    }
    store.identity.validate()
}
fn observe(store: &Store, lab: &Lab) -> Result<View> {
    bound_fixture(store, lab)?;
    let record = store.load()?;
    if !recoverable(&record.snapshot) {
        return Ok(View::UnsupportedOriginal);
    }
    if matches!(record.phase, Phase::Captured) {
        return Ok(View::NotStarted);
    }
    if matches!(record.phase, Phase::Restored) {
        return Ok(View::Finished);
    }
    let current = match lab.observe(record.snapshot.scope) {
        Ok(v) => v,
        Err(_) => return Ok(View::Unknown),
    };
    Ok(if current == Some(record.snapshot.expected) {
        View::ExpectedObserved
    } else if current == record.snapshot.original {
        View::OriginalObserved
    } else {
        View::Conflict
    })
}
fn apply(
    store: &mut Store,
    lab: &Lab,
    after_intent: impl FnOnce(),
    after_write: impl FnOnce(),
) -> Result<()> {
    bound_fixture(store, lab)?;
    let record = store.load()?;
    if !recoverable(&record.snapshot) {
        return Err("NUL original/expected is archival only; no write".into());
    }
    if record.phase != Phase::Captured
        || lab.observe(record.snapshot.scope)? != record.snapshot.original
    {
        return Err("apply precondition/conflict; preserve current value".into());
    }
    store.transition(&record.id, Phase::Captured, Phase::ApplyIntent)?;
    after_intent();
    if lab.observe(record.snapshot.scope)? != record.snapshot.original {
        store.transition(&record.id, Phase::ApplyIntent, Phase::Conflict)?;
        return Err("changed after intent; preserve current value".into());
    }
    // There remains a final-check/write race with non-cooperating writers.
    lab.write(
        record.snapshot.scope,
        Key::Probe,
        Some(&record.snapshot.expected),
    )?;
    after_write();
    if lab.observe(record.snapshot.scope)? != Some(record.snapshot.expected) {
        return Err("apply result unconfirmed".into());
    }
    store.transition(&record.id, Phase::ApplyIntent, Phase::Applied)
}
fn restore(
    store: &mut Store,
    lab: &Lab,
    after_intent: impl FnOnce(),
    after_write: impl FnOnce(),
) -> Result<()> {
    bound_fixture(store, lab)?;
    let record = store.load()?;
    if !recoverable(&record.snapshot) {
        return Err("NUL original/expected is archival only; no restore".into());
    }
    if !matches!(record.phase, Phase::Applied | Phase::UncertainApply)
        || lab.observe(record.snapshot.scope)?.as_ref() != Some(&record.snapshot.expected)
    {
        return Err("restore precondition/conflict; preserve current value".into());
    }
    store.transition(&record.id, record.phase, Phase::RestoreIntent)?;
    after_intent();
    if lab.observe(record.snapshot.scope)?.as_ref() != Some(&record.snapshot.expected) {
        store.transition(&record.id, Phase::RestoreIntent, Phase::Conflict)?;
        return Err("changed after restore intent; preserve current value".into());
    }
    lab.write(
        record.snapshot.scope,
        Key::Probe,
        record.snapshot.original.as_ref(),
    )?;
    after_write();
    if lab.observe(record.snapshot.scope)? != record.snapshot.original {
        return Err("restore result unconfirmed".into());
    }
    store.transition(&record.id, Phase::RestoreIntent, Phase::Restored)
}

fn directory() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    dir
}
fn identity() -> Identity {
    Identity {
        domain: format!("{PREFIX}{}", uuid::Uuid::new_v4()),
        owner: uuid::Uuid::new_v4().to_string(),
    }
}
fn snapshot(identity: &Identity, scope: Scope, original: Option<Atom>) -> Snapshot {
    Snapshot {
        protocol: 1,
        domain: identity.domain.clone(),
        owner: identity.owner.clone(),
        scope,
        key: Key::Probe.text().into(),
        expected: Atom::Boolean(!matches!(
            original,
            Some(Atom::Boolean(true) | Atom::Integer(1))
        )),
        original,
    }
}

#[test]
fn typed_records_survive_close_reopen_without_an_in_memory_ticket() {
    for scope in [Scope::AnyHost, Scope::ThisHost] {
        for original in [
            None,
            Some(Atom::Boolean(false)),
            Some(Atom::Boolean(true)),
            Some(Atom::Integer(0)),
            Some(Atom::Integer(1)),
            Some(Atom::Integer(i64::MIN)),
            Some(Atom::text("")),
            Some(Atom::text("1")),
            Some(Atom::text("中文🚀\0尾")),
        ] {
            let dir = directory();
            let identity = identity();
            let mut store = Store::create(dir.path(), identity.clone()).unwrap();
            let snapshot = snapshot(&identity, scope, original.clone());
            let id = store.insert(&snapshot).unwrap();
            assert!(store.insert(&snapshot).is_err());
            assert!(Store::open(dir.path()).is_err());
            drop(store);
            let store = Store::open(dir.path()).unwrap();
            let record = store.load().unwrap();
            assert_eq!(record.id, id);
            assert_eq!(record.snapshot.original, original);
            assert_eq!(record.phase, Phase::Captured);
            assert_eq!(
                std::fs::metadata(dir.path().join(DATABASE)).unwrap().mode() & 0o777,
                0o600
            );
            assert_eq!(
                serde_json::to_value(&record.snapshot).unwrap(),
                serde_json::to_value(snapshot).unwrap()
            );
        }
    }
}
#[test]
fn illegal_or_replayed_transitions_and_failed_transactions_preserve_the_record() {
    let dir = directory();
    let identity = identity();
    let mut store = Store::create(dir.path(), identity.clone()).unwrap();
    let id = store
        .insert(&snapshot(&identity, Scope::AnyHost, None))
        .unwrap();
    assert!(store
        .transition(&id, Phase::Captured, Phase::Restored)
        .is_err());
    store.db.execute_batch("CREATE TRIGGER fail_intent BEFORE UPDATE ON recovery BEGIN SELECT RAISE(ABORT,'injected storage failure'); END;").unwrap();
    assert!(store
        .transition(&id, Phase::Captured, Phase::ApplyIntent)
        .is_err());
    assert_eq!(store.load().unwrap().phase, Phase::Captured);
    store.db.execute_batch("DROP TRIGGER fail_intent").unwrap();
    store
        .transition(&id, Phase::Captured, Phase::ApplyIntent)
        .unwrap();
    assert!(store
        .transition(&id, Phase::Captured, Phase::ApplyIntent)
        .is_err());
    assert!(store
        .transition(
            &uuid::Uuid::new_v4().to_string(),
            Phase::ApplyIntent,
            Phase::Applied
        )
        .is_err());
    drop(store);
    let mut store = Store::open(dir.path()).unwrap();
    assert_eq!(store.load().unwrap().phase, Phase::UncertainApply);
    assert!(store
        .transition(&id, Phase::UncertainApply, Phase::ApplyIntent)
        .is_err());
}
#[test]
fn changed_records_unknown_schema_and_foreign_fixture_bindings_are_refused() {
    for mutation in [
        "digest", "body", "domain", "owner", "uid", "protocol", "scope", "key", "extra", "size",
    ] {
        let dir = directory();
        let identity = identity();
        let mut store = Store::create(dir.path(), identity.clone()).unwrap();
        let snap = snapshot(&identity, Scope::AnyHost, None);
        store.insert(&snap).unwrap();
        match mutation {
            "digest" => {
                store
                    .db
                    .execute("UPDATE recovery SET digest=?1", ["0".repeat(64)])
                    .unwrap();
            }
            "body" => {
                store
                    .db
                    .execute("UPDATE recovery SET body='{}'", [])
                    .unwrap();
            }
            "domain" => {
                store
                    .db
                    .execute("UPDATE registry SET domain='com.apple.desktopservices'", [])
                    .unwrap();
            }
            "owner" => {
                store
                    .db
                    .execute(
                        "UPDATE registry SET owner=?1",
                        [uuid::Uuid::new_v4().to_string()],
                    )
                    .unwrap();
            }
            "uid" => {
                store
                    .db
                    .execute("UPDATE registry SET uid=uid+1", [])
                    .unwrap();
            }
            "protocol" => {
                store.db.pragma_update(None, "user_version", 99).unwrap();
            }
            "size" => {
                store
                    .db
                    .execute_batch("PRAGMA ignore_check_constraints=ON;")
                    .unwrap();
                store
                    .db
                    .execute(
                        "UPDATE recovery SET body=?1",
                        ["x".repeat(LIMIT as usize + 1)],
                    )
                    .unwrap();
            }
            field => {
                let mut body = serde_json::to_value(snap).unwrap();
                body[field] = match field {
                    "scope" => serde_json::json!("OtherUser"),
                    "key" => serde_json::json!("DSDontWriteNetworkStores"),
                    _ => serde_json::json!(true),
                };
                let body = serde_json::to_string(&body).unwrap();
                let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
                store
                    .db
                    .execute(
                        "UPDATE recovery SET body=?1,digest=?2",
                        params![body, digest],
                    )
                    .unwrap();
            }
        }
        drop(store);
        assert!(Store::open(dir.path()).is_err(), "mutation={mutation}");
    }
}
#[test]
fn unsafe_database_permissions_and_aliases_are_refused_without_repair() {
    use std::os::unix::fs::symlink;
    for kind in ["symlink", "hardlink", "mode"] {
        let dir = directory();
        let identity = identity();
        drop(Store::create(dir.path(), identity).unwrap());
        let path = dir.path().join(DATABASE);
        match kind {
            "mode" => {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap()
            }
            _ => {
                let real = dir.path().join("other.sqlite");
                std::fs::rename(&path, &real).unwrap();
                if kind == "symlink" {
                    symlink(&real, &path).unwrap();
                } else {
                    std::fs::hard_link(&real, &path).unwrap();
                }
            }
        }
        assert!(Store::open(dir.path()).is_err());
    }
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn child(directory: &Path, mode: &str) -> ChildGuard {
    ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CHILD_TEST, "--ignored", "--nocapture"])
            .env(DIRECTORY_ENV, directory)
            .env(MODE_ENV, mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    )
}
fn await_checkpoint(child: &mut ChildGuard) {
    use std::io::BufRead;
    let stdout = child.0.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout)
            .lines()
            .map_while(std::result::Result::ok)
        {
            if line == "preference-recovery-checkpoint" {
                let _ = tx.send(());
                return;
            }
        }
    });
    rx.recv_timeout(Duration::from_secs(10))
        .expect("fixture child did not reach bounded checkpoint");
}
fn checkpoint() {
    println!("preference-recovery-checkpoint");
    std::io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}
fn wait_success(child: &mut ChildGuard) {
    let began = std::time::Instant::now();
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "fixture child failed");
            return;
        }
        assert!(
            began.elapsed() < Duration::from_secs(10),
            "fixture child timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
#[ignore = "Internal subprocess helper for UUID fixture recovery, not an App entry point"]
fn fixture_subprocess() {
    let directory = std::env::var_os(DIRECTORY_ENV).expect("fixture directory required");
    let mode = std::env::var(MODE_ENV).unwrap();
    assert!([
        "applyBeforeWrite",
        "applyAfterWrite",
        "restoreAfterWrite",
        "restore",
        "rejectNul"
    ]
    .contains(&mode.as_str()));
    let mut store = Store::open(Path::new(&directory)).unwrap();
    let lab = store.lab().unwrap();
    match mode.as_str() {
        "rejectNul" => {
            let before = lab.observe(Scope::AnyHost).unwrap();
            assert_eq!(observe(&store, &lab).unwrap(), View::UnsupportedOriginal);
            assert!(apply(&mut store, &lab, || {}, || {}).is_err());
            assert!(restore(&mut store, &lab, || {}, || {}).is_err());
            assert_eq!(store.load().unwrap().phase, Phase::Captured);
            assert_eq!(lab.observe(Scope::AnyHost).unwrap(), before);
            println!(
                "nul-original-matches-in-child: {}",
                before == store.load().unwrap().snapshot.original
            );
        }
        "applyBeforeWrite" => apply(&mut store, &lab, checkpoint, || {}).unwrap(),
        "applyAfterWrite" => apply(&mut store, &lab, || {}, checkpoint).unwrap(),
        "restoreAfterWrite" => restore(&mut store, &lab, || {}, checkpoint).unwrap(),
        _ => restore(&mut store, &lab, || {}, || {}).unwrap(),
    }
}
#[test]
#[ignore = "Writes/restores only newly minted UUID fixture domains; SIGKILL targets only child handles created by this test"]
fn live_persistent_apply_crash_restores_in_a_new_process_and_binds_exact_types() {
    use std::os::unix::process::ExitStatusExt;
    for (scope, original) in [
        (Scope::AnyHost, None),
        (Scope::ThisHost, Some(Atom::Integer(i64::MIN))),
        (Scope::AnyHost, Some(Atom::text("中文🚀尾"))),
    ] {
        let lab = Lab::new().unwrap();
        lab.write(scope, Key::Probe, original.as_ref()).unwrap();
        let dir = directory();
        let mut store = Store::create(dir.path(), Identity::from_lab(&lab)).unwrap();
        store
            .insert(&snapshot(
                &store.identity,
                scope,
                lab.observe(scope).unwrap(),
            ))
            .unwrap();
        drop(store);
        let mut writer = child(dir.path(), "applyAfterWrite");
        await_checkpoint(&mut writer);
        assert!(Store::open(dir.path()).is_err()); // Cannot reconcile a live owner.
        writer.0.kill().unwrap();
        assert_eq!(writer.0.wait().unwrap().signal(), Some(libc::SIGKILL));
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.load().unwrap().phase, Phase::UncertainApply);
        assert_eq!(observe(&store, &lab).unwrap(), View::ExpectedObserved);
        drop(store);
        let mut restorer = child(dir.path(), "restore");
        wait_success(&mut restorer);
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.load().unwrap().phase, Phase::Restored);
        assert_eq!(lab.observe(scope).unwrap(), original);
        assert_eq!(
            lab.raw(scope, Key::Other).unwrap(),
            Some(Atom::text("keep fixture unrelated data"))
        );
        assert_eq!(observe(&store, &lab).unwrap(), View::Finished);
        drop(store);
        lab.clear().unwrap();
    }
}
#[test]
#[ignore = "Writes/restores only newly minted UUID fixture domains; verifies unknown outcomes without automatic replay"]
fn live_persistent_crash_before_apply_and_during_restore_never_infers_success() {
    for mode in ["applyBeforeWrite", "restoreAfterWrite"] {
        let lab = Lab::new().unwrap();
        let dir = directory();
        let mut store = Store::create(dir.path(), Identity::from_lab(&lab)).unwrap();
        store
            .insert(&snapshot(
                &store.identity,
                Scope::AnyHost,
                lab.observe(Scope::AnyHost).unwrap(),
            ))
            .unwrap();
        if mode == "restoreAfterWrite" {
            apply(&mut store, &lab, || {}, || {}).unwrap();
        }
        drop(store);
        let mut writer = child(dir.path(), mode);
        await_checkpoint(&mut writer);
        writer.0.kill().unwrap();
        writer.0.wait().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        assert_eq!(
            store.load().unwrap().phase,
            if mode == "applyBeforeWrite" {
                Phase::UncertainApply
            } else {
                Phase::UncertainRestore
            }
        );
        assert_eq!(observe(&store, &lab).unwrap(), View::OriginalObserved);
        assert!(apply(&mut store, &lab, || {}, || {}).is_err());
        assert!(restore(&mut store, &lab, || {}, || {}).is_err());
        assert_eq!(lab.observe(Scope::AnyHost).unwrap(), None);
        drop(store);
        lab.clear().unwrap();
    }
}
#[test]
#[ignore = "Writes/restores only newly minted UUID fixture domains; preserves concurrent fixture values"]
fn live_persistent_conflicts_and_storage_failures_preserve_uncertainty() {
    let lab = Lab::new().unwrap();
    let dir = directory();
    let mut store = Store::create(dir.path(), Identity::from_lab(&lab)).unwrap();
    store
        .insert(&snapshot(&store.identity, Scope::AnyHost, None))
        .unwrap();
    store.db.execute_batch("CREATE TRIGGER fail_intent BEFORE UPDATE ON recovery BEGIN SELECT RAISE(ABORT,'injected storage failure'); END;").unwrap();
    assert!(apply(&mut store, &lab, || {}, || {}).is_err());
    assert_eq!(lab.observe(Scope::AnyHost).unwrap(), None);
    store.db.execute_batch("DROP TRIGGER fail_intent").unwrap();
    store.db.execute_batch("CREATE TRIGGER fail_completion BEFORE UPDATE ON recovery WHEN NEW.phase='applied' BEGIN SELECT RAISE(ABORT,'injected completion failure'); END;").unwrap();
    assert!(apply(&mut store, &lab, || {}, || {}).is_err());
    assert_eq!(store.load().unwrap().phase, Phase::ApplyIntent);
    assert_eq!(
        lab.observe(Scope::AnyHost).unwrap(),
        Some(Atom::Boolean(true))
    );
    drop(store);
    lab.write(
        Scope::AnyHost,
        Key::Probe,
        Some(&Atom::text("new value from fixture writer")),
    )
    .unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    assert_eq!(store.load().unwrap().phase, Phase::UncertainApply);
    assert_eq!(observe(&store, &lab).unwrap(), View::Conflict);
    assert!(restore(&mut store, &lab, || {}, || {}).is_err());
    assert_eq!(
        lab.observe(Scope::AnyHost).unwrap(),
        Some(Atom::text("new value from fixture writer"))
    );
    drop(store);
    lab.clear().unwrap();
    for operation in ["apply", "restore"] {
        let lab = Lab::new().unwrap();
        let dir = directory();
        let mut store = Store::create(dir.path(), Identity::from_lab(&lab)).unwrap();
        store
            .insert(&snapshot(
                &store.identity,
                Scope::AnyHost,
                lab.observe(Scope::AnyHost).unwrap(),
            ))
            .unwrap();
        let external_write = || {
            lab.write(Scope::AnyHost, Key::Probe, Some(&Atom::Integer(2)))
                .unwrap()
        };
        let result = if operation == "apply" {
            apply(&mut store, &lab, external_write, || {})
        } else {
            apply(&mut store, &lab, || {}, || {}).unwrap();
            restore(&mut store, &lab, external_write, || {})
        };
        assert!(result.is_err());
        assert_eq!(store.load().unwrap().phase, Phase::Conflict);
        assert_eq!(lab.observe(Scope::AnyHost).unwrap(), Some(Atom::Integer(2)));
        drop(store);
        lab.clear().unwrap();
    }
}

#[test]
fn nul_records_are_lossless_archives_but_never_recoverable_mutation_authority() {
    let identity = identity();
    let mut snapshot = snapshot(&identity, Scope::AnyHost, Some(Atom::text("中文🚀\0尾")));
    assert!(!recoverable(&snapshot));
    snapshot.original = None;
    snapshot.expected = Atom::text("expected\0tail");
    assert!(!recoverable(&snapshot));
    snapshot.expected = Atom::text("safe text");
    assert!(recoverable(&snapshot));
}
#[test]
#[ignore = "Writes only a newly minted UUID fixture domain; NUL records are archival only, never applied or restored"]
fn live_persistent_nul_records_never_authorize_a_write_after_process_handoff() {
    let lab = Lab::new().unwrap();
    let dir = directory();
    let original = Atom::text("中文🚀\0尾");
    lab.write(Scope::AnyHost, Key::Probe, Some(&original))
        .unwrap();
    let mut store = Store::create(dir.path(), Identity::from_lab(&lab)).unwrap();
    // Archive the exact test input to verify persistence fidelity independently
    // of what cfprefsd makes visible in another process. No mutation is allowed.
    store
        .insert(&snapshot(
            &store.identity,
            Scope::AnyHost,
            Some(original.clone()),
        ))
        .unwrap();
    drop(store);
    let mut observer = child(dir.path(), "rejectNul");
    wait_success(&mut observer);
    let mut output = String::new();
    observer
        .0
        .stdout
        .take()
        .unwrap()
        .take(LIMIT)
        .read_to_string(&mut output)
        .unwrap();
    let observed = output
        .lines()
        .find(|line| line.starts_with("nul-original-matches-in-child:"))
        .unwrap();
    println!("{observed}; archival-only policy retained");
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.load().unwrap().snapshot.original, Some(original));
    assert_eq!(store.load().unwrap().phase, Phase::Captured);
    assert_eq!(observe(&store, &lab).unwrap(), View::UnsupportedOriginal);
    drop(store);
    lab.clear().unwrap();
}

#[test]
fn a_different_fixture_cannot_be_used_with_another_records_originals() {
    let dir = directory();
    let identity = identity();
    let mut store = Store::create(dir.path(), identity.clone()).unwrap();
    store
        .insert(&snapshot(&identity, Scope::AnyHost, None))
        .unwrap();
    let foreign = Lab {
        domain: format!("{PREFIX}{}", uuid::Uuid::new_v4()),
        owner: uuid::Uuid::new_v4().to_string(),
        journals: tempfile::tempdir().unwrap(),
        cleanup_on_drop: false,
    };
    // All three reject before any CFPreferences read/write. This is a pure
    // binding test, not another live preference domain.
    assert!(observe(&store, &foreign).is_err());
    assert!(apply(
        &mut store,
        &foreign,
        || panic!("must not reach intent"),
        || {}
    )
    .is_err());
    assert!(restore(
        &mut store,
        &foreign,
        || panic!("must not reach intent"),
        || {}
    )
    .is_err());
    assert_eq!(store.load().unwrap().phase, Phase::Captured);
}
