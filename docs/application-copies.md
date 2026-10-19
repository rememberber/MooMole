# 应用共享关联的只读观察

应用详情默认折叠「共享关联」。发现其他有效位置命中 Mole 保留规则时，主结论提示「关联数据可能共用」；已有客户端、系统组件、后台范围和官方卸载器限制继续优先显示。展开查看路径、本地版本/构建和同标识、派生标识、渠道包名三类原因。关联数据全部保留，没有退出、卸载或文件修改入口。

## 发现范围

候选来自同一成功 Mole apps 任务的前 64 项，以及 NSWorkspace URLsForApplicationsWithBundleIdentifier 返回的前 32 个系统登记位置。两来源先限额再去重；分别最多取 65/33 项判断超限，不把多余项视为不存在。另有常用安装根、本地卷和收据各最多 128 个候选，总输入至多 480；最终最多显示 32 个共享命中。

Mole 清单路径由后端根据原 taskId/index 所在任务取得，不接受前端路径/ID/HOME 覆盖。不用其他清单条目的缓存 ID、显示名称或来源字段判断共享；每个候选重新读取实际 Info.plist。异常行保留未知占位，超限、失效和来源错误产生 partial。清单过期/淘汰仍按原机制要求重新读取，取得上下文后不将后续缓存变化当成持久授权。

系统登记查询只使用所选包的新鲜本地 ID；当前身份不同/不可用或规则资源不可用不查询任何候选。Mole 未报告 ID 时保留未比对说明。查询 API 由系统决定覆盖范围，不能承诺包含所有大小写变体、未登记的新副本或已离线位置。登记位置若在已挂载本地外接卷，仍可按精确路径核对；额外卷扫描仅覆盖下文声明的有界本地范围。

这两份来源都不代表完整安装扫描。Mole 清单本身可能缓存/去重/省略路径；固定 uninstall_live_bundle_has_other_install 还有安装根、Homebrew Caskroom、Setapp/PlayCover、外接卷和安装收据扫描。常用安装根和本地卷已补充有界发现，完整卷语义与收据故障矩阵仍待完成，未命中不能证明数据独占。

## Mole 保留语义

适配固定修订 ba3a0f57f78e4c22818fda701219cc3dd4d6953f 的 uninstall_bundles_share_remnants 与 _uninstall_strip_version_suffix_into。生产环境不执行 Shell；除既有引擎清单/摘要外，绑定 lib/uninstall/batch.sh 的 SHA-256 f44ed6a61ec5074f8efe89e65dd3d9fbc48dd78533b4d529d8220dd9660bcc54。资源变化必须显式审查，否则保持未知。

按原函数顺序返回首类命中：

1. ID 仅作 ASCII 大小写归一后相同：sameIdentifier。
2. 任一 ID 是另一 ID 的点分延续：relatedIdentifier。例如 org.example.app 与 org.example.app.share，不能把 org.example.apple 当延续。
3. 以上未命中时，用实际路径的 .app 包名，经一次渠道后缀处理和 ASCII 大小写归一后相同，且两边至少 2 字节：channelName。此类仅提示保留，不认证产品关系或所有权。

后缀保留原大小写敏感、LC_ALL=C 字节和 POSIX 贪婪匹配语义：Nightly/Beta/Alpha/Dev/Canary/Preview/Insider/Edge/Stable/Release/RC/LTS/Developer Edition/Technology Preview。不折叠连续空白、不反复去后缀、不做 Unicode 大小写折叠；Foo  Beta 留下一处空格，Foo Technology Preview 按原贪婪捕获得到 Foo Technology。不能为更直观而改变原保护范围。显式只读对照用 1,620 组上下文核对原函数，没有进入卸载生命周期。

保留谓词不能用于发现删除对象或建立独占归属；所有 16 个精确 ID 关联位置继续保留，即使命中缓存/日志。没有把渠道名字相似解释成可以删除其数据。

## 核对、变化与结果

只接受有界、无控制字符、可读取的本地绝对 .app 路径，拒绝可识别链接、网络/已知 dataless 布局和嵌套辅助应用。复用 Bundle/Evidence 的本地目录、单链接 Info.plist、最多 1MiB XML/binary 读取和复核；平面 PlayCover、Wrapper 等布局仍可能无法读取。

先按路径、再按设备/inode 去重，排除当前对象。命中项保留候选目录、Contents/Info.plist 句柄和元数据；未命中保存三个元数据快照并释放句柄，返回前重新无链接打开复核。变化项撤下，有效其他项继续展示；当前所选包变化仍整份拒绝。没有重新查询全部登记状态，读取不是事务，返回后仍可能变化，不能用于执行前身份凭据。

- observed 仅表示本次有界来源集合已核对，不表示全机安装扫描完整。
- partial 表示来源失败/不可用、超限、候选不可读取或变化。折叠摘要直接标出检查不完整，可保留已核实的其他有效命中；没有结果时也不能断言没有其他应用。
- unknown 表示所选身份或固定规则资源不可用，不携带命中。

沿用 2 个实际工作器和 5 秒停止等待，实际阻塞操作结束前持有名额。常用安装根按独立数量和时间预算遍历，清单/登记/目录观察没有新增 Shell 或子进程；收据使用固定只读 pkgutil，仍不做 Launch Services 注册、目标应用启动/终止、权限修改、提权、数据库写入或额外历史。

## 协议与后续

应用检查协议 v16 的 installations 升为 version=5，source=moleListRegistryRootsVolumesAndReceipts，增加固定 revision；copies 至多 32 项，每项 path、bundleId、version/build 及 relation。registryUnavailable/listUnavailable 归为 partial，允许保留另一来源的有效命中。前端校验精确版本/来源/修订、身份关系、数量/去重、路径、有界 ASCII ID、relation 与 ID 边界的一致性；channelName 的包名依据由后端固定规则验证，不由前端重建另一套后缀规则。

未知协议/矛盾结果整体拒绝；重查撤下旧观察、一分钟较早提示、任务绑定、保护设置变更后重查和迟到响应处理沿用原机制。

完整 Mole 安装根/卷/收据发现、完整关联候选与保护、个人数据默认保留的计划交互、退出协调和执行前复核仍待完成，见 [发布路线图](release-roadmap.md) 和 [精确关联位置](application-associations.md)。

常用安装根的三层范围、数量/时间预算、目录变化与 roots 字段见 [安装根发现](application-discovery.md)。完整外接卷枚举和安装收据仍待完成。

本地卷发现已接入同一候选验证链：固定 /Volumes 直属 App 与 Applications 下三层，另有独立的 128 候选预算；协议 v16 的 volumes 保存挂载/目录状态，partial 不能提升为无其他位置。网络、自动挂载和链接保持不完整；收据已接入有界 /usr/local 与 /opt 补查，见 [收据补查](application-receipts.md)。精确范围及与上游辅助 App 的差异见 [安装目录发现](application-discovery.md)。
