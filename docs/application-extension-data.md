# 扩展关联数据的只读观察

原应用详情的「嵌入扩展」内展示扩展数据：先说明发现、无法确认与受保护数量，按需查看路径和来源标识。所有位置均保留，不读取内容、不估算回收大小，也没有删除勾选或执行入口。

## 固定范围

依据固定 Mole 修订 ba3a0f57f78e4c22818fda701219cc3dd4d6953f 的 find_app_files 中 embedded_candidate 循环，使用 [包内扩展检查](application-embedded.md) 已取得的 ID，不根据目录名、厂商或终端文案推断扩展身份。固定 app_protection.sh/base.sh 资源校验继续生效，生产不执行 Shell、find 或包内程序。

按 ASCII ID 排序、去重，最多取前 8 个。每个 ID 查询以下精确元数据位置：

| 说明 | 相对于后端 HOME 的位置 |
| --- | --- |
| 扩展脚本 | Library/Application Scripts/{id} |
| 文件提供程序 | Library/Application Support/FileProvider/{id} |
| 扩展缓存 | Library/Caches/{id} |
| 扩展沙盒 | Library/Containers/{id} |
| 扩展网络存储 | Library/HTTPStorages/{id} |
| 扩展 Cookie | Library/HTTPStorages/{id}.binarycookies |
| 扩展设置 | Library/Preferences/{id}.plist |
| 扩展网页数据 | Library/WebKit/{id} |

另一次扫描 Library/Preferences/ByHost 的直属条目，按原大小写敏感模式 {id}.*.plist 接受普通文件。星号可以匹配空字符串，因而 {id}..plist 命中，而 {id}.plist 不命中。匹配的链接额外保留未知并标记范围不完整，不跟随；目录不作为 ByHost 文件候选。

每个完整路径只观察和展示一次，sources 保留全部 bundleId/kind。HTTPStorages 中某 ID 的 Cookie 路径可能同时是另一 ID 的网络存储路径；ByHost 的点分前缀也可能匹配多个 ID，均合并来源而不扩大查询对象。不同主 ID/组目录结果仍维持原独立语义，扩展位置的来源不证明独占归属。

## 扩展标识关联的应用组名称补查

客户端新增一层 Library/Group Containers 名称补查，使用同一轮已取得且最终复核有效的前 8 个扩展标识。匹配复用固定 Mole base.sh 的 mole_name_has_bundle_id_boundary：名称等于 ID、以 ID. 开头、以 .ID 结尾或包含 .ID.，保持大小写差异。只接受目录候选；匹配链接另保留未知且范围不完整，不跟随，不进入候选内容。未命中任一已选扩展标识、无点分边界的近似名称或根下深层目录不会被扩大查询。

这是客户端补充观察，不是原版 find_app_files 已经为扩展 ID 执行的组枚举，也不是 entitlements 或签名认证。groups 的 source 固定为 clientEmbeddedIdNameBoundary、version=1，用于与 Mole 原有八类精确位置/ByHost 来源区分。目录名关联不能证明应用组成员、独占归属或没有其他共享者，全部保留。

一个完整路径只保留一条扩展数据行，sources 收录全部命中的已选扩展 ID（kind=groupContainer），不只记录第一个标识。该行和其他扩展位置一样附加最新客户端保护与 Mole 路径保留；源身份变化或撤回时一并撤下路径、来源和组范围。主 ID 与扩展观察仍分别展示；主保留结论按完整相对路径去重计数，避免同一路径在两个来源中被重复计为多个受保护位置，不将此计数当作空间度量。

组扫描最多 1024 个直属条目、64 个不同候选，与精确位置和 ByHost 共享同一轮 500ms 枚举/建证据检查点；返回前复核也共享 500ms。根缺失会保存缺失证据，出现、替换、不可读或链接布局保持明确的不完整/未知。超限不声明无其他组，未复核行不发布为通过。groups 单独记录 observed/partial/notChecked、issue、entries/candidates，partial 要求扩展数据整体 partial；优先保留已有来源/ByHost 的整体原因，并独立展示组问题。无可查询 ID 时不访问用户组目录。

