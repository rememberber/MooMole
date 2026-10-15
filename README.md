# MooMole

Mole CLI 的非官方 macOS 可视化客户端学习项目，在 CLI 能力之上提供更人性化的使用流程。使用 React + Vite + TypeScript + Tauri 2 + SQLite，参考简洁的侧栏、卡片与分组设置风格。产品宗旨及各命令的适配进度见 [产品方向](docs/product-direction.md)。

> 本项目仅供学习和研究使用，是独立的非官方项目。使用前请理解操作影响并备份重要数据，由此产生的问题与使用风险由使用者自行承担。
>
> 部分能力基于 [Mole CLI](https://github.com/tw93/Mole)，感谢原作者及贡献者。为尊重和支持 Mole 的开发成果，日常使用建议选择 [Mole 官方产品](https://mole.fit/zh/)。

## 当前版本

第一阶段是可运行的只读桌面原型：

- 概览、磁盘空间、CPU/内存及进程快照。
- 选择目录、真实空间分析、目录下钻、名称筛选及 Finder 定位。
- 通过固定版本 Mole 的 `clean --dry-run` 读取清理预览，去除终端控制序列后展示；尚未完成结构化逐项审阅。
- 测量 Xcode DerivedData、npm 和 Homebrew 缓存，展示影响及保护状态。
- 通过 Mole 获取已安装应用列表与来源。
- Rust 侧 SQLite 设置、保护路径、任务历史及启动后的中断识别。
- 任务取消、进程组清理、输出上限和超时处理。
- 浅色/深色/跟随系统、首次使用说明、关于页与官方产品入口。

**本阶段不提供删除、卸载或系统维护。** 缓存大小是目录测量值，不代表可安全删除或实际可释放空间；无法测量与零占用分别展示，部分扫描标记为 `+`。

客户端保护列表目前作用于补充的开发缓存检查；完整 `clean --dry-run` 遵循 Mole 自有白名单。后续实际清理接口必须统一执行两者的保护约束。

正式发行按无付费 Apple 开发者证书方案推进：架构专用 DMG、ad-hoc 签名、GitHub Releases 下载；后续客户端负责检查、下载和校验更新，用户打开 DMG 手动替换 App。该流程参考 MooTool Electron，仍由 Tauri/Rust 实现。首次打开可能需要用户按 macOS 提示确认，Developer ID 和公证不作为发布前提。当前更新功能尚未实现，详见 [安装升级方案](docs/distribution.md)。

## 环境

- macOS 14+；目前在 Apple Silicon 开发，Intel 尚未验证。
- Node.js 24+、npm。
- Rust stable 与 Xcode Command Line Tools。
- Go 1.26+（用于编译固定版本 Mole 引擎）。

本次开发使用的 Rust/Go 工具链安装于项目 `.cache/`，运行脚本会优先识别它们；新克隆项目仍需自行安装上述工具。`.cache/` 不进入版本控制，也不修改 shell 配置。

## 启动

```bash
npm ci
npm run engine:prepare
npm run desktop
```

`engine:prepare` 从上游获取固定提交并验证 Git HEAD，复制必要 Shell 资源、保留许可证并编译分析/监控二进制。引擎资源生成于 `src-tauri/resources/mole/`，不提交编译产物；应用运行时不下载或更新引擎。

```bash
# 只查看界面：显式展示示例数据，不访问本机文件
npm run dev

# 本地应用包（当前尚未完成正式 DMG 发行与安装升级验证）
npm run tauri -- build --bundles app
```

浏览器预览与桌面端有清楚标记；真实扫描、Finder 和目录选择仅在桌面端可用。若引擎未准备，界面会显示具体原因。

## 检查

```bash
npm test
npm run build
npm run check:rust
npx playwright install chromium
npm run test:e2e
```

Rust 测试覆盖 SQLite 迁移/中断恢复、无效设置拒绝、路径限制、取消和超时。浏览器测试覆盖预览隔离、导航、筛选、主题、设置弹窗及用途说明。

## 结构与数据

```text
src/                  React 界面、类型及受限 IPC 调用
src-tauri/src/db.rs    SQLite 迁移、设置、历史和恢复
src-tauri/src/engine.rs 固定版本 Mole 适配、取消与超时
src-tauri/src/lib.rs   Tauri 命令与应用生命周期
scripts/              引擎准备与开发工具链入口
tests/e2e/            浏览器交互测试
third-party/          上游许可证和来源说明
docs/                 需求、评估、开发记录与界面参考
```

数据库保存在 macOS 应用数据目录 `~/Library/Application Support/io.github.rememberber.moomole/moomole.sqlite`，不保存文件正文或密码。历史默认保留 30 天，可选 7/90 天，启动和保存设置时清理过期记录。未完成的任务在下次启动标记为中断，不自动继续执行。

前端没有通用 Shell、任意 SQL 或直接文件访问入口。Rust 仅启动固定引擎的已知只读命令。Mole 读取应用时可能维护自己的元数据缓存；“只读”指不删除或修改被检查目标，不代表完全没有本地缓存和日志写入。

## 后续阶段

下一阶段优先结构化 Mole 清理结果、设计带文件身份及有效期的清理计划，在隔离测试目录验证“扫描 → 审阅 → 执行前验证 → 按项执行”。随后推进卸载、维护、工程产物和安装包流程，以及引擎历史与长驻状态流。清理接口完成之前不添加可用的删除按钮。缓存测量属于补充功能；客户端持续以 CLI 能力可视化与易用性改进为主线。

## 来源与许可证

Mole CLI 基线：[`ba3a0f57f78e4c22818fda701219cc3dd4d6953f`](https://github.com/tw93/Mole/commit/ba3a0f57f78e4c22818fda701219cc3dd4d6953f)。上游引擎源码未修改，可通过准备脚本在 `.cache/mole-source/` 取得对应源码和依赖记录。应用自己的适配层与界面源码位于本仓库。

本项目按 [GPL-3.0](LICENSE) 提供。上游许可证见 [Mole-LICENSE](third-party/Mole-LICENSE)，来源和构建说明见 [第三方说明](third-party/NOTICE.md)。Mole 名称及品牌资产属于原项目，本项目不代表官方或获得官方背书；公开名称仍需按上游品牌政策确认。
