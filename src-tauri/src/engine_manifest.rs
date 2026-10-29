//! Integrity against the prepared build manifest, not publisher authentication.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path},
};

const MAX_MANIFEST: u64 = 1024 * 1024;
const REQUIRED: &[&str] = &[
    "mole",
    "mo",
    "LICENSE",
    "TRADEMARK.md",
    "bin/analyze-go",
    "bin/status-go",
    "bin/clean.sh",
    "bin/uninstall.sh",
    "moomole-clean-preview.sh",
    "moomole-review-rules.sh",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    schema_version: u32,
    repository: String,
    pub revision: String,
    pub version: String,
    platform: String,
    pub arch: String,
    pub clean_protocol: u64,
    pub rules_protocol: u64,
    build: Build,
    files: BTreeMap<String, Record>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Build {
    recipe_version: u32,
    go_version: String,
    goos: String,
    goarch: String,
}
#[derive(Deserialize)]
struct Record {
    size: u64,
    sha256: String,
    executable: bool,
}

pub fn architecture() -> String {
    std::env::consts::ARCH
        .replace("aarch64", "arm64")
        .replace("x86_64", "x64")
}

fn regular(path: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.nlink() != 1 {
        return Err("引擎必须使用独立普通文件。".into());
    }
    Ok(file)
}

fn read_manifest(root: &Path) -> Result<Manifest, String> {
    let mut bytes = Vec::new();
    regular(&root.join("UPSTREAM.json"))?
        .take(MAX_MANIFEST + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_MANIFEST {
        return Err("引擎清单超过上限。".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("引擎清单无法识别：{e}"))
}

pub fn verify(root: &Path) -> Result<Manifest, String> {
    let meta = std::fs::symlink_metadata(root).map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("引擎目录不能是链接。".into());
    }
    let manifest = read_manifest(root)?;
    if manifest.schema_version != 1
        || manifest.repository != "https://github.com/tw93/Mole"
        || manifest.revision != crate::engine::REVISION
        || manifest.platform != "darwin"
        || manifest.arch != architecture()
        || manifest.build.recipe_version != 1
        || manifest.build.goos != "darwin"
        || manifest.build.goarch
            != if manifest.arch == "x64" {
                "amd64"
            } else {
                "arm64"
            }
        || !manifest.build.go_version.starts_with("go version go")
        || manifest.build.go_version.len() > 200
        || manifest.clean_protocol != crate::clean_preview::PROTOCOL
        || manifest.rules_protocol != crate::rules::PROTOCOL
    {
        return Err("引擎清单、架构或适配版本与应用不匹配。".into());
    }
    if manifest.files.is_empty()
        || manifest.files.len() > 512
        || REQUIRED
            .iter()
            .any(|name| !manifest.files.contains_key(*name))
    {
        return Err("引擎清单缺少必要资源或超过数量上限。".into());
    }
    for (name, record) in &manifest.files {
        if name.is_empty()
            || name == "UPSTREAM.json"
            || name.starts_with('/')
            || name
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || Path::new(name)
                .components()
                .any(|p| !matches!(p, Component::Normal(_)))
            || record.size > 64 * 1024 * 1024
            || record.sha256.len() != 64
            || !record
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("引擎清单包含无效路径或摘要。".into());
        }
    }
    let mut found = BTreeSet::new();
    let mut entries = 0;
    walk(root, root, &mut found, &mut entries, 0)?;
    if found != manifest.files.keys().cloned().collect() {
        return Err("引擎资源缺失或存在未登记文件。".into());
    }
    let mut total = 0_u64;
    for (name, record) in &manifest.files {
        let mut file = regular(&root.join(name))?;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        total += record.size;
        if total > 128 * 1024 * 1024
            || meta.len() != record.size
            || (meta.mode() & 0o111 != 0) != record.executable
        {
            return Err(format!("引擎资源大小或权限不一致：{name}"));
        }
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 65536];
        let mut bytes = 0;
        loop {
            let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            bytes += count as u64;
            if bytes > record.size {
                return Err(format!("引擎资源读取期间变化：{name}"));
            }
            digest.update(&buffer[..count]);
        }
        if bytes != record.size || format!("{:x}", digest.finalize()) != record.sha256 {
            return Err(format!("引擎资源摘要不一致：{name}"));
        }
    }
    for binary in ["bin/analyze-go", "bin/status-go"] {
        if !manifest.files[binary].executable {
            return Err(format!("引擎缺少执行权限：{binary}"));
        }
        let mut header = [0; 8];
        regular(&root.join(binary))?
            .read_exact(&mut header)
            .map_err(|e| e.to_string())?;
        let cpu = if manifest.arch == "arm64" {
            0x0100000c
        } else {
            0x01000007
        };
        if u32::from_le_bytes(header[..4].try_into().unwrap()) != 0xfeedfacf
            || u32::from_le_bytes(header[4..].try_into().unwrap()) != cpu
        {
            return Err(format!("引擎可执行文件架构不符：{binary}"));
        }
    }
    Ok(manifest)
}

