# 普通 App 的后台与管理线索检查

2026-10-24：已实现 `uninstall_support`，接入确认准备和 [封闭执行器](app-uninstall-execution.md)。只有 [包内运行观察](app-uninstall-runtime.md) 与本次声明范围内的检查均取得新鲜、未发现限制的结果，才准备固定选择。后端已接明确提交/取消/结果读取；[前端入口](app-uninstall-interface.md) 已接，完整发布 App 矩阵仍未通过。报告保持 `canExecute=false`，不能直接当作修改许可；v2 确认的提交范围另行说明。

## 声明范围

首批继续针对 Applications 根的普通完整 App 包，包外数据全部保留。以下只读证据用于**保留 App**，不据名称或文本线索取得配置归属、服务停止或文件删除许可：

- 包内 `Contents/Library/` 的 LoginItems、LaunchAgents、LaunchDaemons、HelperTools、SystemExtensions 存在即保留，包括空目录或禁用/未运行的服务；Info.plist 声明 SMPrivilegedExecutables 或 SMAuthorizedClients 也保留。
- 当前账户 `~/Library/LaunchAgents`、`/Library/LaunchAgents`、`/Library/LaunchDaemons` 的直接 plist：匹配应用 ID/子 ID 的文件名、Label、AssociatedBundleIdentifiers，或 Program、ProgramArguments、BundleProgram、WorkingDirectory、WatchPaths、QueueDirectories 中的包路径/ID 引用，均保留。用 Foundation 读取 XML/二进制 plist，不执行其中任何程序。解析失败、类型不支持、无法核对的别名或不支持的相对程序路径保留未知。
- `/Library/PrivilegedHelperTools` 的应用 ID/子 ID 名称线索保留，不读取或启动 helper。
- `/opt/homebrew/Caskroom`、`/usr/local/Caskroom`：按固定 Mole 的默认根及最多 3 层搜索范围检查，同名 App 包/链接或与 App 名称主体相同的 cask token 均保留。旧版本、同名另一副本或歧义也保留，不依赖旧清单的 `source=App`。不运行 brew、Ruby 或 cask hook，不解析 `brew info` 的终端文案，不自动转交卸载。

Mole 对 Launch 配置使用精确 ID 文件名和程序路径确定其停止/处理条件；本项目的保留检查更保守，线索不作为可处理对象。Homebrew 的标准 Caskroom 名称/token 也是保留线索，不声称它们单独证明管理归属。

结果 `notDetected` 仅表示上述范围内未发现线索；`detected` 表示保留并使用相应卸载方式；`unknown` 表示检查不完整，不能生成确认草稿。只返回固定原因码和检查数量，不返回其他 App 的配置内容、程序参数或路径列表。

这不是 launchd 全局状态、BTM/MDM 数据库或所有安装管理器的完整证明。包外解释器/脚本、任意外部注册、仅载入而已无配置的服务、自定义 Homebrew 前缀及其他包管理器不在此观察的负面证明范围。后续正式界面和确认必须如实声明普通 App 的支持范围；已知特殊管理来源不得因此取得执行资格。受管理或带特殊服务的应用应使用管理工具/专用卸载器，当前不自动拆除服务或处理包外数据。

## 变化、预算与确认绑定

所有范围均只读取本地已落地的普通对象，不跟随配置链接、不为扫描修改权限。每组最多 4096 次目录条目/对象检查、512 份文件、单文件 512 KiB、合计 8 MiB 和约 2 秒检查点预算；耗尽即未知。重核有独立约 2 秒预算，仍受外层原始 5 秒期限限制，不会续期。此为检查点预算，不承诺可中断单次内核文件调用。

目录从已经固定的句柄通过 fdopendir/readdir 枚举，避免按路径重开时读到临时替换的空目录。读取文件时核对同一无链接对象与前后元数据；配置/目录及祖先身份保存为私有依赖，缺失路径绑定最近存在的祖先。检查完成和生成草稿前重核：新增配置、原文件变化、根替换或原先缺失路径出现均撤回负面结论。单次观察不提供锁或消除最后并发窗口。

报告绑定完整计划历史摘要、原计划 ID、账户目录及原始墙钟/单调时钟，最长 5 秒且不超过计划期限。昂贵的配置检查先于短期进程检查，二者各保留自己的时间。草稿发布仍受后端代次、最新源清单/保护/首次使用确认及更新租约约束。30 秒草稿只固定选择，不能保存这份短期观察作为未来执行许可；未来执行器还须重新检查本次所有条件、权限、取消和已提交的持久意图。

## 验证与剩余工作

自建目录测试覆盖 XML/二进制引用、包内服务和特权声明、同名 cask/链接/副本、配置缺失后出现/内容变化/根替换、链接/特殊文件/超量拒绝、原始新鲜度与确认共同门槛。单独验证临时替换为空目录后，句柄枚举仍读取原对象中的条目，路径绑定也拒绝变化。

普通账户显式只读检查已安装 MooTool：最终本机声明范围结果为后台 `notDetected`（15 次检查）、管理 `notDetected`（2 次检查），约 1.43 秒，仍为不可执行。该次两个 Caskroom 未发现条目；不是有大量已安装 cask 的实机矩阵，也不替代实际打包 App 的卸载验收。没有启动/停止服务或应用、发起授权、处理个人文件、移动 App 或升级用户数据库。

后续已接 [封闭明确提交和执行任务](app-uninstall-execution.md)、最新条件/权限复核、[原生 App 操作](app-uninstall-target.md) 与 [持久记录](app-uninstall-journal.md)。下一步接前端逐项结果/历史与实际 App 的正常/取消/中断及恢复边界。[首发三项真实执行](first-release-execution.md) 门槛继续未完成。
