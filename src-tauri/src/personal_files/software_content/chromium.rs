//! Snapshot only a fixed browser's History (+ WAL) into a private temporary
//! directory. All SQLite access/writes are to the copy; source files stay intact.
use super::*;
use rusqlite::{Connection, OpenFlags};
use std::{fs::OpenOptions, io::Read, os::unix::fs::OpenOptionsExt, sync::atomic::Ordering};
pub(super) struct ReadBound {
    pub(super) held: AnchoredPath,
    pub(super) bytes: Vec<u8>,
}
pub(super) fn read(
    path: &Path,
    limit: u64,
    job: &Job,
    started: Instant,
) -> Result<ReadBound, String> {
    let held = AnchoredPath::open(path)?;
    let m = held.target().metadata().map_err(|e| e.to_string())?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || !held.local_materialized()
        || m.len() > limit
    {
        return Err("来源记录不可读取、未落地或超过本轮读取上限。".into());
    }
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| e.to_string())?;
    let observed = f.metadata().map_err(|e| e.to_string())?;
    if (observed.dev(), observed.ino()) != (m.dev(), m.ino()) {
        return Err("来源记录在读取前变化，请重新扫描。".into());
    }
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 128 * 1024];
    loop {
        if job.cancelled.load(Ordering::SeqCst) || started.elapsed() > Duration::from_secs(10) {
            return Err("来源记录读取已停止，未确认的范围保留。".into());
        }
        let n = f.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if bytes.len() as u64 + n as u64 > limit {
            return Err("来源记录在读取期间变大，原记录保留。".into());
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
    held.validate()?;
    if bytes.len() as u64 != m.len() {
        return Err("来源记录大小变化，请重新扫描。".into());
    }
    Ok(ReadBound { held, bytes })
}
pub(super) fn collect(
    home: &Path,
    source: Source,
    rules: &[String],
    job: Arc<Job>,
) -> Result<Scan, String> {
    if !ordinary(home) {
        return Err("账户目录范围无法核对。".into());
    }
    let started = Instant::now();
    let root = AnchoredPath::open(home)?;
    let meta = root.target().metadata().map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || !root.local_materialized() {
        return Err("账户目录不属于当前用户或本地状态未知。".into());
    }
    let history = source.history(home).ok_or("此来源没有已适配的下载记录。")?;
    if protected(&history, rules)? {
        return Err("浏览器下载记录与保护范围重叠，未读取。".into());
    }
    let database = read(&history, 128 * 1024 * 1024, &job, started)?;
    let wal_path = history.with_file_name("History-wal");
    let wal = match std::fs::symlink_metadata(&wal_path) {
        Ok(_) => Some(read(&wal_path, 32 * 1024 * 1024, &job, started)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("无法确认浏览器 WAL 状态，未读取下载记录。".into()),
    };
    let scratch = tempfile::Builder::new()
        .prefix("MooMole-download-snapshot-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let copy = scratch.path().join("History");
    std::fs::write(&copy, &database.bytes).map_err(|e| e.to_string())?;
    if let Some(wal) = &wal {
        std::fs::write(scratch.path().join("History-wal"), &wal.bytes)
            .map_err(|e| e.to_string())?;
    }
    // Private scratch permits SQLite WAL recovery/SHM. It has no original DB
    // attachment and never receives caller-provided SQL, paths or functions.
    let db = Connection::open_with_flags(
        &copy,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "下载记录快照无法打开，未推断文件归属。")?;
    db.busy_timeout(Duration::from_millis(100))
        .map_err(|e| e.to_string())?;
    db.pragma_update(None, "query_only", true)
        .map_err(|e| e.to_string())?;
    db.pragma_update(None, "trusted_schema", false)
        .map_err(|e| e.to_string())?;
    let (kind,sql):(String,Option<String>)=db.query_row("SELECT type,CASE WHEN length(sql)<=65536 THEN sql END FROM sqlite_master WHERE name='downloads'",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|"没有识别到支持的浏览器下载记录结构。")?;
    if kind != "table"
        || !sql.is_some_and(|s| {
            s.trim_start()
                .to_ascii_uppercase()
                .starts_with("CREATE TABLE")
        })
    {
        return Err("浏览器下载记录不是已适配的普通表，原数据保留。".into());
    }
    let mut query=db.prepare("SELECT id,CASE WHEN typeof(target_path)='text' AND length(CAST(target_path AS BLOB))<=4096 THEN target_path END FROM downloads WHERE state=1 AND current_path=target_path ORDER BY id DESC LIMIT 2001").map_err(|_|"浏览器下载记录列不受支持，请保留原数据。")?;
    let rows = query
        .query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    let mut visited = 0;
    let mut skipped = 0;
    let mut complete = true;
    let mut seen = HashSet::new();
    let mut size = 0usize;
    for entry in rows {
        if job.cancelled.load(Ordering::SeqCst)
            || started.elapsed() > Duration::from_secs(10)
            || visited >= 2000
        {
            complete = false;
            break;
        }
        visited += 1;
        let (_, path) = match entry {
            Ok((id, Some(path))) if id >= 0 => (id, path),
            _ => {
                complete = false;
                skipped += 1;
                continue;
            }
        };
        let path = Path::new(&path);
        if !path.starts_with(home) || path == home || !ordinary(path) || protected(path, rules)? {
            skipped += 1;
            continue;
        }
        let observed = (|| -> Result<Item, String> {
            let held = AnchoredPath::open(path)?;
            let m = held.target().metadata().map_err(|e| e.to_string())?;
            if !m.is_file()
                || m.uid() != unsafe { libc::geteuid() }
                || m.dev() != meta.dev()
                || !held.local_materialized()
                || m.len() > 9_007_199_254_740_991
            {
                return Err("下载文件不是本账户的普通本地文件。".into());
            }
            if !seen.insert((m.dev(), m.ino())) {
                return Err("下载记录重复指向同一文件。".into());
            }
            Ok(Item {
                id: uuid::Uuid::new_v4().to_string(),
                path: path.to_str().ok_or("下载路径无法编码。")?.into(),
                size: m.len(),
                modified_at: m.mtime().saturating_mul(1000),
                witness: held.witness,
            })
        })();
        match observed {
            Ok(item) => {
                let n = serde_json::to_vec(&item).map_err(|e| e.to_string())?.len();
                if size + n > 7 * 1024 * 1024 {
                    complete = false;
                    break;
                }
                size += n;
                items.push(item);
            }
            Err(_) => {
                skipped += 1;
                complete = false;
            }
        }
    }
    database.held.validate()?;
    if let Some(wal) = &wal {
        wal.held.validate()?;
    } else if !matches!(std::fs::symlink_metadata(&wal_path),Err(e) if e.kind()==std::io::ErrorKind::NotFound)
    {
        return Err("读取期间浏览器 WAL 状态变化，请重新扫描。".into());
    }
    let now = db::now();
    let binding = Binding {
        source,
        home: home.to_str().ok_or("账户目录无法编码。")?.into(),
        directory: root.witness,
        allocated_at_scan: 0,
        history: Some(database.held.witness.clone()),
        playlist: None,
    };
    let report = Report {
        id: uuid::Uuid::new_v4().to_string(),
        root: home.to_str().unwrap().into(),
        created_at: now,
        expires_at: now + TTL,
        complete,
        cancelled: job.cancelled.load(Ordering::SeqCst),
        skipped,
        visited,
        items: items.iter().map(row).collect(),
        duplicates: None,
        similar_photos: None,
        software_content: Some(Info {
            source,
            minimum_age_days: 0,
            minimum_directory_bytes: 0,
            minimum_size_met: true,
            execution_supported: true,
            playlist_path: None,
        }),
    };
    Ok(Scan {
        id: report.id.clone(),
        root: report.root.clone(),
        created_at: now,
        expires_at: now + TTL,
        items,
        report,
        software_content: Some(binding),
        allocated_bytes: 0,
    })
}
#[cfg(test)]
mod tests;
