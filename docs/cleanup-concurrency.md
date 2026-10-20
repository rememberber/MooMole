# 单文件清理：对象引用与并发变化

2026-10-20。关联实现：`src-tauri/src/cleanup_target.rs`、`cleanup_target/tests.rs` 和 `execution/tests/controlled.rs`。

## 本轮结论

修正受控原生后端在意图之后才按旧路径建立文件引用的时机：先将引用与原计划身份绑定，再提交意图；调用原生接口前再次验证身份与原位置。引用准备是内部只读构建块，没有独立 IPC、删除或恢复入口；实际废纸篓调用仍仅在测试中编译，并只允许本次专用 fixture 源目录的直接子项。

这一步改善“旧名称已经指向新文件”的风险。本机显式实验中，最后检查前已发生的改名、父目录替换或内容变化均拒绝调用原生接口；最后检查后刻意注入的变化中，Foundation 移动的仍是原选中设备/inode，同名替代文件未移动。但是原选中对象可能已移出原路径，或内容已被改写，因此这些结果记为待核实，不发布成功结论。这是当前机器上的实验结果，不是原生 API 内部原子绑定的保证。

## 准备与最终验证

`PreparedFile::prepare` 要求当前版本的完整路径观察、未变化的普通文件、单硬链接、本账户所有者及已落地的本地路径；目录、缺失/旧观察、链接、其他所有者或未知本地范围均拒绝。它不运行 Mole、检查用户确认、替代权限审阅或授权执行。

准备保留祖先/目标句柄，并在已锚定文件后取得 NSURL 文件引用；取得引用后再次比较原计划的完整观察。若引用建立期间同名路径已被替换，即使新引用解析回同名路径，也不能据此通过。引用不序列化、不跨启动复用，也不在意图后从旧名称重建。

`validate` 分开核对两类事实：

- 设备/inode、类型、所有者、权限、链接数、大小、修改/变更时间及祖先路径绑定仍与原观察一致。
- 引用解析的当前本地路径仍是原计划路径，重新打开后的观察仍与持有的对象一致。引用能够跟随改名，不等于允许跟随到新位置处理。

原生接口返回后仍核对实际位置、元数据身份、原位置与祖先绑定，已知位置留作收据。原生错误默认视为不确定，并保留 API 返回且能观察到的目标。只有在调用原生 API **之前** 拒绝的情况，才能确认为没有尝试文件修改；如果意图已提交，正式记录为 `failed` 并说明“native API not called”，后续取消保留该结果。不能把原生异常或缺少结果收据写成已知未修改。

## 明确排除的路线

没有引入提权、私有 entitlement、后台 helper、文件锁定属性或修改原文件权限。

