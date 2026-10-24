# 版本来源与提交检查

2026-10-17。当前增加的是开发构建和提交检查基础，不是正式发布流水线；不创建 Release、发布标签、更新清单或 DMG。

## 唯一版本来源

只编辑根目录 `package.json` 的 `version`，其余版本是必须提交的镜像：

- `package-lock.json` 的顶层及根 package 版本；依赖版本保持原值。
- `src-tauri/Cargo.toml` 的 `[package]` 版本及 `Cargo.lock` 的本地 moomole package 版本。
- `src-tauri/tauri.conf.json` 的应用版本。

关于页直接使用前端导入的 package 版本，不再手写 `0.1.0`。Rust 诊断摘要继续使用 `CARGO_PKG_VERSION`；构建脚本核对 Cargo、package 和 Tauri 版本一致，直接调用 Cargo 也不能构建出这三处版本不一致的 App。

检查当前版本（只读）：

```bash
npm run version:check
npm run version:check -- --tag v0.1.0
```

标签必须精确等于 `v` 加当前版本，不能缺前缀或混用其他版本。这里只校验传入标签，不创建、移动或推送 Git 标签。

以后明确需要调整版本时，在开发分支中操作，例如：

```bash
npm pkg set version=0.2.0-beta.1
npm run version:sync
npm run version:check -- --tag v0.2.0-beta.1
git diff
```

脚本接受严格 SemVer（含预发布及构建标记），不默认提升当前 `0.1.0`。同步保留镜像中的其他字段、依赖、注释和现有排版，只修改应用版本。它针对仓库现有的明确 JSON/TOML 布局，缺失/重复/继承版本或未知锁文件格式会停止，不是通用 TOML 编辑器。

同步先验证所有目标结构并复核输入没有变化，再写镜像；多文件写入不是文件系统事务。意外中断或写入失败后应检查 Git diff，再重跑同步；不会修改源 package 或替用户提交。`npm run build` 及 Tauri 启动入口均先检查镜像，版本不一致时提示同步。

本地 DMG 资产名称已使用该版本来源；未来更新清单、源码包和正式标签仍需接入相同来源。预发布在 Apple 包元数据及升级比较中的完整行为也仍需发行验收。

2026-10-24：新增 [固定提交源码包](source-bundle.md)，版本读取并核对导出提交的相同镜像，不读取未提交版本。CI 已加入跨平台源码归档 fixture；真实远端运行、依赖汇总及最终 App/源码/标签配对仍待完成。

## GitHub Actions

工作流：`.github/workflows/check.yml`。PR、main/master/dev 的 push、`v*` 标签及手动运行会检查；相同 ref 的新运行会取消旧运行。使用 `pull_request`，没有 `pull_request_target`、密钥读取或发布写入；仓库权限为 `contents: read`，checkout 不保留 Git 凭据。

所有 Actions 绑定核对过的完整提交 SHA，不使用浮动分支执行。当前 Node 24.21.0、Go 1.26.0、Rust 1.99.0 固定在工作流中；npm 使用所选 Node 分发自带版本并记录实际版本，npm/Cargo 依赖分别通过 `npm ci`、`cargo test --locked` 固定。系统镜像、Xcode/SDK 和 npm 仍不是完全固定的可重复发布环境。

前端 job 在 Ubuntu 24.04 执行版本/标签检查、Prettier、版本脚本测试、前端单元、TypeScript/Vite 与 Chromium 浏览器回归。浏览器失败证据保留 7 天，只上传测试目录。

原生 job 等前端通过后，分别在 `macos-14`（arm64）与 `macos-15-intel`（x64）运行：检查实际主机架构、安装固定 Rust、从固定 Mole 源码准备引擎、执行资源/架构 fixture 和两组 Shell 适配 fixture、Rust 常规测试，再构建 debug App。使用现有引擎准备脚本，不依赖开发机 `.cache`；新的 runner 仍需要联网取得源码、工具链和依赖。标签与架构对应关系按 [GitHub 官方 runner 表](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) 核对，镜像变化后需再次确认。

成功产物以 `ditto` 将 `.app` 打成 ZIP，保留包结构和可执行权限；同时保存引擎清单、实际工具链版本和 ZIP 的 SHA-256，按架构分开保留 7 天。只上传显式生成的 `ci-artifacts/`，不上传用户数据库、整个缓存或工作区。调试包不是正式安装器，不代表已完成 ad-hoc/嵌套签名、Gatekeeper 或干净机器验收；没有 Apple 证书、Apple ID、公证或付费发布服务的配置要求。

工作流参考官方 [checkout](https://github.com/actions/checkout)、[setup-node](https://github.com/actions/setup-node)、[setup-go](https://github.com/actions/setup-go) 和 [upload-artifact](https://github.com/actions/upload-artifact) 文档。修改工作流时可使用 [actionlint](https://github.com/rhysd/actionlint) 检查；本轮核对版本为 v1.7.12。

## 当前验证范围

版本脚本的 5 组隔离测试覆盖 SemVer、各镜像漂移、标签不符、依赖/注释/排版保留、重复/未知布局和写入前整体校验。另以真实编译出的 Cargo build script 在自建临时目录中分别注入 package/Tauri 版本漂移，均在原生构建开始前拒绝。

本机对应检查与 debug 构建结果见开发记录第三十六阶段。工作流已通过 actionlint 静态检查；尚未推送或触发远端运行，不把配置矩阵当成 Intel/旧系统已验证，也不标记完整 CI/RC 验收完成。首次远端运行、配额/权限/runner 差异、完整安装升级及发布产物验证仍见 [发布路线图](release-roadmap.md)。

后续已增加 [本地优化 App/DMG/免费签名验收](local-app-build.md) 的独立入口；CI 的前端/原生 job 已加入产物测试，Linux 跳过仅 macOS 可运行的签名/DMG fixture。CI 当前仍保存 debug 包，没有自动调用本地优化产物入口或公开发布。


## 浏览器测试服务隔离

Playwright 默认在 127.0.0.1:1431 启动本项目服务，关闭 reuseExistingServer 并传入 --strictPort；1431 已被占用时直接报错，不尝试连接或关闭未知进程。可显式设置 MOOMOLE_E2E_PORT 为 1..65535 的整数字符串选择空闲端口；非法值在启动前拒绝。应用/Tauri 的开发端口仍为 1420，CI 使用相同的独立测试服务。

此约束来自实际环境串用：另一项目占用 1420 时，旧配置把它当作已就绪的 MooMole 服务，导致导航测试在错误页面超时。测试使用浏览器 fixture，无须改变其他项目、原生设置、权限或系统主题。
