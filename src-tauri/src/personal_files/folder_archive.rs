//! Explicit whole-folder ZIP with AppleDouble metadata. Never deletes sources.
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
    path::PathBuf,
    sync::{atomic::Ordering, Arc, Mutex},
};
pub(crate) mod commands;
pub(super) mod journal;
mod source;
#[cfg(test)]
mod tests;
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
pub(super) struct Plan {
    draft: Draft,
    witness: Witness,
    nodes: Vec<source::Node>,
    directory: movement::Movement,
    rules: Vec<String>,
    issued: Instant,
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
impl Plan {
    fn prepare(
        root: &Path,
        directory: &str,
        filename: &str,
        settings: &db::Settings,
        job: &Job,
    ) -> Result<Self, String> {
        let started = Instant::now();
        if !settings.acknowledged
            || unsafe { libc::geteuid() } == 0
            || filename.len() > 200
            || !filename.ends_with(".zip")
            || filename.contains(['/', '\\', ':'])
        {
            return Err("请选择普通账户、完整文件夹、现有目标和 .zip 文件名。".into());
        }
        let parent = movement::directory(directory, settings)?;
        if Path::new(directory).starts_with(root) {
            return Err("归档目标不能位于来源文件夹内。".into());
        }
        let output = Path::new(directory).join(filename);
        if !ordinary(&output)
            || protected(&output, &settings.protected_paths)?
            || !matches!(std::fs::symlink_metadata(&output),Err(e)if e.kind()==std::io::ErrorKind::NotFound)
        {
            return Err("归档名称已存在、受保护或无法核对，请另选名称；不会覆盖。".into());
        }
        let (witness, nodes) = source::inventory(root, &settings.protected_paths, job, started)?;
        let source_sha256 = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(&witness, &nodes)).map_err(|e| e.to_string())?)
        );
        let now = db::now();
        let draft = Draft {
            id: uuid::Uuid::new_v4().to_string(),
            root: root.to_str().ok_or("归档源目录无法编码。")?.into(),
            output_path: output.to_str().ok_or("归档目标无法编码。")?.into(),
            files: nodes.iter().filter(|n| !n.directory).count(),
            directories: nodes.iter().filter(|n| n.directory).count(),
            hidden: nodes
                .iter()
                .filter(|n| n.name.split('/').skip(1).any(|p| p.starts_with('.')))
                .count(),
            bytes: nodes.iter().map(|n| n.size).sum(),
            metadata_bytes: nodes.iter().map(|n| n.metadata_bytes).sum(),
            source_sha256,
            samples: nodes
                .iter()
                .take(50)
                .map(|n| Sample {
                    name: n.name.clone(),
                    directory: n.directory,
                })
                .collect(),
            created_at: now,
            expires_at: now + 120_000,
        };
        capacity(&parent, draft.bytes.saturating_add(draft.metadata_bytes))?;
        Ok(Self {
            draft,
            witness,
            nodes,
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
            Err("文件夹归档确认已过期，请重新检查。".into())
        } else {
            Ok(())
        }
    }
    fn validate(
        &self,
        settings: &db::Settings,
        job: &Job,
        started: Instant,
    ) -> Result<AnchoredPath, String> {
        if !settings.acknowledged
            || settings.protected_paths != self.rules
            || protected(
                Path::new(&self.draft.output_path),
                &settings.protected_paths,
            )?
        {
            return Err("归档保护设置已变化，整份未发布。".into());
        }
        let parent = self.directory.validate_directory(settings)?;
        capacity(&parent, 0)?;
        if !matches!(std::fs::symlink_metadata(&self.draft.output_path),Err(e)if e.kind()==std::io::ErrorKind::NotFound)
        {
            return Err("归档目标已存在或无法核对，不覆盖。".into());
        }
        let (witness, nodes) = source::inventory(
            Path::new(&self.draft.root),
            &settings.protected_paths,
            job,
            started,
        )?;
        if witness != self.witness || nodes != self.nodes {
            return Err("文件夹内容、层级或元数据已变化，请重新准备整份归档。".into());
        }
        Ok(parent)
    }
}
fn capacity(parent: &AnchoredPath, bytes: u64) -> Result<(), String> {
    let mut s = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    if unsafe { libc::fstatfs(parent.target().as_raw_fd(), s.as_mut_ptr()) } != 0 {
        return Err("归档目标可用空间无法核对。".into());
    }
    let s = unsafe { s.assume_init() };
    if (s.f_bavail as u64).saturating_mul(s.f_bsize as u64) < bytes.saturating_add(64 * 1024 * 1024)
    {
        return Err("归档目标空间不足，需保留内容、元数据及 64 MiB 余量。".into());
    }
    Ok(())
}
fn options(n: &source::Node) -> Result<zip::write::FullFileOptions<'static>, String> {
    let seconds = n.stamp[6] as libc::time_t;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::zeroed();
    if unsafe { libc::localtime_r(&seconds, tm.as_mut_ptr()) }.is_null() {
        return Err("ZIP 修改时间无法核对。".into());
    }
    let tm = unsafe { tm.assume_init() };
    let date = zip::DateTime::from_date_and_time(
        (tm.tm_year + 1900) as u16,
        (tm.tm_mon + 1) as u8,
        tm.tm_mday as u8,
        tm.tm_hour as u8,
        tm.tm_min as u8,
        tm.tm_sec as u8,
    )
    .unwrap_or_default();
    let mut options = zip::write::FullFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(n.stamp[2] as u32 & 0o777)
        .last_modified_time(date)
        .large_file(n.size >= u32::MAX as u64);
    let mut ut = vec![1];
    ut.extend((seconds as u32).to_le_bytes());
    options
        .add_extra_data(0x5455, ut.into_boxed_slice(), false)
        .map_err(|e| e.to_string())?;
    // macOS ditto emits and consumes the original Info-ZIP Unix field (0x5855)
    // for second-precision time. Its reader ignores UT in the native probe.
    let mut unix = n.accessed_at.to_le_bytes().to_vec();
    unix.extend((seconds as u32).to_le_bytes());
    options
        .add_extra_data(0x5855, unix.into_boxed_slice(), false)
        .map_err(|e| e.to_string())?;
    Ok(options)
}
fn build(plan: &Plan, file: &mut File, job: &Job, started: Instant) -> Result<(), String> {
    let mut zip = zip::ZipWriter::new(&mut *file);
    let mut contents = Vec::new();
    let base = Path::new(&plan.draft.root)
        .parent()
        .ok_or("归档根目录缺少父目录。")?;
    // Native extractors need explicit directories before their descendants.
    for n in &plan.nodes {
        archive::check(job, started)?;
        let path = base.join(&n.name);
        let held = AnchoredPath::open(&path)?;
        if !held.witness.nodes.starts_with(&plan.witness.nodes) {
            return Err("来源祖先身份已变化，整份未发布。".into());
        }
        let name = CString::new(
            path.file_name()
                .and_then(|s| s.to_str())
                .ok_or("归档内部路径无效。")?,
        )
        .map_err(|e| e.to_string())?;
        let mut input = source::open(held.parent(), &name, n.directory)?;
        if source::stamp(&input.metadata().map_err(|e| e.to_string())?) != n.stamp
            || source::attributes(&input)?.0 != n.metadata_hash
        {
            return Err("归档来源已变化，整份未发布。".into());
        }
        let entry = if n.directory {
            format!("{}/", n.name)
        } else {
            n.name.clone()
        };
        if n.directory {
            zip.add_directory(&entry, options(n)?)
                .map_err(|e| e.to_string())?;
        } else {
            zip.start_file(&entry, options(n)?)
                .map_err(|e| e.to_string())?;
            let mut hash = Sha256::new();
            let mut count = 0u64;
            let mut buf = [0u8; 128 * 1024];
            loop {
                archive::check(job, started)?;
                let size = input.read(&mut buf).map_err(|e| e.to_string())?;
                if size == 0 {
                    break;
                }
                count = count.saturating_add(size as u64);
                if count > n.size {
                    return Err("文件夹归档来源增长，整份未发布。".into());
                }
                zip.write_all(&buf[..size]).map_err(|e| e.to_string())?;
                hash.update(&buf[..size]);
            }
            if count != n.size || format!("{:x}", hash.finalize()) != n.hash {
                return Err("文件夹归档来源内容不匹配，整份未发布。".into());
            }
            input.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        }
        contents.push((entry, n.size, n.hash.clone(), n.directory));
        if source::stamp(&input.metadata().map_err(|e| e.to_string())?) != n.stamp
            || source::attributes(&input)?.0 != n.metadata_hash
        {
            return Err("来源元数据在归档时变化，整份未发布。".into());
        }
        held.validate()?;
    }
    // AppleDouble entries follow the complete ordinary tree. Interleaving
    // metadata makes native ditto lose a populated root directory's mtime.
    for n in &plan.nodes {
        archive::check(job, started)?;
        let path = base.join(&n.name);
        let held = AnchoredPath::open(&path)?;
        if !held.witness.nodes.starts_with(&plan.witness.nodes) {
            return Err("来源祖先身份已变化，整份未发布。".into());
        }
        let name = CString::new(
            path.file_name()
                .and_then(|s| s.to_str())
                .ok_or("归档内部路径无效。")?,
        )
        .map_err(|e| e.to_string())?;
        let input = source::open(held.parent(), &name, n.directory)?;
        if source::stamp(&input.metadata().map_err(|e| e.to_string())?) != n.stamp
            || source::attributes(&input)?.0 != n.metadata_hash
        {
            return Err("归档来源已变化，整份未发布。".into());
        }
        let mut metadata = source::packed(&input)?;
        let size = metadata.metadata().map_err(|e| e.to_string())?.len();
        metadata
            .seek(SeekFrom::Start(0))
            .map_err(|e| e.to_string())?;
        let (parent, leaf) = n
            .name
            .rsplit_once('/')
            .map_or(("", n.name.as_str()), |(a, b)| (a, b));
        let entry = if parent.is_empty() {
            format!("__MACOSX/._{leaf}")
        } else {
            format!("__MACOSX/{parent}/._{leaf}")
        };
        zip.start_file(
            &entry,
            options(n)?.unix_permissions(0o600).large_file(false),
        )
        .map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let mut count = 0;
        let mut buf = [0u8; 128 * 1024];
        loop {
            archive::check(job, started)?;
            let got = metadata.read(&mut buf).map_err(|e| e.to_string())?;
            if got == 0 {
                break;
            }
            count += got as u64;
            if count > size {
                return Err("元数据封装在读取时变化。".into());
            }
            zip.write_all(&buf[..got]).map_err(|e| e.to_string())?;
            hash.update(&buf[..got]);
        }
        if count != size {
            return Err("元数据未完整写入，整份未发布。".into());
        }
        contents.push((entry, size, format!("{:x}", hash.finalize()), false));
        if source::stamp(&input.metadata().map_err(|e| e.to_string())?) != n.stamp
            || source::attributes(&input)?.0 != n.metadata_hash
        {
            return Err("来源元数据在归档时变化，整份未发布。".into());
        }
        held.validate()?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } != 0 {
        return Err("整份归档同步未完成，未发布。".into());
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut verify = zip::ZipArchive::new(&mut *file).map_err(|e| e.to_string())?;
    if verify.len() != contents.len() {
        return Err("归档内部项目数不匹配，未发布。".into());
    }
    for (i, (name, size, hash, dir)) in contents.iter().enumerate() {
        let mut entry = verify.by_index(i).map_err(|e| e.to_string())?;
        if entry.name() != name || entry.size() != *size || entry.is_dir() != *dir {
            return Err("归档内部结构不匹配，未发布。".into());
        }
        if !dir && archive::digest(&mut entry, job, started, *size)? != *hash {
            return Err("归档内容或元数据校验不匹配，未发布。".into());
        }
    }
    Ok(())
}
fn run(
    database: &Mutex<db::Database>,
    plan: Plan,
    request_id: String,
    job: &Job,
) -> Result<Record, String> {
    let started = Instant::now();
    plan.time()?;
    let temporary_name = format!(".MooMole-folder-archive-{}.partial", plan.draft.id);
    let mut record = Record {
        id: plan.draft.id.clone(),
        request_id,
        draft: plan.draft.clone(),
        temporary_path: Path::new(&plan.directory.directory)
            .join(&temporary_name)
            .to_string_lossy()
            .into_owned(),
        created_at: db::now(),
        finished_at: None,
        state: "running".into(),
        phase: "building".into(),
        artifact: None,
        reason: "正在创建整份文件夹归档，原文件保留。".into(),
        reviewed_at: None,
    };
    {
        let mut db = database.lock().map_err(|e| e.to_string())?;
        journal::begin(&mut db, &record)?;
    }
    let result = (|| -> Result<(), String> {
        let settings = db::settings(&*database.lock().map_err(|e| e.to_string())?)?;
        let parent = plan.validate(&settings, job, started)?;
        capacity(
            &parent,
            plan.draft.bytes.saturating_add(plan.draft.metadata_bytes),
        )?;
        let mut temporary = archive::create(&parent, &temporary_name)?;
        build(&plan, &mut temporary.file, job, started)?;
        let meta = temporary.file.metadata().map_err(|e| e.to_string())?;
        temporary
            .file
            .seek(SeekFrom::Start(0))
            .map_err(|e| e.to_string())?;
        let hash = archive::digest(&mut temporary.file, job, started, meta.len())?;
        let settings = db::settings(&*database.lock().map_err(|e| e.to_string())?)?;
        let current = plan.validate(&settings, job, started)?;
        if current.witness.nodes != parent.witness.nodes
            || meta.nlink() != 1
            || archive::stamp(&meta)
                != archive::stamp(&temporary.file.metadata().map_err(|e| e.to_string())?)
        {
            return Err("归档临时文件或目标身份变化，未发布。".into());
        }
        let observed = AnchoredPath::open(Path::new(&record.temporary_path))?;
        let m = observed.target().metadata().map_err(|e| e.to_string())?;
        if (m.dev(), m.ino(), m.mode(), m.len(), m.nlink())
            != (meta.dev(), meta.ino(), meta.mode(), meta.len(), 1)
            || observed.witness.nodes[..observed.witness.nodes.len() - 1] != parent.witness.nodes
        {
            return Err("归档临时名称身份变化，未发布。".into());
        }
        observed.validate()?;
        archive::check(job, started)?;
        record.artifact = Some(Artifact {
            device: meta.dev().to_string(),
            inode: meta.ino().to_string(),
            bytes: meta.len(),
            sha256: hash,
        });
        record.phase = "publishing".into();
        record.reason = "即将发布已核验的完整目录和元数据 ZIP。".into();
        journal::save(&*database.lock().map_err(|e| e.to_string())?, &record)?;
        let name = CString::new(
            Path::new(&plan.draft.output_path)
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or("归档名称无效。")?,
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
            return Err("整份归档发布未确认，请核对目标；不会覆盖或重试。".into());
        }
        parent.target().sync_all().map_err(|e| e.to_string())?;
        let published = AnchoredPath::open(Path::new(&plan.draft.output_path))?;
        let m = published.target().metadata().map_err(|e| e.to_string())?;
        let mut input = archive::open_read(&published)?;
        let verify = Job::new(TaskKind::PersonalArchive);
        if (m.dev(), m.ino(), m.len()) != (meta.dev(), meta.ino(), meta.len())
            || archive::digest(&mut input, &verify, started, m.len())?
                != record.artifact.as_ref().unwrap().sha256
        {
            return Err("发布后的完整归档未通过核对，请保留目标检查。".into());
        }
        published.validate()?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            record.state = "completed".into();
            record.reason =
                "文件夹 ZIP 已创建，内容和 AppleDouble 元数据已逐项核验，原文件保留。".into();
        }
        Err(e) => {
            record.state = if record.phase == "publishing" {
                "unknown"
            } else if job.cancelled.load(Ordering::SeqCst) {
                "cancelled"
            } else {
                "retained"
            }
            .into();
            record.reason = e;
        }
    }
    record.phase = "finished".into();
    record.finished_at = Some(db::now().max(record.created_at));
    journal::save(&*database.lock().map_err(|e| e.to_string())?, &record)?;
    Ok(record)
}
