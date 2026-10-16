# 扫描调用来源与模块绑定

2026-10-16。Mole 基线仍为 `1.59.1` / `ba3a0f57f78e4c22818fda701219cc3dd4d6953f`。clean 协议 v2、来源结构 v1、rules 协议 v3、模块观察规则 v2。

## 记录什么

来源来自**候选实际进入 Mole ledger 时**的结构化观察，不从终端文字、section 名称或路径分类推断。

自有只读适配器在加载固定引擎后，用进程内包装观察 `append_dry_run_cleanup_target`：保留原函数行为，记录设备/inode 去重键、路径、从内到外的 `FUNCNAME` 调用链、动态可见的逐项 `delete_guard` 及 `_MOLE_SAFE_REMOVE_FINAL_GUARD` 名称。后者是最终 guard 的**声明**，dry-run 不执行真实删除 sink，不代表该最终检查已在实际操作中运行。

包装通过 `declare -f` 复制已加载函数，仅替换固定函数名后 `eval`。可执行文本来自包内固定源码的函数定义，不拼入用户路径、报告、日志或前端字符串；重复安装拒绝。上游源码文件不改写，适配脚本仍由引擎清单记录摘要。资源完整性不是发布者认证，更不能将本地可改写的数据库当作执行授权。

调用原函数前后读取去重身份；不一致、记录失败或超限会写入失败标志，最终报告不发布。副本记录放在 Rust 本次私有任务目录，子 shell 继承包装及输出路径；即使某处忽略普通错误，失败标志仍会阻止结果发布。身份前后匹配不证明期间没有替换，更不封闭后续文件操作竞态。

## 去重与协议

上游候选仍按原逻辑去重，大小和父级覆盖计算不变。来源观察另行汇入，不能随第一个候选记录一起丢弃。

```text
MooMole.clean.v2 NUL revision NUL systemIncluded NUL
[identity NUL sizeKiB NUL count NUL known NUL section NUL path NUL coveredBy NUL]*
ledger NUL
[identity NUL sizeKiB NUL count NUL known NUL section NUL observedPath NUL]*
origins NUL
[identity NUL observedPath NUL commaSeparatedFunctions NUL deleteGuard NUL finalGuard NUL]*
complete NUL
```

Rust 先逐条对账原始 ledger 与来源记录：每个 `(identity,path)` 的原始追加次数必须与来源记录次数完全一致，包括最终候选去重掉的重复项；每个身份的第一条原始记录还须与最终候选一致。之后按 identity 汇入来源，并要求候选具有同路径观察。这样重复候选最后一次追加若中断在来源写入之前，不会被它的旧来源掩盖。其他路径的同身份观察也保留，以识别别名来源；未知身份、来源缺失、截断、非法函数名或非法路径使整个报告失败，不合成默认来源。

- 同一个完全相同的观察去重，其他观察完整保留，最多每项 16 种。
- 原始 ledger 和来源副本的发布上限各为 8 MiB，各最多 20000 条原始记录；函数链最多 32 层，函数/guard 名最多 96 字节，限制为 ASCII 标识符，guard 可为空。
- 完整报告继续最多 32 MiB；保存扫描/计划继续各 8 MiB。超限要求缩小范围，不截断来源后继续当作完整。
- 单条来源使用一次 NUL `printf` 追加；不宣称多个进程的任意大小写入是原子事务。交错、截断或配对异常拒绝读取，不作为有效证据。

来源结构作为候选 `provenance` 保存进扫描、选中计划及执行记录层的计划副本；不保存文件正文。查看列表或计划可展开来源。v1 扫描/旧计划仍可读取，但要求重扫，不保存为新计划；缺少来源的 JSON 字段兼容反序列化，不补造观察。本次无需新数据库迁移。

## 绑定规则

前端只提交扫描/候选或计划 ID。Rust 从保存的候选及所有观察推导 binding，规则 IPC 不接受前端提供来源或路径。

仅有两份已核对的来源契约：

| binding | 必须匹配的调用链前缀 | 两个 guard 均须匹配 |
| --- | --- | --- |
| `userCache` | `record_dry_run_cleanup_target → _safe_clean_impl → safe_clean_guarded → clean_user_essentials` | `_user_cache_deno_delete_guard` |
| `userLogs` | `record_dry_run_cleanup_target → _safe_clean_impl → safe_clean_guarded → _clean_user_log_directory → clean_user_essentials` | `_user_log_delete_guard` |

全部观察必须属于同一已适配契约，且观察路径与候选一致。存在另一模块、未知来源和已知来源混合、或同身份别名路径，返回 `mixed`；只有未知契约返回 `unsupported`；缺少/版本不支持的来源返回 `missing`。这些状态不会退回按位置选择模块。

规则请求绑定 `(id,path,binding)`，报告原样回传；Shell 仍独立核对准确路径范围，Rust 再核对报告组合。只有来源和当前范围同时匹配才调用相应模块判断。binding 不匹配、缺失、混合及未适配时保留路径保护结果，模块显示未适配及具体原因。

这是部分模块判断的来源约束，不是通用清理资格。观察到其他模块并不等于知道它的全部条件；两个已适配契约也不证明目录每个后代都经过保护、文件未被使用、用户已确认或最终原子操作安全。执行入口继续关闭。

## 验证范围

Shell fixture 使用独立 HOME，调用真实用户缓存/日志模块，验证真实函数链和两个 guard；同一个候选再经另一个函数及子 shell 记录，确认候选去重但来源不丢失。追加失败被上层忽略时仍拒绝发布，原 fixture 文件保留。

Rust 测试验证配对、混合/别名来源、完全相同观察去重、缺失/孤立来源、已有旧来源但后次原始追加缺少来源的中断、非法名称、数量预算、序列化往返、旧协议，以及 rules 的 binding/HOME/范围/状态一致性。浏览器覆盖来源进入计划、混合来源提示、旧扫描禁用保存及既有取消/过期行为。

尚未验证：真实进程强杀/断电、并发写入故障矩阵、其他来源契约及其模块规则、目录后代保护、操作瞬间的源/父目录/内容竞态与跨卷/权限矩阵。后续在这些条件明确后再连接用户确认、受限执行器及持久结果。
