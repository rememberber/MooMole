# 1.0 功能边界与 Mole CLI 能力映射

2026-10-16。源码基线固定为 Mole `1.59.1` / `ba3a0f57f78e4c22818fda701219cc3dd4d6953f`。本表核对该提交的实际解析代码，不代表其他版本，也不把规划标成已实现。

## 1.0 范围基线

| 模块 | 1.0 要交付的完整流程 | 当前状态 |
| --- | --- | --- |
| 清理 | 结构化扫描 → 用途/影响与保护说明 → 明确选中 → 当次规则及身份复核 → 按项处理 → 结果与恢复边界 | 扫描/审阅/保护与有限模块复核已接入，来源明确的单文件日志已有受限确认/执行循环；本机实际占用未知仍保留，发布 App 验收未完成 |
| 应用卸载 | 应用清单 → 精确归属的关联数据 → 个人数据默认保留 → 运行状态确认 → 选中卸载 → 按项结果 | 只读清单已接入 |
| 磁盘分析 | 选择范围 → 完整性与测量说明 → 分页/筛选/图形联动/下钻 → Finder 定位；可修改范围沿用统一计划 | 已有只读空间图/列表联动、分页/筛选、会话导航及查阅上下文恢复、读取来源、返回子项完整性及 Mole 大文件独立查阅/筛选/定位；完整发现与完整测量矩阵待补 |
| 系统状态 | 快照与单进程 NDJSON 流 → 有限长度图表 → 暂停/恢复与休眠策略 → 缺失/陈旧说明 | 快照及可选 1/2/5 秒的单进程流、暂停/重开、有界趋势和磁盘/逐接口网络速率已接入；完整资源/休眠及其余指标待完成 |
| 维护 | 维护目录与预览 → 条件/权限/影响说明 → 逐项选择 → 按需授权 → 实际结果 | 已提供 19 项说明、Finder/旧覆盖两项设置与项目级保护、三固定文件的路径保护/访问观察；其余条件、实际写入权限、选择、恢复和执行待接入 |
| 工程产物 | 项目根目录 → 结构化候选 → 活动构建/源码/Git 保护 → 选中处理 → 结果 | 已接入选择目录、按需读取 Mole 已保存范围、34 个固定名称有界发现、Node/Rust 线索、范围内项目/工作区标记分组与筛选/组内排序/分页、客户端保护、逐项文件占用/不完整原因与仓库/密钥名称观察、按需普通 Git 索引记录与空间分析跳转；完整归属/规则与执行待接入 |
| 安装包 | 明确扫描范围 → 包类型/占用状态 → 保护挂载与活动映像 → 选中处理 → 结果 | 已有 12 个固定位置/自选目录的明确范围、筛选/排序及六类扩展名只读文件线索、逻辑大小/未知/保护及 Finder 定位；ZIP 可逐项检查有界列表的安装条目线索及按需数据/CRC 测试，DMG 可手动核对内部 CRC32；DMG/ISO 已有系统连接路径观察，六类文件已有可见进程占用线索；映像身份/完整占用、实际格式与处理待接入 |
| 历史与本地数据 | 客户端任务与 Mole 操作记录分来源 → 搜索/筛选/详情 → 本地保留与清除 → 脱敏诊断预览/主动导出 | 客户端历史、任务接回、只读计划及执行记录层/只读详情已接入，真实执行器未连接 |
| 安装升级 | 免费架构专用 DMG → 校验下载 → 用户打开并手动替换 → 数据迁移/权限及新版启动检查 | 本地 debug App 已验证，正式分发待做 |

以上是验收范围，不是当前可用性承诺。某项只有清单、预览或实验不能算交付了完整流程。用户明确选择的对象必须和实际处理对应；永久删除、清空废纸篓和不可撤销维护分别定义协议、影响和确认，不能代替受限执行端。

首发界面以简体中文为基线；原始引擎字段/日志保留来源，后续统一术语与无障碍验收。当前目标为 macOS 14+，按 arm64/x64 分包设计；目前只有 Apple Silicon / macOS 27.0.1 实测，Intel 与旧系统验证仍为发布条件。公开名称、品牌许可边界、图标、维护联系渠道不在此擅自定案。

无障碍验收目标为核心流程键盘可达、焦点清晰、状态不只依靠颜色、VoiceOver 能读出任务/选择/错误；尚需真实验证。初始分发渠道采用 GitHub Releases；历史默认本地保留 30 天，可选 7/90 天，不上传用户路径或日志，诊断导出先预览并主动确认。对外地址和维护身份另行明确。

