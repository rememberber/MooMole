use super::*;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Offer {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub publisher: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ResultProof {
    pub version: String,
    pub publisher: String,
    pub bundle_sha256: String,
    pub receipt_sha256: String,
}
fn valid_url(s: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(s) else {
        return false;
    };
    #[cfg(test)]
    if url.scheme() == "file" {
        return url.to_file_path().is_ok_and(|p| {
            p.starts_with(
                std::env::temp_dir()
                    .canonicalize()
                    .unwrap_or_else(|_| std::env::temp_dir()),
            ) && !p
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir))
        });
    }
    url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && s.len() <= 4096
}
pub(super) fn valid(offer: &Offer, installed: &str) -> bool {
    valid_url(&offer.url)
        && crate::uninstall_plan::hash(&offer.sha256)
        && !offer.publisher.is_empty()
        && offer.publisher.len() <= 128
        && !offer.publisher.chars().any(char::is_control)
        && crate::application_update::version(&offer.version)
            .is_ok_and(|v| crate::application_update::version(installed).is_ok_and(|old| v > old))
}
pub(super) fn offer(row: &serde_json::Value) -> Result<Offer, String> {
    let version = text(&row["version"], 128)?;
    let installed = text(&row["installed"], 128)?;
    if row["outdated"].as_bool() != Some(true)
        || !row
            .get("auto_updates")
            .is_some_and(|v| v.is_null() || v.as_bool() == Some(false))
        || row["disabled"].as_bool() != Some(false)
    {
        return Err("包不属于可核对的普通数值版本升级。".into());
    }
    let deps = row.get("depends_on").ok_or("依赖声明未知。")?;
    if !(deps.is_null()
        || deps
            .as_object()
            .is_some_and(|d| d.keys().all(|k| k == "macos" || k == "arch")))
    {
        return Err("带其他软件依赖的 Cask 需使用原管理工具升级。".into());
    }
    let result = Offer {
        version: version.into(),
        url: text(&row["url"], 4096)?.into(),
        sha256: text(&row["sha256"], 64)?.into(),
        publisher: "pending".into(),
    };
    if !valid(&result, installed) {
        return Err("下载校验或新版顺序无法核对。".into());
    }
    Ok(result)
}
pub(super) fn command(
    tool: &Tool,
    home: &Path,
    token: &str,
) -> Result<tokio::process::Command, String> {
    if !valid_token(token) {
        return Err("包标识无法核对。".into());
    }
    let mut c = tool.command(home);
    c.args(["upgrade", "--cask", "--require-sha", "--no-quit", token]);
    Ok(c)
}
pub(super) fn fetch(
    tool: &Tool,
    home: &Path,
    token: &str,
) -> Result<tokio::process::Command, String> {
    if !valid_token(token) {
        return Err("包标识无法核对。".into());
    }
    let mut c = tool.command(home);
    // `brew fetch` validates the non-optional checksum already bound in the
    // offer. It has no --require-sha switch in Homebrew 7.
    c.args(["fetch", "--cask", token]);
    Ok(c)
}
pub(super) fn after(plan: &Plan, catalog: &Catalog, ctx: &Context) -> Result<ResultProof, String> {
    let offer = plan.draft.upgrade.as_ref().ok_or("升级范围缺失。")?;
    let matches = catalog
        .packages
        .iter()
        .filter(|p| {
            p.token == plan.draft.token && p.path == plan.draft.path && p.version == offer.version
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err("命令返回后没有发现确切新版包登记，结果待核对。".into());
    }
    let path = Path::new(&plan.draft.path);
    let snapshot = identity::capture(path, &plan.draft.bundle_id)?;
    simple_app(path)?;
    if !matches!(
        snapshot.background,
        crate::app_background::Marker::Absent | crate::app_background::Marker::Disabled
    ) {
        return Err("新版应用的后台声明需要核对；App 可能已替换，不会自动重试或回滚。".into());
    }
    if crate::application_update::version(
        snapshot
            .app_version
            .as_deref()
            .ok_or("新版 App 缺少版本。")?,
    )? != crate::application_update::version(&offer.version)?
    {
        return Err("App 实际版本与包登记不一致，可能已经替换，请核对。".into());
    }
    plan.publisher
        .as_ref()
        .ok_or("原发布者证据缺失。")?
        .verify(path)?;
    let receipt = receipt::capture(&plan.tool, matches[0], ctx)?;
    if identity::capture(path, &plan.draft.bundle_id)? != snapshot {
        return Err("升级核对期间 App 再次变化，请核对。".into());
    }
    Ok(ResultProof {
        version: offer.version.clone(),
        publisher: offer.publisher.clone(),
        bundle_sha256: format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?)
        ),
        receipt_sha256: receipt.digest().into(),
    })
}
