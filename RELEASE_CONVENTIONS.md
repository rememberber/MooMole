# MooMole 发布约定

发布和更新自动化沿用并级 MooTool Electron 的结构：`prepare → package → release → publish-update-manifest`。工作流为 `.github/workflows/release.yml`，统一脚本为 `scripts/manage_releases.py`。保持 Tauri 技术栈和现有单产品更新协议。

## 日常发布

1. 只修改 `package.json` 的版本，运行 `npm run version:sync`。它同步 Cargo/Tauri/锁文件和 npm 源码来源的锁文件摘要。
2. 在 `release-notes/{version}.md` 编写说明，首行为 `# MooMole {version}`，依次包含 `## English`、`## 中文`、`## 日本語`。这是 GitHub Release 和更新清单的唯一人工说明来源，UTF-8 正文不超过客户端允许的 16 KiB。
3. 完成当前功能和文档修改后提交到 main，推送 main 和版本标签，例如：

```sh
# 示例版本，按实际下一版本替换
npm pkg set version=0.2.1
npm run version:sync
# 编写 release-notes/0.2.1.md
# 可选的本地快速预检；CI prepare 会自动运行同一项
npm run release:check -- --tag v0.2.1
git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json third-party/npm-source-origins.json release-notes/0.2.1.md
git commit -m "release: prepare 0.2.1"
git tag v0.2.1
git push origin main v0.2.1
```

发布过程由 GitHub runner 完成，不要求开发者下载 Actions 大包、逐项重新审阅源码、在本机重新封装/上传，再下载一遍公开包。已经发布的 0.2.0 标签、二进制与 Release 保持原样。

## 自动完成的步骤

- **prepare**：快速核对应用版本、标签和三语说明，避免打包结束后才发现缺少发布说明或版本漂移。不运行 npm 安装、浏览器回归或 Rust 全量测试。
- **package**：arm64/x64 同时在各自 macOS runner 上生成一次优化 App 和 DMG。使用 npm、Go 和 Rust/Cargo 缓存，不先等待前端测试或另一个架构，也不先构建一份 debug App。保留现有免费 ad-hoc 签名、基本架构/资源和 DMG 封装处理，以及对应源码与依赖材料。
- **release**：直接在 GitHub 内汇总构建产物，生成 SHA512SUMS，使用与 MooTool 相同的 `softprops/action-gh-release` 发布三语正文、两个 DMG 和两个完整 materials ZIP。稳定版设为 Latest，预发布版不设为 Latest。构建与最终打包只做一次，不经过开发机中转。
- **publish-update-manifest**：Release 成功后，只下载几 KB 的发布元数据，通过 GitHub API 确认资产名、大小与 SHA-256 对应，再生成客户端需要的大小/SHA-512 和三语说明。保留旧发布记录，自动更新三份 README 的版本与下载区块并提交 main。这里不重复下载几百 MB 的材料。

README 中 `release`、`downloads` 注释区块由发布工具维护；功能表、截图、风险、上游致谢和其余文字仍随功能开发维护。预发布版和低于已有稳定版的补丁不会把 README 下载入口降级。

## 构建、测试与恢复

- Actions 中手动运行 **Build and release MooMole** 可以只打包，目标可选 `all`、`arm64` 或 `x64`。与 MooTool 一样，手动构建不发布 Release、不改更新清单。
- 常规 **Checks** 独立于发布：前端和两架构原生检查并行。标签不再额外触发一套 Checks；纯 README、文档、发布说明或更新清单改动不触发完整回归。检查中不再生成、压缩和上传 debug App。
- 发布主流程不串行重跑 320 项浏览器测试、560 项 Rust 测试和源码逐项对照，也不重复本机 re-seal、签名/DMG 挂载验收及全部公开包下载复验。需要专项验收时仍可使用现有本地产物工具和测试命令。
- 失败后在 Actions 选择 **Re-run failed jobs**，继续使用本次成功架构的产物；保存期为 14 天。更新清单作业失败时只重跑该作业，不重新构建二进制。不要移动已经发布的标签来“修复”版本；代码修复应使用新版本。
- 资产仍必须齐全、来源提交/架构匹配，清单在 Release 上传成功后才生效。客户端实际下载仍严格校验字节数/SHA-512，仍由用户打开 DMG 并手动替换；不会关闭 Gatekeeper、移除安全属性或静默覆盖 App。清理、卸载和维护的运行时保护未作放宽。

首次使用新缓存仍可能需要完整编译。上述变更消除的是重复等待和本地网络搬运，不承诺一个未经实测的固定发布时间；冷/热缓存的实际耗时以新工作流记录为准。

## 参考

2026-10-29 核对 MooTool 提交 `d12cd67b69f0a6195f6ff9734fe71fbc893224f1`：仓库 `RELEASE_CONVENTIONS.md`、`.github/workflows/next-build-installers.yml`、`scripts/manage_releases.py`、`next/doc/update-products-and-assets.md`；Rust 缓存参考同仓库 `.github/workflows/next-tauri-ci.yml`。这里只借鉴分层、说明来源、缓存和发布顺序，不引入 Electron 运行时或 MooTool 的多产品更新 JSON。
