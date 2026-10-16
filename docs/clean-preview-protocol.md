# 清理只读预览协议 v2

适用于固定 Mole 提交 `ba3a0f57f78e4c22818fda701219cc3dd4d6953f`。此协议只表达扫描候选，**不是可执行计划**。

## 来源与生命周期

Mole 的 `bin/clean.sh` 已提供源码加载保护以及 NUL ledger。MooMole 不修改上游文件，由自己的 `scripts/mole-clean-preview.sh` 加载该文件，调用 `start_cleanup`、`perform_cleanup`，在退出清理之前调用 `emit_deduplicated_dry_run_ledger`。上游默认保护、白名单和活动应用检查继续由 Mole 负责。

适配器固定启用 `MOLE_DRY_RUN=1`、禁止管理员授权、关闭操作日志；不接受清理参数或前端提供的 Shell 命令。Rust 为每个任务创建私有临时目录，报告、文本预览和子进程临时文件位于该目录，任务结束、失败或取消后清理。程序被强制终止时可能留下临时文件，后续需增加有界回收。

原始 stdout 仅作为诊断日志展示。候选路径和大小只来自结构化 ledger，不从日志中推导。用户原有的 `~/.config/mole/clean-list.txt` 不会被这条适配流程覆盖。

## 传输格式

报告采用 UTF-8，各字段以 NUL 分隔，保留路径中的空格、换行、制表符及通配符字面量：

```text
MooMole.clean.v2 NUL upstream-revision NUL system-included NUL
[identity NUL size-kib NUL count NUL size-known NUL section NUL path NUL covered-by NUL]*
ledger NUL
[identity NUL size-kib NUL count NUL size-known NUL section NUL observed-path NUL]*
origins NUL
[identity NUL observed-path NUL functions NUL delete-guard NUL final-guard NUL]*
complete NUL
```

- 头部绑定协议和引擎提交；当前 `system-included` 为 `false`。
- `identity` 是 Mole 去重键，可能来自设备/ inode，也可能是路径回退；不保证能作为未来执行时的身份依据。
- 大小单位来自上游 KiB，Rust 检查溢出后转换为字节；`size-known=false` 输出 `null`，不伪装为零。
- `covered-by` 是已测量父候选的路径，完整预览累计时避免重复；不是删除依赖。
- `count` 是引擎记录的项目计数，不保证等于递归文件总数。
- Rust 拒绝缺少完成标记、截断、非法编码、未知协议/提交、无效枚举/数值、重复身份/路径、相对路径和错误的父级关系。报告与日志各限制 32 MiB。
- Rust 补充当前目录类型及解析路径，前端用于保护重叠匹配；这不是扫描时冻结的文件身份。

v2 在不改变候选去重/计量的基础上，保存实际 ledger 调用链及 guard 声明；原始追加与来源按身份/路径逐条数量对账，包含被候选去重掉的重复项，再将全部来源按身份汇入；缺失/错配/超限不补造默认来源。来源结构与绑定细节见 [扫描来源](provenance.md)。旧 v1 扫描仍可查看，但要求重扫后保存新计划。

## 审阅与保护

默认不勾选。界面支持分类展开、路径搜索、大小/路径排序、状态筛选及每页 100 项；批量勾选仅作用于当前筛选后的本页。统计对当前选中子集重新计算父子覆盖：未选中父目录时，子项按自身大小计入；大小未知的父目录不吞掉已测量子项。

客户端保护按字面路径边界匹配候选、其父子目录及可解析别名，不把 `*` 等字符当作模式。新增保护后，当前重叠项目即时取消勾选，后续扫描也会禁选；记录仍显示供核对。客户端不会改写 Mole 的白名单文件。补充的开发缓存测量继续直接跳过保护项目。

未保存的勾选只在当前页面内存中，离开页面清空。保存计划的流程见下文；审阅结果仍不能据此执行删除，禁止重新运行全量清理替代用户审阅的选中项。

## 持久化审阅计划 v1

SQLite 自 v2 起保存最近 10 份扫描及最多 20 份关联审阅计划，同时遵循设置中的 7/30/90 天保留期限。扫描回收时关联计划一并回收；原始 stdout 日志不入库。单份扫描或计划 JSON 上限 8 MiB。旧 v1 数据库升级前通过 SQLite `VACUUM INTO` 创建包含 WAL 内容的一致性备份，备份失败则停止升级。备份在应用数据目录独立保留。

