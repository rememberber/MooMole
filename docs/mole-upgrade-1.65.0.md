# Mole CLI 1.65.0 升级

2026-10-29，通过 GitHub Releases API 与远端 tag 核对，最新 macOS 稳定版为 [V1.65.0](https://github.com/tw93/Mole/releases/tag/V1.65.0)，发布于 2026-10-27。固定提交为 `308bf9911179ddfa40e5df9fdc42095a18ba5cfc`，替换原 `1.59.1` / `ba3a0f57f78e4c22818fda701219cc3dd4d6953f`。不使用上游安装器的 `latest` 参数，它是未发布 main 的旧别名。

## 适配审阅

- 准备脚本继续从固定 Git 树导出未修改的上游源码，并重建 analyze/status 两个 Darwin Go 程序。清理协议 v2、来源 v1、rules v4 和模块观察 v2 保持兼容；适配脚本、Rust、前端证据及测试同步提交绑定。
- 清理仍读取结构化 NUL ledger。上游改为估算文件块、排除根目录以外的硬链接和共享 APFS 块；客户端继续显示测量与未知边界，移入废纸篓不等于释放空间。用户缓存/日志实际调用链、guard 和当次身份/占用复查由隔离样例验证。
- 卸载规则七份源文件的摘要已复核更新。新版 Steam 根及其游戏/账户子目录保护接入客户端固定规则解析，沿用 criticalData 保留原因；原上游谓词与客户端逐路径对照，包括大小写、子路径、相似兄弟名称和特殊 HOME。
- 上游新增 UUID 容器元数据、浏览器 NativeMessagingHosts、JetBrains selector、更新器/崩溃报告及 Claude-3p 残留识别。客户端关联位置仍是已声明的有界子集，普通应用执行仍保留包外数据；本次不把新发现能力当作已实现的完整卸载。
- 维护仍为 19 项目录，Quick Look 两步执行的上游顺序未变。偏好修复现在保留不可读文件并使用系统 plutil；DNS 使用固定系统服务标签且系统拒绝信号时跳过。这些尚未开放的任务不因摘要更新而获得执行入口。
- purge 的 34 个名称、项目/工作区标记，以及 installer 的六种扩展名、12 个位置、深度与 ZIP 窗口未变；对应源文件摘要已更新，保留源码漂移门禁。
- 新增 `slim`、`uninstall --with-data`、`optimize -n`、`purge -h` 及 update/remove 帮助入口已同步[能力映射](mole-capability-map.md)。slim 与个人数据预选暂不提供 GUI 入口。
- analyze 显式目录 JSON、status JSON/NDJSON 和 history JSON 维持客户端契约。history 的操作日志输入从 `MOLE_OPERATIONS_LOG` 改为 `OPERATIONS_LOG_FILE`，客户端已修正，继续只向上游读取器传私有副本并核对返回来源。上游内嵌 bundle ID helper 改用 ASCII 转小写，源码对照测试同步提取真实依赖函数。

## 验证

引擎准备及离线重建成功，清单和两个 Mach-O 架构校验通过。Go 使用项目现有的 `go1.26.0 darwin/arm64` 与 `GOTOOLCHAIN=local`；新版 go.mod 的 `go 1.26.0` 下限未变，`toolchain go1.26.10` 建议未触发自动下载，清单如实记录实际编译器。

- 前端 190 项、默认 Rust 560 项（69 项显式集成/原生测试继续默认忽略）、上游源码对照 22 项通过。
- 历史读取 6 项（含两项真实上游读取器隔离测试）、真实 status warm collector 连续快照/停止通过。首次历史集成测试暴露旧变量失效，修正后两项重跑通过。
- 引擎完整性/原子发布 5 项及维护、工程产物、安装包三个目录门禁通过；两套 Shell fixture 通过，文件保持原内容。
- 分发物、候选、源码包、依赖声明及材料相关测试 29 项通过；相关浏览器界面测试 115 项通过。TypeScript/Vite 构建、三语目录、Prettier、Rust 格式和 diff 检查通过。
- 最终 arm64 release App 在 `local-artifacts/MooMole-0.1.0-mac-arm64-nUgDwF/`，本地 ad-hoc 签名、嵌套可执行文件、资源清单及 App ZIP 解包校验通过。没有创建公开发布或替换安装目录中的 App。
- 空闲旧实例正常退出后，启动上述最终 App，首页显示 `v1.65.0` / `308bf9911179` / 引擎就绪；真实系统快照完成。自建 `.cache/mole-upgrade-smoke` 返回 3 个直接子项/31 B，包含中文目录、26 B 文件及 0 B 空文件。首次输入被 UI 键入截短，改用粘贴并核对完整路径后实际扫描通过，未扫描其他目录。
- 原生历史读取完成并如实显示当前账户两份 Mole 原始日志缺失，已有客户端计划/执行记录仍显示；没有注入模拟数据或实际执行清理、卸载与维护。macOS CI 增加 22 项源码对照及两项私有历史输入测试。

本次验证结果与原生产发布矩阵分开记录；既有 Intel、旧系统、正式安装升级和三个真实执行模块的首发验收仍按发布路线图推进。旧开发日志与按日期记录的技术说明保留其原 1.59.1 基线，当前引擎以本页、清单、能力映射及源码为准。
