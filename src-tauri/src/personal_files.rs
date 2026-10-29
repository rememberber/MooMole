//! Explicit personal-file selection, independent of Mole cache eligibility.
//! Snapshots are data; only a fresh single-use confirmation reaches the writer.
use crate::{observation::Witness, review::ScanItem};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::{Duration, Instant},
};

pub(crate) mod archive;
pub(crate) mod commands;
pub(crate) mod duplicates;
mod media;
pub(crate) mod movement;
pub(crate) mod preview;
mod scan;
pub(crate) mod similar_photos;
pub(crate) mod software_content;
mod writer;
pub(crate) const MODULE: &str = "personalFiles";
pub(crate) const REVISION: &str = "MooMole.personal-files.v1";
const TTL: i64 = 15 * 60 * 1000;
const MAX_SELECTED: usize = 20;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Item {
    pub id: String,
    pub path: String,
    pub size: u64,
    pub modified_at: i64,
    pub witness: Witness,
}
impl Item {
    pub(crate) fn target(&self) -> ScanItem {
        ScanItem::personal_file(&self.id, &self.path, self.size, self.witness.clone())
    }
}
#[derive(Clone)]
struct Scan {
    id: String,
    root: String,
    created_at: i64,
    expires_at: i64,
    items: Vec<Item>,
    report: Report,
    software_content: Option<software_content::Binding>,
    allocated_bytes: u64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileRow {
    id: String,
    path: String,
    name: String,
    size: u64,
    modified_at: i64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    id: String,
    root: String,
    created_at: i64,
    expires_at: i64,
    complete: bool,
    cancelled: bool,
    skipped: usize,
    visited: usize,
    items: Vec<FileRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duplicates: Option<duplicates::Report>,
    #[serde(skip_serializing_if = "Option::is_none")]
    similar_photos: Option<similar_photos::Report>,
    #[serde(skip_serializing_if = "Option::is_none")]
    software_content: Option<software_content::Info>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Plan {
    pub module: String,
    pub protocol: u64,
    pub revision: String,
    pub id: String,
    pub scan_id: String,
    pub root: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub items: Vec<Item>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub movement: Option<movement::Movement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicates: Option<Vec<duplicates::Keep>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub similar_photos: Option<Vec<similar_photos::Keep>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub software_content: Option<software_content::Binding>,
}
impl Plan {
    pub(crate) fn origin(&self) -> &'static str {
        if self.software_content.is_some() {
            software_content::MODULE
        } else if self.similar_photos.is_some() {
            similar_photos::MODULE
        } else if self.duplicates.is_some() {
            duplicates::MODULE
        } else {
            MODULE
        }
    }
    pub(crate) fn allows(&self, path: &Path) -> bool {
        self.software_content
            .as_ref()
            .map_or_else(|| ordinary(path), |s| s.allows(path))
    }
    pub(crate) fn validate_record(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        let mut paths = HashSet::new();
        if self.module
            != if self.software_content.is_some() {
                software_content::MODULE
            } else if self.similar_photos.is_some() {
                similar_photos::MODULE
            } else if self.duplicates.is_some() {
                duplicates::MODULE
            } else {
                MODULE
            }
            || self.protocol != 1
            || self.revision
                != if self.software_content.is_some() {
                    software_content::REVISION
                } else if self.similar_photos.is_some() {
                    similar_photos::REVISION
                } else if self.duplicates.is_some() {
                    duplicates::REVISION
                } else if self.movement.is_some() {
                    movement::REVISION
                } else {
                    REVISION
                }
            || uuid::Uuid::parse_str(&self.id).is_err()
            || uuid::Uuid::parse_str(&self.scan_id).is_err()
            || self.created_at < 0
            || self.expires_at <= self.created_at
            || self.expires_at - self.created_at > TTL
            || self.items.is_empty()
            || self.items.len() > MAX_SELECTED
            || !self.allows(Path::new(&self.root))
            || self.items.iter().any(|item| {
                uuid::Uuid::parse_str(&item.id).is_err()
                    || !ids.insert(&item.id)
                    || !paths.insert(&item.path)
                    || item.size > 9_007_199_254_740_991
                    || !self.allows(Path::new(&item.path))
                    || !Path::new(&item.path).starts_with(&self.root)
                    || item.path == self.root
                    || item
                        .witness
                        .nodes
                        .last()
                        .is_none_or(|node| node.path != Path::new(&item.path))
            })
        {
            return Err("个人文件计划无法核对，请重新扫描。".into());
        }
        if let Some(source) = &self.software_content {
            if self.movement.is_some() || self.duplicates.is_some() || self.similar_photos.is_some()
            {
                return Err("软件附件计划不能混入其他处理方式。".into());
            }
            source.validate_record(&self.root, &self.items)?;
        }
        if let Some(movement) = &self.movement {
            movement.validate_record(&self.items)?;
        }
        if let Some(proof) = &self.similar_photos {
            if self.movement.is_some() || self.duplicates.is_some() {
                return Err("相似照片计划只支持独立的选择处理。".into());
            }
            similar_photos::validate_record(proof, &self.items, &self.root)?;
        }
        if let Some(proof) = &self.duplicates {
            if self.movement.is_some() {
                return Err("重复文件计划不接受移动方式。".into());
            }
            duplicates::validate_record(proof, &self.items, &self.root)?;
        }
        Ok(())
    }
}
struct Offered {
    plan: Plan,
    deadline: Instant,
    expires_at: i64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Confirmation {
    id: String,
    root: String,
    created_at: i64,
    expires_at: i64,
    items: Vec<FileRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    movement: Option<movement::Confirmation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kept_files: Option<Vec<FileRow>>,
}
#[derive(Default)]
pub(crate) struct Store {
    scan: Option<Scan>,
    offers: HashMap<String, Offered>,
    phase: &'static str,
    request_id: Option<String>,
    run_id: Option<String>,
    error: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot<'a> {
    phase: &'static str,
    request_id: &'a Option<String>,
    run_id: &'a Option<String>,
    report: Option<&'a Report>,
    error: &'a Option<String>,
}
impl Store {
    fn snapshot(&self) -> Snapshot<'_> {
        Snapshot {
            phase: if self.phase.is_empty() {
                "idle"
            } else {
                self.phase
            },
            request_id: &self.request_id,
            run_id: &self.run_id,
            report: self.scan.as_ref().map(|s| &s.report),
            error: &self.error,
        }
    }
    fn offer(&mut self, ids: &[String], now: i64) -> Result<Confirmation, String> {
        self.offers
            .retain(|_, offer| now < offer.expires_at && Instant::now() < offer.deadline);
        let scan = self.scan.as_ref().ok_or("请先扫描个人文件。")?;
        if scan
            .software_content
            .as_ref()
            .is_some_and(|b| b.source == software_content::Source::MailSandboxDownloads)
        {
            return Err(
                "此 Mail 容器位置受到 Mole 核心规则保护，仅供预览；不会生成处理确认。".into(),
            );
        }
        let selected: HashSet<_> = ids.iter().collect();
        if now < scan.created_at
            || now >= scan.expires_at
            || ids.is_empty()
            || ids.len() > MAX_SELECTED
            || selected.len() != ids.len()
            || self.offers.len() >= 8
        {
            return Err("选择为空、重复、超过 20 项或扫描已过期，请重新选择。".into());
        }
        let items: Vec<_> = scan
            .items
            .iter()
            .filter(|item| selected.contains(&item.id))
            .cloned()
            .collect();
        if items.len() != ids.len() {
            return Err("选择不属于本次扫描。".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let expires_at = (now + 30_000).min(scan.expires_at);
        let proof = scan
            .report
            .duplicates
            .as_ref()
            .map(|r| duplicates::selection(r, &scan.items, &items))
            .transpose()?;
        let similar = scan
            .report
            .similar_photos
            .as_ref()
            .map(|r| similar_photos::selection(r, &scan.items, &items))
            .transpose()?;
        let plan = Plan {
            module: if scan.software_content.is_some() {
                software_content::MODULE
            } else if similar.is_some() {
                similar_photos::MODULE
            } else if proof.is_some() {
                duplicates::MODULE
            } else {
                MODULE
            }
            .into(),
            protocol: 1,
            revision: if scan.software_content.is_some() {
                software_content::REVISION
            } else if similar.is_some() {
                similar_photos::REVISION
            } else if proof.is_some() {
                duplicates::REVISION
            } else {
                REVISION
            }
            .into(),
            id: id.clone(),
            scan_id: scan.id.clone(),
            root: scan.root.clone(),
            created_at: now,
            expires_at: scan.expires_at,
            items,
            movement: None,
            duplicates: proof,
            similar_photos: similar,
            software_content: scan.software_content.clone(),
        };
        plan.validate_record()?;
        let confirmation = Confirmation {
            id: id.clone(),
            root: scan.root.clone(),
            created_at: now,
            expires_at,
            items: plan.items.iter().map(row).collect(),
            movement: None,
            kept_files: if let Some(proof) = &plan.similar_photos {
                Some(proof.iter().map(|p| row(&p.keeper)).collect())
            } else {
                plan.duplicates
                    .as_ref()
                    .map(|p| p.iter().map(|k| row(&k.keeper)).collect())
            },
        };
        self.offers.insert(
            id,
            Offered {
                plan,
                expires_at,
                deadline: Instant::now() + Duration::from_millis((expires_at - now) as u64),
            },
        );
        Ok(confirmation)
    }
    fn take(&mut self, id: &str, now: i64) -> Result<Plan, String> {
        let offered = self
            .offers
            .remove(id)
            .ok_or("个人文件确认已使用或撤销，请重新选择。")?;
        if now < offered.plan.created_at
            || now >= offered.expires_at
            || Instant::now() >= offered.deadline
        {
            return Err("个人文件确认已过期，请重新检查。".into());
        }
        Ok(offered.plan)
    }
    fn offer_move(
        &mut self,
        ids: &[String],
        directory: &str,
        conflict: movement::Conflict,
        settings: &crate::db::Settings,
        now: i64,
    ) -> Result<Confirmation, String> {
        let mut confirmation = self.offer(ids, now)?;
        let mut offered = self.offers.remove(&confirmation.id).ok_or("确认已撤销。")?;
        let movement = movement::prepare(&offered.plan.items, directory, conflict, settings)?;
        confirmation.movement = Some(movement.confirmation());
        offered.plan.movement = Some(movement);
        offered.plan.revision = movement::REVISION.into();
        offered.plan.validate_record()?;
        self.offers.insert(confirmation.id.clone(), offered);
        Ok(confirmation)
    }
}
fn row(item: &Item) -> FileRow {
    FileRow {
        id: item.id.clone(),
        path: item.path.clone(),
        name: Path::new(&item.path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        size: item.size,
        modified_at: item.modified_at,
    }
}
fn sensitive(name: &str) -> bool {
    name.starts_with('.')
        || [
            "Library",
            "Applications",
            "System",
            "Keychains",
            "bin",
            "sbin",
            "usr",
            "etc",
            "dev",
            "proc",
        ]
        .iter()
        .any(|part| name.eq_ignore_ascii_case(part))
        || [
            ".app",
            ".bundle",
            ".framework",
            ".photoslibrary",
            ".musiclibrary",
            ".sparsebundle",
            ".key",
            ".pem",
            ".p12",
            ".pfx",
            ".keychain-db",
        ]
        .iter()
        .any(|suffix| name.to_ascii_lowercase().ends_with(suffix))
}
fn ordinary(path: &Path) -> bool {
    let Some(text) = path.to_str() else {
        return false;
    };
    text.len() <= 4096
        && text.starts_with('/')
        && text.len() > 1
        && !text.chars().any(char::is_control)
        && text[1..]
            .split('/')
            .all(|name| !name.is_empty() && !sensitive(name))
}
fn protected(path: &Path, rules: &[String]) -> Result<bool, String> {
    Ok(!crate::app_protection::check(
        path.to_str().ok_or("路径无法编码。")?,
        None,
        None,
        "",
        rules,
    )?
    .reasons
    .is_empty())
}

#[cfg(test)]
mod tests;