每次扫描由后端分配独立扫描 ID，候选 ID 包含该扫描的命名空间。扫描完成后记录路径各级祖先与目标的设备、inode、类型/模式、所有者/组，以及目标大小、链接数、mtime/ctime（含纳秒）。目标必须与 Mole ledger 的 inode 去重键吻合；无法验证、特殊文件、共享硬链接或含符号链接的路径不能保存为计划。字段观测发生在扫描完成时，不保证代表整个扫描期间的一致快照。

前端保存时只提交扫描 ID 和 1–500 个候选 ID。后端拒绝未知、重复、跨扫描、过期或受保护项目，并重新读取元数据。计划到期时间固定为扫描后 15 分钟，保存不会延长；系统时间回退也要求重扫。重新打开计划时，按当前客户端保护规则、版本、有效期及文件元数据返回逐项 `unchanged / changed / protected / unavailable / expired / versionMismatch`。

`unchanged` 仅表示该次本地元数据观测未发现变化，**不表示已经获得执行资格**。目前始终返回 `readOnly=true`、`requiresMoleRecheck=true`：Mole 白名单、活动应用、逐项影响和权限还需重新检查；未来执行还需防止路径检查与实际操作之间的竞态，并明确递归目录内容变化的验证方式。此阶段没有删除、通用 Shell 或按路径执行 IPC。

重启后恢复最近扫描并显示原扫描时间，历史页提供已保存计划的查看与重新复核。扫描过期仍可查看，不能保存新计划或显示为可执行。已经保存的保护目录即使暂时不存在，也保留原保护意图，不阻塞无关设置保存；新加目录仍需通过目录验证。

## Mole 路径与有限模块规则复查 v3

审阅计划新增显式“复查 Mole 规则”，不随历史查看自动启动较慢的进程/占用探测。前端只提交计划 ID 和请求 UUID；后端从保存的计划取路径，对本地身份/有效期/客户端保护仍通过的项目复查。不存在前端传路径或任意 Shell 的入口。

自有 `scripts/mole-review-rules.sh` 加载原版 `clean.sh`，不调用扫描或清理生命周期，只调用与其候选验证相同的路径级函数：`should_protect_path`、`is_path_whitelisted`、`holds_compiled_model_cache`、`_mole_should_refuse_live_user_cache_path`，以及适用时的 SQLite 占用判断。明确禁用卸载模式、测试捷径和容器探测延后选项；不请求管理员权限。白名单读取失败、异常类型或被忽略的非法规则会中止复查，避免默默忽略保护意图。

请求为 `MooMole.rules.request.v3 NUL [id NUL path NUL binding NUL]* complete NUL`；响应为 `MooMole.rules.v3 NUL revision NUL moduleVersion NUL home NUL [id NUL path NUL binding NUL pathCode NUL scope NUL moduleCode NUL]* complete NUL`。Rust 校验项目一一对应、完整性、去重、HOME、准确模块范围、规则版本及已知状态组合；禁止将日志作为判断依据。路径状态仍为 `passed / coreProtection / whitelist / compiledModel / activeOrUncertain / sqliteInUse / sqliteUnknown / checkFailed`。

模块观察版本 2 仅接入非隐藏直接用户缓存的 Deno 保留约束，以及用户日志/诊断报告直接项的根目录身份约束；直接调用固定版 Mole 的原始判断函数，不调用清理入口。诊断报告根目录本身要求保留，其他深层/隐藏/未知范围显示尚未适配。`checked` 仅表示这个命名判断已完成，不表示其他模块条件满足；必须同时匹配已保存的实际调用来源契约与当前范围；缺少、混合或未适配来源不会退回按路径选择模块。详见 [模块条件与剩余工作](module-rules.md)。旧规则资源/响应拒绝，清理扫描协议为 v2，旧只读计划仍可查看并要求重扫。

其中 `activeOrUncertain` 保留上游合并后的语义，不能声称已经确定应用运行或空闲。`passed` 只表示这次路径级探测未要求保留，**不是每个清理模块的完整规则/影响审计或执行许可**。规则结果只在当前弹窗内存中展示，不写入持久计划，也不跨重启复用。真正执行前仍需重新检查，因此 `requiresMoleRecheck` 继续为 `true`。

