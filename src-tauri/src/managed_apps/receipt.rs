//! Installed receipt/config bind the installed artifacts, not a newer catalog's
//! declarations. These are only read; Homebrew owns their mutation.
use super::*;
#[derive(Clone, PartialEq)]
pub(super) struct Stamp {
    receipt: crate::observation::Witness,
    config: crate::observation::Witness,
    digest: String,
}
fn read(path: &Path, ctx: &Context) -> Result<(crate::observation::Witness, Vec<u8>), String> {
    if ctx
        .protected_paths
        .iter()
        .any(|p| path.starts_with(p) || Path::new(p).starts_with(path))
    {
        return Err("Homebrew 收据与保护范围重叠，未读取。".into());
    }
    let h = AnchoredPath::open(path)?;
    let m = h.target().metadata().map_err(|e| e.to_string())?;
    if !m.is_file()
        || m.nlink() != 1
        || m.uid() != unsafe { libc::geteuid() }
        || m.len() > 512 * 1024
        || !h.local_materialized()
    {
        return Err("Homebrew 安装收据身份或容量无法核对。".into());
    }
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| e.to_string())?;
    let n = f.metadata().map_err(|e| e.to_string())?;
    if (n.dev(), n.ino()) != (m.dev(), m.ino()) {
        return Err("Homebrew 收据在读取前变化。".into());
    }
    let mut b = vec![];
    f.take(512 * 1024 + 1)
        .read_to_end(&mut b)
        .map_err(|e| e.to_string())?;
    h.validate()?;
    if b.len() as u64 != m.len() {
        return Err("Homebrew 收据读取期间变化。".into());
    }
    Ok((h.witness, b))
}
pub(super) fn capture(tool: &Tool, p: &Package, ctx: &Context) -> Result<Stamp, String> {
    let prefix = if tool.path.starts_with("/opt/homebrew") {
        Path::new("/opt/homebrew")
    } else {
        Path::new("/usr/local")
    };
    let leaf = p.token.rsplit('/').next().ok_or("包标识缺失。")?;
    let root = prefix.join("Caskroom").join(leaf).join(".metadata");
    let (receipt, a) = read(&root.join("INSTALL_RECEIPT.json"), ctx)?;
    let (config, b) = read(&root.join("config.json"), ctx)?;
    let r: serde_json::Value =
        serde_json::from_slice(&a).map_err(|_| "Homebrew 收据格式无法核对。")?;
    let c: serde_json::Value =
        serde_json::from_slice(&b).map_err(|_| "Homebrew 安装配置无法核对。")?;
    validate_fields(&r, &c, p)?;
    Ok(Stamp {
        receipt,
        config,
        digest: format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(p.sha256.as_str(), a, b)).map_err(|e| e.to_string())?
            )
        ),
    })
}
fn validate_fields(
    r: &serde_json::Value,
    c: &serde_json::Value,
    p: &Package,
) -> Result<(), String> {
    let expected_tap = p
        .token
        .rsplit_once('/')
        .map(|(tap, _)| tap)
        .unwrap_or("homebrew/cask");
    if r["source"]["tap"].as_str() != Some(expected_tap)
        || r["source"]["version"].as_str() != Some(&p.version)
        || r["uninstall_flight_blocks"].as_bool() != Some(false)
    {
        return Err("Homebrew 已安装来源、版本或卸载脚本状态不受支持。".into());
    }
    let artifacts = r["uninstall_artifacts"]
        .as_array()
        .filter(|v| v.len() == 1)
        .ok_or("安装收据包含多个或未支持的处理对象，请使用原管理工具。")?;
    let art = artifacts[0]
        .as_object()
        .filter(|d| d.len() == 1 && d.contains_key("app"))
        .ok_or("安装收据包含其他处理方式，保留。")?;
    let names = art["app"]
        .as_array()
        .filter(|v| v.len() == 1)
        .ok_or("安装收据中的应用声明不支持。")?;
    let name = text(&names[0], 255)?;
    if !name.ends_with(".app") || name.contains(['/', '\\']) {
        return Err("安装收据中的应用名无法核对。".into());
    }
    let dir = c["explicit"]
        .get("appdir")
        .or_else(|| c["env"].get("appdir"))
        .or_else(|| c["default"].get("appdir"))
        .ok_or("安装收据缺少应用目标目录。")?;
    let dir = text(dir, 4096)?;
    if !crate::uninstall_plan::absolute(dir) || Path::new(dir).join(name) != Path::new(&p.path) {
        return Err("已安装收据的实际目标与所选 App 不同，保留。".into());
    }
    Ok(())
}

impl Stamp {
    pub(super) fn digest(&self) -> &str {
        &self.digest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_receipts_refuse_changed_targets_versions_scripts_and_non_app_artifacts() {
        let p = Package {
            token: "author/tap/owned".into(),
            version: "1.0".into(),
            path: "/Users/demo/Applications/Owned.app".into(),
            sha256: "a".repeat(64),
            upgrade: None,
        };
        let r = serde_json::json!({"source":{"tap":"author/tap","version":"1.0"},"uninstall_flight_blocks":false,"uninstall_artifacts":[{"app":["Owned.app"]}]});
        let c = serde_json::json!({"default":{"appdir":"/Applications"},"env":{},"explicit":{"appdir":"/Users/demo/Applications"}});
        assert!(validate_fields(&r, &c, &p).is_ok());
        for (key, value) in [
            ("uninstall_flight_blocks", serde_json::json!(true)),
            (
                "uninstall_artifacts",
                serde_json::json!([{"app":["Other.app"]}]),
            ),
            (
                "uninstall_artifacts",
                serde_json::json!([{"app":["Owned.app"]},{"binary":["owned"]}]),
            ),
            (
                "source",
                serde_json::json!({"tap":"other/tap","version":"1.0"}),
            ),
            (
                "source",
                serde_json::json!({"tap":"author/tap","version":"2.0"}),
            ),
        ] {
            let mut changed = r.clone();
            changed[key] = value;
            assert!(validate_fields(&changed, &c, &p).is_err());
        }
        let changed = serde_json::json!({"default":{"appdir":"/Users/demo/Applications"},"explicit":{"appdir":"/Applications"}});
        assert!(validate_fields(&r, &changed, &p).is_err());
    }
}
