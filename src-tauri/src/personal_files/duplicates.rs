//! Content-confirmed duplicates of ordinary personal files. No automatic choice.
use super::*;
use crate::{engine::Job, observation::AnchoredPath};
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    sync::{atomic::Ordering, Arc},
};

pub(crate) const MODULE: &str = "duplicateFiles";
pub(super) const REVISION: &str = "MooMole.duplicates.v1";
const READ_LIMIT: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Keep {
    pub digest: String,
    pub keeper: Item,
    pub selected_ids: Vec<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Group {
    pub id: String,
    pub size: u64,
    pub item_ids: Vec<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    pub groups: Vec<Group>,
    pub bytes_read: u64,
    pub checked_files: usize,
    pub extra_bytes: u64,
}
struct Budget {
    started: Instant,
    read: u64,
}
impl Budget {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            read: 0,
        }
    }
}
fn hash(item: &Item, job: &Job, budget: &mut Budget) -> Result<String, String> {
    if item.size > READ_LIMIT.saturating_sub(budget.read) {
        return Err("本轮内容读取预算已用完，未核对的文件保留。".into());
    }
    let held = AnchoredPath::open(Path::new(&item.path))?;
    if held.witness != item.witness || !held.local_materialized() {
        return Err("内容核对时文件已变化或未落地，保留。".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(&item.path)
        .map_err(|e| e.to_string())?;
    let m = file.metadata().map_err(|e| e.to_string())?;
    let node = held.witness.nodes.last().ok_or("缺少文件身份。")?;
    if !m.is_file()
        || m.nlink() != 1
        || m.uid() != unsafe { libc::geteuid() }
        || (m.dev(), m.ino()) != (node.dev, node.ino)
    {
        return Err("内容读取对象身份无法核对。".into());
    }
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];
    let mut read = 0u64;
    loop {
        if job.cancelled.load(Ordering::SeqCst)
            || budget.started.elapsed() > Duration::from_secs(120)
        {
            return Err("内容核对已停止，未确认的文件保留。".into());
        }
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        read += n as u64;
        budget.read += n as u64;
        if read > item.size || budget.read > READ_LIMIT {
            return Err("读取期间大小变化或超过预算，保留。".into());
        }
        digest.update(&buffer[..n]);
    }
    held.validate()?;
    let current = AnchoredPath::open(Path::new(&item.path))?;
    if current.witness != item.witness || read != item.size {
        return Err("内容核对期间文件变化，保留。".into());
    }
    Ok(format!("{:x}", digest.finalize()))
}
pub(super) fn collect(mut scan: Scan, job: Arc<Job>) -> Result<Scan, String> {
    let mut sizes: HashMap<u64, usize> = HashMap::new();
    for item in &scan.items {
        if item.size > 0 {
            *sizes.entry(item.size).or_default() += 1;
        }
    }
    let mut matches: HashMap<(u64, String), Vec<String>> = HashMap::new();
    let mut budget = Budget::new();
    let mut checked = 0;
    for item in &scan.items {
        if sizes.get(&item.size).copied().unwrap_or(0) < 2 {
            continue;
        }
        match hash(item, &job, &mut budget) {
            Ok(digest) => {
                matches
                    .entry((item.size, digest))
                    .or_default()
                    .push(item.id.clone());
                checked += 1;
            }
            Err(_) => {
                scan.report.skipped += 1;
                scan.report.complete = false;
            }
        }
        if job.cancelled.load(Ordering::SeqCst)
            || budget.started.elapsed() > Duration::from_secs(120)
        {
            scan.report.complete = false;
            break;
        }
    }
    let mut groups: Vec<_> = matches
        .into_iter()
        .filter(|(_, ids)| ids.len() > 1)
        .map(|((size, id), item_ids)| Group { id, size, item_ids })
        .collect();
    groups.sort_by(|a, b| {
        (b.size * (b.item_ids.len() as u64 - 1))
            .cmp(&(a.size * (a.item_ids.len() as u64 - 1)))
            .then(a.id.cmp(&b.id))
    });
    let ids: HashSet<_> = groups.iter().flat_map(|g| g.item_ids.iter()).collect();
    scan.items.retain(|i| ids.contains(&i.id));
    scan.report.items = scan.items.iter().map(row).collect();
    scan.report.cancelled = job.cancelled.load(Ordering::SeqCst);
    scan.report.duplicates = Some(Report {
        extra_bytes: groups
            .iter()
            .map(|g| g.size * (g.item_ids.len() as u64 - 1))
            .sum(),
        groups,
        bytes_read: budget.read,
        checked_files: checked,
    });
    Ok(scan)
}
pub(super) fn selection(
    report: &Report,
    all: &[Item],
    selected: &[Item],
) -> Result<Vec<Keep>, String> {
    let chosen: HashSet<_> = selected.iter().map(|i| &i.id).collect();
    let mut proof = Vec::new();
    for group in &report.groups {
        let selected_ids: Vec<_> = group
            .item_ids
            .iter()
            .filter(|id| chosen.contains(id))
            .cloned()
            .collect();
        if selected_ids.is_empty() {
            continue;
        }
        let keeper = group
            .item_ids
            .iter()
            .filter(|id| !chosen.contains(id))
            .find_map(|id| all.iter().find(|i| &i.id == id))
            .ok_or("每组至少保留一份，请撤销一个文件的选择。")?;
        proof.push(Keep {
            digest: group.id.clone(),
            keeper: keeper.clone(),
            selected_ids,
        });
    }
    if proof.iter().map(|p| p.selected_ids.len()).sum::<usize>() != selected.len() {
        return Err("选择不属于本次内容核对分组。".into());
    }
    Ok(proof)
}
pub(super) fn validate_record(proof: &[Keep], items: &[Item], root: &str) -> Result<(), String> {
    let mut covered = HashSet::new();
    if proof.is_empty()
        || proof.len() > MAX_SELECTED
        || proof.iter().any(|p| {
            p.digest.len() != 64
                || !p
                    .digest
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                || uuid::Uuid::parse_str(&p.keeper.id).is_err()
                || !ordinary(Path::new(&p.keeper.path))
                || !Path::new(&p.keeper.path).starts_with(root)
                || p.keeper
                    .witness
                    .nodes
                    .last()
                    .is_none_or(|n| n.path != Path::new(&p.keeper.path))
                || p.selected_ids.is_empty()
                || items
                    .iter()
                    .any(|i| i.id == p.keeper.id || i.path == p.keeper.path)
                || p.selected_ids.iter().any(|id| {
                    !covered.insert(id)
                        || items
                            .iter()
                            .find(|i| &i.id == id)
                            .is_none_or(|i| i.size != p.keeper.size)
                })
        })
        || covered.len() != items.len()
    {
        return Err("重复文件计划或保留副本无法核对。".into());
    }
    Ok(())
}
pub(super) fn verify(plan: &Plan, item: &Item, job: &Job) -> Result<(), String> {
    let proof = plan
        .duplicates
        .as_ref()
        .and_then(|p| p.iter().find(|p| p.selected_ids.contains(&item.id)))
        .ok_or("缺少保留副本的证据。")?;
    let mut budget = Budget::new();
    if hash(item, job, &mut budget)? != proof.digest
        || hash(&proof.keeper, job, &mut budget)? != proof.digest
    {
        return Err("所选文件与保留副本不再相同，本项保留。".into());
    }
    Ok(())
}
pub(super) fn validate_keeper(plan: &Plan, item: &Item) -> Result<(), String> {
    let proof = plan
        .duplicates
        .as_ref()
        .and_then(|p| p.iter().find(|p| p.selected_ids.contains(&item.id)))
        .ok_or("缺少保留副本。")?;
    let held = AnchoredPath::open(Path::new(&proof.keeper.path))?;
    if held.witness != proof.keeper.witness || !held.local_materialized() {
        return Err("保留副本已变化或无法读取，本项保留。".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
