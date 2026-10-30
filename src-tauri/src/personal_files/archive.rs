//! Selected file data -> verified ZIP -> exclusive publication. Originals stay.
use super::*;
use crate::{db, engine::Job, observation::AnchoredPath};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::PathBuf,
    sync::{atomic::Ordering, Arc, Mutex},
};
pub(crate) mod commands;
pub(crate) mod journal;
const MAX_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const BUDGET: Duration = Duration::from_secs(15 * 60);
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Plan {
    source: super::Plan,
    directory: movement::Movement,
    output_path: String,
    temporary_name: String,
    names: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Entry {
    id: String,
    path: String,
    name: String,
    size: u64,
    state: String,
    sha256: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Artifact {
    device: u64,
    inode: u64,
    size: u64,
    sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Record {
    id: String,
    request_id: String,
    created_at: i64,
    finished_at: Option<i64>,
    state: String,
    phase: String,
    output_path: String,
    temporary_path: String,
    entries: Vec<Entry>,
    artifact: Option<Artifact>,
    reason: String,
    reviewed_at: Option<i64>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Draft {
    id: String,
    scan_id: String,
    expires_at: i64,
    output_path: String,
    items: Vec<FileRow>,
    names: Vec<String>,
}
#[derive(Default)]
pub(crate) struct Store {
    plans: HashMap<String, (Plan, Instant)>,
}
impl Store {
    fn publish(&mut self, plan: Plan) -> Result<Draft, String> {
        self.plans
            .retain(|_, (_, deadline)| Instant::now() < *deadline);
        if self.plans.len() >= 8 {
            return Err("归档确认过多，请关闭旧确认后重试。".into());
        }
        let s = &plan.source;
        let (expires_at, deadline) = confirmation_window(s, db::now())?;
        let draft = Draft {
            id: s.id.clone(),
            scan_id: s.scan_id.clone(),
            expires_at,
            output_path: plan.output_path.clone(),
            items: s.items.iter().map(row).collect(),
            names: plan.names.clone(),
        };
        self.plans.insert(s.id.clone(), (plan, deadline));
        Ok(draft)
    }
    fn take(&mut self, id: &str) -> Result<Plan, String> {
        let (p, deadline) = self
            .plans
            .remove(id)
            .ok_or("归档确认已使用或撤销，请重新检查。")?;
        if Instant::now() >= deadline || confirmation_window(&p.source, db::now()).is_err() {
            return Err("归档确认已过期，请重新检查。".into());
        }
        Ok(p)
    }
}
fn prepare(
    source: super::Plan,
    directory: &str,
    filename: &str,
    settings: &db::Settings,
) -> Result<Plan, String> {
    source.validate_record()?;
    if source.origin() != MODULE
        || source.movement.is_some()
        || source.items.iter().map(|i| i.size).sum::<u64>() > MAX_BYTES
        || filename.is_empty()
        || filename.len() > 200
        || filename.contains('/')
        || !filename.ends_with(".zip")
    {
        return Err("请选择本轮普通文件和 .zip 名称；每批最多 20 项、合计 16 GiB。".into());
    }
    let held = movement::directory(directory, settings)?;
    let output = Path::new(directory).join(filename);
    if !ordinary(&output)
        || protected(&output, &settings.protected_paths)?
        || !matches!(std::fs::symlink_metadata(&output), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
    {
        return Err("归档名称已存在、受保护或无法核对，请另选名称；不会覆盖。".into());
    }
    let names = source
        .items
        .iter()
        .map(|item| {
            let relative = Path::new(&item.path)
                .strip_prefix(&source.root)
                .map_err(|e| e.to_string())?;
            relative
                .to_str()
                .map(str::to_owned)
                .ok_or("归档内路径无法编码。".into())
        })
        .collect::<Result<Vec<_>, String>>()?;
    if names.iter().any(|n| n.contains('\\') || n.contains(':')) {
        return Err("本轮归档内路径不支持反斜杠或冒号，请先调整名称。".into());
    }
    let directory = movement::Movement {
        directory: directory.into(),
        witness: held.witness,
        conflict: movement::Conflict::Skip,
        targets: vec![],
    };
    let plan = Plan {
        temporary_name: format!(".MooMole-archive-{}.partial", source.id),
        source,
        directory,
        output_path: output.to_str().ok_or("归档路径无法编码。")?.into(),
        names,
    };
    validate(&plan, settings)?;
    Ok(plan)
}
fn validate(plan: &Plan, settings: &db::Settings) -> Result<AnchoredPath, String> {
    plan.source.validate_record()?;
    if !settings.acknowledged || protected(Path::new(&plan.output_path), &settings.protected_paths)?
    {
        return Err("归档目标已受保护，未发布。".into());
    }
    let target = plan.directory.validate_directory(settings)?;
    for item in &plan.source.items {
        if protected(Path::new(&item.path), &settings.protected_paths)? {
            return Err("所选文件已受保护，归档未发布。".into());
        }
        let held = AnchoredPath::open(Path::new(&item.path))?;
        if held.witness != item.witness
            || !held.local_materialized()
            || held.target().metadata().map_err(|e| e.to_string())?.uid()
                != unsafe { libc::geteuid() }
        {
            return Err("所选文件已变化、未落地或不属于当前账户，请重新扫描。".into());
        }
    }
    Ok(target)
}
fn check(job: &Job, started: Instant) -> Result<(), String> {
    if job.cancelled.load(Ordering::SeqCst) {
        Err("归档已取消，原文件保留。".into())
    } else if started.elapsed() > BUDGET {
        Err("归档超过本轮十五分钟预算，原文件保留。".into())
    } else {
        Ok(())
    }
}
// Own the exact temporary inode under a held physical parent. Cleanup never
// unlinks a replacement file or uses an unvalidated absolute path.
struct Temporary {
    parent: File,
    name: CString,
    file: File,
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let own = self.file.metadata();
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0
        {
            let s = unsafe { stat.assume_init() };
            if own.is_ok_and(|m| (m.dev(), m.ino()) == (s.st_dev as u64, s.st_ino)) {
                unsafe {
                    libc::unlinkat(self.parent.as_raw_fd(), self.name.as_ptr(), 0);
                }
            }
        }
    }
}
fn create(parent: &AnchoredPath, name: &str) -> Result<Temporary, String> {
    let name = CString::new(name).map_err(|e| e.to_string())?;
    let fd = unsafe {
        libc::openat(
            parent.target().as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err("临时归档无法创建，原文件保留。".into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    Ok(Temporary {
        parent: parent.target().try_clone().map_err(|e| e.to_string())?,
        name,
        file,
    })
}
fn open_read(held: &AnchoredPath) -> Result<File, String> {
    let node = held.witness.nodes.last().ok_or("缺少文件身份。")?;
    let name = CString::new(
        node.path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("文件名无法编码。")?,
    )
    .map_err(|e| e.to_string())?;
    let fd = unsafe {
        libc::openat(
            held.parent().as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err("归档文件内容不可读取，未发布。".into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let m = file.metadata().map_err(|e| e.to_string())?;
    if !m.is_file() || (m.dev(), m.ino()) != (node.dev, node.ino) || m.nlink() != 1 {
        return Err("归档文件身份变化，未读取。".into());
    }
    held.validate()?;
    Ok(file)
}
fn digest(
    mut input: impl Read,
    job: &Job,
    started: Instant,
    expected: u64,
) -> Result<String, String> {
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0; 128 * 1024];
    loop {
        check(job, started)?;
        let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > expected {
            return Err("归档内容大小变化，未发布。".into());
        }
        hash.update(&buffer[..n]);
    }
    if count != expected {
        return Err("归档内容未完整读取，未发布。".into());
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn stamp(m: &std::fs::Metadata) -> (u64, u64, u64, i64, i64, i64, i64, u64, u32, u32, u32) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
        m.nlink(),
        m.mode(),
        m.uid(),
        m.gid(),
    )
}
fn build(
    plan: &Plan,
    file: &mut File,
    record: &mut Record,
    job: &Job,
    started: Instant,
) -> Result<std::fs::Metadata, String> {
    let mut zip = zip::ZipWriter::new(&mut *file);
    for (index, item) in plan.source.items.iter().enumerate() {
        check(job, started)?;
        let held = AnchoredPath::open(Path::new(&item.path))?;
        if held.witness != item.witness || !held.local_materialized() {
            return Err("归档来源已变化。".into());
        }
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o600)
            .large_file(item.size >= u32::MAX as u64);
        zip.start_file(&plan.names[index], options)
            .map_err(|e| e.to_string())?;
        let mut input = open_read(&held)?;
        let mut hash = Sha256::new();
        let mut count = 0u64;
        let mut buffer = [0u8; 128 * 1024];
        loop {
            check(job, started)?;
            let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > item.size {
                return Err("来源文件变大，未发布归档。".into());
            }
            zip.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            hash.update(&buffer[..n]);
        }
        if count != item.size {
            return Err("来源文件未完整读取，未发布归档。".into());
        }
        held.validate()?;
        record.entries[index].sha256 = Some(format!("{:x}", hash.finalize()));
    }
    zip.finish().map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } != 0 {
        return Err("归档同步未完成，未发布。".into());
    }
    let verified = file.metadata().map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut verify = zip::ZipArchive::new(&mut *file).map_err(|e| e.to_string())?;
    if verify.len() != plan.source.items.len() {
        return Err("归档项目数不匹配，未发布。".into());
    }
    for (index, entry) in record.entries.iter_mut().enumerate() {
        let mut item = verify.by_index(index).map_err(|e| e.to_string())?;
        if item.name() != entry.name
            || item.size() != entry.size
            || item.is_dir()
            || Some(digest(&mut item, job, started, entry.size)?) != entry.sha256
        {
            return Err("归档内容校验不匹配，未发布。".into());
        }
        entry.state = "verified".into();
    }
    drop(verify);
    if stamp(&verified) != stamp(&file.metadata().map_err(|e| e.to_string())?) {
        return Err("归档在校验期间变化，未发布。".into());
    }
    Ok(verified)
}
fn run(
    database: Arc<Mutex<db::Database>>,
    plan: Plan,
    request: String,
    job: Arc<Job>,
) -> Result<Record, String> {
    let started = Instant::now();
    let mut record = Record {
        id: plan.source.id.clone(),
        request_id: request,
        created_at: db::now(),
        finished_at: None,
        state: "running".into(),
        phase: "building".into(),
        output_path: plan.output_path.clone(),
        temporary_path: Path::new(&plan.directory.directory)
            .join(&plan.temporary_name)
            .to_string_lossy()
            .into_owned(),
        reviewed_at: None,
        entries: plan
            .source
            .items
            .iter()
            .zip(&plan.names)
            .map(|(i, name)| Entry {
                id: i.id.clone(),
                path: i.path.clone(),
                name: name.clone(),
                size: i.size,
                state: "pending".into(),
                sha256: None,
            })
            .collect(),
        artifact: None,
        reason: String::new(),
    };
    {
        let mut db = database.lock().map_err(|e| e.to_string())?;
        if db::now() < plan.source.created_at || db::now() >= plan.source.expires_at {
            return Err("归档确认已过期。".into());
        }
        validate(&plan, &db::settings(&db)?)?;
        journal::insert(&mut db, &plan, &record)?;
    }
    let mut published = false;
    let result = (|| -> Result<(), String> {
        check(&job, started)?;
        let parent = validate(
            &plan,
            &db::settings(&*database.lock().map_err(|e| e.to_string())?)?,
        )?;
        let mut temporary = create(&parent, &plan.temporary_name)?;
        let meta = build(&plan, &mut temporary.file, &mut record, &job, started)?;
        record.phase = "publishing".into();
        if meta.nlink() != 1 {
            return Err("临时归档身份变化，未发布。".into());
        }
        temporary
            .file
            .seek(SeekFrom::Start(0))
            .map_err(|e| e.to_string())?;
        let sha256 = digest(&mut temporary.file, &job, started, meta.len())?;
        record.artifact = Some(Artifact {
            device: meta.dev(),
            inode: meta.ino(),
            size: meta.len(),
            sha256,
        });
        journal::save(&*database.lock().map_err(|e| e.to_string())?, &record)?;
        let current = validate(
            &plan,
            &db::settings(&*database.lock().map_err(|e| e.to_string())?)?,
        )?;
        if current.witness.nodes != parent.witness.nodes {
            return Err("目标目录已变化，未发布。".into());
        }
        check(&job, started)?;
        if stamp(&meta) != stamp(&temporary.file.metadata().map_err(|e| e.to_string())?) {
            return Err("临时归档内容或属性变化，未发布。".into());
        }
        // Validate the exact temporary name/inode through the physical parent.
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                parent.target().as_raw_fd(),
                temporary.name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err("临时归档名称变化，未发布。".into());
        }
        let s = unsafe { stat.assume_init() };
        if (s.st_dev as u64, s.st_ino, s.st_size) != (meta.dev(), meta.ino(), meta.len() as i64)
            || s.st_nlink != 1
        {
            return Err("临时归档身份变化，未发布。".into());
        }
        let name = CString::new(
            Path::new(&plan.output_path)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
        )
        .map_err(|e| e.to_string())?;
        if unsafe {
            libc::renameatx_np(
                parent.target().as_raw_fd(),
                temporary.name.as_ptr(),
                parent.target().as_raw_fd(),
                name.as_ptr(),
                libc::RENAME_EXCL,
            )
        } != 0
        {
            return Err("归档发布失败或同名文件已出现，原文件保留且不覆盖。".into());
        }
        published = true;
        parent.target().sync_all().map_err(|e| e.to_string())?;
        let observed = AnchoredPath::open(Path::new(&plan.output_path))?;
        if observed.witness.nodes != {
            let mut n = parent.witness.nodes.clone();
            n.push(crate::observation::Node {
                path: PathBuf::from(&plan.output_path),
                dev: meta.dev(),
                ino: meta.ino(),
                mode: meta.mode(),
                uid: meta.uid(),
                gid: meta.gid(),
            });
            n
        } {
            return Err("发布后的归档身份无法核对，请检查目标。".into());
        }
        let m = observed.target().metadata().map_err(|e| e.to_string())?;
        let mut f = open_read(&observed)?;
        if m.len() != meta.len()
            || m.nlink() != 1
            || digest(&mut f, &job, started, m.len())? != record.artifact.as_ref().unwrap().sha256
        {
            return Err("发布后的归档内容无法核对，请检查目标。".into());
        }
        observed.validate()?;
        Ok(())
    })();
    record.finished_at = Some(db::now());
    record.phase = "finished".into();
    match result {
        Ok(()) => {
            record.state = "completed".into();
            record.reason =
                "ZIP 已生成并逐项校验，原文件保持原样；归档本身不释放原文件占用。".into();
            for e in &mut record.entries {
                e.state = "archived".into();
            }
        }
        Err(reason) => {
            record.state = if published {
                "uncertain"
            } else if job.cancelled.load(Ordering::SeqCst) {
                "cancelled"
            } else {
                "failed"
            }
            .into();
            record.reason = format!(
                "{reason} 原文件保留；临时文件若仍存在，请按记录路径核对，不会自动清理其他文件。"
            );
            for e in &mut record.entries {
                e.state = if published {
                    "uncertain"
                } else {
                    "notArchived"
                }
                .into();
            }
        }
    }
    journal::save(&*database.lock().map_err(|e| e.to_string())?, &record)?;
    Ok(record)
}
#[cfg(test)]
mod tests;
