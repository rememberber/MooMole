# Mole 模块条件的分步接入

2026-10-16。固定基线仍为 Mole `1.59.1` / `ba3a0f57f78e4c22818fda701219cc3dd4d6953f`。模块观察规则版本为 `2`，NUL rules 协议为 `4`。

## 当前能力

审阅计划中的“复查 Mole 规则”先运行已有路径保护，再对已适配范围调用原版 Mole 模块函数。整个流程只读取，不调用 `start_cleanup`、`perform_cleanup`、`safe_clean`、`safe_clean_guarded`、`safe_remove` 等扫描/处理入口。上游文件未修改，自有适配位于 `scripts/mole-review-rules.sh`。

这一步让客户端解释“路径级检查没有要求保留，但模块条件仍要求保留或还没检查”的差别，不表示已经完成选中项的全部规则审计。计划继续 `readOnly=true`、`requiresMoleRecheck=true`，没有新增删除或恢复 IPC。

## 已接入的两个范围

| 范围 | 源码依据与实际检查 | 尚未证明 |
| --- | --- | --- |
| 当前 HOME 下 `Library/Caches` 的非隐藏直接子项 | `lib/clean/user.sh:clean_user_essentials`、`_user_cache_deno_delete_guard`；直接调用上游函数，重新解析有效 `DENO_DIR`，核对词法与适用的物理路径，保留 Deno 根、它的祖先及后代；解析失败也保留 | 其他缓存模块的活动/锁/版本等附加条件，目录每个后代的全部规则，以及可删除性 |
| `Library/Logs` 的非隐藏直接子项、`DiagnosticReports` 中的非隐藏直接子项 | `_clean_user_log_directory` 的物理根前置约束、`_mole_snapshot_path_identity`、`_user_log_delete_guard`；观察日志根/父目录及适用的诊断目录身份，调用原版最终判断 | 日志未被使用、内容不再变化、其他模块条件，以及检查之后的原子操作 |

Mole 日志模块保留 `DiagnosticReports` 目录本身，只生成其中的候选；规则层对来源与范围均匹配的根目录请求防御性返回 `preserveRoot`，不将目录本身提升为可处理。日志根以下的 Library/Logs 等目录出现链接时拒绝；HOME 可能是系统规范别名，遵循上游对 HOME 的边界。身份无法读取、函数缺失或异常返回不作为检查完成。

缓存中的深层路径、其他日志子树、隐藏项、同名前缀兄弟、其他 HOME、外接卷、废纸篓、Application Support、开发工具等范围返回 `unsupported`。路径级保护仍可显示，但这些范围会标记“模块规则尚未适配”。未知不折算为通过。

规则选择现在同时要求**实际扫描调用来源与准确路径范围**匹配。clean v2 在 ledger 记录边界收集调用链、两个 guard 声明及身份/路径，去重后仍保留全部来源；Rust 只为已适配契约且全部观察一致的候选分配 binding。混合模块、别名、缺失或未知契约不会退回按路径选择。细节见 [扫描来源协议](provenance.md)。来源观察仍不是执行资格，也不表示其他模块/后代规则完整覆盖。

## 请求与结果

前端只提交已保存的计划 ID 和复查请求 UUID。Rust 从计划读取路径，在当前身份/有效期/客户端保护通过的项目上检查，并在子进程结束后再次复核本地条件。

Rust 使用应用提供的 HOME 运行适配器，移除继承的 `SUDO_USER`；Shell 要求 `MOLE_USER_HOME == HOME`。报告回传 HOME，Rust 核对它与应用上下文一致，避免检查另一个账户的白名单或范围。`DENO_DIR` 遵循本次子进程的有效环境，不是对所有终端、应用或未来时刻配置的证明。

- 请求：`MooMole.rules.request.v4 NUL [id NUL path NUL binding NUL]* complete NUL`。
- 响应：`MooMole.rules.v4 NUL revision NUL moduleVersion NUL activityVersion NUL home NUL [id NUL path NUL binding NUL pathCode NUL scope NUL moduleCode NUL activityCode NUL]* complete NUL`。
- `binding`：来自保存候选的 `userCache / userLogs / unsupported / mixed / missing`，报告必须与请求一致，前端不能提供。
- `scope`：`userCache / userLogs / unsupported`；Rust 独立重算准确层级并核对。
- `moduleCode`：`checked / retainOrUnknown / preserveRoot / checkFailed / unsupported / notChecked`。

路径未通过时必须为 `notChecked`；路径通过时不能是 `notChecked`。`checked` 只适用于已支持范围；`preserveRoot` 只适用于准确的诊断报告根。旧协议、模块版本不匹配、HOME/范围不匹配、未知组合、缺项、重复、截断均报错。资源清单的 `rulesProtocol=4` 随 App 升级；旧资源要求重新准备，不能默默兼容缺少模块证据的旧响应。

当前 `RuleCheck` 分开保留路径结果和 `module`（version/binding/scope/code/source/reason）。模块保留显示 `moduleRetained`，未适配显示 `moduleUnsupported`，错误显示 `ruleUnavailable`；不覆盖本地过期、变化或客户端保护。`checked` 仍只有“元数据未发现变化”，不新增可执行标记。

复查继续使用最多 500 项、8 MiB 报告、45 秒预算及可取消进程组。结果只在当前弹窗内存中保留，不写持久计划、不跨重启复用。取消、错误和开始新的本地/规则复核都会清除旧证据；本地复核失败也显示无法确认身份，避免遗留上次结果。

## 已验证与剩余工作

隔离 HOME 的 Shell fixture 调用真实上游函数，验证默认/自定义/非法 Deno 根、包含自定义根的父目录、普通日志/诊断报告、诊断根保留、根目录在快照后替换、诊断目录换成链接、深层/隐藏/兄弟范围、缺失函数/异常返回、白名单、SQLite 占用、NUL 特殊字符和旧请求拒绝。仅在测试临时目录替换 fixture 目录，原始日志与报告正文保留。

Rust 验证协议绑定、旧协议/模块版本、范围与状态组合，以及模块结果不覆盖客户端保护/过期；Node/Rust 引擎检查拒绝旧 `cleanProtocol=1` / `rulesProtocol=1/2/3` 清单。浏览器验证部分覆盖、未知范围、原因/来源版本、筛选、重复检查取消及本地重查失败后不保留旧证据。

后续需要补齐：更多来源契约与其他模块条件及其数据归属、活动状态与锁语义、每个目录后代的保护、用户确认与影响绑定、最后检查和实际操作之间的源/父目录/内容竞态策略、权限/卷/真实进程故障矩阵，再连接原生处理和执行记录。两个已接入判断不是通用执行端。

附加占用观察 v1 与模块判断分开展示，只有路径及适用模块均满足才运行；未知/预算不足均不能视为空闲，详见 [附加占用观察](activity-checks.md)。

目录审阅新增有界的 [后代路径保护观察](descendant-protection.md)，与模块、占用证据分开展示，不代表后代全部条件或执行资格。