Windows/Linux、账号/云同步、支付订阅、AI 自动清理、无人值守删除、风扇控制和菜单栏扩展不进入默认 1.0 必交范围。付费 Apple 开发者账户不是前置条件；遵循 [免费分发方案](distribution.md)。

## 全局入口和终端专属操作

“已接入”仅指表内明确范围；“待接入”属于上述模块的后续工作；“GUI 替代”指交互意图被客户端流程承担，不表示将参数原样传给 CLI。

| CLI 入口/参数 | 客户端处理 | 依据 |
| --- | --- | --- |
| `mole` / `mo` 无参数交互菜单 | GUI 导航替代，不启动终端菜单 | `mole:interactive_main_menu/main` |
| `help`、`--help`、`-h` | GUI 使用说明替代；具体错误与模块说明继续补齐 | `mole:main` |
| `version`、`--version`、`-V` | 已在首页/关于显示内置版本与源码基线，不依赖启动 CLI 取版本 | `mole:VERSION/show_version/main` |
| 全局 `--debug` | 不透传任意参数；原始清理预览可查看，脱敏诊断预览/导出待做 | `mole:mole_collect_cli_args` |
| `update [--force|-f] [--nightly]` | 内置 CLI 自更新不适用；App 与引擎成套升级，客户端手动 DMG 更新另行实现 | `mole:main` |
| `remove [--dry-run|-n]` | 内置 CLI 自删除不适用；应用卸载/本地数据保留说明另行提供 | `mole:main` |
| `completion` 无参数、`bash` / `zsh` / `fish`、`--dry-run|-n` | 终端补全安装/生成不适用，不修改 shell 配置 | `bin/completion.sh` 参数归一化与末尾 `case` |
| `touchid [enable|disable|status] [--dry-run|-n] [--help|-h]` | PAM/Touch ID 配置不进入默认首发，单独评估；不作为清理授权前置条件 | `bin/touchid.sh:main` |

## 清理、卸载与维护

| CLI 参数/输入 | 客户端状态与处理 | 依据 |
| --- | --- | --- |
| `clean --dry-run` / `-n` | 已接入固定版本 NUL ledger，只读候选与原始日志，跳过需要管理员权限的系统项目 | `bin/clean.sh:main`；客户端 `mole-clean-preview.sh` |
| `clean` 无参数 | 已有有限日志按项确认/执行，本机占用未知仍保留；其他范围与发布 App 权限验收待完成，不能重新跑全量 clean 替代计划 | 同上 |
| `clean --external PATH` | 待接入外接卷范围及卸载/重挂载身份检查；当前清理页不提供 | 同上 |
| `clean --whitelist` | Mole 有效规则只读复查已接入；管理界面待做。客户端保护列表单独保存，不等同于修改上游白名单 | 同上；`lib/manage/whitelist.sh` |
| `clean --debug`、`--help|-h` | 全局诊断/模块说明替代 | `bin/clean.sh:main` |
| `clean --select`、`--categories`、`--exclude` | 此提交已移除并报错。GUI 勾选/筛选是对结构化候选的客户端操作，不宣称调用这些参数 | `bin/clean.sh:main` 明确拒绝分支 |
| `uninstall --list` | 已接入只读 JSON 应用清单；不执行卸载 | `bin/uninstall.sh:main` 的 list 短路分支 |
| `uninstall [APP_NAME…]` | 待接入应用身份选择；不以模糊名字作为关联文件删除依据 | `bin/uninstall.sh:main` |
| `uninstall --dry-run|-n` | 关联文件结构化预览待适配；当前仅应用清单不等于卸载计划 | 同上 |
| `uninstall --permanent` | 不开放直接透传；永久删除另需明确协议、确认和结果记录 | 同上；默认删除方式为 trash 的设置 |
| `uninstall --debug`、`--help|-h` | 全局诊断/模块说明替代 | 同上 |
| `uninstall --whitelist` | 此提交不支持；不能在 GUI 暗中调用或声称具备该能力 | 同上明确拒绝分支 |
| `optimize` / `optimise`、`--dry-run` | 已提供 19 项作用/影响说明，两项设置/项目与文件保护的只读观察、选择及限时条件审阅；不运行入口或 dry-run，不开放执行。其余条件及执行待适配 | `mole:main`；`bin/optimize.sh:main`；`lib/optimize/catalog.sh` 与处理函数；[维护边界](system-maintenance.md)、[审阅协议](maintenance-review.md) |
| `optimize --whitelist` | 待适配维护保护规则；和 clean 的规则用途区分 | `bin/optimize.sh:main` |
| `optimize --debug`、`--help|-h` | 全局诊断/模块说明替代 | 同上；该分支没有 `-n` 别名 |