fn walk(
    root: &Path,
    directory: &Path,
    found: &mut BTreeSet<String>,
    entries: &mut usize,
    depth: usize,
) -> Result<(), String> {
    if depth > 64 {
        return Err("引擎目录层级超过限制。".into());
    }
    for entry in std::fs::read_dir(directory).map_err(|e| e.to_string())? {
        *entries += 1;
        if *entries > 1000 {
            return Err("引擎资源超过目录项上限。".into());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            return Err("引擎资源包含链接。".into());
        }
        if kind.is_dir() {
            walk(root, &path, found, entries, depth + 1)?;
        } else if kind.is_file() {
            let name = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_str()
                .ok_or("引擎路径编码无效。")?
                .to_owned();
            if name != "UPSTREAM.json" {
                found.insert(name);
            }
        } else {
            return Err("引擎包含非普通文件。".into());
        }
    }
    Ok(())
}

#[cfg(test)]
pub fn fixture(root: &Path) {
    use std::os::unix::fs::PermissionsExt;
    for name in REQUIRED {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        if !path.exists() {
            std::fs::write(
                &path,
                if *name == "mole" {
                    "VERSION=\"1.65.0\"\n"
                } else {
                    "fixture"
                },
            )
            .unwrap();
        }
        if name.ends_with("-go") {
            let cpu: u32 = if architecture() == "arm64" {
                0x0100000c
            } else {
                0x01000007
            };
            std::fs::write(
                &path,
                [0xfeedfacf_u32.to_le_bytes(), cpu.to_le_bytes()].concat(),
            )
            .unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let files: BTreeMap<_, _> = REQUIRED.iter().map(|name| {
        let bytes = std::fs::read(root.join(name)).unwrap();
        let mode = std::fs::metadata(root.join(name)).unwrap().mode();
        (*name, serde_json::json!({"size": bytes.len(), "sha256": format!("{:x}", Sha256::digest(bytes)), "executable": mode & 0o111 != 0}))
    }).collect();
    let manifest = serde_json::json!({"schemaVersion":1, "repository":"https://github.com/tw93/Mole", "revision":crate::engine::REVISION,
        "version":"1.65.0", "platform":"darwin", "arch":architecture(), "cleanProtocol":crate::clean_preview::PROTOCOL,"rulesProtocol":crate::rules::PROTOCOL,
        "build":{"recipeVersion":1,"goVersion":"go version go1.26.0 darwin/fixture","goos":"darwin","goarch": if architecture()=="x64" {"amd64"} else {"arm64"}},"files":files});
    std::fs::write(root.join("UPSTREAM.json"), manifest.to_string()).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn integrity_rejects_same_length_changes_extra_files_missing_resources_and_links() {
        for mode in ["modified", "extra", "missing", "link", "manifest", "arch"] {
            let tmp = tempfile::tempdir().unwrap();
            fixture(tmp.path());
            assert!(verify(tmp.path()).is_ok());
            let target = tmp.path().join("mo");
            match mode {
                "modified" => std::fs::write(&target, "changed").unwrap(),
                "extra" => std::fs::write(tmp.path().join("extra"), "extra").unwrap(),
                "missing" => std::fs::remove_file(&target).unwrap(),
                "link" => {
                    std::fs::remove_file(&target).unwrap();
                    std::os::unix::fs::symlink("mole", &target).unwrap();
                }
                "manifest" => std::fs::write(tmp.path().join("UPSTREAM.json"), "{}").unwrap(),
                "arch" => {
                    let path = tmp.path().join("UPSTREAM.json");
                    let mut value: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                    value["arch"] = "other".into();
                    std::fs::write(path, value.to_string()).unwrap();
                }
                _ => unreachable!(),
            }
            assert!(verify(tmp.path()).is_err(), "{mode}");
        }
    }

    #[test]
    fn malformed_paths_wrong_binary_headers_and_deep_trees_are_rejected() {
        for mode in [
            "path", "header", "deep", "rules-v1", "rules-v2", "rules-v3", "clean-v1",
        ] {
            let tmp = tempfile::tempdir().unwrap();
            fixture(tmp.path());
            let manifest_path = tmp.path().join("UPSTREAM.json");
            let mut value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
            if mode == "clean-v1" {
                value["cleanProtocol"] = 1.into();
            }
            if mode == "rules-v3" {
                value["rulesProtocol"] = 3.into();
            }
            if mode == "rules-v2" {
                value["rulesProtocol"] = 2.into();
            }
            if mode == "rules-v1" {
                value["rulesProtocol"] = 1.into();
            }
            if mode == "path" {
                value["files"]["../escape"] = value["files"]["mo"].clone();
            }
            if mode == "header" {
                let bytes = b"notmacho";
                std::fs::write(tmp.path().join("bin/status-go"), bytes).unwrap();
                value["files"]["bin/status-go"]["sha256"] =
                    format!("{:x}", Sha256::digest(bytes)).into();
            }
            if mode == "deep" {
                let mut path = tmp.path().to_owned();
                for _ in 0..66 {
                    path.push("d");
                }
                std::fs::create_dir_all(path).unwrap();
            }
            std::fs::write(manifest_path, value.to_string()).unwrap();
            assert!(verify(tmp.path()).is_err(), "{mode}");
        }
    }
}
