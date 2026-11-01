//! Explicit ZIP data extraction into a new, exclusively published directory.
//! No source deletion, execution, overwrite, link creation or metadata unpacking.
use super::*;
use crate::{
    db,
    engine::{Job, TaskKind},
    observation::AnchoredPath,
};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    sync::{atomic::Ordering, Arc, Mutex},
};
pub(crate) mod commands;
mod inventory;
pub(crate) mod journal;
#[cfg(test)]
mod tests;
mod tree;
const MAX_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_PACKED: u64 = 2 * 1024 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Sample {
    name: String,
    directory: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Draft {
    id: String,
    root: String,
    output_path: String,
    files: usize,
    directories: usize,
    hidden: usize,
    bytes: u64,
    metadata_bytes: u64,
    source_sha256: String,
    samples: Vec<Sample>,
    created_at: i64,
    expires_at: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Artifact {
    device: String,
    inode: String,
    bytes: u64,
    sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Record {
    id: String,
    request_id: String,
    draft: Draft,
    temporary_path: String,
    created_at: i64,
    finished_at: Option<i64>,
    state: String,
    phase: String,
    artifact: Option<Artifact>,
    reason: String,
    reviewed_at: Option<i64>,
}
#[derive(Clone, PartialEq, Eq)]
struct Entry {
    name: String,
    directory: bool,
    size: u64,
    sha256: String,
    index: Option<usize>,
}
pub(super) struct Plan {
    draft: Draft,
    witness: Witness,
    snapshot: File,
    entries: Vec<Entry>,
    quarantine: Option<Vec<u8>>,
    directory: movement::Movement,
    rules: Vec<String>,
    issued: Instant,
}
fn check(job: &Job, start: Instant) -> Result<(), String> {
    archive::check(job, start)
}
fn source(path: &Path, settings: &db::Settings) -> Result<AnchoredPath, String> {
    if !settings.acknowledged
        || unsafe { libc::geteuid() } == 0
        || !ordinary(path)
        || protected(path, &settings.protected_paths)?
        || !path
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("zip"))
    {
        return Err("请选择未受保护的本地 ZIP 文件。".into());
    }
    let h = AnchoredPath::open(path)?;
    let m = h.target().metadata().map_err(|e| e.to_string())?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || !h.local_materialized()
        || m.len() > MAX_PACKED
    {
        return Err("ZIP 必须是本账户的普通本地文件，压缩包最多 2 GiB。".into());
    }
    Ok(h)
}
fn quarantine(file: &File) -> Result<Option<Vec<u8>>, String> {
    let n = unsafe {
        libc::fgetxattr(
            file.as_raw_fd(),
            c"com.apple.quarantine".as_ptr(),
            std::ptr::null_mut(),
            0,
            0,
            0,
        )
    };
    if n < 0 {
        if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOATTR) {
            return Ok(None);
        }
        return Err("ZIP 隔离属性无法核对，未解压。".into());
    }
    if n > 65536 {
        return Err("ZIP 隔离属性超过读取上限。".into());
    }
    let mut v = vec![0; n as usize];
    if unsafe {
        libc::fgetxattr(
            file.as_raw_fd(),
            c"com.apple.quarantine".as_ptr(),
            v.as_mut_ptr().cast(),
            v.len(),
            0,
            0,
        )
    } != n
    {
        return Err("ZIP 隔离属性在读取期间变化。".into());
    }
    Ok(Some(v))
}
fn capacity(file: &File, bytes: u64) -> Result<(), String> {
    let mut v = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    if unsafe { libc::fstatfs(file.as_raw_fd(), v.as_mut_ptr()) } != 0 {
        return Err("解压目标空间无法核对。".into());
    }
    let v = unsafe { v.assume_init() };
    if (v.f_bavail as u64).saturating_mul(v.f_bsize as u64) < bytes.saturating_add(64 * 1024 * 1024)
    {
        return Err("解压空间不足，需保留完整内容及 64 MiB 余量。".into());
    }
    Ok(())
}
impl Plan {
    fn prepare(
        root: &Path,
        directory: &str,
        filename: &str,
        settings: &db::Settings,
        job: &Job,
    ) -> Result<Self, String> {
        let start = Instant::now();
        check(job, start)?;
        if filename.is_empty() || filename.len() > 200 || filename.contains(['/', '\\', ':']) {
            return Err("请选择一个新的普通解压文件夹名称。".into());
        }
        let parent = movement::directory(directory, settings)?;
        let output = Path::new(directory).join(filename);
        if !ordinary(&output)
            || protected(&output, &settings.protected_paths)?
            || !matches!(std::fs::symlink_metadata(&output),Err(e)if e.kind()==std::io::ErrorKind::NotFound)
        {
            return Err("解压目标已存在、受保护或无法核对；不会覆盖。".into());
        }
        let held = source(root, settings)?;
        let mut input = archive::open_read(&held)?;
        let m = input.metadata().map_err(|e| e.to_string())?;
        let q = quarantine(&input)?;
        let mut snapshot = tempfile::tempfile().map_err(|e| e.to_string())?;
        capacity(&snapshot, m.len())?;
        let mut buffer = [0; 128 * 1024];
        let mut size = 0;
        let mut hash = Sha256::new();
        loop {
            check(job, start)?;
            let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            size += n as u64;
            if size > m.len() {
                return Err("ZIP 在读取时变化，请重新检查。".into());
            }
            snapshot
                .write_all(&buffer[..n])
                .map_err(|e| e.to_string())?;
            hash.update(&buffer[..n]);
        }
        held.validate()?;
        if size != m.len()
            || archive::stamp(&m) != archive::stamp(&input.metadata().map_err(|e| e.to_string())?)
            || quarantine(&input)? != q
        {
            return Err("ZIP 在读取时变化，请重新检查。".into());
        }
        let entries = inventory::read(&mut snapshot, job, start)?;
        let bytes = entries.iter().map(|e| e.size).sum();
        capacity(parent.target(), bytes)?;
        let now = db::now();
        let draft = Draft {
            id: uuid::Uuid::new_v4().to_string(),
            root: root.to_str().ok_or("ZIP 路径无法编码。")?.into(),
            output_path: output.to_str().ok_or("解压路径无法编码。")?.into(),
            files: entries.iter().filter(|e| !e.directory).count(),
            directories: entries.iter().filter(|e| e.directory).count() + 1,
            hidden: entries
                .iter()
                .filter(|e| e.name.split('/').any(|s| s.starts_with('.')))
                .count(),
            bytes,
            metadata_bytes: entries
                .iter()
                .filter(|e| {
                    e.name.starts_with("__MACOSX/")
                        || e.name.split('/').any(|s| s.starts_with("._"))
                })
                .map(|e| e.size)
                .sum(),
            source_sha256: format!("{:x}", hash.finalize()),
            samples: entries
                .iter()
                .take(50)
                .map(|e| Sample {
                    name: e.name.clone(),
                    directory: e.directory,
                })
                .collect(),
            created_at: now,
            expires_at: now + 120000,
        };
        Ok(Self {
            draft,
            witness: held.witness,
            snapshot,
            entries,
            quarantine: q,
            directory: movement::Movement {
                directory: directory.into(),
                witness: parent.witness,
                conflict: movement::Conflict::Skip,
                targets: vec![],
            },
            rules: settings.protected_paths.clone(),
            issued: Instant::now(),
        })
    }
    fn time(&self) -> Result<(), String> {
        let now = db::now();
        if self.issued.elapsed() >= Duration::from_secs(120)
            || now < self.draft.created_at
            || now >= self.draft.expires_at
        {
            return Err("解压确认已过期，请重新检查。".into());
        }
        Ok(())
    }
    fn validate(
        &self,
        settings: &db::Settings,
        job: &Job,
        start: Instant,
    ) -> Result<AnchoredPath, String> {
        check(job, start)?;
        if self.rules != settings.protected_paths
            || protected(
                Path::new(&self.draft.output_path),
                &settings.protected_paths,
            )?
        {
            return Err("解压保护范围已变化，请重新检查。".into());
        }
        let parent = self.directory.validate_directory(settings)?;
        let held = source(Path::new(&self.draft.root), settings)?;
        if held.witness != self.witness {
            return Err("原 ZIP 或祖先身份已变化，请重新检查。".into());
        }
        let mut f = archive::open_read(&held)?;
        let size = f.metadata().map_err(|e| e.to_string())?.len();
        if archive::digest(&mut f, job, start, size)? != self.draft.source_sha256
            || quarantine(&f)? != self.quarantine
        {
            return Err("原 ZIP 内容或隔离属性变化，未发布解压结果。".into());
        }
        held.validate()?;
        Ok(parent)
    }
}
fn run(
    database: &Mutex<db::Database>,
    mut plan: Plan,
    request: String,
    job: &Job,
) -> Result<Record, String> {
    plan.time()?;
    let start = Instant::now();
    let mut r = Record {
        id: plan.draft.id.clone(),
        request_id: request,
        draft: plan.draft.clone(),
        temporary_path: Path::new(&plan.directory.directory)
            .join(format!(".MooMole-zip-extract-{}.partial", plan.draft.id))
            .to_string_lossy()
            .into_owned(),
        created_at: db::now().max(plan.draft.created_at),
        finished_at: None,
        state: "running".into(),
        phase: "building".into(),
        artifact: None,
        reason: "正在核对并解压 ZIP，原压缩包保留。".into(),
        reviewed_at: None,
    };
    journal::begin(&mut *database.lock().map_err(|e| e.to_string())?, &r)?;
    let mut stage = None;
    let result = (|| -> Result<(), String> {
        let settings = db::settings(&*database.lock().map_err(|e| e.to_string())?)?;
        let parent = plan.validate(&settings, job, start)?;
        capacity(parent.target(), plan.draft.bytes)?;
        stage = Some(tree::Stage::create(&parent, &plan.draft.id)?);
        let s = stage.as_mut().unwrap();
        s.extract(
            &mut plan.snapshot,
            &plan.entries,
            plan.quarantine.as_deref(),
            job,
            start,
        )?;
        s.verify(&plan.entries, plan.quarantine.as_deref(), job, start)?;
        let settings = db::settings(&*database.lock().map_err(|e| e.to_string())?)?;
        let current = plan.validate(&settings, job, start)?;
        if current.witness.nodes != parent.witness.nodes {
            return Err("解压目标身份变化，未发布。".into());
        }
        capacity(parent.target(), 0)?;
        s.validate()?;
        check(job, start)?;
        let m = s.root.metadata().map_err(|e| e.to_string())?;
        let manifest = plan
            .entries
            .iter()
            .map(|e| (&e.name, e.directory, e.size, &e.sha256))
            .collect::<Vec<_>>();
        r.artifact = Some(Artifact {
            device: m.dev().to_string(),
            inode: m.ino().to_string(),
            bytes: plan.draft.bytes,
            sha256: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&manifest).map_err(|e| e.to_string())?)
            ),
        });
        r.phase = "publishing".into();
        r.reason = "即将发布已核对的解压文件夹。".into();
        journal::save(&*database.lock().map_err(|e| e.to_string())?, &r)?;
        s.publish(
            Path::new(&plan.draft.output_path)
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or("解压名称无法编码。")?,
        )?;
        let output = AnchoredPath::open(Path::new(&plan.draft.output_path))?;
        let n = output.target().metadata().map_err(|e| e.to_string())?;
        if (m.dev(), m.ino()) != (n.dev(), n.ino()) {
            return Err("已发布解压文件夹身份无法核对，请检查实际位置。".into());
        }
        s.verify(
            &plan.entries,
            plan.quarantine.as_deref(),
            &Job::new(TaskKind::PersonalArchive),
            start,
        )?;
        output.validate()?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            r.state = "completed".into();
            r.reason =
                "ZIP 正文已解压并核验，原压缩包保留；Mac 元数据文件保留为独立文件，未应用。".into();
        }
        Err(e) => {
            r.state = if r.phase == "publishing" {
                "unknown"
            } else if job.cancelled.load(Ordering::SeqCst) {
                "cancelled"
            } else {
                "retained"
            }
            .into();
            r.reason = e;
        }
    }
    if let Some(s) = stage.as_mut() {
        if !s.published && !s.cleanup() {
            r.state = "unknown".into();
            r.reason = "解压临时目录未能完整清理，请核对临时位置；不会自动删除或重试。".into();
        }
    } else if std::fs::symlink_metadata(&r.temporary_path).is_ok() {
        r.state = "unknown".into();
        r.reason = "解压临时位置出现未知内容，请核对实际位置；不会自动删除或重试。".into();
    }
    r.phase = "finished".into();
    r.finished_at = Some(db::now().max(r.created_at));
    journal::save(&*database.lock().map_err(|e| e.to_string())?, &r)?;
    Ok(r)
}
