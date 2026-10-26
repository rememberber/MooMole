# 优化安装包候选构建

此流程生成可供验收的候选材料，安装、Gatekeeper、许可审阅和首发真实执行门槛仍为 pending。它不会创建公开 Release、发布标签、更新清单或替换已安装 App。

## 入口和顺序

工作流 `.github/workflows/candidate.yml` 在 `codex/release-candidate-*` 分支 push 时运行；另配置手动入口，合入默认分支后再验证手动调度。候选分支使用需要验证的干净提交，例如：

```sh
git push origin HEAD:refs/heads/codex/release-candidate-validation
```

先通过同一提交的现有 Checks（前端全量、两个架构的引擎/资源/适配器/后端测试和 debug 构建），然后在新的 macOS 14 arm64、macOS 15 Intel runner 上各自构建优化 App 和 DMG。Node/Go/Rust 固定为既有检查使用的版本，checkout 不保留凭据；全流程只有 `contents: read` 权限，不继承秘密或访问 Apple 身份服务。

调用现有 `build:local-dmg` 配方，使用 `--no-sign` 中间包、独立副本的最终 ad-hoc 签名和原生签名/ZIP/DMG 挂载核对。没有付费 Developer ID、公证或常驻 helper 的前置要求。每个 runner 必须确认自身架构，不交叉混用开发机缓存或另一架构的引擎。

## 材料绑定与保存

`scripts/candidate-artifacts.mjs` 分两步运行：

1. `stage --report FILE --output NEW_DIR` 核对当前干净提交/版本/架构/固定引擎和构建报告，再逐份验证并复制 App ZIP、DMG 及原始报告。要求物理普通文件、单链接、固定名称/大小/SHA-512；目标必须全新，不覆盖未知文件。它复用构建器的原生验证证据，不独立认证维护者身份或把任意 JSON 当作签名证明。
2. 使用已有源码包和依赖包工具，将两份材料绑定同一原始报告和实际 App ZIP。`seal --output DIR` 核对内外提交、架构、引擎、许可摘要、依赖数量和五份资产摘要，只允许九个预期输入文件；随后写 `CANDIDATE.json` 和覆盖这些文件的 `SHA512SUMS`。未知文件、链接、共享文件、混用材料或输入变化拒绝；失败可能留下部分目录，不能据此认定候选完整。

构建器负责实际 App/嵌套签名/ZIP/DMG 校验，源码和依赖工具负责各自归档验证；汇总器负责对应关系、传输完整性和允许的文件集合，不替代这些检查或完整对应源码审阅。操作前后再次核对源提交和干净状态，不在工作区变化后继续生成可接受结论。

每架构 artifact 只保存明确的候选目录，包含 App ZIP、DMG、构建报告、客户端/完整 Mole 源码归档、依赖归档及对应清单。保留三天，不上传用户数据库、整个工作区或缓存。实际发布前仍须另行完成产物下载复核、支持环境的实际安装/升级、三条真实执行剩余矩阵和许可审阅，最后才发布 GitHub Release 与更新清单。

## 当前验证范围

4 组可移植测试使用明确的元数据/传输夹具，验证准确复制、完整汇总和 33 类拒绝场景；夹具不是真实 App、DMG 或许可正文。另用阶段 154 的真实干净 `e3ee0dc` App/DMG 与原已绑定源码、依赖材料实际执行 stage/seal，九份有效载荷及摘要汇总通过。此项证明已有实际材料的汇总格式兼容；首次远程构建和实际产物结果见下文。


## 首次双架构实际结果（2026-10-26，阶段 157）

源提交 `a4c553c8a1af746a700032d65d52e8fd69ef777a` 的 [Candidate installers 40201866081](https://github.com/rememberber/MooMole/actions/runs/40201866081) 已 completed/success：前端、两个原生检查及两个优化候选构建共五个作业全部成功。不是仅验证工作流配置或旧产物。

| 实际 artifact | 官方外层字节数 | 官方 SHA-256 |
| --- | ---: | --- |
| arm64 `12603136641` | 119,085,131 | `d5f00b646d7f2abe838db4894f90e86a912b7d9e5beb09dfb722519cb9a7684f` |
| x64 `12602742982` | 120,831,979 | `6ce250174900541a6c054f0580d06053a9dbeac7a3024142970a6e7a3a469a3d` |

两份已实际下载并核对官方大小/摘要。每份外层严格 11 个预期文件，九份有效载荷及 SHA512SUMS 全部匹配；客户端 489 项与固定 Mole 299 项源码逐项匹配本地对应 Git 树，326 份公开依赖分发物按归档清单复验。App ZIP 全 CRC 通过，再在本机核对实际三份可执行文件的对应架构、最终 ad-hoc 签名/标识、完整引擎与许可摘要；两个只读 UDZO/HFS+ DMG 均实际挂载核对内容并确认卸载。arm64/x64 DMG 分别为 15,915,905 / 16,851,095 字节。复制原九份材料重新 seal，CANDIDATE.json 和 SHA512SUMS 与 CI 逐字节一致。

私有证据在 `local-artifacts/candidate-ci-157-_zea0eez/`。没有启动两份候选 App 或替换已安装副本；本机的 Intel 文件签名/结构检查也不等于 Intel 原生用户流程。13 项许可核查、完整首选修改源码/对应源码闭包、真实安装升级、公开浏览器下载的 Gatekeeper/权限行为和三条实际执行剩余关口仍 pending；认证下载 Actions artifact 不算客户更新器或公开 Release 下载验收。
