//! Explicit standing authorization for bounded recurring file processing.
//! Kept separate from observation-only folder schedules and their IPC.
use super::*;
use crate::{
    db,
    engine::{Job, TaskKind},
    observation::Node,
    rules,
};
use std::{
    os::fd::AsRawFd,
    os::unix::fs::MetadataExt,
    path::PathBuf,
    sync::{atomic::Ordering, Arc, Mutex},
};
mod journal;
pub(crate) mod runtime;
pub(crate) const MODULE: &str = "scheduledFiles";
pub(super) const REVISION: &str = "MooMole.scheduled-files.v1";
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Action {
    Move,
    Trash,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Rule {
    pub root: String,
    pub action: Action,
    pub destination: Option<String>,
    pub extensions: Vec<String>,
    pub minimum_age_days: u32,
    pub maximum_files: usize,
    pub maximum_bytes: u64,
    pub interval_minutes: u32,
}
impl Rule {
    fn validate(&self) -> Result<(), String> {
        if !ordinary(Path::new(&self.root))
            || self.root.len() > 4096
            || !(30..=3650).contains(&self.minimum_age_days)
            || !(1..=20).contains(&self.maximum_files)
            || !(1..=1_073_741_824).contains(&self.maximum_bytes)
            || ![60, 360, 1440].contains(&self.interval_minutes)
            || self.extensions.is_empty()
            || self.extensions.len() > 8
            || self.extensions.windows(2).any(|w| w[0] >= w[1])
            || self.extensions.iter().any(|s| {
                s.is_empty()
                    || s.len() > 12
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            })
        {
            return Err(
                "请选择具体目录、30 天以上的修改日期条件、确切扩展名及有效处理上限。".into(),
            );
        }
        match (self.action, &self.destination) {
            (Action::Trash, None) => Ok(()),
            (Action::Move, Some(p)) if ordinary(Path::new(p)) && !overlap(&self.root, p) => Ok(()),
            _ => Err("移动目标须为源目录之外的独立普通目录；废纸篓操作不接受其他目标。".into()),
        }
    }
    fn matches(&self, item: &Item, now: i64) -> bool {
        let cutoff = now.saturating_sub(self.minimum_age_days as i64 * 86_400_000);
        item.modified_at > 0
            && item.modified_at <= cutoff
            && item.size <= self.maximum_bytes
            && Path::new(&item.path).starts_with(&self.root)
            && item.path != self.root
            && Path::new(&item.path)
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| self.extensions.contains(&s.to_ascii_lowercase()))
    }
}
fn overlap(a: &str, b: &str) -> bool {
    Path::new(a).starts_with(b) || Path::new(b).starts_with(a)
}
fn nodes_overlap(a: &[Node], b: &[Node]) -> bool {
    a.last()
        .is_some_and(|last| b.iter().any(|n| (n.dev, n.ino) == (last.dev, last.ino)))
        || b.last()
            .is_some_and(|last| a.iter().any(|n| (n.dev, n.ino) == (last.dev, last.ino)))
}
pub(crate) fn allow_restoration(database: &db::Database, directory: &[Node]) -> Result<(), String> {
    if journal::policies(database)?.iter().any(|p| {
        p.policy.enabled
            && p.nodes.last().is_some_and(|root| {
                directory
                    .iter()
                    .any(|n| (n.dev, n.ino) == (root.dev, root.ino))
            })
    }) {
        return Err("原位置仍有启用的定期处理规则，请先暂停，避免恢复后再次处理。".into());
    }
    Ok(())
}
fn bind(
    rule: &Rule,
    home: &Path,
    settings: &db::Settings,
) -> Result<(Vec<Node>, Option<Vec<Node>>), String> {
    rule.validate()?;
    if !settings.acknowledged
        || unsafe { libc::geteuid() } == 0
        || unsafe { libc::geteuid() } != unsafe { libc::getuid() }
        || home.starts_with(&rule.root)
    {
        return Err("定期处理仅支持当前普通账户明确选择的具体文件夹，不接受账户根目录。".into());
    }
    let root = movement::directory(&rule.root, settings)?;
    let root_meta = root
        .target()
        .metadata()
        .map_err(|_| "定期目录所在卷无法核对。")?;
    let home_meta = std::fs::metadata(home).map_err(|_| "当前账户目录无法核对。")?;
    if (root_meta.dev(), root_meta.ino()) == (home_meta.dev(), home_meta.ino()) {
        return Err("定期处理不接受账户根目录，请选择具体文件夹。".into());
    }
    let destination = rule
        .destination
        .as_ref()
        .map(|p| movement::directory(p, settings))
        .transpose()?;
    for directory in std::iter::once(&root).chain(destination.iter()) {
        let mut raw = std::mem::MaybeUninit::<libc::statfs>::zeroed();
        if unsafe { libc::fstatfs(directory.target().as_raw_fd(), raw.as_mut_ptr()) } != 0
            || unsafe { raw.assume_init() }.f_flags & libc::MNT_RDONLY as u32 != 0
        {
            return Err("所选卷为只读或无法核对写入状态，不能启用定期处理。".into());
        }
    }
    if destination
        .as_ref()
        .is_some_and(|d| nodes_overlap(&root.witness.nodes, &d.witness.nodes))
    {
        return Err("源目录与目标物理范围重叠，不能启用定期处理。".into());
    }
    let device = root
        .target()
        .metadata()
        .map_err(|_| "定期目录所在卷无法核对。")?
        .dev();
    if destination
        .as_ref()
        .map(|d| d.target().metadata().map(|m| m.dev()))
        .transpose()
        .map_err(|_| "定期目标所在卷无法核对。")?
        .is_some_and(|target| target != device)
    {
        return Err("本轮定期移动仅支持同一磁盘，跨盘请使用手动复制核验流程。".into());
    }
    Ok((root.witness.nodes, destination.map(|d| d.witness.nodes)))
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Policy {
    pub id: String,
    pub rule: Rule,
    pub enabled: bool,
    pub updated_at: i64,
    pub last_attempt_at: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    policy: Policy,
    generation: String,
    nodes: Vec<Node>,
    destination_nodes: Option<Vec<Node>>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Binding {
    pub request_id: String,
    saved: Saved,
    started_at: i64,
    #[serde(skip)]
    issued: Option<Instant>,
}
impl Binding {
    pub(crate) fn rule(&self) -> Rule {
        self.saved.policy.rule.clone()
    }
    pub(super) fn validate_record(&self, plan: &super::Plan) -> Result<(), String> {
        self.saved.validate()?;
        let r = &self.saved.policy.rule;
        if !valid_id(&self.request_id)
            || self.started_at < 0
            || !self.saved.policy.enabled
            || plan.root != r.root
            || plan.items.len() > r.maximum_files
            || plan
                .items
                .iter()
                .try_fold(0u64, |n, i| n.checked_add(i.size))
                .is_none_or(|n| n > r.maximum_bytes)
            || plan.items.iter().any(|i| !r.matches(i, plan.created_at))
            || plan.movement.as_ref().map(|m| m.directory.as_str()) != r.destination.as_deref()
            || plan.movement.as_ref().is_some_and(|m| {
                m.conflict != movement::Conflict::Skip
                    || Some(&m.witness.nodes) != self.saved.destination_nodes.as_ref()
            })
        {
            return Err("定期处理计划与已确认规则不一致。".into());
        }
        Ok(())
    }
}
impl Saved {
    fn validate(&self) -> Result<(), String> {
        self.policy.rule.validate()?;
        if self.version != 1
            || !valid_id(&self.policy.id)
            || !valid_id(&self.generation)
            || self.policy.updated_at < 0
            || self.policy.last_attempt_at.is_some_and(|n| n < 0)
            || self.nodes.is_empty()
            || self.nodes.len() > 128
            || self
                .destination_nodes
                .as_ref()
                .is_some_and(|n| n.len() > 128)
            || self
                .nodes
                .last()
                .is_none_or(|n| n.path != Path::new(&self.policy.rule.root))
            || self
                .destination_nodes
                .as_ref()
                .map(|n| n.last().map(|n| n.path.as_path()))
                != self
                    .policy
                    .rule
                    .destination
                    .as_ref()
                    .map(|p| Some(Path::new(p)))
        {
            return Err("定期处理规则或目录身份无法核对，未继续执行。".into());
        }
        Ok(())
    }
    fn validate_paths(&self, home: &Path, settings: &db::Settings) -> Result<(), String> {
        self.validate()?;
        let (n, d) = bind(&self.policy.rule, home, settings)?;
        if n != self.nodes || d != self.destination_nodes {
            return Err("定期处理目录或目标已变化，请移除旧规则后重新选择。".into());
        }
        Ok(())
    }
}
fn valid_id(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok()
}
pub(super) fn current(
    database: &db::Database,
    plan: &super::Plan,
    home: &Path,
) -> Result<(), String> {
    let Some(binding) = &plan.automation else {
        return Ok(());
    };
    binding.validate_record(plan)?;
    let issued = binding
        .issued
        .ok_or("定期执行许可不在本次运行中，不能重放。")?;
    if issued.elapsed() > Duration::from_secs(120)
        || (db::now() as i128 - binding.started_at as i128 - issued.elapsed().as_millis() as i128)
            .abs()
            > 5000
    {
        return Err("定期执行时间基准变化，本项保留。".into());
    }
    let current = journal::policies(database)?
        .into_iter()
        .find(|p| p.policy.id == binding.saved.policy.id)
        .ok_or("定期规则已移除，本项保留。")?;
    if !current.policy.enabled
        || current.generation != binding.saved.generation
        || current.policy.rule != binding.saved.policy.rule
        || current.nodes != binding.saved.nodes
        || current.destination_nodes != binding.saved.destination_nodes
    {
        return Err("定期规则已暂停或变化，本项保留。".into());
    }
    current.validate_paths(home, &db::settings(database)?)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Edit {
    id: Option<String>,
    rule: Rule,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Draft {
    id: String,
    policy_id: Option<String>,
    rule: Rule,
    created_at: i64,
    expires_at: i64,
    matched_files: usize,
    selected: Vec<FileRow>,
    held_by_limits: usize,
}
struct Offered {
    draft: Draft,
    nodes: Vec<Node>,
    destination_nodes: Option<Vec<Node>>,
    issued: Instant,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Record {
    pub id: String,
    pub policy_id: String,
    generation: String,
    pub rule: Rule,
    pub trigger: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub state: String,
    pub matched_files: usize,
    pub selected_files: usize,
    pub held_by_limits: usize,
    pub execution_id: Option<String>,
    pub reason: String,
}
fn select(scan: &Scan, rule: &Rule, now: i64) -> (usize, Vec<String>, usize) {
    let mut matches = scan
        .items
        .iter()
        .filter(|i| rule.matches(i, now))
        .collect::<Vec<_>>();
    matches.sort_by(|a, b| a.modified_at.cmp(&b.modified_at).then(a.path.cmp(&b.path)));
    let count = matches.len();
    let mut bytes = 0u64;
    let mut ids = Vec::new();
    for item in matches {
        if ids.len() < rule.maximum_files
            && bytes
                .checked_add(item.size)
                .is_some_and(|b| b <= rule.maximum_bytes)
        {
            ids.push(item.id.clone());
            bytes += item.size;
        }
    }
    (count, ids.clone(), count - ids.len())
}
fn preview(mut edit: Edit, home: &Path, settings: &db::Settings) -> Result<Offered, String> {
    edit.rule.extensions.sort();
    edit.rule.extensions.dedup();
    let (nodes, destination_nodes) = bind(&edit.rule, home, settings)?;
    let scan = scan::collect(
        Path::new(&edit.rule.root),
        &settings.protected_paths,
        Arc::new(Job::new(TaskKind::PersonalScan)),
    )?;
    if !scan.report.complete || scan.report.cancelled {
        return Err("扫描未完整结束，不能启用自动处理；请选择更具体的目录。".into());
    }
    let now = db::now();
    let (matched, selected, held) = select(&scan, &edit.rule, now);
    Ok(Offered {
        draft: Draft {
            id: uuid::Uuid::new_v4().to_string(),
            policy_id: edit.id,
            rule: edit.rule,
            created_at: now,
            expires_at: now + 120_000,
            matched_files: matched,
            selected: scan
                .items
                .iter()
                .filter(|i| selected.contains(&i.id))
                .map(row)
                .collect(),
            held_by_limits: held,
        },
        nodes,
        destination_nodes,
        issued: Instant::now(),
    })
}
fn due(saved: &Saved, now: i64, boot: i64) -> bool {
    saved.policy.enabled
        && now
            >= saved
                .policy
                .updated_at
                .max(saved.policy.last_attempt_at.unwrap_or(0))
                .max(boot)
                .saturating_add(saved.policy.rule.interval_minutes as i64 * 60_000)
}
async fn process(
    database: Arc<Mutex<db::Database>>,
    home: PathBuf,
    engine: PathBuf,
    saved: Saved,
    record_id: String,
    job: Arc<Job>,
) -> Result<(), String> {
    let started_at = db::now();
    let issued = Instant::now();
    let settings = db::settings(&*database.lock().map_err(|e| e.to_string())?)?;
    saved.validate_paths(&home, &settings)?;
    let root = saved.policy.rule.root.clone();
    let rule = saved.policy.rule.clone();
    let scan_job = job.clone();
    let paths = settings.protected_paths.clone();
    let scan = tauri::async_runtime::spawn_blocking(move || {
        scan::collect(Path::new(&root), &paths, scan_job)
    })
    .await
    .map_err(|e| e.to_string())??;
    if !scan.report.complete || scan.report.cancelled || job.cancelled.load(Ordering::SeqCst) {
        return Err("扫描未完整结束或已取消，未按不完整结果处理文件。".into());
    }
    let (matched, ids, held) = select(&scan, &rule, db::now());
    journal::selection(
        &*database.lock().map_err(|e| e.to_string())?,
        &record_id,
        matched,
        ids.len(),
        held,
    )?;
    if ids.is_empty() {
        return Ok(());
    }
    let mut store = Store {
        scan: Some(scan),
        ..Default::default()
    };
    let offered = if let Some(destination) = &rule.destination {
        store.offer_move(
            &ids,
            destination,
            movement::Conflict::Skip,
            &settings,
            db::now(),
        )?
    } else {
        store.offer(&ids, db::now())?
    };
    let mut plan = store.take(&offered.id, db::now())?;
    plan.automation = Some(Binding {
        request_id: record_id.clone(),
        saved: saved.clone(),
        started_at,
        issued: Some(issued),
    });
    plan.module = MODULE.into();
    plan.revision = REVISION.into();
    plan.validate_record()?;
    current(&*database.lock().map_err(|e| e.to_string())?, &plan, &home)?;
    let rule_home = home.clone();
    let rule_engine = engine;
    let bind_db = database.clone();
    let bind_id = record_id;
    let bind_job = job.clone();
    let result = writer::run_with(
        database,
        home,
        plan,
        job,
        move |requested, job| {
            let home = rule_home.clone();
            let engine = rule_engine.clone();
            async move { rules::run(&engine, &home, &requested, job).await }
        },
        move |id| {
            let saved = bind_db
                .lock()
                .map_err(|e| e.to_string())
                .and_then(|db| journal::execution(&db, &bind_id, id));
            if saved.is_err() {
                bind_job.cancel();
            }
        },
    )
    .await?;
    if result.summary.state != "completed"
        || result.summary.moved_count != result.summary.item_count
    {
        return Err("本次处理未全部完成，规则已暂停；请先核对逐项历史，不会自动重试。".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests;
