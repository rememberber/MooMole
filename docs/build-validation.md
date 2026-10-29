# 版本来源与提交检查

> 2026-10-29：发布与日常检查已分离，当前入口和顺序以 [发布约定](../RELEASE_CONVENTIONS.md) 为准。Checks 前端/两架构原生作业现在并行运行，不再生成 debug App；版本标签只触发自动发布，不再叠加完整 Checks。下文带日期的历史运行证据保持原样。

2026-10-17。当前增加的是开发构建和提交检查基础，不是正式发布流水线；不创建 Release、发布标签、更新清单或 DMG。

## 唯一版本来源

只编辑根目录 `package.json` 的 `version`，其余版本是必须提交的镜像：

- `package-lock.json` 的顶层及根 package 版本；依赖版本保持原值。
- `src-tauri/Cargo.toml` 的 `[package]` 版本及 `Cargo.lock` 的本地 moomole package 版本。
- `src-tauri/tauri.conf.json` 的应用版本。
- `third-party/npm-source-origins.json` 中绑定 `package-lock.json` 原始字节的 SHA-256；升版同步更新此摘要，依赖来源和清单保持原值。

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

脚本接受严格 SemVer（含预发布及构建标记），不默认提升版本。同步保留镜像中的其他字段、依赖、注释和现有排版，只修改应用版本及对应锁文件摘要。它针对仓库现有的明确 JSON/TOML 布局，缺失/重复/继承版本或未知锁文件格式会停止，不是通用 TOML 编辑器。

2026-10-29：0.2.0 首次候选在 npm 源码封装时发现应用升版使锁文件摘要变化、来源清单仍绑定旧值。版本检查现已提前核对绑定，同步仅在修改前的锁文件仍匹配已审阅摘要时更新新摘要；如果依赖或锁文件字节已发生未审阅变化，同步拒绝写入，须先单独核查来源。此次已与 v0.1.0 的原锁文件逐项比较，确认只有顶层/根 package 的应用版本变化后修正绑定，未重新认可或替换任何依赖来源。

同步先验证所有目标结构并复核输入没有变化，再写镜像；多文件写入不是文件系统事务。意外中断或写入失败后应检查 Git diff，再重跑同步；不会修改源 package 或替用户提交。`npm run build` 及 Tauri 启动入口均先检查镜像，版本不一致时提示同步。

本地 DMG 和固定提交源码包的资产名称已使用该版本来源；实际更新清单和正式发布标签仍需核对相同来源。预发布在 Apple 包元数据及升级比较中的完整行为也仍需发行验收。

2026-10-24：新增 [固定提交源码包](source-bundle.md)，版本读取并核对导出提交的相同镜像，不读取未提交版本。CI 已加入跨平台源码归档 fixture；阶段 154 两个原生作业的该项均通过，最终发行 App/源码/标签及完整依赖审阅仍待完成。

## GitHub Actions

工作流：`.github/workflows/check.yml`。PR、main/master/dev 的 push、`v*` 标签及手动运行会检查；相同 ref 的新运行会取消旧运行。使用 `pull_request`，没有 `pull_request_target`、密钥读取或发布写入；仓库权限为 `contents: read`，checkout 不保留 Git 凭据。

所有 Actions 绑定核对过的完整提交 SHA，不使用浮动分支执行。当前 Node 24.21.0、Go 1.26.0、Rust 1.99.0 固定在工作流中；npm 使用所选 Node 分发自带版本并记录实际版本，npm/Cargo 依赖分别通过 `npm ci`、`cargo test --locked` 固定。系统镜像、Xcode/SDK 和 npm 仍不是完全固定的可重复发布环境。

前端 job 在 Ubuntu 24.04 执行版本/标签检查、Prettier、版本脚本测试、前端单元、TypeScript/Vite 与 Chromium 浏览器回归。浏览器失败证据保留 7 天，只上传测试目录。

原生 job 等前端通过后，分别在 `macos-14`（arm64）与 `macos-15-intel`（x64）运行：检查实际主机架构、安装固定 Rust、从固定 Mole 源码准备引擎、执行资源/架构 fixture 和两组 Shell 适配 fixture、Rust 常规测试，再构建 debug App。使用现有引擎准备脚本，不依赖开发机 `.cache`；新的 runner 仍需要联网取得源码、工具链和依赖。标签与架构对应关系按 [GitHub 官方 runner 表](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) 核对，镜像变化后需再次确认。