查阅固定 Apple XNU `f6217f891ac0bb64f3d375211650a4c1ff8ca1ea` 的文件租约实现，`F_SETLEASE` 由私有 `com.apple.private.vfs.file-leases` 权限控制；本机对自建普通文件的探测也返回 `EPERM`，文件内容保持不变。不能把它作为普通 App 的可用前置条件，购买通常的开发者签名也不能据此假定可获得私有权限。依据：[XNU 租约实现](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/vfs/vfs_subr.c#L12734)、[Apple 的租约测试权限](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/tests/file_leases.entitlements)。不调整系统开关或尝试绕过权限。

Apple 的文件引用说明支持运行期间按对象定位，并不授予目录范围；文件协调接口面向注册的 file presenters，也没有给出阻止所有非协调 POSIX 操作的承诺，因此本项目不将它当作强制全局锁。依据：[文件定位说明](https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/FileSystemProgrammingGuide/AccessingFilesandDirectories/AccessingFilesandDirectories.html)、[NSFileCoordinator](https://developer.apple.com/documentation/foundation/nsfilecoordinator)。

## 已验证的边界

4 项普通只读测试覆盖：原始数据不变、缺失/旧版本/目录拒绝、建立引用两个检查点的名称替换、引用跟随改名但范围仍拒绝，以及文件消失、符号链接和新增硬链接。没有调用废纸篓：

```bash
node scripts/cargo.mjs test cleanup_target
```

原生检查默认忽略，显式运行只处理自建文件：

```bash
node scripts/cargo.mjs test cleanup_target::tests::native_prepared_reference_gate_and_final_window -- --ignored --exact --nocapture
node scripts/cargo.mjs test execution::tests::controlled::native_ -- --ignored --nocapture
```

原生对象检查的 7 个场景：

| 场景 | 本机结果 |
| --- | --- |
| 稳定普通文件 | 返回原选中身份，核对后为 moved；按实际收据取回 |
| 最终检查前替换名称 | 拒绝，不调用原生 API，原文件和替代文件都保留 |
| 最终检查前替换父目录 | 拒绝，不调用原生 API，原目录数据保留 |
| 最终检查前改写内容 | 拒绝，不调用原生 API，变化后的数据保留 |
| 最终检查后替换名称 | 原选中对象被移动，替代文件保持原处；记录 uncertain，取回确切对象 |
| 最终检查后替换父目录 | 原选中对象被移动，替代目录数据保留；记录 uncertain，取回确切对象 |
| 最终检查后改写内容 | 移动的是原对象但内容变化；记录 uncertain，变化后内容完整取回 |

同一准备对象已接入受控 SQLite 循环。稳定两文件的移动/重开/冲突取回联验通过；另有三种“意图后、最终检查前”的变化联验：当前项记录 failed 且无收据，下一项取消，重开后结果不变，原/替代/未选文件均保留。这里只证明已知变化被拒绝；不声称测试过原生接口内部的每个时刻。

所有发生原生移动的专用文件均按实际返回位置取回，核对设备/inode 与内容；成功后私有测试根移除。失败时根、收据报告和 SQLite（存在时）保留并打印位置，禁止盲目覆盖、枚举或清空既有废纸篓。并发检查不代替跨卷、Finder 放回、实际 App 进程、权限拒绝或原生操作中强杀验收。

## 首批流程的继续方向

真实规则至只读准备、客户端范围摘要的接入现已完成，见 [日志准备接入](log-cleanup-preparation.md)；原生引用不进入报告或持久计划，占用未知仍保留。这不改变下述真实执行边界，独立短期确认及受限日志执行接口现已接入，见 [日志执行](log-cleanup-execution.md)。

首批候选继续限定为来源契约明确的用户日志普通文件；不扩大到目录、共享数据、数据库、系统范围或提权。准备对象已接到当次真实 Mole 日志/占用条件和用户确认流程；隔离 HOME 已验证真实白名单在确认后仍阻止执行，后续继续验收发布 App 权限及故障。普通元数据与 fixture 规则不能产生真实执行资格。

采用的候选策略是：**先绑定选中对象，最后复核原路径；已知变化不调用原生接口，操作结果不确定则停止并保留实际收据。** 不以“没有发现打开句柄”承诺文件持续静止，不声称取得了全局锁。最终窗口的范围/内容变化属于仍需验收的失败路径，不能仅以同 inode 将它记为成功，也不自动重试或回滚。当前仅开发版开放限定日志的明确确认入口，完整真实范围验收仍未完成；本轮并发实验已形成明确结论，后续不以反复增加同类名称规则或元数据变体替代真实流程接入。

## 最终权限门槛补充（2026-10-20）

最终路径/对象验证还重新观察目标及父目录权限、ACL、锁定属性与卷状态，再核对身份后才调用 Foundation。父目录 ACL/flags 可在身份和模式不变时改变，不能只依赖路径 witness。三类意图后真实限制测试确认调用前拒绝、限制不被解除；失败原因保存在逐项记录。此检查仍不消除最后观察至系统调用的变化窗口。实际日志循环五个强杀关口另见 [原生强杀记录](execution-crash-recovery.md)。
