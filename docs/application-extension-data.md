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

## 证据与预算

扩展 ID 初始来自选中 App 同一轮的有界身份扫描；数据观察完成后再结束包内目录/身份复核。身份变化或撤回任一已选 ID 时，扩展数据整体变为未知并撤下旧标识、路径及 ByHost 结果；主应用原身份复核失败仍整份检查拒绝。部分包内扫描已取得且复核有效的 ID 可以继续观察，扩展数据保持 partial，不伪装成完整集合。

位置使用原关联 Probe：从根逐段无链接相对打开本地、非已知 dataless 目录，共享已验证父句柄；终点只观察元数据，不打开内容。类型、跨设备、文件硬链接或权限问题保留未知。返回前复核路径映射、设备/inode、类型/权限、flags、uid/gid、大小与时间；缺失点出现、替换和变化撤为 unknown/changed。ByHost 根也保存存在或缺失证据，返回前复核，不把范围变化当作无其他设置。

最多 8 个不同扩展 ID、64 个合并前精确模板；ByHost 最多扫描 1024 条目、保留 64 个不同候选。合并后最多 128 个数据位置，每项最多 8 个不同来源。数据枚举/探查检查点 500ms，返回前复核另有 500ms 检查点；达到边界明确不完整或未知，不承诺硬中断阻塞 I/O。没有取得可查询 ID 时不探查用户目录，空集合不代表没有扩展数据。

异步观察完成后，主 App、原关联位置和扩展数据统一读取最新客户端保护设置。每个路径按组件核对父/子/自身重叠；所选 App 或任一来源 ID 与当前客户端一致时保留自身原因。保护独立于存在状态，未发现但受保护的位置也展示保留意图，重查重新附加并清除旧提示。规则读取或附加失败仍整份拒绝。

## 协议与界面

应用协议 v17 新增 extensionData version=2、固定 revision、observed/partial/notChecked/unknown、issue、identifiers、entries 与 byHost。entries 每项为 relativePath、完整 sources、present/notFound/unknown、issue、protectionReasons、molePathProtection。byHost 单独包含 observed/partial/notChecked、issue、扫描条目数和不同候选数。

前端要求已选 ID 来自本次最终 embedded，已查询状态必须对应排序前 8 个；校验完整精确模板及合并来源、ByHost 全部匹配来源、路径唯一/直属名称、数量、保护和状态关系。来源丢失、遗漏精确位置、越界路径或矛盾协议整体拒绝。原 associations version=4 与 installations version=5 不变，IPC 仍只接受 taskId/index，没有前端目录、ID 或 HOME 覆盖。

数据状态在原嵌入扩展区内显示；扩展数据的客户端保护参与主保留结论，范围不完整不会隐藏身份限制。用户可查看具体问题并重新检查；一分钟较早状态、任务/设置绑定和迟到响应隔离继续生效。

本阶段仍未认证签名/entitlements、应用组成员或共享/个人数据归属，也不覆盖完整 Mole 候选/白名单/保护、退出协调或执行前验证。全部数据保持保留；未发现或没有命中保护不构成安全清理资格。

第六十二阶段 extensionData version=2 的路径同样附加 [Mole 卸载字面路径保留](application-uninstall-paths.md)，与来源 ID、存在状态和客户端保护独立。来源身份撤回仍整体撤下路径及保护观察；规则未知不取消客户端已有保留原因。
