//! Whole-App identity for an independent uninstall plan, not the partial display
//! inspection or a cleanup-file witness. No contents execute and no data changes.
use super::{parse, stamp, Bundle, Evidence};
use crate::{
    app_background::Marker,
    observation::{AnchoredPath, Witness},
    tree::{self, BundleWitness},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    path::{Component, Path},
    time::{Duration, Instant},
};

mod returned;

enum HeldPath {
    Source(AnchoredPath),
    Returned(returned::ReturnedPath),
}
impl HeldPath {
    fn open(path: &Path, returned: bool) -> Result<Self, String> {
        if returned {
            Ok(Self::Returned(returned::ReturnedPath::open(path)?))
        } else {
            Ok(Self::Source(AnchoredPath::open(path)?))
        }
    }
    fn target(&self) -> &File {
        match self {
            Self::Source(p) => p.target(),
            Self::Returned(p) => p.target(),
        }
    }
    fn witness(&self) -> &Witness {
        match self {
            Self::Source(p) => &p.witness,
            Self::Returned(p) => &p.witness,
        }
    }
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Source(p) => p.validate(),
            Self::Returned(p) => p.validate(),
        }
    }
    fn local_materialized(&self) -> bool {
        match self {
            Self::Source(p) => p.local_materialized(),
            Self::Returned(p) => p.local_materialized(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Snapshot {
    pub version: u32,
    pub bundle_id: String,
    pub app_version: Option<String>,
    pub build: Option<String>,
    pub background: Marker,
    pub executable_name: String,
    pub root: Witness,
    pub contents: Witness,
    pub info: Witness,
    pub executable: Witness,
    pub info_sha256: String,
    pub inventory: BundleWitness,
}

pub(crate) fn capture(path: &Path, expected_id: &str) -> Result<Snapshot, String> {
    capture_with(path, expected_id, false)
}

// Post-call evidence for an EXACT native return only. Never use this weaker
// ancestor observation as a source/plan or mutation target preparation route.
pub(crate) fn capture_returned(path: &Path, expected_id: &str) -> Result<Snapshot, String> {
    capture_with(path, expected_id, true)
}

fn capture_with(path: &Path, expected_id: &str, returned: bool) -> Result<Snapshot, String> {
    let started = Instant::now();
    let raw = path.to_str().ok_or("应用路径无法编码。")?;
    let root = HeldPath::open(path, returned)?;
    if !root.local_materialized() {
        return Err("本次只记录已落地的本地 App 包。".into());
    }
    let bundle = Bundle::open(raw)?;
    if stamp(&root.target().metadata().map_err(|_| "无法读取应用身份。")?)
        != stamp(&bundle.metadata)
    {
        return Err("应用位置已被替换，请重新读取列表。".into());
    }
    let (evidence, bytes) = Evidence::read(&bundle)?;
    let fields = parse(&bytes)?;
    let id = fields
        .id
        .filter(|id| id == expected_id)
        .ok_or("本地 App 身份与 Mole 清单不一致，请重新读取列表。")?;
    if fields.package_type.as_deref() != Some("APPL") {
        return Err("此对象未声明为完整应用包，本次不生成卸载计划。".into());
    }
    let name = fields
        .executable
        .filter(|name| {
            name != "."
                && name != ".."
                && Path::new(name).components().count() == 1
                && Path::new(name)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_)))
        })
        .ok_or("应用主程序名称缺失或不是单个文件名。")?;
    let contents = HeldPath::open(&path.join("Contents"), returned)?;
    let info = HeldPath::open(&path.join("Contents/Info.plist"), returned)?;
    let executable = HeldPath::open(&path.join("Contents/MacOS").join(&name), returned)?;
    let metadata = executable
        .target()
        .metadata()
        .map_err(|_| "无法核对应用主程序。")?;
    use std::os::unix::fs::MetadataExt;
    if !metadata.is_file()
        || metadata.mode() & 0o111 == 0
        || metadata.nlink() != 1
        || !contents.local_materialized()
        || !info.local_materialized()
        || !executable.local_materialized()
        || stamp(
            &info
                .target()
                .metadata()
                .map_err(|_| "无法核对应用身份文件。")?,
        ) != stamp(&evidence.plist_meta)
        || metadata.dev() != bundle.metadata.dev()
    {
        return Err("应用身份/主程序的文件类型、链接、权限或本地布局不支持。".into());
    }
    let node = root.witness().nodes.last().ok_or("缺少应用目录身份。")?;
    let inventory = if returned {
        tree::inspect_bundle_held(
            root.target(),
            path,
            node.dev,
            node.ino,
            &mut tree::Budget::new(),
        )?
    } else {
        tree::inspect_bundle(path, node.dev, node.ino, &mut tree::Budget::new())?
    };
    root.validate()?;
    contents.validate()?;
    info.validate()?;
    executable.validate()?;
    if !evidence.verify(&bundle) || started.elapsed() >= Duration::from_secs(5) {
        return Err("应用在检查期间变化或超过 5 秒预算，请重新读取列表。".into());
    }
    Ok(Snapshot {
        version: 1,
        bundle_id: id,
        app_version: fields.version,
        build: fields.build,
        background: fields.background,
        executable_name: name,
        root: root.witness().clone(),
        contents: contents.witness().clone(),
        info: info.witness().clone(),
        executable: executable.witness().clone(),
        info_sha256: format!("{:x}", Sha256::digest(&bytes)),
        inventory,
    })
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::fixture;