成功产物以 `ditto` 将 `.app` 打成 ZIP，保留包结构和可执行权限；同时保存引擎清单、实际工具链版本和 ZIP 的 SHA-256，按架构分开保留 7 天。只上传显式生成的 `ci-artifacts/`，不上传用户数据库、整个缓存或工作区。调试包不是正式安装器，不代表已完成 ad-hoc/嵌套签名、Gatekeeper 或干净机器验收；没有 Apple 证书、Apple ID、公证或付费发布服务的配置要求。

工作流参考官方 [checkout](https://github.com/actions/checkout)、[setup-node](https://github.com/actions/setup-node)、[setup-go](https://github.com/actions/setup-go) 和 [upload-artifact](https://github.com/actions/upload-artifact) 文档。修改工作流时可使用 [actionlint](https://github.com/rhysd/actionlint) 检查；本轮核对版本为 v1.7.12。

## 当前验证范围

版本脚本的 5 组隔离测试覆盖 SemVer、各镜像漂移、标签不符、依赖/注释/排版保留、重复/未知布局和写入前整体校验。另以真实编译出的 Cargo build script 在自建临时目录中分别注入 package/Tauri 版本漂移，均在原生构建开始前拒绝。

本机初始对应检查与 debug 构建结果见开发记录第三十六阶段，工作流已通过 actionlint 静态检查。2026-10-25 已实际推送并运行远程检查，不能继续将其描述为尚未触发：

| 源提交与运行 | 实际结果 | 尚未证明 |
| --- | --- | --- |
| `27a5cb0` / [40190509571](https://github.com/rememberber/MooMole/actions/runs/40190509571) | 前端浏览器 272 通过、2 项旧约定失败；原生矩阵被跳过 | 不算双架构验证 |
| `e3ee0dc` / [40192723897](https://github.com/rememberber/MooMole/actions/runs/40192723897) | 前端 155 单元、274 浏览器通过；macOS 14 arm64 后端 527 通过、62 显式忽略，debug App 构建/归档成功；macOS 15 Intel 后端 520 通过、7 失败、62 显式忽略，未构建 App | 整体 CI 失败；不算 Intel 构建、发行 DMG、原生用户操作或干净机器验收 |
| `4670961` / [40195691399](https://github.com/rememberber/MooMole/actions/runs/40195691399) | 整体成功；前端 155 单元、274 浏览器通过；arm64 和 Intel 分别 527 后端通过、62 显式忽略，两份 debug App 构建与归档成功 | 不算两种架构的优化发行 DMG、原生用户操作、安装升级或干净机器验收 |

串行仅隔离独立测试之间的资源使用，测试自身主动制造的并发继续执行；不续期生产观察、不移除断言或增加忽略项。原 Intel 7 项失败在新一轮全部通过；它证明调整后的完整测试可通过，未单独证明原拒绝的具体原因。原失败与本机串行结果见 [阶段 154](development.md#阶段-154远程检查收尾与-api-许可来源关联)。完整安装升级、最低支持环境及发布产物验证仍见 [发布路线图](release-roadmap.md)。

阶段 155 已实际下载本轮两份 artifact，官方大小/SHA-256、内层 App ZIP 的 SHA256SUMS/完整 CRC 和资源检查均通过。实际 App 版本/标识、三份 Mach-O 的对应架构、完整内嵌引擎和许可摘要相符；这补齐产物读回证据，不是仅看配置或绿色作业。没有启动或验证其发行签名，仍为 debug 证据，详见 [实际产物核对](development.md#阶段-155双架构远程检查与实际产物核对)。

后续已增加 [本地优化 App/DMG/免费签名验收](local-app-build.md) 的独立入口；CI 的前端/原生 job 已加入产物测试，Linux 跳过仅 macOS 可运行的签名/DMG fixture。CI 当前仍保存 debug 包，没有自动调用本地优化产物入口或公开发布。

阶段 156 新增独立 [候选安装包构建](candidate-build.md)：以 `workflow_call` 复用同一提交的 Checks，通过后再在干净双架构 runner 调用既有优化 DMG 配方，绑定源码/依赖并只保存候选 artifact。默认提交检查仍只保存 debug 包；候选流程的新增原生结果须以实际运行验证，不根据 YAML 或夹具标记为通过。复用方式见 [GitHub 官方文档](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows)。

阶段 157 已核对 `a4c553c` 的 [40201866081](https://github.com/rememberber/MooMole/actions/runs/40201866081)：同提交检查与两个优化候选作业共五个全部成功。实际下载 arm64/x64 两份候选后，官方摘要、源码/依赖配对、嵌套 ad-hoc 签名、只读 DMG 挂载/卸载及候选汇总再生成均通过。默认 Checks 仍保存 debug 包，优化产物来自独立候选作业；真实安装/升级、Gatekeeper 和完整许可闭包仍待完成，详见 [候选实际结果](candidate-build.md#首次双架构实际结果2026-10-26阶段-157)。


## 浏览器测试服务隔离

Playwright 默认在 127.0.0.1:1431 启动本项目服务，关闭 reuseExistingServer 并传入 --strictPort；1431 已被占用时直接报错，不尝试连接或关闭未知进程。可显式设置 MOOMOLE_E2E_PORT 为 1..65535 的整数字符串选择空闲端口；非法值在启动前拒绝。应用/Tauri 的开发端口仍为 1420，CI 使用相同的独立测试服务。

此约束来自实际环境串用：另一项目占用 1420 时，旧配置把它当作已就绪的 MooMole 服务，导致导航测试在错误页面超时。测试使用浏览器 fixture，无须改变其他项目、原生设置、权限或系统主题。


2026-10-26：维护停止原因修正的 `5db9282` / [40253783857](https://github.com/rememberber/MooMole/actions/runs/40253783857) 前端与两个原生作业均 completed/success。源码输入候选 `09aa8bb` / [40255171488](https://github.com/rememberber/MooMole/actions/runs/40255171488) 的前端、双架构原生检查与两个优化候选作业现已全部成功；新增 npm 源码输入已在干净 runner 实际打包/绑定，但新产物下载读回尚待完成。核查修正 26cc757 的 [40259126051](https://github.com/rememberber/MooMole/actions/runs/40259126051) 是独立的新候选，五个作业也已全部 completed/success；阶段 163 已实际下载复核其两架构 App/DMG、全部材料及 npm 原始输入，原生签名/只读挂载/推出与重新汇总通过，见 [实际候选结果](candidate-build.md#v7-与源码输入的双架构候选2026-10-26阶段-163)。不推造未读取原日志的测试数量。


阶段 162 的后续界面修正 `7d1633d` / [40278309175](https://github.com/rememberber/MooMole/actions/runs/40278309175) 前端与两个原生作业最终均 completed/success；它不是上述已下载的 26cc757 候选。本机干净 7d1633d 的 arm64 release App、三处最终 ad-hoc 及 ZIP 解压复验通过，安装及 Gatekeeper 仍 pending。

阶段 164 的 `6dd4785` / [40284567893](https://github.com/rememberber/MooMole/actions/runs/40284567893) 前端和 arm64 成功；Intel 后端 542 通过、1 失败、63 明确可选，未构建候选。失败是维护观察者断开测试的 8 秒清理轮询超时，旧日志不能区分真实执行尚未结束与注册表未释放。阶段 166 将真实执行与结果发布/清理分成独立屏障：执行观察采用原确认的 30 秒范围，不续期；保留原 8 秒清理界限，并真正撤下测试自身的数据库句柄，验证仅后台 worker 持有占用。生产代码和期限不变。本机相关 8 项及带阶段诊断的单项通过，Intel 新提交的完整复验另行跟踪；不把此本机结果当作远程已修复。

阶段 168：`2285173` / [40289653976](https://github.com/rememberber/MooMole/actions/runs/40289653976) 前端作业被取消，GitHub check annotation 明确为整作业超过 15 分钟；浏览器 291 项开始时仅剩约 4 分 43 秒，取消前没有断言失败，不算全部通过。原生与优化候选均跳过，因此尚未验证阶段 166 的 Intel 修正。只将前端整作业预算由 15 调为 30 分钟，为安装与完整检查留出空间；不删测试、不改变单项期限或产品执行时效。新提交重新执行完整候选流水线，结果以远程终态为准。

阶段 168 复验已读回实际原日志：fd48157 / 40320305923 前端 35 个文件、161 项单元和 291 项浏览器通过（浏览器 8.8 分钟）；两个原生检查均 success。Intel 后端 543 通过、0 失败、63 明确可选，原 observer_loss_retains_job_update_and_database_ownership_until_worker_finishes 已通过。证明这次完整复验通过，不追溯断言旧失败的唯一根因。arm64 优化候选已成功并开始实际下载，Intel 优化作业仍单独跟踪；未据检查通过宣称两份发行资产或公开发布完成。
