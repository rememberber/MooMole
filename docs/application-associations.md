# 应用关联位置的只读观察

应用详情按需展示「关联位置」，先报告发现与无法确认的数量，展开查看路径、保留说明和下一步。所有位置均保留，没有删除勾选、卸载计划或可回收大小。目录名命中不证明独占归属，缓存也可能包含离线文件、剪贴板或其他个人数据。

## 当前范围

以本次成功 Mole apps 任务的 taskId/index 绑定应用；后端重新读取并复核包内身份，使用新鲜 Bundle ID。与清单不同或无法读取时不查询位置；清单未报告 ID 时保留未比对说明。只接受固定 Mole mole_is_reverse_dns_bundle_id 的 ASCII 分段语法，并额外保留 255 字节预算。

本轮适配固定修订 ba3a0f57f78e4c22818fda701219cc3dd4d6953f 的 find_app_files 中 16 个精确标识模板：

| 说明 | 相对于后端 HOME 的位置 |
| --- | --- |
| 应用数据 | Library/Application Support/{id} |
| 缓存 | Library/Caches/{id} |
| 日志 | Library/Logs/{id} |
| 设置 | Library/Preferences/{id}.plist |
| 恢复状态 | Library/Saved Application State/{id}.savedState |
| 沙盒数据 | Library/Containers/{id} |
| 网页数据 | Library/WebKit/{id} |
| 网络存储 | Library/HTTPStorages/{id} |
| 网页内容 | Library/WebKit/com.apple.WebKit.WebContent/{id} |
| 网络 Cookie | Library/HTTPStorages/{id}.binarycookies |
| Cookie | Library/Cookies/{id}.binarycookies |
| 应用脚本 | Library/Application Scripts/{id} |
| 输入法扩展 | Library/Input Methods/{id}.app |
| 自动保存 | Library/Autosave Information/{id} |
| 同步设置 | Library/SyncedPreferences/{id}.plist |
| 崩溃报告 | Library/Caches/com.plausiblelabs.crashreporter.data/{id} |

逻辑通过既有引擎清单和 app_protection.sh 固定摘要校验；生产环境不执行 Shell，不解析 find_app_files 的终端输出。模板是已审查原逻辑的有界子集，不是原函数完整调用。原源码模板对照和标识语法对照需显式运行相应忽略项。

未遍历目录、读取候选文件内容或计算大小；不包含名称变体、ByHost、应用组、扩展容器、厂商/产品特例、Darwin 缓存根、系统数据、嵌套搜索和独立 CLI 数据。显示「未发现」只针对这 16 个位置，不代表无其他关联文件。普通文件与目录均只观察元数据，不认证发布者或数据所有者；大小写不敏感卷上的路径命中也不能建立归属。

## 文件系统与结果边界

从根目录以相对句柄逐段查询；中间目录不跟随符号链接，打开使用 O_NOFOLLOW、O_DIRECTORY、O_NONBLOCK 和 O_CLOEXEC。只继续本地、非已知 dataless 的目录。允许系统正常本地目录跨设备映射；候选终点跨设备、链接、非普通文件/目录或普通文件多重硬链接均保持未知。候选终点只做 fstatat，不打开内容；权限读取失败不折成不存在。标准路径的链接布局也保守显示未知。

同一轮查询按绝对组件前缀共享已验证的父目录句柄，避免 16 份查询重复打开共同祖先；每次复用仍核对当前目录映射、类型/元数据和本地条件。每个查询独立保留逐段元数据和缺失位置；返回前重新验证路径映射、设备/inode、类型/链接数/flags、uid/gid、大小和 mtime/ctime，持有目录同时复核本地文件系统条件。缺失位置出现、对象替换或元数据变化时撤回为 changed；重新查询失败为 unavailable。未支持布局为 unsupported。HOME 及其内目录的其他条目变动也可能保守地使结果未知；HOME 以上祖先仅复核对象、类型/权限、uid/gid 和 flags，避免公共临时目录中无关条目变动干扰结果。

读取不是文件系统事务，返回后仍可能变化；这些观察不构成修改身份凭据。没有基于此查询同标识所有安装副本、判断共享数据、检查个人数据内容或适配完整 Mole 数据保护。前端不提供以候选路径为参数的修改入口。

沿用两个实际工作器、5 秒停止等待和真实工作器完成前持有名额的约束；阻塞 I/O 不因超时而被声称已经终止。关闭详情和迟到响应不保留正面结论，一分钟后连同原观察标为较早。重新检查先撤下旧观察。没有自动轮询、数据库写入、额外任务历史、权限修改、服务控制或提权。

## 逐项客户端保护

异步文件观察结束后，从 SQLite 读取最新保护目录，再对所选 App 和 16 个精确关联路径附加客户端保护提示。目录规则仍按组件比较父/子/自身重叠，使用已保存路径的意图，不访问、改写或要求其当前存在。当前客户端同标识数据保留自身保护，系统目录位置也保留额外客户端限制。

每项 protectionReasons 可包含 systemLocation/currentApplication/clientDirectory；不回传重复的绝对保护目录。命中独立于 present/notFound/unknown，不把路径保护当作存在或独占归属。摘要提示受保护位置数；默认展示发现/未知与受保护的未发现位置。未命中也全部保留，没有安全、可清理或可执行状态。

保持原设置变更后的重查和迟到响应隔离；再次附加新规则会替换旧原因。设置读取失败、规则异常仍整份拒绝，不能把未知规则当作空规则。当前仅适配客户端保护，不把 Mole cleanup 的 should_protect_data 当作卸载资格；完整 should_protect_path、白名单和归属检查仍待完成。路径别名、大小写与挂载同对象关系不由这份字面规则证明。

## 协议与后续

应用检查协议 v12 的 associations 包含 version=2、固定 revision、observed/unknown、issue 和 entries。观测成功时必须按固定顺序返回 16 项；每项 kind、relativePath、present/notFound/unknown、issue 和唯一/有界的 protectionReasons 需互相一致。前端校验精确修订、身份、完整数量、精确 ID 路径和状态组合，未知版本/矛盾结果整体拒绝。只向界面返回相对路径，不接受前端 HOME、路径或 ID 覆盖。

observed 只表示进行了有界查询，16 项仍可能全部未知。共享或个人数据未确认时全部保留，既有客户端、系统组件、后台条件和官方卸载器限制继续显示。完整发布仍需多副本/共享归属、完整候选与保护规则、个人数据默认保留的计划交互、退出协调、执行前验证、逐项结果及真实权限/云/网络/卷变化矩阵。路线图中的完整关联文件项保持未完成。

关联数据旁新增 [安装副本](application-copies.md) 的系统登记位置观察；范围有限，完整扫描及其他标识共享关系仍待核实，全部保留策略不变。

[共享关联](application-copies.md) 现使用固定 Mole 保留谓词核对同一清单与系统登记候选，命中解释保留原因，未命中仍不证明独占归属。

共享关联已增加 [常用安装根的有界发现](application-discovery.md)，补查未登记/被清单省略的位置；目录问题、变化和上限保持不完整。命中仍持有证据，未命中释放句柄并在返回前重新核对元数据，不提供执行资格。本地卷和收据也已有有界补查，完整语义/权限/故障矩阵仍待验收。
