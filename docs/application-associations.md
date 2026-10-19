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

这 16 项只查询精确元数据，不读取内容或计算大小；名称变体、ByHost、厂商/产品特例、Darwin 缓存根、系统数据与独立 CLI 数据仍未覆盖。应用组和派生位置另按下文有界扫描，不递归用户数据。显示「未发现」只针对这 16 个位置，不代表无其他关联文件。普通文件与目录均只观察元数据，不认证发布者或数据所有者；大小写不敏感卷上的路径命中也不能建立归属。

## 应用组与派生目录

额外按固定 find_app_files 范围枚举下面 4 个目录的直属条目，不进入候选内容：

| 说明 | 相对于后端 HOME 的根 |
| --- | --- |
| 应用组容器 | Library/Group Containers |
| 派生应用脚本 | Library/Application Scripts |
| 派生沙盒数据 | Library/Containers |
| 文件提供程序 | Library/Application Support/FileProvider |

使用新鲜主 Bundle ID 与固定 base.sh 的 mole_name_has_bundle_id_boundary 语义：文件名等于 ID、以 ID. 开头、以 .ID 结尾或包含 .ID.；保留 ASCII 大小写差异，不做松散子串或大小写归一。base.sh 另绑定 SHA-256 58627b0717c02c6d27dbd28d33ff038affad9d6e8fec4770905c1bb3f1fda6f3。两个源文件都需通过固定资源校验；生产不执行原 Shell。显式测试以 78 组文件名/标识与 3 个原辅助函数核对。

仅接收目录候选；匹配名称若是链接则额外保留为未知、不跟随，并将范围标为不完整（比原 find -type d 的忽略更保守）。普通文件不作为目录候选。与前 16 项重叠的位置只显示一次，动态路径按原字符串排序，最多 64 项。所有动态位置仍使用同一安全 Probe、返回前元数据复核及最新客户端保护。

最多扫描 1024 个直属条目、4 个根、64 个不同候选，枚举检查点预算 500ms；达到边界保守标为不完整，不把未访问范围视为不存在。根缺失时保存缺失证据，无法访问、链接/布局不支持、目录或访问状态变化分别保持可见。结束前复核根及已见候选，目录变化不能留下完整扫描声明；有效的独立候选观察可保留，未知状态不被覆盖。元数据 I/O 不承诺硬中断，整体仍受原工作器/停止等待约束。

目录名规则命中不证明应用组成员、已安装扩展、发布者、共享关系或数据独占。UI 提示「可能由多个应用共用，保留」，不打开内容、不提供勾选或删除。未按主 ID 命名的共享组、签名/entitlements 和完整归属仍待完成。已另行支持选中 App 内 appex/XPC/LoginItems 的有界标识读取，见 [嵌入扩展](application-embedded.md)；已取得并返回前复核的标识另用于 [扩展数据位置](application-extension-data.md)，不改变本节目录名候选的证据或归属。

## 文件系统与结果边界

从根目录以相对句柄逐段查询；中间目录不跟随符号链接，打开使用 O_NOFOLLOW、O_DIRECTORY、O_NONBLOCK 和 O_CLOEXEC。只继续本地、非已知 dataless 的目录。允许系统正常本地目录跨设备映射；候选终点跨设备、链接、非普通文件/目录或普通文件多重硬链接均保持未知。候选终点只做 fstatat，不打开内容；权限读取失败不折成不存在。标准路径的链接布局也保守显示未知。

同一轮查询按绝对组件前缀共享已验证的父目录句柄，避免 16 份查询重复打开共同祖先；每次复用仍核对当前目录映射、类型/元数据和本地条件。每个查询独立保留逐段元数据和缺失位置；返回前重新验证路径映射、设备/inode、类型/链接数/flags、uid/gid、大小和 mtime/ctime，持有目录同时复核本地文件系统条件。缺失位置出现、对象替换或元数据变化时撤回为 changed；重新查询失败为 unavailable。未支持布局为 unsupported。HOME 及其内目录的其他条目变动也可能保守地使结果未知；HOME 以上祖先仅复核对象、类型/权限、uid/gid 和 flags，避免公共临时目录中无关条目变动干扰结果。

