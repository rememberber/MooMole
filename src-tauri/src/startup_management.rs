//! User LaunchAgents only. Preserve plist bytes; move an exact selected file
//! out of launchd's .plist suffix, with separately journaled session requests.
use crate::{
    db,
    engine::Job,
    observation::{AnchoredPath, Witness},
    permissions,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    ffi::CString,
    fs::File,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc, Mutex},
    time::{Duration, Instant},
};
pub(crate) mod commands;
pub(crate) mod journal;
mod plist;
mod runtime;
const SUFFIX: &str = ".moomole-disabled";
#[derive(Clone)]
struct Context {
    home: PathBuf,
    settings: db::Settings,
    own_app: Option<PathBuf>,
}
impl Context {
    fn root(&self) -> PathBuf {
        self.home.join("Library/LaunchAgents")
    }
    fn protected(&self, path: &Path, label: Option<&str>) -> Result<bool, String> {
        // Treat alternate case as protected too, including on case-sensitive
        // volumes; this conservative guard never grants write authority.
        let folded = PathBuf::from(path.to_string_lossy().to_lowercase());
        let spelling_guard = folded.starts_with("/system")
            || self
                .own_app
                .as_ref()
                .is_some_and(|app| folded.starts_with(app.to_string_lossy().to_lowercase()))
            || self.settings.protected_paths.iter().any(|rule| {
                let r = PathBuf::from(rule.to_lowercase());
                folded.starts_with(&r) || r.starts_with(&folded)
            });
        Ok(!crate::app_protection::check(
            path.to_str().ok_or("路径无法编码。")?,
            label,
            self.own_app.as_deref(),
            "io.github.rememberber.moomole",
            &self.settings.protected_paths,
        )?
        .reasons
        .is_empty()
            || spelling_guard
            || self
                .own_app
                .as_ref()
                .is_some_and(|app| path.starts_with(app))
            || label.is_some_and(|s| {
                ["com.apple.", "com.openai.", "io.github.rememberber.moomole"]
                    .iter()
                    .any(|p| s.to_ascii_lowercase().starts_with(p))
            }))
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Definition {
    label: String,
    program: String,
    arguments: Vec<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Row {
    id: String,
    name: String,
    path: String,
    label: String,
    program: String,
    arguments: Vec<String>,
    paused: bool,
    eligible: bool,
    reason: String,
}
#[derive(Clone)]
struct Candidate {
    row: Row,
    witness: Witness,
    sha256: String,
    definition: Definition,
}
#[derive(Clone)]
struct Scan {
    report: Report,
    items: Vec<Candidate>,
    root: Witness,
    deadline: Instant,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    id: String,
    created_at: i64,
    expires_at: i64,
    root: String,
    complete: bool,
    cancelled: bool,
    skipped: usize,
    items: Vec<Row>,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Action {
    Pause,
    Resume,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Plan {
    id: String,
    scan_id: String,
    created_at: i64,
    expires_at: i64,
    action: Action,
    home: PathBuf,
    root: Witness,
    path: String,
    target: String,
    source: Witness,
    sha256: String,
    definition: Definition,
    program: Witness,
    program_sha256: String,
    expected: Option<runtime::Service>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Draft {
    id: String,
    scan_id: String,
    item_id: String,
    expires_at: i64,
    action: Action,
    path: String,
    target: String,
    label: String,
    program: String,
    arguments: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Record {
    id: String,
    request_id: String,
    created_at: i64,
    finished_at: Option<i64>,
    action: Action,
    path: String,
    target: String,
    label: String,
    program: String,
    state: String,
    phase: String,
    configuration_changed: bool,
    receipt: Option<crate::execution::Receipt>,
    moved_witness: Option<Witness>,
    sha256: String,
    reason: String,
    reviewed_at: Option<i64>,
}
#[derive(Default)]
pub(crate) struct Store {
    scan: Option<Scan>,
    plans: HashMap<String, (Plan, Instant)>,
}
fn root(ctx: &Context) -> Result<AnchoredPath, String> {
    if !ctx.settings.acknowledged
        || unsafe { libc::geteuid() } == 0
        || unsafe { libc::geteuid() } != unsafe { libc::getuid() }
        || ctx.protected(&ctx.root(), None)?
    {
        return Err("登录配置范围受保护或账户无法核对。".into());
    }
    let held = AnchoredPath::open(&ctx.root())?;
    let m = held.target().metadata().map_err(|e| e.to_string())?;
    if !m.is_dir()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o022 != 0
        || !held.local_materialized()
    {
        return Err("用户登录目录不是本账户的本地物理目录，或允许其他账户改写。".into());
    }
    Ok(held)
}
fn read(path: &Path, limit: u64, job: &Job) -> Result<(Vec<u8>, Witness), String> {
    if job.cancelled.load(Ordering::SeqCst) {
        return Err("启动管理已停止，未继续操作。".into());
    }
    let held = AnchoredPath::open(path)?;
    let m = held.target().metadata().map_err(|e| e.to_string())?;
    if !m.is_file() || m.nlink() != 1 || !held.local_materialized() || m.len() > limit {
        return Err("配置或程序不是受支持的本地普通文件。".into());
    }
    let name = CString::new(
        path.file_name()
            .and_then(|s| s.to_str())
            .ok_or("文件名无法编码。")?,
    )
    .map_err(|e| e.to_string())?;
    let fd = unsafe {
        libc::openat(
            held.parent().as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err("配置或程序不可读取，保持原样。".into());
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let observed = file.metadata().map_err(|e| e.to_string())?;
    if (observed.dev(), observed.ino()) != (m.dev(), m.ino()) {
        return Err("读取前身份变化。".into());
    }
    let started = Instant::now();
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 128 * 1024];
    loop {
        if job.cancelled.load(Ordering::SeqCst) || started.elapsed() > Duration::from_secs(10) {
            return Err("读取已停止或超过本轮预算。".into());
        }
        let n = file.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if bytes.len() as u64 + n as u64 > limit {
            return Err("读取范围变化。".into());
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    held.validate()?;
    if bytes.len() as u64 != m.len() {
        return Err("读取期间大小变化。".into());
    }
    Ok((bytes, held.witness))
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn scan(ctx: &Context, records: &[Record], job: &Job) -> Result<Scan, String> {
    let held = root(ctx)?;
    let started = Instant::now();
    let mut items = vec![];
    let mut skipped = 0;
    let mut complete = true;
    let mut labels = HashMap::new();
    let mut read_bytes = 0;
    for (index, entry) in std::fs::read_dir(ctx.root())
        .map_err(|e| e.to_string())?
        .enumerate()
    {
        if read_bytes >= 8 * 1024 * 1024
            || index >= 200
            || started.elapsed() > Duration::from_secs(10)
            || job.cancelled.load(Ordering::SeqCst)
        {
            complete = false;
            break;
        }
        let path = match entry {
            Ok(e) => e.path(),
            Err(_) => {
                complete = false;
                skipped += 1;
                continue;
            }
        };
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            complete = false;
            skipped += 1;
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if !(name.ends_with(".plist") || name.ends_with(&format!(".plist{SUFFIX}"))) {
            continue;
        }
        let candidate = (|| -> Result<Candidate, String> {
            let (bytes, witness) = read(&path, 512 * 1024, job)?;
            read_bytes += bytes.len();
            if read_bytes > 8 * 1024 * 1024 {
                return Err("配置总量超过本轮范围。".into());
            }
            let definition = plist::parse(&bytes)?;
            *labels.entry(definition.label.clone()).or_insert(0usize) += 1;
            let sha256 = hash(&bytes);
            let paused = name.ends_with(SUFFIX);
            let eligible = !ctx.protected(&path, Some(&definition.label))?
                && !ctx.protected(Path::new(&definition.program), Some(&definition.label))?
                && witness
                    .nodes
                    .last()
                    .is_some_and(|n| n.uid == unsafe { libc::geteuid() } && n.mode & 0o022 == 0)
                && (!paused || journal::resume_proof(records, &path, &witness, &sha256));
            Ok(Candidate {
                row: Row {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: name
                        .trim_end_matches(SUFFIX)
                        .trim_end_matches(".plist")
                        .into(),
                    path: path.to_str().ok_or("路径无法编码。")?.into(),
                    label: definition.label.clone(),
                    program: definition.program.clone(),
                    arguments: definition.arguments.clone(),
                    paused,
                    eligible,
                    reason: if eligible {
                        ""
                    } else if paused {
                        "缺少可核对的暂停收据或配置已变化，请先核对记录。"
                    } else {
                        "配置、程序或账户受到保护，仅供查看。"
                    }
                    .into(),
                },
                witness,
                sha256,
                definition,
            })
        })();
        match candidate {
            Ok(item) => items.push(item),
            Err(_) => {
                skipped += 1;
                complete = false;
            }
        }
    }
    for item in &mut items {
        if labels.get(&item.definition.label).copied().unwrap_or(0) > 1 {
            item.row.eligible = false;
            item.row.reason = "发现同标签配置，归属不明确，本轮保留。".into();
        }
    }
    // Identity is stable while timestamps can change as other software updates its entries.
    let current = root(ctx)?;
    if current.witness.nodes != held.witness.nodes {
        return Err("登录目录已变化，请重新读取。".into());
    }
    items.sort_by(|a, b| a.row.name.cmp(&b.row.name));
    let now = db::now();
    let report = Report {
        id: uuid::Uuid::new_v4().to_string(),
        created_at: now,
        expires_at: now + 5 * 60 * 1000,
        root: ctx.root().to_string_lossy().into_owned(),
        complete,
        cancelled: job.cancelled.load(Ordering::SeqCst),
        skipped,
        items: items.iter().map(|i| i.row.clone()).collect(),
    };
    Ok(Scan {
        report,
        items,
        root: held.witness,
        deadline: Instant::now() + Duration::from_secs(5 * 60),
    })
}
fn prepare(
    scan: &Scan,
    item_id: &str,
    action: Action,
    ctx: &Context,
    records: &[Record],
    job: &Job,
) -> Result<(Plan, Draft), String> {
    if db::now() < scan.report.created_at
        || db::now() >= scan.report.expires_at
        || Instant::now() >= scan.deadline
    {
        return Err("登录配置清单已过期，请重新读取。".into());
    }
    let item = scan
        .items
        .iter()
        .find(|i| i.row.id == item_id)
        .ok_or("选择不属于当前清单。")?;
    if !item.row.eligible || item.row.paused != (action == Action::Resume) {
        return Err("此配置不可执行所选操作。".into());
    }
    if action == Action::Resume
        && !journal::resume_proof(
            records,
            Path::new(&item.row.path),
            &item.witness,
            &item.sha256,
        )
    {
        return Err("暂停收据不再适用，请核对原记录。".into());
    }
    let path = Path::new(&item.row.path);
    let target = if action == Action::Pause {
        format!("{}{SUFFIX}", item.row.path)
    } else {
        item.row
            .path
            .strip_suffix(SUFFIX)
            .ok_or("暂停名称无法核对。")?
            .into()
    };
    if Path::new(&target)
        .file_name()
        .unwrap()
        .as_encoded_bytes()
        .len()
        > 255
        || !matches!(std::fs::symlink_metadata(&target),Err(e) if e.kind()==std::io::ErrorKind::NotFound)
    {
        return Err("目标名称已存在或无法核对；不会覆盖。".into());
    }
    let (program_bytes, program) =
        read(Path::new(&item.definition.program), 256 * 1024 * 1024, job)?;
    let node = program.nodes.last().ok_or("缺少程序身份。")?;
    if ![0, unsafe { libc::geteuid() }].contains(&node.uid)
        || node.mode & 0o022 != 0
        || node.mode & 0o111 == 0
    {
        return Err("启动程序账户或权限无法核对。".into());
    }
    let expected = runtime::query(&item.definition.label)?;
    runtime::matches(&expected, &item.definition)?;
    let now = db::now();
    let p = Plan {
        id: uuid::Uuid::new_v4().to_string(),
        scan_id: scan.report.id.clone(),
        created_at: now,
        expires_at: now + 30_000,
        action,
        home: ctx.home.clone(),
        root: scan.root.clone(),
        path: path.to_str().unwrap().into(),
        target,
        source: item.witness.clone(),
        sha256: item.sha256.clone(),
        definition: item.definition.clone(),
        program,
        program_sha256: hash(&program_bytes),
        expected,
    };
    validate(&p, ctx, job, false, true)?;
    let draft = Draft {
        id: p.id.clone(),
        scan_id: p.scan_id.clone(),
        item_id: item_id.into(),
        expires_at: p.expires_at,
        action,
        path: p.path.clone(),
        target: p.target.clone(),
        label: p.definition.label.clone(),
        program: p.definition.program.clone(),
        arguments: p.definition.arguments.clone(),
    };
    Ok((p, draft))
}
fn validate(
    p: &Plan,
    ctx: &Context,
    job: &Job,
    moved: bool,
    check_runtime: bool,
) -> Result<(), String> {
    if p.home != ctx.home
        || ctx.protected(Path::new(&p.path), Some(&p.definition.label))?
        || ctx.protected(Path::new(&p.target), Some(&p.definition.label))?
        || ctx.protected(Path::new(&p.definition.program), Some(&p.definition.label))?
    {
        return Err("账户或保护范围变化，本项保留。".into());
    }
    if root(ctx)?.witness.nodes != p.root.nodes {
        return Err("登录目录或祖先变化。".into());
    }
    let at = if moved { &p.target } else { &p.path };
    let (bytes, witness) = read(Path::new(at), 512 * 1024, job)?;
    if hash(&bytes) != p.sha256
        || (!moved && witness != p.source)
        || witness.nodes.last().map(|n| (n.dev, n.ino, n.uid, n.mode))
            != p.source.nodes.last().map(|n| (n.dev, n.ino, n.uid, n.mode))
    {
        return Err("登录配置身份或内容变化，保持当前状态。".into());
    }
    let (bytes, witness) = read(Path::new(&p.definition.program), 256 * 1024 * 1024, job)?;
    if witness != p.program || hash(&bytes) != p.program_sha256 {
        return Err("启动程序已变化，请重新读取。".into());
    }
    if moved
        && !matches!(std::fs::symlink_metadata(&p.path),Err(e) if e.kind()==std::io::ErrorKind::NotFound)
    {
        return Err("原路径已重新出现，后台状态保持未知。".into());
    }
    if check_runtime && runtime::query(&p.definition.label)? != p.expected {
        return Err("后台注册已变化，请重新读取。".into());
    }
    Ok(())
}
fn move_config(p: &Plan, ctx: &Context, job: &Job) -> Result<Witness, String> {
    validate(p, ctx, job, false, true)?;
    let held = AnchoredPath::open(Path::new(&p.path))?;
    let access = permissions::observe(&held, &permissions::Budget::new())?;
    if access.code != permissions::Code::NoRestrictionObserved {
        return Err(access.refusal_reason());
    }
    let from = CString::new(Path::new(&p.path).file_name().unwrap().as_encoded_bytes())
        .map_err(|e| e.to_string())?;
    let to = CString::new(Path::new(&p.target).file_name().unwrap().as_encoded_bytes())
        .map_err(|e| e.to_string())?;
    if job.cancelled.load(Ordering::SeqCst) {
        return Err("配置更改前已取消。".into());
    }
    held.validate()?;
    if unsafe {
        libc::renameatx_np(
            held.parent().as_raw_fd(),
            from.as_ptr(),
            held.parent().as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_EXCL,
        )
    } != 0
    {
        return Err("系统未完成配置更名，未覆盖目标；请核对当前路径。".into());
    }
    let after = AnchoredPath::open(Path::new(&p.target))?;
    if after
        .witness
        .nodes
        .last()
        .map(|n| (n.dev, n.ino, n.uid, n.mode))
        != p.source.nodes.last().map(|n| (n.dev, n.ino, n.uid, n.mode))
        || after.witness.size != p.source.size
    {
        return Err("配置更名后的身份无法核对，请检查两个位置。".into());
    }
    held.parent()
        .sync_all()
        .map_err(|_| "配置目录落盘状态未知，请核对位置。")?;
    Ok(after.witness)
}
fn run(
    database: Arc<Mutex<db::Database>>,
    p: Plan,
    request_id: String,
    ctx: impl Fn() -> Result<Context, String>,
    job: Arc<Job>,
) -> Result<Record, String> {
    let mut r = Record {
        id: p.id.clone(),
        request_id,
        created_at: db::now(),
        finished_at: None,
        action: p.action,
        path: p.path.clone(),
        target: p.target.clone(),
        label: p.definition.label.clone(),
        program: p.definition.program.clone(),
        state: "running".into(),
        phase: "configurationIntent".into(),
        configuration_changed: false,
        receipt: None,
        moved_witness: None,
        sha256: p.sha256.clone(),
        reason: String::new(),
        reviewed_at: None,
    };
    if db::now() < p.created_at || db::now() >= p.expires_at {
        return Err("启动管理确认已过期。".into());
    }
    validate(&p, &ctx()?, &job, false, true)?;
    journal::insert(&mut *database.lock().map_err(|e| e.to_string())?, &p, &r)?;
    let mut attempted_move = false;
    let result = (|| -> Result<(), String> {
        if job.cancelled.load(Ordering::SeqCst) {
            return Err("更改前已取消，配置保留。".into());
        }
        attempted_move = true;
        let witness = move_config(&p, &ctx()?, &job)?;
        r.configuration_changed = true;
        r.moved_witness = Some(witness.clone());
        let n = witness.nodes.last().unwrap();
        r.receipt = Some(crate::execution::Receipt {
            destination: p.target.clone(),
            device: n.dev,
            inode: n.ino,
        });
        r.phase = "configurationChanged".into();
        journal::save(&*database.lock().map_err(|e| e.to_string())?, &r)?;
        validate(&p, &ctx()?, &job, true, true)?;
        if (p.action == Action::Pause && p.expected.is_some())
            || (p.action == Action::Resume && p.expected.is_none())
        {
            r.phase = "serviceIntent".into();
            journal::save(&*database.lock().map_err(|e| e.to_string())?, &r)?;
            let attempt =
                runtime::request(p.action, &p.definition.label, Path::new(&p.target), &job)?;
            if attempt.exit_code != Some(0) || attempt.stop.is_some() {
                return Err("配置已更改，后台请求未取得完整完成证据；不会自动重试。".into());
            }
        }
        validate(&p, &ctx()?, &job, true, false)?;
        r.moved_witness = Some(AnchoredPath::open(Path::new(&p.target))?.witness);
        let observed = runtime::query(&p.definition.label)?;
        if p.action == Action::Pause && observed.is_some() {
            return Err("登录配置已暂停，但仍观察到后台注册，请核对。".into());
        }
        if p.action == Action::Resume {
            runtime::matches(&observed, &p.definition)?;
            if observed.is_none() {
                return Err("登录配置已恢复，尚未确认后台注册。".into());
            }
        }
        Ok(())
    })();
    r.finished_at = Some(db::now());
    r.phase = "finished".into();
    match result {
        Ok(()) => {
            r.state = "completed".into();
            r.reason = if p.action == Action::Pause {
                "此登录配置已暂停，本次未再观察到该后台注册；应用可能自行重新注册。"
            } else {
                "此登录配置已恢复，并观察到相同程序的后台注册；不代表应用功能已验收。"
            }
            .into();
        }
        Err(e) => {
            r.state = if r.configuration_changed {
                "partial"
            } else if attempted_move {
                "uncertain"
            } else if job.cancelled.load(Ordering::SeqCst) {
                "cancelled"
            } else {
                "failed"
            }
            .into();
            r.reason = e;
        }
    }
    journal::save(&*database.lock().map_err(|e| e.to_string())?, &r)?;
    Ok(r)
}
#[cfg(test)]
mod tests;
