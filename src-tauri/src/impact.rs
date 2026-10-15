//! Client explanations of exact directory scopes, never deletion eligibility.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Impact {
    pub version: u32,
    pub kind: String,
    pub title: String,
    pub purpose: String,
    pub consequence: String,
    pub recovery: String,
    pub requires_review: bool,
}

pub fn describe(path: &Path, home: &Path) -> Impact {
    let scope = |relative: &str| path.starts_with(home.join(relative));
    let (kind, title, purpose, consequence, recovery) =
        if scope("Library/Caches/Homebrew/downloads") {
            (
                "downloadCache",
                "Homebrew 下载缓存",
                "该范围通常存放 Homebrew 下载的安装包、归档和清单。",
                "清理后再次安装或升级可能需要联网下载，离线安装可能受影响。",
                "来源或旧版本可能不再可用，不保证能重新取得相同文件。",
            )
        } else if scope(".npm/_cacache") {
            (
                "downloadCache",
                "npm 内容缓存",
                "该范围通常存放 npm 缓存的包内容与索引。",
                "后续安装可能需要重新下载依赖，离线安装可能失败或变慢。",
                "包被撤回或离线时不保证可取回；重新下载不等于恢复原文件。",
            )
        } else if scope("Library/Logs/DiagnosticReports") {
            (
                "diagnostics",
                "崩溃与诊断报告",
                "该范围通常保存已有崩溃、卡顿或系统问题的诊断记录。",
                "清理会失去这次故障的历史证据，仍在排查问题时建议保留。",
                "新报告不能重建过去的崩溃现场，既有报告通常无法重新生成。",
            )
        } else if scope("Library/Logs") {
            (
                "logs",
                "应用运行日志",
                "该范围通常存放应用运行和排查问题的历史记录。",
                "清理可能影响故障追踪、审计或支持人员分析历史问题。",
                "历史日志通常不能重新生成；未来日志不能补回过去的记录。",
            )
        } else if scope("Library/Caches") && path != home.join("Library/Caches") {
            (
                "appCache",
                "应用缓存范围",
                "位置处于应用缓存范围，但实际内容仍由各应用决定。",
                "可能触发重建或重新下载，也可能影响离线使用、会话或数据库状态。",
                "无法统一判断可恢复性；重要会话、离线内容和未同步数据应保留。",
            )
        } else {
            (
                "unknown",
                "用途尚未确认",
                "目前没有与该位置对应的明确用途说明，不能仅凭名称或大小判断。",
                "对应用、项目或个人数据的影响尚不明确，建议先在 Finder 中核对。",
                "尚未确认可恢复性，应保留无法确认用途的重要内容。",
            )
        };
    Impact {
        version: 1,
        kind: kind.into(),
        title: title.into(),
        purpose: purpose.into(),
        consequence: consequence.into(),
        recovery: recovery.into(),
        requires_review: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_scopes_do_not_claim_sibling_paths_or_similarly_named_files() {
        let home = Path::new("/Users/fixture");
        for (path, kind) in [
            ("/Users/fixture/.npm/_cacache/content", "downloadCache"),
            ("/Users/fixture/.npm/_cacache-backup", "unknown"),
            (
                "/Users/fixture/Library/Caches/Homebrew/downloads/file",
                "downloadCache",
            ),
            (
                "/Users/fixture/Library/Caches/Homebrew/downloads-other/file",
                "appCache",
            ),
            (
                "/Users/fixture/Library/Logs/DiagnosticReports/crash",
                "diagnostics",
            ),
            ("/Users/fixture/Library/Logs/DiagnosticReports-old", "logs"),
            ("/Users/fixture/Documents/Cache/file.log", "unknown"),
            ("/Users/fixture-other/Library/Caches/file", "unknown"),
        ] {
            let impact = describe(Path::new(path), home);
            assert_eq!(impact.kind, kind);
            assert!(impact.requires_review);
            assert!(!impact.recovery.is_empty());
        }
    }
}
