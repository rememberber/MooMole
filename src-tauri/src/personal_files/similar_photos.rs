use super::*;
use crate::{engine::Job, observation::AnchoredPath};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
};
mod capture;
pub(crate) mod video;
mod vision;
pub(crate) const MODULE: &str = "similarPhotos";
pub(super) const REVISION: &str = "MooMole.similar-photos.v1";
const DISTANCE: f32 = 0.15;
pub(super) fn revision(proof: &[Keep]) -> &'static str {
    if proof.first().is_some_and(|p| p.video) {
        video::REVISION
    } else {
        REVISION
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Selected {
    pub id: String,
    pub digest: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Keep {
    pub keeper: Item,
    pub digest: String,
    pub selected: Vec<Selected>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub video: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Image {
    pub id: String,
    pub width: u64,
    pub height: u64,
    pub thumbnail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<String>,
    #[serde(skip_serializing)]
    digest: String,
    #[serde(skip_serializing)]
    capture: Option<capture::Metadata>,
    #[serde(skip_serializing)]
    parent: PathBuf,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Group {
    pub id: String,
    pub item_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_span_ms: Option<u32>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Sequence {
    pub missing_metadata: usize,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    pub groups: Vec<Group>,
    pub images: Vec<Image>,
    pub checked_files: usize,
    pub bytes_read: u64,
    pub method: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<Sequence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub videos: Option<Vec<video::Video>>,
}

fn image(item: &Item, bytes: &[u8], sequences: bool) -> Result<(Image, vision::Feature), String> {
    let raster = media::Raster::decode(bytes, 384)?;
    if raster.frames != 1 {
        return Err("多帧图片保留，不按单张照片比较。".into());
    }
    let png = raster.png()?;
    use objc2::{rc::autoreleasepool, AnyThread};
    use objc2_foundation::{NSData, NSDataBase64DecodingOptions, NSString};
    let feature = autoreleasepool(|_| {
        let data = NSData::initWithBase64EncodedString_options(
            NSData::alloc(),
            &NSString::from_str(
                png.strip_prefix("data:image/png;base64,")
                    .ok_or("缩略图编码无法核对。")?,
            ),
            NSDataBase64DecodingOptions::empty(),
        )
        .ok_or("缩略图无法解码。")?;
        vision::Feature::new(&data.to_vec())
    })?;
    // Persist a small comparison thumbnail; full preview is fetched separately.
    let thumbnail = media::Raster::decode(bytes, 160)?.png()?;
    let capture = if sequences {
        capture::read(bytes)
    } else {
        None
    };
    Ok((
        Image {
            id: item.id.clone(),
            width: raster.width,
            height: raster.height,
            thumbnail,
            captured_at: capture.as_ref().map(|c| c.label.clone()),
            capture,
            parent: Path::new(&item.path)
                .parent()
                .ok_or("照片缺少父目录。")?
                .into(),
            digest: format!("{:x}", Sha256::digest(bytes)),
        },
        feature,
    ))
}
pub(super) fn collect(scan: Scan, job: Arc<Job>, rules: &[String]) -> Result<Scan, String> {
    collect_with_sequence(scan, job, rules, false)
}
pub(super) fn collect_with_sequence(
    mut scan: Scan,
    job: Arc<Job>,
    rules: &[String],
    sequences: bool,
) -> Result<Scan, String> {
    let started = Instant::now();
    let mut photos = Vec::new();
    let mut bytes_read = 0u64;
    let mut reserved_read = 0u64;
    let mut total_thumbs = 0usize;
    scan.items.sort_by(|a, b| a.path.cmp(&b.path));
    for item in &scan.items {
        let extension = Path::new(&item.path)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ![
            "png", "jpg", "jpeg", "heic", "heif", "webp", "tif", "tiff", "bmp",
        ]
        .contains(&extension.as_str())
        {
            continue;
        }
        if item.size > 32 * 1024 * 1024 {
            scan.report.skipped += 1;
            scan.report.complete = false;
            continue;
        }
        if job.cancelled.load(Ordering::SeqCst)
            || started.elapsed() >= Duration::from_secs(120)
            || photos.len() >= 200
            || reserved_read.saturating_add(item.size) > 512 * 1024 * 1024
            || total_thumbs > 6 * 1024 * 1024
        {
            scan.report.complete = false;
            break;
        }
        reserved_read = reserved_read.saturating_add(item.size);
        let result = preview::read(item, rules, 32 * 1024 * 1024, true).and_then(|bytes| {
            bytes_read += bytes.len() as u64;
            image(item, &bytes, sequences)
        });
        match result {
            Ok((row, feature)) => {
                total_thumbs += row.thumbnail.len();
                photos.push((row, feature));
            }
            Err(_) => {
                scan.report.skipped += 1;
                scan.report.complete = false;
            }
        }
    }
    let checked_files = photos.len();
    let missing_metadata = photos.iter().filter(|p| p.0.capture.is_none()).count();
    if sequences {
        photos.sort_by_key(|p| p.0.capture.as_ref().map(|c| (c.date.clone(), c.nanos)));
    }
    let mut groups = Vec::new();
    let mut used = HashSet::new();
    for i in 0..photos.len() {
        if used.contains(&i) {
            continue;
        }
        if sequences && photos[i].0.capture.is_none() {
            continue;
        }
        if job.cancelled.load(Ordering::SeqCst) || started.elapsed() >= Duration::from_secs(120) {
            scan.report.complete = false;
            break;
        }
        let mut members = vec![i];
        for j in i + 1..photos.len() {
            let capture_matches = !sequences
                || photos[j].0.parent == photos[i].0.parent
                    && photos[j]
                        .0
                        .capture
                        .as_ref()
                        .zip(photos[i].0.capture.as_ref())
                        .is_some_and(|(candidate, anchor)| candidate.follows(anchor));
            if !used.contains(&j) && capture_matches {
                match photos[i].1.distance(&photos[j].1) {
                    Ok(d) if d <= DISTANCE => members.push(j),
                    Ok(_) => {}
                    Err(_) => scan.report.complete = false,
                }
            }
        }
        if members.len() > 1 {
            let item_ids = members.iter().map(|n| photos[*n].0.id.clone()).collect();
            let capture_span_ms = if sequences {
                let nanos = members
                    .iter()
                    .filter_map(|n| photos[*n].0.capture.as_ref().map(|c| c.nanos))
                    .max()
                    .unwrap()
                    - photos[i].0.capture.as_ref().unwrap().nanos;
                Some(nanos.div_ceil(1_000_000) as u32)
            } else {
                None
            };
            used.extend(members);
            groups.push(Group {
                id: uuid::Uuid::new_v4().to_string(),
                item_ids,
                capture_span_ms,
            });
        }
    }
    let ids: HashSet<_> = groups
        .iter()
        .flat_map(|g| g.item_ids.iter().cloned())
        .collect();
    scan.items.retain(|i| ids.contains(&i.id));
    scan.report.items = scan.items.iter().map(row).collect();
    scan.report.cancelled = job.cancelled.load(Ordering::SeqCst);
    scan.report.similar_photos = Some(Report {
        groups,
        images: photos
            .into_iter()
            .filter(|p| ids.contains(&p.0.id))
            .map(|p| p.0)
            .collect(),
        checked_files,
        bytes_read,
        method: "visionFeaturePrintV2",
        sequence: sequences.then_some(Sequence { missing_metadata }),
        videos: None,
    });
    Ok(scan)
}
pub(super) fn selection(
    report: &Report,
    all: &[Item],
    chosen: &[Item],
) -> Result<Vec<Keep>, String> {
    let ids: HashSet<_> = chosen.iter().map(|i| &i.id).collect();
    let mut proof = Vec::new();
    for group in &report.groups {
        let selected: Vec<_> = group
            .item_ids
            .iter()
            .filter(|id| ids.contains(id))
            .map(|id| {
                let image = report
                    .images
                    .iter()
                    .find(|p| &p.id == id)
                    .ok_or("照片内容证据缺失。")?;
                Ok(Selected {
                    id: id.clone(),
                    digest: image.digest.clone(),
                })
            })
            .collect::<Result<_, String>>()?;
        if selected.is_empty() {
            continue;
        }
        let keeper = group
            .item_ids
            .iter()
            .filter(|id| !ids.contains(id))
            .find_map(|id| all.iter().find(|i| &i.id == id))
            .ok_or("每组至少保留一份，请撤销一个文件的选择。")?
            .clone();
        let digest = report
            .images
            .iter()
            .find(|p| p.id == keeper.id)
            .ok_or("保留照片证据缺失。")?
            .digest
            .clone();
        proof.push(Keep {
            keeper,
            digest,
            selected,
            video: report.method == video::METHOD,
        });
    }
    if proof.iter().map(|p| p.selected.len()).sum::<usize>() != chosen.len() {
        return Err("选择不属于当前相似媒体分组。".into());
    }
    Ok(proof)
}
pub(super) fn validate_record(proof: &[Keep], items: &[Item], root: &str) -> Result<(), String> {
    let hash = |s: &str| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    let mut covered = HashSet::new();
    if proof.is_empty()
        || proof.len() > MAX_SELECTED
        || proof.iter().any(|p| p.video != proof[0].video)
        || proof.iter().any(|p| {
            !hash(&p.digest)
                || (p.video
                    && (!video::supported(Path::new(&p.keeper.path))
                        || items.iter().any(|i| {
                            !video::supported(Path::new(&i.path)) || i.size > video::FILE_LIMIT
                        })
                        || p.keeper.size > video::FILE_LIMIT))
                || uuid::Uuid::parse_str(&p.keeper.id).is_err()
                || !ordinary(Path::new(&p.keeper.path))
                || !Path::new(&p.keeper.path).starts_with(root)
                || p.keeper
                    .witness
                    .nodes
                    .last()
                    .is_none_or(|n| n.path != Path::new(&p.keeper.path))
                || p.selected.is_empty()
                || items
                    .iter()
                    .any(|i| i.id == p.keeper.id || i.path == p.keeper.path)
                || p.selected.iter().any(|s| {
                    !hash(&s.digest)
                        || !covered.insert(&s.id)
                        || !items.iter().any(|i| i.id == s.id)
                })
        })
        || covered.len() != items.len()
    {
        return Err("相似媒体选择与保留原件无法核对。".into());
    }
    Ok(())
}
fn proof<'a>(plan: &'a Plan, item: &Item) -> Result<&'a Keep, String> {
    plan.similar_photos
        .as_ref()
        .and_then(|p| {
            p.iter()
                .find(|p| p.selected.iter().any(|s| s.id == item.id))
        })
        .ok_or("缺少本组保留原件的记录。".into())
}
pub(super) fn validate_keeper(plan: &Plan, item: &Item) -> Result<(), String> {
    let p = proof(plan, item)?;
    let held = AnchoredPath::open(Path::new(&p.keeper.path))?;
    if held.witness != p.keeper.witness || !held.local_materialized() {
        return Err("保留原件已变化或不再位于本机，本项保留。".into());
    }
    Ok(())
}
pub(super) fn verify(plan: &Plan, item: &Item, job: &Job, rules: &[String]) -> Result<(), String> {
    let p = proof(plan, item)?;
    if job.cancelled.load(Ordering::SeqCst) {
        return Err("照片核对已取消。".into());
    }
    if p.video {
        let started = Instant::now();
        let chosen = video::content(item, rules, job, started, None, &mut 0)?;
        let keeper = video::content(&p.keeper, rules, job, started, None, &mut 0)?;
        if chosen
            != p.selected
                .iter()
                .find(|s| s.id == item.id)
                .ok_or("所选视频证据缺失。")?
                .digest
            || keeper != p.digest
        {
            return Err("视频或保留原件的内容已变化，本项保留。".into());
        }
        return Ok(());
    }
    let chosen = preview::read(item, rules, 32 * 1024 * 1024, true)?;
    let keeper = preview::read(&p.keeper, rules, 32 * 1024 * 1024, true)?;
    if format!("{:x}", Sha256::digest(chosen))
        != p.selected
            .iter()
            .find(|s| s.id == item.id)
            .ok_or("所选照片证据缺失。")?
            .digest
        || format!("{:x}", Sha256::digest(keeper)) != p.digest
    {
        return Err("照片或保留原件的内容已变化，本项保留。".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests;
