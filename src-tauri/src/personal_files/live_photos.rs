//! Exported photo/movie pairs. Filenames propose candidates; PhotoKit must
//! accept both resources. A pair selection never means an atomic filesystem move.
use super::*;
use crate::{engine::Job, observation::AnchoredPath};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
};
#[cfg(test)]
mod tests;
pub(crate) mod worker;
pub(super) const REVISION: &str = "MooMole.personal-live-photos.v1";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Proof {
    pub id: String,
    pub photo_id: String,
    pub movie_id: String,
    photo_sha256: String,
    movie_sha256: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Pair {
    id: String,
    photo_id: String,
    movie_id: String,
    #[serde(skip_serializing)]
    proof: Proof,
}
pub(super) fn rows(proofs: &[Proof]) -> Vec<Pair> {
    proofs
        .iter()
        .map(|p| Pair {
            id: p.id.clone(),
            photo_id: p.photo_id.clone(),
            movie_id: p.movie_id.clone(),
            proof: p.clone(),
        })
        .collect()
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    pairs: Vec<Pair>,
    checked_candidates: usize,
    rejected_candidates: usize,
    unpaired_files: usize,
    bytes_read: u64,
}
fn hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn is_photo(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| ["jpg", "jpeg", "heic", "heif"].contains(&s.to_ascii_lowercase().as_str()))
}
fn is_movie(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("mov"))
}
fn key(path: &Path) -> Option<(PathBuf, String)> {
    Some((path.parent()?.into(), path.file_stem()?.to_str()?.into()))
}
pub(super) fn collect(mut scan: Scan, job: Arc<Job>, rules: &[String]) -> Result<Scan, String> {
    let began = Instant::now();
    let mut candidates: std::collections::BTreeMap<(PathBuf, String), (Vec<Item>, Vec<Item>)> =
        Default::default();
    for item in &scan.items {
        let path = Path::new(&item.path);
        if let Some(k) = key(path) {
            if is_photo(path) {
                candidates.entry(k).or_default().0.push(item.clone())
            } else if is_movie(path) {
                candidates.entry(k).or_default().1.push(item.clone())
            }
        }
    }
    let mut report = Report {
        pairs: vec![],
        checked_candidates: 0,
        rejected_candidates: 0,
        unpaired_files: 0,
        bytes_read: 0,
    };
    let mut selected = vec![];
    let mut reserved = 0u64;
    for (_, (photos, movies)) in candidates {
        if photos.len() != 1 || movies.len() != 1 {
            report.unpaired_files += photos.len() + movies.len();
            continue;
        }
        let photo = &photos[0];
        let movie = &movies[0];
        if job.cancelled.load(Ordering::SeqCst)
            || began.elapsed() >= Duration::from_secs(120)
            || report.checked_candidates >= 64
            || reserved
                .saturating_add(photo.size)
                .saturating_add(movie.size)
                > 512 * 1024 * 1024
        {
            scan.report.complete = false;
            break;
        }
        reserved = reserved
            .saturating_add(photo.size)
            .saturating_add(movie.size);
        report.checked_candidates += 1;
        if photo.size > 32 * 1024 * 1024 || movie.size > 128 * 1024 * 1024 {
            report.rejected_candidates += 1;
            scan.report.complete = false;
            continue;
        }
        match worker::analyze(
            photo,
            movie,
            rules,
            job.clone(),
            began,
            &mut report.bytes_read,
        ) {
            Ok((proof, true)) => {
                selected.push(photo.clone());
                selected.push(movie.clone());
                report.pairs.push(Pair {
                    id: proof.id.clone(),
                    photo_id: photo.id.clone(),
                    movie_id: movie.id.clone(),
                    proof,
                })
            }
            Ok((_, false)) => report.rejected_candidates += 1,
            Err(error) => {
                #[cfg(test)]
                eprintln!("owned Live Photo validation: {error}");
                #[cfg(not(test))]
                let _ = error;
                report.rejected_candidates += 1;
                scan.report.complete = false
            }
        }
    }
    scan.items = selected;
    scan.report.items = scan.items.iter().map(row).collect();
    scan.report.cancelled = job.cancelled.load(Ordering::SeqCst);
    scan.report.live_photos = Some(report);
    Ok(scan)
}
pub(super) fn selection(report: &Report, items: &[Item]) -> Result<Vec<Proof>, String> {
    let ids = items.iter().map(|i| i.id.as_str()).collect::<HashSet<_>>();
    let mut out = vec![];
    for pair in &report.pairs {
        let a = ids.contains(pair.photo_id.as_str());
        let b = ids.contains(pair.movie_id.as_str());
        if a != b {
            return Err("Live Photo 必须同时选择照片和短视频，不能只处理其中一份。".into());
        }
        if a {
            out.push(pair.proof.clone())
        }
    }
    if out.len() * 2 != items.len() || out.is_empty() {
        return Err("所选文件不属于已核对的 Live Photo 对。".into());
    }
    Ok(out)
}
pub(super) fn validate_record(proofs: &[Proof], items: &[Item]) -> Result<(), String> {
    let mut pair_ids = HashSet::new();
    let mut seen = HashSet::new();
    if proofs.is_empty() || proofs.len() > 10 || proofs.len() * 2 != items.len() {
        return Err("Live Photo 计划范围无法核对。".into());
    }
    for (p, files) in proofs.iter().zip(items.chunks_exact(2)) {
        if uuid::Uuid::parse_str(&p.id).is_err()
            || !pair_ids.insert(&p.id)
            || !hash(&p.photo_sha256)
            || !hash(&p.movie_sha256)
            || files[0].id != p.photo_id
            || files[1].id != p.movie_id
            || !seen.insert(&p.photo_id)
            || !seen.insert(&p.movie_id)
            || !is_photo(Path::new(&files[0].path))
            || !is_movie(Path::new(&files[1].path))
            || key(Path::new(&files[0].path)) != key(Path::new(&files[1].path))
        {
            return Err("Live Photo 照片、短视频或顺序无法核对。".into());
        }
    }
    Ok(())
}
fn proof<'a>(plan: &'a Plan, item: &Item) -> Result<&'a Proof, String> {
    plan.live_photos
        .as_ref()
        .and_then(|p| {
            p.iter()
                .find(|p| p.photo_id == item.id || p.movie_id == item.id)
        })
        .ok_or("Live Photo 配对证据缺失。".into())
}
pub(super) fn peer<'a>(plan: &'a Plan, item: &Item) -> Result<Option<&'a Item>, String> {
    if plan.live_photos.is_none() {
        return Ok(None);
    }
    let p = proof(plan, item)?;
    if item.id != p.photo_id {
        return Ok(None);
    }
    plan.items
        .iter()
        .find(|i| i.id == p.movie_id)
        .map(Some)
        .ok_or("Live Photo 短视频缺失。".into())
}
pub(super) fn verify(
    plan: &Plan,
    item: &Item,
    job: &Job,
    rules: &[String],
    db: &rusqlite::Connection,
    run: &str,
) -> Result<(), String> {
    let p = proof(plan, item)?;
    let mut bytes = 0;
    if item.id == p.photo_id {
        let movie = peer(plan, item)?.ok_or("Live Photo 短视频缺失。")?;
        if raw_photo::content(item, rules, job, Instant::now(), None, &mut bytes)? != p.photo_sha256
            || similar_photos::video::content(movie, rules, job, Instant::now(), None, &mut bytes)?
                != p.movie_sha256
        {
            return Err("Live Photo 配对文件的完整内容变化，本对保留。".into());
        }
    } else {
        let detail = crate::execution::detail(db, run)?;
        if !detail
            .items
            .iter()
            .any(|i| i.id == p.photo_id && i.state == "moved" && i.receipt.is_some())
        {
            return Err("Live Photo 照片未取得本批已移入收据，短视频保留。".into());
        }
        if similar_photos::video::content(item, rules, job, Instant::now(), None, &mut bytes)?
            != p.movie_sha256
        {
            return Err("Live Photo 短视频完整内容变化，保留并核对本对结果。".into());
        }
    }
    Ok(())
}
