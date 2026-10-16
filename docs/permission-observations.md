# 审阅计划中的本机权限观察

2026-10-17。客户端权限观察 v1，仅返回在本次复核中取得的解释信息，不授予执行权限。Mole clean v2、rules v4、路径观察 v2、目录摘要 v2、SQLite v3 均不变。

## 观察内容

复用 `AnchoredPath` 已持有的目标和直接父目录句柄；仅在版本、有效期、当前保护和保存身份等前提满足后进行。没有新增按任意路径查询的 IPC，也不读取文件正文、试写文件、创建废纸篓目录或发起管理员授权。

| 项目 | 使用的信息 | 结果边界 |
| --- | --- | --- |
| 卷状态 | 对目标和父目录分别调用 `fstatfs`，检查 `MNT_RDONLY` | 已知只读为受限，读取失败为未知；不更改挂载状态 |
| 父目录访问 | 在持有的父目录 fd 下对 `.` 调用 `faccessat(W_OK \| X_OK, AT_EACCESS)` | 使用当前有效身份，拒绝访问为受限；不推断具体是普通权限还是隐私控制，不保证实际移动成功 |
| 锁定属性 | 目标/父目录的 UF/SF immutable、append 标记 | 发现不可更改或只追加标记则受限，不自动解除 |
| 目录所有权保护 | 父目录 sticky 位及当前有效 UID、目标/目录所有者 | 无相应所有权且非 root 时标为受限，不改所有者 |
| 扩展访问控制 | 在两个句柄上调用 Darwin `acl_get_fd_np`，观察是否有扩展 ACL 条目 | 存在条目时为未知，不自行计算删除/移入权限；无法读取或文件系统不支持也为未知 |
| 挂载边界 | 目标与直接父目录的设备标识 | 不同设备为未知；相同不代表废纸篓或跨卷策略已验证 |

普通文件的写入位不作为移动许可或拒绝依据；文件为 0444 时，仍分别观察父目录和其他条件。目录仅观察所选根与父目录，不递归计算后代权限。

Darwin ACL 的返回值与 Linux 不同。对新取得的有效 ACL，`acl_get_entry` 成功返回 0，空条目返回 -1/EINVAL。`acl_get_fd_np` 可能在 FILESEC_ACL 不存在时返回 NULL/ENOENT；本机普通文件/目录与实际附加 ACL fixture 均验证该分支。此处理只用于仍被持有并在结束时重新验证的目标；其他读取错误不退回普通权限位猜测。

## 状态与生命周期

- `noRestrictionObserved`：六项有限观察均未发现已检查范围内的限制，界面明确“不代表已获准清理”。
- `restricted`：至少一项发现限制，即使同时存在未知，也保留受限与全部原因。
- `unknown`：部分条件无法读取/尚未判定，不能当作允许。
- `notChecked`：本地前提不满足，或本批 5 秒预算耗尽，不发布以前的证据。

权限与本地元数据、Mole 规则、占用和后代保护分开展示。`state=unchanged` 仅描述本地观察，不能忽略独立的权限结果。权限受限不妨碍保存一份**只读审阅计划**；生产记录层 `begin` 也不是执行授权，未来执行器必须重新验证实际操作所需的完整条件。

目标/父目录的身份、权限位、所有者、flags、ctime 及卷只读状态在观察后再次核对，并复查路径绑定；变化时丢弃本次权限结果。本次观察仍非原子快照，不证明检查之间或检查结束后没有变化。

5 秒是同一批计划复核中的共享墙钟预算，只在开始每项之前检查；不能打断挂起的内核调用，也不是整份复核或网络卷的超时保证。每项原生调用数固定，不枚举 ACL 内容或目录后代。

报告仅存在于 `PlanView`，不写入扫描/审阅计划 JSON 或数据库，不跨重启复用。重新本地/Mole 复核前清除旧证据；取消、失败和前端自动过期也不保留旧结果。问题筛选纳入受限、未知、未检查，以及缺字段/未知版本/不完整的正面报告。

## 尚未完成

这里不提供 Full Disk Access 或系统隐私授权的全局状态，不区分 EPERM/EACCES 的唯一成因；也没有完成 ACL 删除权计算、目录后代权限、实际废纸篓目标/容量、跨卷、特权调用方身份或源对象与实际操作绑定。界面不自动要求购买证书、安装 helper、提权、解锁或修改系统安全设置。

权限与活动检查均不能锁住外部应用。首批范围、完整 Mole 模块条件、用户确认和受限执行器继续是待办，不开放真实修改入口。

## 验证与依据

六项新增 Rust 模块测试使用自建临时目录，覆盖 0444 文件、有效身份/父目录拒绝、目标及父目录的实际用户 immutable/append flags、实际扩展 ACL、观察中改名/权限变化、预算与状态优先级。只读卷错误分支和其他所有者的 sticky 判定使用隔离输入验证，不冒充真实只读卷或跨用户验收。另有审阅集成测试验证权限独立、保护/过期跳过、结果不持久化及后续变化要求重扫。

fixture 为验证限制自行设置权限/用户 flags/ACL，随后恢复自己的文件；生产观察没有这些写入操作。未修改用户的权限、ACL、文件属性或系统设置。

浏览器验证六项报告、问题筛选、缺失/未知版本/不完整正面证据、重新复核、读取失败、规则取消和自动过期。实际 App 与测试计数见 [开发记录](development.md)。

实现核对本机 Xcode SDK `sys/acl.h`、`sys/stat.h`、`sys/mount.h` 与 `sys/fcntl.h`，并参考 Apple 的 [access](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/access.2.html)、[unlink](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/unlink.2.html)、[ACL 读取](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/acl_get_fd_np.3.html)、[ACL 条目](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/acl_get_entry.3.html)及 [Darwin ACL getter 源码](https://github.com/apple-oss-distributions/Libc/blob/main/posix1e/acl_file.c)。访问预检不是实际操作权限的保证，不能用“检查通过再操作”封闭竞态。