复查使用独立私有临时目录，最多 500 项、报告上限 8 MiB、45 秒超时，注册为可取消任务并清理整个进程组；退出 App 也会停止该任务。复查结束后再次读取当前客户端保护、身份与有效期；Mole 的 `passed` 不覆盖本地失效或新增保护。取消、超时、协议损坏或探测错误不保留旧的通过状态，界面显示未完成。重新进行本地复核也清除旧规则证据，失败显示身份无法确认。

## 检查

### 路径观察与目录内容元数据摘要 v2

保存目录计划时，新增受限递归元数据记录：最多共 10000 个检查节点（包括所选根目录），最多 64 层深度；每次遍历在系统调用之间检查 5 秒预算。保存时记录一次并再遍历确认，可能使用两次预算；无法中断已挂起的操作系统文件调用，网络卷/异常设备的响应仍需专门验证。

路径观察从根目录描述符逐层 `openat`，保留全部祖先句柄并在返回前复查名称绑定；普通文件使用 `O_EVTONLY` 只观察元数据，拒绝共享硬链接，路径最多 128 个组件。目录打开使用 `O_NOFOLLOW`；列举与子项属性通过 `fdopendir/readdir/fstatat(AT_SYMLINK_NOFOLLOW)` 完成。记录名称字节及类型、设备/inode、所有者、模式、链接数、大小和 mtime/ctime，按名称排序后生成 SHA-256 摘要，不读取文件正文，不保存完整子项路径清单。每个子项在观察/递归后复查名称与属性，遍历结束再核对所选根的全部祖先绑定及目标属性，变化会中止检查。

符号链接、共享硬链接、特殊文件、其他挂载卷、无法列举或超过预算时不生成目录计划，提示缩小选择或单独审阅。复核时重新计算摘要，深层变化显示 `contentsChanged`，无法确认显示 `inventoryUnavailable`；没有摘要的旧目录计划显示 `inventoryMissing`，要求重新生成。路径观察缺少 v2 版本的旧扫描/计划仍可读取，但显示 `versionMismatch` 并要求重新扫描，不能作为当前证据；新目录摘要版本为 v2。字段缺失兼容反序列化，SQLite 仍为 v3，无须迁移数据库。

逐项观察不能构成原子快照：深层文件在其观察之后才被修改，父目录属性可能不变，本轮摘要仍可能匹配；下一轮才能发现。测试明确保留这一剩余窗口，详见 [路径观察与竞态边界](path-observation.md)。

摘要记录发生在**保存审阅计划时**，不是扫描全程的一致快照，也不证明每个子项都经过 Mole 模块级规则检查。匹配摘要不封闭检查与真正操作之间的竞态，不是文件内容校验或执行许可；后续执行端仍需最终身份/规则/权限检查与受限文件操作。

### 用途与影响说明 v1

用途说明由客户端按路径边界整理，和 Mole 原始保护结果分开展示。当前明确范围为默认 npm `_cacache`、Homebrew `Library/Caches/Homebrew/downloads`、用户 `Library/Logs/DiagnosticReports`、其他 `Library/Logs`，以及需确认具体内容的 `Library/Caches` 子项。日志和诊断报告说明历史信息可能无法再生成；下载缓存也不承诺能重新取得同一版本。

参考固定基线中的 `lib/clean/dev.sh`（npm 残留及 Homebrew 下载缓存）、`lib/clean/user.sh`（用户日志与诊断报告）和 `lib/clean/brew.sh`（Homebrew 缓存）。未匹配的位置、同名前缀兄弟路径和自定义 npm 缓存保留为未知。每项均标记 `requiresReview=true`，不设置“安全可删”属性，不改变默认未勾选策略，也不用于生成删除目标。

旧扫描/计划没有说明时显示“用途尚未确认”。界面显示用途、可能影响、可恢复性和说明范围，并可按用途筛选；不把生成缓存与恢复原文件混为一谈。打开的扫描/计划每秒更新到期状态，失效后隐藏旧规则证据并停用规则复查；后端仍独立校验有效期。

### 自动化检查

```bash
npm run engine:prepare
npm run test:adapter
npm test
npm run check:rust
npm run test:e2e
```

本机真实只读扫描为显式执行的忽略测试，常规测试不会扫描个人目录：

```bash
node scripts/cargo.mjs test real_clean_preview_smoke -- --ignored --nocapture
```
