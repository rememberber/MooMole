//! Explicit ZIP data extraction into a new, exclusively published directory.
//! No source deletion, execution, overwrite or link creation. Mac restoration is opt-in.
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
mod mac_metadata;
mod password;
mod tar;
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
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Format {
    #[default]
    Zip,
    Tar,
    TarGzip,
}
impl Format {
    fn is_zip(&self) -> bool {
        *self == Self::Zip
    }
    fn from_path(path: &Path) -> Option<Self> {
        let n = path.file_name()?.to_str()?.to_ascii_lowercase();
        if n.ends_with(".tar.gz") || n.ends_with(".tgz") {
            Some(Self::TarGzip)
        } else if n.ends_with(".tar") {
            Some(Self::Tar)
        } else if n.ends_with(".zip") {
            Some(Self::Zip)
        } else {
            None
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Draft {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    encrypted: bool,
    #[serde(default, skip_serializing_if = "Format::is_zip")]
    format: Format,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mac_metadata: Option<mac_metadata::Summary>,
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
    offset: Option<u64>,
}
pub(super) struct Plan {
    draft: Draft,
    witness: Witness,
    snapshot: File,
    entries: Vec<Entry>,
    quarantine: Option<Vec<u8>>,
    source_quarantine: Option<Vec<u8>>,
    metadata: Option<mac_metadata::Metadata>,
    directory: movement::Movement,
    rules: Vec<String>,
    issued: Instant,
    password: Option<password::Password>,
}
fn check(job: &Job, start: Instant) -> Result<(), String> {
    archive::check(job, start)
}
fn source(path: &Path, settings: &db::Settings) -> Result<AnchoredPath, String> {
    if !settings.acknowledged
        || unsafe { libc::geteuid() } == 0
        || !ordinary(path)
        || protected(path, &settings.protected_paths)?
        || Format::from_path(path).is_none()
    {
        return Err("请选择未受保护的本地 ZIP、TAR 或 TAR.GZ/TGZ 文件。".into());
    }
    let h = AnchoredPath::open(path)?;
    let m = h.target().metadata().map_err(|e| e.to_string())?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || !h.local_materialized()
        || m.len() > MAX_PACKED
    {
        return Err("压缩包必须是本账户的普通本地文件，压缩包最多 2 GiB。".into());
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
        return Err("压缩包隔离属性无法核对，未解压。".into());
    }
    if n > 65536 {
        return Err("压缩包隔离属性超过读取上限。".into());
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
        return Err("压缩包隔离属性在读取期间变化。".into());
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
    #[cfg(test)]
    fn prepare(
        root: &Path,
        directory: &str,
        filename: &str,
        settings: &db::Settings,
        job: &Job,
    ) -> Result<Self, String> {
        Self::prepare_mode(root, directory, filename, settings, job, false)
    }
    pub(super) fn prepare_mode(
        root: &Path,
        directory: &str,
        filename: &str,
        settings: &db::Settings,
        job: &Job,
        restore_metadata: bool,
    ) -> Result<Self, String> {
        Self::prepare_password(
            root,
            directory,
            filename,
            settings,
            job,
            restore_metadata,
            None,
        )
    }
    fn prepare_password(
        root: &Path,
        directory: &str,
        filename: &str,
        settings: &db::Settings,
        job: &Job,
        restore_metadata: bool,
        password: Option<password::Password>,
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
                return Err("压缩包在读取时变化，请重新检查。".into());
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
            return Err("压缩包在读取时变化，请重新检查。".into());
        }
        let format = Format::from_path(root).ok_or("不支持的压缩包格式。")?;
        if password.is_some() && format != Format::Zip {
            return Err("密码解压目前支持 ZIP；TAR 请清空密码后检查。".into());
        }
        if restore_metadata && format != Format::Zip {
            return Err("Mac 元数据恢复目前支持 ZIP；TAR 可选择仅解压正文。".into());
        }
        if format == Format::TarGzip {
            snapshot = tar::inflate(&mut snapshot, job, start)?;
        }
        let (mut entries, encrypted) = if format == Format::Zip {
            inventory::read_password(&mut snapshot, job, start, password.as_ref())?
        } else {
            (tar::read(&mut snapshot, job, start)?, false)
        };
        let bytes = entries.iter().map(|e| e.size).sum();
        let metadata = if restore_metadata {
            Some(mac_metadata::Metadata::read(
                &mut snapshot,
                &mut entries,
                job,
                start,
                password.as_ref(),
            )?)
        } else {
            None
        };
        let output_quarantine = if restore_metadata {
            Some(mac_metadata::fresh_quarantine()?)
        } else {
            q.clone()
        };
        capacity(parent.target(), bytes)?;
        let now = db::now();
        let draft = Draft {
            encrypted,
            format,
            mac_metadata: metadata.as_ref().map(|m| m.summary.clone()),
            id: uuid::Uuid::new_v4().to_string(),
            root: root.to_str().ok_or("压缩包路径无法编码。")?.into(),
            output_path: output.to_str().ok_or("解压路径无法编码。")?.into(),
            files: entries.iter().filter(|e| !e.directory).count(),
            directories: entries.iter().filter(|e| e.directory).count() + 1,
            hidden: entries
                .iter()
                .filter(|e| e.name.split('/').any(|s| s.starts_with('.')))
                .count(),
            bytes,
            metadata_bytes: if let Some(m) = &metadata {
                m.bytes
            } else {
                entries
                    .iter()
                    .filter(|e| {
                        e.name.starts_with("__MACOSX/")
                            || e.name.split('/').any(|s| s.starts_with("._"))
                    })
                    .map(|e| e.size)
                    .sum()
            },
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
            quarantine: output_quarantine,
            source_quarantine: q,
            metadata,
            directory: movement::Movement {
                directory: directory.into(),
                witness: parent.witness,
                conflict: movement::Conflict::Skip,
                targets: vec![],
            },
            rules: settings.protected_paths.clone(),
            issued: Instant::now(),
            password,
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
            return Err("原压缩包或祖先身份已变化，请重新检查。".into());
        }
        let mut f = archive::open_read(&held)?;
        let size = f.metadata().map_err(|e| e.to_string())?.len();
        if archive::digest(&mut f, job, start, size)? != self.draft.source_sha256
            || quarantine(&f)? != self.source_quarantine
        {
            return Err("原压缩包内容或隔离属性变化，未发布解压结果。".into());
        }
        held.validate()?;
        Ok(parent)
    }
}
pub(super) fn run(
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
        reason: "正在核对并解压压缩包，原压缩包保留。".into(),
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
            plan.draft.format,
            &plan.entries,
            plan.quarantine.as_deref(),
            job,
            start,
            plan.password.as_ref(),
        )?;
        s.verify(&plan.entries, plan.quarantine.as_deref(), job, start)?;
        if let Some(metadata) = &plan.metadata {
            s.restore(metadata, job, start)?;
            s.verify_metadata(
                &plan.entries,
                plan.quarantine.as_deref(),
                metadata,
                job,
                start,
            )?;
        }
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
        let mut artifact_hash = Sha256::new();
        artifact_hash.update(serde_json::to_vec(&manifest).map_err(|e| e.to_string())?);
        if let Some(metadata) = &plan.metadata {
            metadata.hash_into(&mut artifact_hash);
        }
        r.artifact = Some(Artifact {
            device: m.dev().to_string(),
            inode: m.ino().to_string(),
            bytes: plan.draft.bytes,
            sha256: format!("{:x}", artifact_hash.finalize()),
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
        let verification = Job::new(TaskKind::PersonalArchive);
        if let Some(metadata) = &plan.metadata {
            s.verify_metadata(
                &plan.entries,
                plan.quarantine.as_deref(),
                metadata,
                &verification,
                start,
            )?;
        } else {
            s.verify(
                &plan.entries,
                plan.quarantine.as_deref(),
                &verification,
                start,
            )?;
        }
        output.validate()?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            r.state = "completed".into();
            r.reason = if plan.metadata.is_some() {
                "ZIP 正文、Mac 数据属性、普通权限和修改时间已恢复并核验；原包保留，输出使用新的隔离标记，历史安全批准和所有者未恢复。"
            } else { "压缩包正文已解压并核验，原压缩包保留；原权限、时间、所有者与嵌入元数据未应用。" }.into();
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