## 分析、状态、工程产物、安装包和历史

| CLI 参数/输入 | 客户端状态与处理 | 依据 |
| --- | --- | --- |
| `analyze` / `analyse [PATH] --json` | 已接入显式目录 JSON，路径选择/下钻替代 TUI；GUI 始终明确传目录。CLI 无路径的全机概览不等同于直接扫描 `/`，当前未接入全机概览 | `cmd/analyze/main.go:parseArgs/resolveScanTarget` |
| `analyze --help|-h` | 模块说明替代；输入路径转为规范绝对目录 | 同上 |
| `status --json` | 已接入一次快照 | `cmd/status/main.go:parseArgs/main` |
| `status --watch --interval DURATION` | 已接入单一 warm collector，可选 1/2/5s（默认 2），暂停后调整，新会话绑定周期；离页/隐藏停止，完整休眠验收待完成 | `cmd/status/main.go`；`watch.go` |
| `status --proc-cpu-alerts`、`--proc-cpu-threshold PERCENT`、`--proc-cpu-window DURATION` | 配置与高 CPU 提示待适配；当前快照保持引擎默认值，不宣称已有持续告警 | `cmd/status/main.go` flag 注册与 `validateFlags` |
| `status --help|-h` | 模块说明替代 | 同上 |
| `purge --paths` | 已支持按需只读配置并逐处选择，不自动发现/扫描或改配置；编辑/保存待接入，不直接启动交互式编辑器 | `bin/purge.sh:main` |
| `purge --dry-run|-n`、`--include-empty` | 已有不运行入口的固定名称只读发现，可逐项测量文件分配块，未知不当作零；完整候选/归属与规则待接入，见 [工程产物](project-artifacts.md) | 同上 |
| `purge --yes` | 无人值守确认不适用；GUI 以具体选中计划及用户确认替代 | 同上 |
| `purge --debug`、`--help` | 全局诊断/模块说明替代；该解析分支没有 `-h` 别名 | 同上 |
| `installer --dry-run|-n` | 已有不运行入口的指定范围只读文件线索；ZIP 已有手动、有界、只读的前 50 条名称线索检查及另行手动数据/CRC 测试，以及 DMG 内部 CRC32 核对；DMG/ISO 已有手动系统连接路径观察，可见进程占用线索另有逐项检查；映像身份/完整占用与完整结构化候选仍待接入，见 [安装包](installers.md) | `bin/installer.sh:main` |
| `installer --debug`、`--help|-h` | 全局诊断/模块说明替代；实际处理待受限执行协议 | 同上 |
| `history --json --limit N`（1–200） | 已接入原版 JSON，固定每类最多 200 条，分批展示与搜索/筛选；会话保留 run_id/归属，删除日志独立，不与客户端执行记录混合。仅默认位置当前日志，见 [历史读取](mole-history.md) | `mole:mole_dispatch_history_early`；`bin/history.sh:main` |
| `history --help|-h` | 已用日志来源、范围与归属说明及筛选界面替代 | 同上 |

此表面向用户可调用入口，不穷举每个内部 Shell 函数、环境变量或外部工具参数。`MO_ANALYZE_PATH` 等环境变量不作为前端任意配置入口；按次管理员授权也不能绕过文件隐私权限。

## 追踪与升级

源码证据均来自固定提交的 [主入口](https://github.com/tw93/Mole/blob/ba3a0f57f78e4c22818fda701219cc3dd4d6953f/mole)、[Shell 入口目录](https://github.com/tw93/Mole/tree/ba3a0f57f78e4c22818fda701219cc3dd4d6953f/bin)、[分析入口](https://github.com/tw93/Mole/blob/ba3a0f57f78e4c22818fda701219cc3dd4d6953f/cmd/analyze/main.go)、[状态入口](https://github.com/tw93/Mole/blob/ba3a0f57f78e4c22818fda701219cc3dd4d6953f/cmd/status/main.go)。这些文件在 `.cache/mole-source/` 可核对，表内使用函数名定位。

升级固定提交时，先比较以上解析分支、默认行为、已移除参数和保护规则，再更新映射、适配协议与测试；不能只替换版本字符串。每项实现后更新此表“当前状态”和 [release-roadmap](release-roadmap.md) 中对应验收项。品牌、真实机器矩阵、免费 DMG 发布与完整用户流程仍为独立验收条件。
