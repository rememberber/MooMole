//! Shared preparation for read-only observations within discovered artifact scopes.
use super::{catalog, MAX_DEPTH};
use crate::observation::AnchoredPath;
use std::path::Path;
pub(super) fn open(root: &Path, path: &Path) -> Result<(AnchoredPath, AnchoredPath), String> {
    let names = catalog()?.targets;
    let valid = |p: &Path| {
        p.to_str().is_some_and(|s| {
            s.len() <= 4096
                && s.starts_with('/')
                && !s.ends_with('/')
                && !s.chars().any(char::is_control)
                && !s[1..].split('/').any(|v| ["", ".", ".."].contains(&v))
        })
    };
    if !valid(root)
        || !valid(path)
        || root
            .file_name()
            .and_then(|v| v.to_str())
            .is_some_and(|n| names.iter().any(|v| v == n))
    {
        return Err("请选择有效的项目范围并重新扫描。".into());
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "测量目录不在所选项目范围内。")?;
    let parts = relative
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect::<Vec<_>>();
    if parts.is_empty()
        || parts.len() > MAX_DEPTH
        || !names.iter().any(|v| v == parts[parts.len() - 1])
        || parts[..parts.len() - 1].iter().any(|n| {
            n.starts_with('.')
                || ["Library", "Applications"].contains(n)
                || names.iter().any(|v| v == n)
        })
    {
        return Err("测量目录不属于本次名称发现范围。".into());
    }
    let scope = AnchoredPath::open(root)?;
    let target = AnchoredPath::open(path)?;
    if !scope.target().metadata().is_ok_and(|m| m.is_dir())
        || !target.target().metadata().is_ok_and(|m| m.is_dir())
        || !scope.local_materialized()
        || !target.local_materialized()
    {
        return Err("只能测量已下载到本机的物理目录。".into());
    }
    Ok((scope, target))
}