## 证据与预算

扩展 ID 初始来自选中 App 同一轮的有界身份扫描；数据观察完成后再结束包内目录/身份复核。身份变化或撤回任一已选 ID 时，扩展数据整体变为未知并撤下旧标识、路径及 ByHost/应用组结果；主应用原身份复核失败仍整份检查拒绝。部分包内扫描已取得且复核有效的 ID 可以继续观察，扩展数据保持 partial，不伪装成完整集合。

位置使用原关联 Probe：从根逐段无链接相对打开本地、非已知 dataless 目录，共享已验证父句柄；终点只观察元数据，不打开内容。类型、跨设备、文件硬链接或权限问题保留未知。返回前复核路径映射、设备/inode、类型/权限、flags、uid/gid、大小与时间；缺失点出现、替换和变化撤为 unknown/changed。ByHost 根也保存存在或缺失证据，返回前复核，不把范围变化当作无其他设置。

最多 8 个不同扩展 ID、64 个合并前精确模板；ByHost 和应用组各最多扫描 1024 条目、保留 64 个不同候选。精确位置和 ByHost 合并后最多 128 个数据位置，另有最多 64 个应用组候选，合计最多 192 个位置，每项最多 8 个不同来源。数据枚举/探查检查点 500ms，返回前复核另有 500ms 检查点；达到边界明确不完整或未知，不承诺硬中断阻塞 I/O。没有取得可查询 ID 时不探查用户目录，空集合不代表没有扩展数据。

异步观察完成后，主 App、原关联位置和扩展数据统一读取最新客户端保护设置。每个路径按组件核对父/子/自身重叠；所选 App 或任一来源 ID 与当前客户端一致时保留自身原因。保护独立于存在状态，未发现但受保护的位置也展示保留意图，重查重新附加并清除旧提示。规则读取或附加失败仍整份拒绝。

## 协议与界面

应用协议 v20 的 extensionData version=3、固定 revision、observed/partial/notChecked/unknown、issue、identifiers、entries、byHost 与 groups。entries 每项为 relativePath、完整 sources、present/notFound/unknown、issue、protectionReasons、molePathProtection。byHost 单独包含 observed/partial/notChecked、issue、扫描条目数和不同候选数。

前端要求已选 ID 来自本次最终 embedded，已查询状态必须对应排序前 8 个；校验完整精确模板及合并来源、ByHost/应用组全部匹配来源、路径唯一/直属名称、数量、保护和状态关系。来源丢失、遗漏精确位置、越界路径或矛盾协议整体拒绝。当前 associations version=6 与 installations version=5，IPC 仍只接受 taskId/index，没有前端目录、ID 或 HOME 覆盖。

数据状态在原嵌入扩展区内显示；应用组行明确「目录名与扩展标识关联，可能由多个应用共用，保留；组成员身份未认证」，范围不完整给出原因。扩展数据的客户端保护参与主保留结论，范围不完整不会隐藏身份限制。用户可查看具体问题并重新检查；一分钟较早状态、任务/设置绑定和迟到响应隔离继续生效。

本阶段仍未认证签名/entitlements、应用组成员或共享/个人数据归属，也不覆盖完整 Mole 候选/白名单/保护、退出协调或执行前验证。全部数据保持保留；未发现或没有命中保护不构成安全清理资格。

第六十二阶段 extensionData version=2 的路径同样附加 [Mole 卸载字面路径保留](application-uninstall-paths.md)，与来源 ID、存在状态和客户端保护独立。来源身份撤回仍整体撤下路径及保护观察；规则未知不取消客户端已有保留原因。

第六十五阶段扩展数据包含上述客户端应用组名称补查；独立于主 ID 四根和 Mole 原精确模板，不改变原 CLI 执行语义或开放卸载。groups 候选须为直属 Group Containers 路径，前端要求来源为全部匹配的已选 ID，拒绝伪造 Mole 认证来源、遗漏/重复来源、越界路径及计数/状态矛盾。
