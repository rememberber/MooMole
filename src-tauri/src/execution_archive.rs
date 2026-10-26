//! Data-only portable evidence. Publish a verified private copy before releasing
//! journal capacity. No archive can create a Session or resume an operation.
use crate::{
    execution, maintenance::refresh_journal as maintenance, uninstall_journal as uninstall,
};
use rusqlite::{
    params_from_iter,
    types::{Value as SqlValue, ValueRef},
    Connection, TransactionBehavior,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};
const MAX_BODY: usize = 128 * 1024 * 1024;
const MAX_ROWS: usize = 32768;
const MAX_HEADER: usize = 2048;
const TABLES: [&str; 11] = [
    "execution_runs",
    "execution_items",
    "execution_events",
    "execution_reviews",
    "uninstall_runs",
    "uninstall_events",
    "uninstall_reviews",
    "maintenance_runs",
    "maintenance_steps",
    "maintenance_events",
    "maintenance_reviews",
];
const WIDTHS: [usize; 11] = [9, 7, 7, 3, 12, 6, 3, 9, 8, 8, 3];
const ROOTS: [&str; 3] = ["execution_runs", "uninstall_runs", "maintenance_runs"];
type Tables = Vec<Vec<Vec<Value>>>;
fn err(e: impl std::fmt::Display) -> String {
    format!("执行归档无法完成，请保留证据并刷新核对：{e}")
}
fn hash(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
fn valid_id(id: &str) -> Result<(), String> {
    if uuid::Uuid::parse_str(id).is_ok_and(|v| v.to_string() == id) {
        Ok(())
    } else {
        Err("归档标识无效。".into())
    }
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Kind {
    Cleanup,
    Uninstall,
    Maintenance,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Counts {
    pub cleanup: usize,
    pub uninstall: usize,
    pub maintenance: usize,
}
impl Counts {
    fn total(&self) -> usize {
        self.cleanup
            .saturating_add(self.uninstall)
            .saturating_add(self.maintenance)
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Header {
    format: String,
    version: u32,
    pub id: String,
    pub created_at: i64,
    pub counts: Counts,
    pub payload_sha256: String,
    pub payload_bytes: usize,
}
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Preview {
    pub counts: Counts,
    pub fingerprint: String,
    pub reserved_bytes: i64,
}
#[derive(Serialize)]
pub(crate) struct Entry {
    pub kind: Kind,
    pub id: String,
    pub summary: Value,
}
#[derive(Serialize)]
pub(crate) struct View {
    pub header: Header,
    pub entries: Vec<Entry>,
}
#[derive(Serialize)]
pub(crate) struct Listing {
    pub archives: Vec<Header>,
    pub complete: bool,
    pub unreadable: usize,
}
struct Evidence {
    body: Vec<u8>,
    entries: Vec<Entry>,
    counts: Counts,
    reserved: i64,
}
struct Loaded {
    header: Header,
    database: Connection,
    entries: Vec<Entry>,
}
fn memory() -> Result<Connection, String> {
    let db = Connection::open_in_memory().map_err(err)?;
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")
        .map_err(err)?;
    for schema in [
        execution::SCHEMA,
        uninstall::SCHEMA,
        maintenance::SCHEMA,
        maintenance::REVIEW_SCHEMA,
        crate::execution_review::SCHEMA,
    ] {
        db.execute_batch(schema).map_err(err)?;
    }
    Ok(db)
}
fn ids(db: &Connection, table: &str) -> Result<Vec<String>, String> {
    let mut q = db
        .prepare(&format!("SELECT id FROM {table} ORDER BY id LIMIT 65"))
        .map_err(err)?;
    let rows = q
        .query_map([], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(rows)
}
fn entries(db: &Connection) -> Result<(Vec<Entry>, Counts, i64), String> {
    let mut out = Vec::new();
    let mut counts = Counts::default();
    let mut reserved = 0_i64;
    for (index, table) in ROOTS.iter().enumerate() {
        for id in ids(db, table)? {
            let (kind, summary) = match index {
                0 => {
                    let d = execution::detail_in_snapshot(db, &id)?;
                    if d.summary.state == "running"
                        || d.summary.finished_at.is_none()
                        || (d.summary.uncertain_count > 0 && d.summary.reviewed_at.is_none())
                    {
                        return Err("请先结束任务并核查清理的未知结果，再归档。".into());
                    }
                    counts.cleanup += 1;
                    (Kind::Cleanup, serde_json::to_value(d.summary).map_err(err)?)
                }
                1 => {
                    let d = uninstall::detail_in_snapshot(db, &id)?;
                    if d.summary.state == "running"
                        || d.summary.finished_at.is_none()
                        || (d.summary.item_state == "uncertain" && d.summary.reviewed_at.is_none())
                    {
                        return Err("请先结束任务并核查卸载的未知结果，再归档。".into());
                    }
                    counts.uninstall += 1;
                    (
                        Kind::Uninstall,
                        serde_json::to_value(d.summary).map_err(err)?,
                    )
                }
                _ => {
                    let d = maintenance::detail_in_snapshot(db, &id)?;
                    if d.summary.state == "running"
                        || d.summary.finished_at.is_none()
                        || (d.summary.uncertain_count > 0 && d.summary.reviewed_at.is_none())
                    {
                        return Err("请先结束任务并核查维护的未知结果，再归档。".into());
                    }
                    counts.maintenance += 1;
                    (
                        Kind::Maintenance,
                        serde_json::to_value(d.summary).map_err(err)?,
                    )
                }
            };
            let size: i64 = db
                .query_row(
                    &format!("SELECT reserved_bytes FROM {table} WHERE id=?1"),
                    [&id],
                    |r| r.get(0),
                )
                .map_err(err)?;
            if size <= 0 {
                return Err("执行容量字段无效，保留原记录。".into());
            }
            reserved = reserved.checked_add(size).ok_or("执行容量溢出。")?;
            out.push(Entry { kind, id, summary });
        }
    }
    if counts.total() == 0
        || counts.total() > 64
        || reserved > crate::execution_admission::MAX_RESERVED
    {
        return Err("暂无可归档执行记录，或记录超出可验证范围。".into());
    }
    Ok((out, counts, reserved))
}
fn collect(db: &Connection) -> Result<Evidence, String> {
    let (saved_entries, counts, reserved) = entries(db)?;
    let mut tables = Vec::new();
    let mut rows = 0;
    let mut bytes = 0;
    for (table, width) in TABLES.iter().zip(WIDTHS) {
        let mut q = db
            .prepare(&format!(
                "SELECT * FROM {table} ORDER BY rowid LIMIT {}",
                MAX_ROWS + 1
            ))
            .map_err(err)?;
        if q.column_count() != width {
            return Err("执行表结构不匹配，保留原记录。".into());
        }
        let mut cursor = q.query([]).map_err(err)?;
        let mut data = Vec::new();
        while let Some(row) = cursor.next().map_err(err)? {
            rows += 1;
            if rows > MAX_ROWS {
                return Err("归档条目超过预算。".into());
            }
            let mut cells = Vec::new();
            for index in 0..width {
                cells.push(match row.get_ref(index).map_err(err)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(v) => Value::from(v),
                    ValueRef::Text(v) => {
                        bytes += v.len();
                        if bytes > MAX_BODY {
                            return Err("归档超出字节预算。".into());
                        }
                        Value::String(std::str::from_utf8(v).map_err(err)?.into())
                    }
                    _ => return Err("归档含不支持的数据类型，保留原记录。".into()),
                });
            }
            data.push(cells);
        }
        tables.push(data);
    }
    let body = serde_json::to_vec(&tables).map_err(err)?;
    if body.len() > MAX_BODY {
        return Err("归档超出字节预算。".into());
    }
    let copy = rebuild(&tables)?;
    let (_, copy_counts, copy_reserved) = entries(&copy)?;
    if counts != copy_counts || reserved != copy_reserved {
        return Err("归档读回不一致。".into());
    }
    Ok(Evidence {
        body,
        entries: saved_entries,
        counts,
        reserved,
    })
}
fn rebuild(tables: &Tables) -> Result<Connection, String> {
    if tables.len() != TABLES.len() || tables.iter().map(Vec::len).sum::<usize>() > MAX_ROWS {
        return Err("归档表集合或数量无效。".into());
    }
    let mut db = memory()?;
    let t = db.transaction().map_err(err)?;
    for ((table, width), rows) in TABLES.iter().zip(WIDTHS).zip(tables) {
        let mut insert = t
            .prepare(&format!(
                "INSERT INTO {table} VALUES ({})",
                vec!["?"; width].join(",")
            ))
            .map_err(err)?;
        for row in rows {
            if row.len() != width {
                return Err("归档字段数量不符。".into());
            }
            let cells = row
                .iter()
                .map(|v| match v {
                    Value::Null => Ok(SqlValue::Null),
                    Value::String(s) => Ok(SqlValue::Text(s.clone())),
                    Value::Number(n) => n.as_i64().map(SqlValue::Integer).ok_or("归档整数无效。"),
                    _ => Err("归档类型无效。"),
                })
                .collect::<Result<Vec<_>, _>>()?;
            insert.execute(params_from_iter(cells)).map_err(err)?;
        }
    }
    t.commit().map_err(err)?;
    Ok(db)
}
pub(crate) fn prepare(db: &mut Connection) -> Result<Preview, String> {
    let t = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)?;
    let d = collect(&t)?;
    Ok(Preview {
        counts: d.counts,
        fingerprint: hash(&d.body),
        reserved_bytes: d.reserved,
    })
}
fn directory(root: &Path) -> Result<File, String> {
    if !root.is_absolute() {
        return Err("归档目录必须为固定绝对路径。".into());
    }
    for p in root.ancestors().skip(1) {
        if !fs::symlink_metadata(p).map_err(err)?.is_dir() {
            return Err("归档目录不能经过链接。".into());
        }
    }
    if !root.exists() {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(root)
            .map_err(err)?;
        File::open(root.parent().ok_or("归档父目录无效。")?)
            .and_then(|f| f.sync_all())
            .map_err(err)?;
    }
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)
        .map_err(err)?;
    let m = f.metadata().map_err(err)?;
    if m.uid() != unsafe { libc::getuid() } || m.mode() & 0o077 != 0 {
        return Err("归档目录不是当前用户的私有目录。".into());
    }
    Ok(f)
}
fn filename(root: &Path, id: &str) -> Result<PathBuf, String> {
    valid_id(id)?;
    Ok(root.join(format!("{id}.moomole-history")))
}
fn open_file(path: &Path, private: bool) -> Result<File, String> {
    if !path.is_absolute() {
        return Err("归档路径无效。".into());
    }
    for p in path.ancestors().skip(1) {
        if !fs::symlink_metadata(p).map_err(err)?.is_dir() {
            return Err("归档不能经过链接。".into());
        }
    }
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(err)?;
    let m = f.metadata().map_err(err)?;
    if !m.is_file()
        || m.nlink() != 1
        || (private && (m.uid() != unsafe { libc::getuid() } || m.mode() & 0o077 != 0))
        || m.len() > (MAX_BODY + MAX_HEADER) as u64
    {
        return Err("归档类型、大小或私有属性不符。".into());
    }
    Ok(f)
}
fn header(reader: &mut impl BufRead, length: u64) -> Result<Header, String> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_HEADER + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(err)?;
    if bytes.len() > MAX_HEADER || bytes.last() != Some(&b'\n') {
        return Err("归档文件头不完整或超限。".into());
    }
    let h: Header = serde_json::from_slice(&bytes).map_err(err)?;
    valid_id(&h.id)?;
    if h.format != "MooMole execution evidence"
        || h.version != 1
        || h.created_at < 0
        || h.counts.total() == 0
        || h.counts.total() > 64
        || h.payload_bytes == 0
        || h.payload_bytes > MAX_BODY
        || h.payload_bytes as u64 + bytes.len() as u64 != length
        || h.payload_sha256.len() != 64
        || !h
            .payload_sha256
            .bytes()
            .all(|v| v.is_ascii_hexdigit() && !v.is_ascii_uppercase())
    {
        return Err("归档格式、版本或范围无效。".into());
    }
    Ok(h)
}
fn load(path: &Path, private: bool) -> Result<Loaded, String> {
    let f = open_file(path, private)?;
    let before = f.metadata().map_err(err)?;
    let mut reader = BufReader::new(f);
    let h = header(&mut reader, before.len())?;
    let mut bytes = Vec::new();
    reader
        .take((MAX_BODY + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.len() != h.payload_bytes || hash(&bytes) != h.payload_sha256 {
        return Err("归档正文摘要不符。".into());
    }
    let after = fs::symlink_metadata(path).map_err(err)?;
    if (
        before.dev(),
        before.ino(),
        before.len(),
        before.mtime(),
        before.mtime_nsec(),
        before.ctime(),
        before.ctime_nsec(),
    ) != (
        after.dev(),
        after.ino(),
        after.len(),
        after.mtime(),
        after.mtime_nsec(),
        after.ctime(),
        after.ctime_nsec(),
    ) || !after.is_file()
        || after.nlink() != 1
    {
        return Err("读取期间归档已变化。".into());
    }
    let tables: Tables = serde_json::from_slice(&bytes).map_err(err)?;
    let database = rebuild(&tables)?;
    let (entries, counts, _) = entries(&database)?;
    if counts != h.counts {
        return Err("归档摘要与实际批次不符。".into());
    }
    Ok(Loaded {
        header: h,
        database,
        entries,
    })
}
fn sync(file: &File) -> Result<(), String> {
    file.sync_all().map_err(err)?;
    #[cfg(target_os = "macos")]
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } != 0 {
        return Err(err(std::io::Error::last_os_error()));
    }
    Ok(())
}
fn publish(root: &Path, h: &Header, body: &[u8]) -> Result<(), String> {
    let dir = directory(root)?;
    let path = filename(root, &h.id)?;
    let mut temporary = tempfile::NamedTempFile::new_in(root).map_err(err)?;
    let mut head = serde_json::to_vec(h).map_err(err)?;
    head.push(b'\n');
    temporary
        .write_all(&head)
        .and_then(|_| temporary.write_all(body))
        .map_err(err)?;
    sync(temporary.as_file())?;
    let published = temporary
        .persist_noclobber(&path)
        .map_err(|e| err(e.error))?;
    dir.sync_all().map_err(err)?;
    // Flush the final name and directory publication before the SQLite commit.
    sync(&published)?;
    if load(&path, true)?.header != *h {
        return Err("归档发布读回不一致；原记录未释放。".into());
    }
    Ok(())
}
pub(crate) fn archive(
    db: &mut Connection,
    root: &Path,
    expected: &str,
    at: i64,
) -> Result<Header, String> {
    archive_with(db, root, expected, at, |_, _| Ok(()))
}
fn archive_with(
    db: &mut Connection,
    root: &Path,
    expected: &str,
    at: i64,
    after_publish: impl FnOnce(&Connection, &Header) -> Result<(), String>,
) -> Result<Header, String> {
    let sync: i64 = db
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(err)?;
    let version: u32 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(err)?;
    let fk: bool = db
        .pragma_query_value(None, "foreign_keys", |r| r.get(0))
        .map_err(err)?;
    if sync < 2 || version != 7 || !fk || at < 0 {
        return Err("归档需要 v7、外键和 FULL 同步模式。".into());
    }
    let t = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)?;
    let triggers: i64 = t
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='trigger'",
            [],
            |r| r.get(0),
        )
        .map_err(err)?;
    if triggers != 0 {
        return Err("数据库有非预期触发器，保留原记录。".into());
    }
    // Do not cascade a journal deletion into a non-archived extension table.
    let names = {
        let mut q = t
            .prepare("SELECT name FROM sqlite_master WHERE type='table' LIMIT 65")
            .map_err(err)?;
        let names = q
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        names
    };
    if names.len() > 64 {
        return Err("数据库结构超过归档验证范围。".into());
    }
    for name in names.iter().filter(|name| !TABLES.contains(&name.as_str())) {
        let quoted = name.replace('"', "\"\"");
        let mut q = t
            .prepare(&format!("PRAGMA foreign_key_list(\"{quoted}\")"))
            .map_err(err)?;
        let references = q
            .query_map([], |r| r.get::<_, String>(2))
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        if references.iter().any(|r| TABLES.contains(&r.as_str())) {
            return Err("其他表引用执行证据，未释放活动记录。".into());
        }
    }
    let d = collect(&t)?;
    let digest = hash(&d.body);
    if digest != expected {
        return Err("执行记录已变化，请重新检查归档范围。".into());
    }
    let h = Header {
        format: "MooMole execution evidence".into(),
        version: 1,
        id: uuid::Uuid::new_v4().to_string(),
        created_at: at,
        counts: d.counts,
        payload_sha256: digest,
        payload_bytes: d.body.len(),
    };
    publish(root, &h, &d.body)?;
    let saved = fs::symlink_metadata(filename(root, &h.id)?).map_err(err)?;
    let saved_root = fs::symlink_metadata(root).map_err(err)?;
    after_publish(&t, &h)?;
    let current = fs::symlink_metadata(filename(root, &h.id)?).map_err(err)?;
    let current_root = fs::symlink_metadata(root).map_err(err)?;
    let stamp = |m: &std::fs::Metadata| {
        (
            m.dev(),
            m.ino(),
            m.len(),
            m.mode(),
            m.uid(),
            m.nlink(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )
    };
    if stamp(&saved) != stamp(&current)
        || (
            saved_root.dev(),
            saved_root.ino(),
            saved_root.mode(),
            saved_root.uid(),
        ) != (
            current_root.dev(),
            current_root.ino(),
            current_root.mode(),
            current_root.uid(),
        )
        || !current_root.is_dir()
    {
        return Err("归档发布后文件或目录已变化，活动记录未释放。".into());
    }
    for e in d.entries {
        let table = match e.kind {
            Kind::Cleanup => ROOTS[0],
            Kind::Uninstall => ROOTS[1],
            Kind::Maintenance => ROOTS[2],
        };
        if t.execute(&format!("DELETE FROM {table} WHERE id=?1"), [e.id])
            .map_err(|e| format!("归档副本已保存，活动记录提交失败；请刷新核对：{e}"))?
            != 1
        {
            return Err("归档已保存，但原批次未完整释放。".into());
        }
    }
    t.commit()
        .map_err(|e| format!("归档副本已保存，活动记录提交未确认；请刷新核对：{e}"))?;
    Ok(h)
}
pub(crate) fn list(root: &Path) -> Result<Listing, String> {
    let _dir = directory(root)?;
    let mut archives = Vec::new();
    let mut complete = true;
    let mut unreadable = 0;
    for (index, item) in fs::read_dir(root).map_err(err)?.enumerate() {
        if index >= 1024 {
            complete = false;
            break;
        }
        let entry = item.map_err(err)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(id) = name.strip_suffix(".moomole-history") else {
            continue;
        };
        if valid_id(id).is_err() {
            continue;
        }
        let checked = (|| {
            let file = open_file(&filename(root, id)?, true)?;
            let len = file.metadata().map_err(err)?.len();
            let h = header(&mut BufReader::new(file), len)?;
            if h.id != id {
                return Err("归档名称与标识不符。".into());
            }
            Ok::<_, String>(h)
        })();
        match checked {
            Ok(h) => archives.push(h),
            Err(_) => unreadable += 1,
        }
    }
    archives.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
    Ok(Listing {
        archives,
        complete,
        unreadable,
    })
}
pub(crate) fn view(root: &Path, id: &str) -> Result<View, String> {
    let _dir = directory(root)?;
    let d = load(&filename(root, id)?, true)?;
    if d.header.id != id {
        return Err("归档标识不符。".into());
    }
    Ok(View {
        header: d.header,
        entries: d.entries,
    })
}
pub(crate) fn detail(root: &Path, id: &str, kind: Kind, run: &str) -> Result<Value, String> {
    valid_id(run)?;
    let _dir = directory(root)?;
    let d = load(&filename(root, id)?, true)?;
    if d.header.id != id || !d.entries.iter().any(|e| e.kind == kind && e.id == run) {
        return Err("归档中没有该批次。".into());
    }
    match kind {
        Kind::Cleanup => serde_json::to_value(execution::detail(&d.database, run)?),
        Kind::Uninstall => serde_json::to_value(uninstall::detail(&d.database, run)?),
        Kind::Maintenance => serde_json::to_value(maintenance::detail(&d.database, run)?),
    }
    .map_err(err)
}
pub(crate) fn import(root: &Path, path: &Path) -> Result<Header, String> {
    let d = load(path, false)?;
    let _dir = directory(root)?;
    let destination = filename(root, &d.header.id)?;
    if destination.exists() {
        if load(&destination, true)?.header == d.header {
            return Ok(d.header);
        }
        return Err("已有同标识的不同归档，未覆盖。".into());
    }
    let evidence = collect(&d.database)?;
    if hash(&evidence.body) != d.header.payload_sha256 {
        return Err("归档不是原格式的完整记录。".into());
    }
    publish(root, &d.header, &evidence.body)?;
    Ok(d.header)
}
pub(crate) fn export(root: &Path, id: &str, path: &Path) -> Result<(), String> {
    let _dir = directory(root)?;
    let source = filename(root, id)?;
    let d = load(&source, true)?;
    if d.header.id != id || !path.is_absolute() {
        return Err("归档标识或导出位置无效。".into());
    }
    if path.extension().and_then(|s| s.to_str()) != Some("moomole-history") {
        return Err("请选择 .moomole-history 文件名。".into());
    }
    let parent = path.parent().ok_or("导出目录无效。")?;
    for component in parent.ancestors() {
        if !fs::symlink_metadata(component).map_err(err)?.is_dir() {
            return Err("导出目录不能经过链接。".into());
        }
    }
    let mut copy = tempfile::NamedTempFile::new_in(parent).map_err(err)?;
    let file = open_file(&source, true)?;
    std::io::copy(
        &mut file.take((MAX_BODY + MAX_HEADER + 1) as u64),
        &mut copy,
    )
    .map_err(err)?;
    sync(copy.as_file())?;
    let published = copy.persist_noclobber(path).map_err(|e| err(e.error))?;
    File::open(parent).and_then(|f| f.sync_all()).map_err(err)?;
    sync(&published)?;
    if load(path, false)?.header != d.header {
        return Err("导出归档读回不一致，原副本保留。".into());
    }
    Ok(())
}
pub(crate) fn folder(root: &Path) -> Result<PathBuf, String> {
    let _dir = directory(root)?;
    Ok(root.into())
}
#[cfg(test)]
mod tests;