读取不是文件系统事务，返回后仍可能变化；这些观察不构成修改身份凭据。没有基于此查询同标识所有安装副本、判断共享数据、检查个人数据内容或适配完整 Mole 数据保护。前端不提供以候选路径为参数的修改入口。

沿用两个实际工作器、5 秒停止等待和真实工作器完成前持有名额的约束；阻塞 I/O 不因超时而被声称已经终止。关闭详情和迟到响应不保留正面结论，一分钟后连同原观察标为较早。重新检查先撤下旧观察。没有自动轮询、数据库写入、额外任务历史、权限修改、服务控制或提权。

## 逐项客户端保护

异步文件观察结束后，从 SQLite 读取最新保护目录，再对所选 App、16 个精确关联路径与本轮组/派生候选附加客户端保护提示。目录规则仍按组件比较父/子/自身重叠，使用已保存路径的意图，不访问、改写或要求其当前存在。所选标识与客户端相同时，相关候选受自身保留规则约束，系统目录位置也保留额外客户端限制。

每项 protectionReasons 可包含 systemLocation/currentApplication/clientDirectory；不回传重复的绝对保护目录。命中独立于 present/notFound/unknown，不把路径保护当作存在或独占归属。摘要提示受保护位置数；默认展示发现/未知与受保护的未发现位置。未命中也全部保留，没有安全、可清理或可执行状态。

保持原设置变更后的重查和迟到响应隔离；再次附加新规则会替换旧原因。设置读取失败、规则异常仍整份拒绝，不能把未知规则当作空规则。当前仅适配客户端保护，不把 Mole cleanup 的 should_protect_data 当作卸载资格；完整 should_protect_path、白名单和归属检查仍待完成。路径别名、大小写与挂载同对象关系不由这份字面规则证明。

## 协议与后续

应用检查协议 v15 的 associations 包含 version=3、固定 revision、observed/unknown、issue 和 entries。观测成功时先按固定顺序返回 16 项，后接最多 64 个动态目录候选；每项 kind、relativePath、present/notFound/unknown、issue 和唯一/有界的 protectionReasons 需互相一致。discovery 记录组/派生范围的 observed/partial/notChecked、原因、尝试根数、条目与候选数和优先问题根；缺失根也计入尝试数，未知主身份/规则时不检查且计数为零。partial 会在折叠摘要显示，动态路径必须位于 4 个声明根直属且符合名称边界，不接受任意路径或重复位置。前端校验精确修订、身份、完整数量、精确 ID 路径和状态组合，未知版本/矛盾结果整体拒绝。只向界面返回相对路径，不接受前端 HOME、路径或 ID 覆盖。

observed 只表示进行了有界查询，16 项仍可能全部未知。共享或个人数据未确认时全部保留，既有客户端、系统组件、后台条件和官方卸载器限制继续显示。完整发布仍需多副本/共享归属、完整候选与保护规则、个人数据默认保留的计划交互、退出协调、执行前验证、逐项结果及真实权限/云/网络/卷变化矩阵。路线图中的完整关联文件项保持未完成。

关联数据旁新增 [安装副本](application-copies.md) 的系统登记位置观察；范围有限，完整扫描及其他标识共享关系仍待核实，全部保留策略不变。

[共享关联](application-copies.md) 现使用固定 Mole 保留谓词核对同一清单与系统登记候选，命中解释保留原因，未命中仍不证明独占归属。

共享关联已增加 [常用安装根的有界发现](application-discovery.md)，补查未登记/被清单省略的位置；目录问题、变化和上限保持不完整。命中仍持有证据，未命中释放句柄并在返回前重新核对元数据，不提供执行资格。本地卷和收据也已有有界补查，完整语义/权限/故障矩阵仍待验收。

[扩展数据位置](application-extension-data.md) 按已取得标识查询八类精确位置和一层 ByHost 文件，单独记录范围与身份撤回；没有扩大本节主 ID 四根的匹配规则。全部新位置同样在异步结束后附加最新客户端保护。
