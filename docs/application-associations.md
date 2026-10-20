# 应用关联位置的只读观察

应用详情按需展示「关联位置」，先报告发现与无法确认的数量，展开查看路径、保留说明和下一步。所有位置均保留，没有删除勾选、卸载计划或可回收大小。目录名命中不证明独占归属，缓存也可能包含离线文件、剪贴板或其他个人数据。

## 当前范围

以本次成功 Mole apps 任务的 taskId/index 绑定应用；后端重新读取并复核包内身份，使用新鲜 Bundle ID。与清单不同或无法读取时不查询位置；清单未报告 ID 时保留未比对说明。只接受固定 Mole mole_is_reverse_dns_bundle_id 的 ASCII 分段语法，并额外保留 255 字节预算。

本轮适配固定修订 ba3a0f57f78e4c22818fda701219cc3dd4d6953f 的 find_app_files 中 21 个精确标识模板：

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
| 设置目录 | Library/Preferences/{id} |
| 下载缓存 | Library/Caches/com.apple.nsurlsessiond/Downloads/{id} |
| 最近文档记录 | Library/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments/{id}.sfl2 / .sfl3 / .sfl4（分别观察） |

逻辑通过既有引擎清单和 app_protection.sh 固定摘要校验；生产环境不执行 Shell，不解析 find_app_files 的终端输出。模板是已审查原逻辑的有界子集，不是原函数完整调用。原源码模板对照和标识语法对照需显式运行相应忽略项。

这 21 项只查询精确元数据，不读取内容或计算大小；新增设置目录/下载缓存要求目录类型，三类最近记录要求普通文件，类型不符为未知。名称变体、厂商/产品特例、Darwin 缓存根、系统数据与独立 CLI 数据仍未覆盖。应用组和派生位置另按下文有界扫描，不递归用户数据。本节 21 项的「未发现」不代表无其他关联文件；整体摘要另计下文名称及动态候选，不将未查询范围算作不存在。普通文件与目录均只观察元数据，不认证发布者或数据所有者；大小写不敏感卷上的路径命中也不能建立归属。

## 应用组与派生目录

额外按固定 find_app_files 范围枚举下面 4 个目录的直属条目，不进入候选内容：

| 说明 | 相对于后端 HOME 的根 |
| --- | --- |
| 应用组容器 | Library/Group Containers |
| 派生应用脚本 | Library/Application Scripts |
| 派生沙盒数据 | Library/Containers |
| 文件提供程序 | Library/Application Support/FileProvider |

使用新鲜主 Bundle ID 与固定 base.sh 的 mole_name_has_bundle_id_boundary 语义：文件名等于 ID、以 ID. 开头、以 .ID 结尾或包含 .ID.；保留 ASCII 大小写差异，不做松散子串或大小写归一。base.sh 另绑定 SHA-256 58627b0717c02c6d27dbd28d33ff038affad9d6e8fec4770905c1bb3f1fda6f3。两个源文件都需通过固定资源校验；生产不执行原 Shell。显式测试以 78 组文件名/标识与 3 个原辅助函数核对。

仅接收目录候选；匹配名称若是链接则额外保留为未知、不跟随，并将范围标为不完整（比原 find -type d 的忽略更保守）。普通文件不作为目录候选。与前 21 项重叠的位置只显示一次，动态路径按原字符串排序，最多 64 项。所有动态位置仍使用同一安全 Probe、返回前元数据复核及最新客户端保护。

最多扫描 1024 个直属条目、4 个根、64 个不同候选，枚举检查点预算 500ms；达到边界保守标为不完整，不把未访问范围视为不存在。根缺失时保存缺失证据，无法访问、链接/布局不支持、目录或访问状态变化分别保持可见。结束前复核根及已见候选，目录变化不能留下完整扫描声明；有效的独立候选观察可保留，未知状态不被覆盖。元数据 I/O 不承诺硬中断，整体仍受原工作器/停止等待约束。

目录名规则命中不证明应用组成员、已安装扩展、发布者、共享关系或数据独占。UI 提示「可能由多个应用共用，保留」，不打开内容、不提供勾选或删除。未命中主 ID 或已取得扩展 ID 名称边界的共享组、签名/entitlements 和完整归属仍待完成。已另行支持选中 App 内 appex/XPC/LoginItems 的有界标识读取，见 [嵌入扩展](application-embedded.md)；已取得并返回前复核的标识另用于 [扩展数据位置](application-extension-data.md)，不改变本节目录名候选的证据或归属。

## 主应用设备设置与个人记录

另按固定 Mole 的主 ID ByHost 分支，有界读取 Library/Preferences/ByHost 直属名称，只接受普通文件且名称以 {id}. 开头、以 .plist 结尾。大小写敏感，保留原 mole_name_starts_with_bundle_id_boundary 语义；{id}.plist、{id}..plist 和 {id}.DEVICE.plist 都命中。它与扩展的 {id}.*.plist 不同，不能直接复用扩展匹配式。前后点分边界不对称：TEAM.{id}.DEVICE.plist 和 {id}le.DEVICE.plist 均不命中。

最多扫描 1024 个条目、64 个文件候选；枚举/建证据和返回前复核各有 500ms 检查点，与组/派生目录的预算和不完整状态分别记录。所有候选只观察元数据、不读取设置内容；链接额外保留未知且范围不完整，目录不作为文件候选。根缺失会保存缺失证据，返回前出现、替换或访问变化不能当作无设备设置。未能建齐或复核的候选保持未知，超限/读取失败/不支持/变化在折叠摘要和展开原因中可见。

主模板、组目录、设备文件返回路径唯一，依次为 21 个固定模板、最多 64 个组/派生目录和最多 64 个设备设置，另加最多 73 个 Library/XDG 名称及 VS Code 专属位置，另有最多 64 个厂商嵌套候选，合计最多 286 项。设备文件同样附加最新客户端保护和固定 Mole 路径保护；范围检查不完整不取消已有正面保留。原主应用身份失败仍整份拒绝。

最近文档记录是个人使用状态，仅显示元数据并明确保留，不打开或处理所指向的原文档；设备设置明确保留。下载缓存也不推导可安全删除。当前仍无卸载计划、清理勾选或执行入口，未证明这些位置独占归属于选中应用。

## Library 名称位置

补入固定 Mole find_app_files 的 24 类 Library 字面名称模板。名称来自选中条目原 Mole 清单，不从包文件名、厂商、CFBundleDisplayName 或前端路径重建。前端用所选条目的名称与结果绑定；本地身份仍按原流程复核，但这些查询不认证本地显示名称或独占归属。

| 名称线索 | 相对于后端 HOME 的位置 |
| --- | --- |
| 应用数据 | Library/Application Support/{name} |
| 缓存 | Library/Caches/{name} |
| 日志 | Library/Logs/{name} |
| 设置位置 | Library/Preferences/{name} |
| 设置文件 | Library/Preferences/{name}.plist |
| 恢复状态 | Library/Saved Application State/{name}.savedState |

| 服务或组件名称线索 | 相对于后端 HOME 的位置 |
| --- | --- |
| 服务工作流 | Library/Services/{name}.workflow |
| Quick Look | Library/QuickLook/{name}.qlgenerator |
| 网络插件 | Library/Internet Plug-Ins/{name}.plugin |
| 音频组件 | Library/Audio/Plug-Ins/Components/{name}.component |
| VST / VST3 | Library/Audio/Plug-Ins/VST/{name}.vst / VST3/{name}.vst3（分别观察） |
| Digidesign | Library/Audio/Plug-Ins/Digidesign/{name}.dpm |
| 偏好面板 | Library/PreferencePanes/{name}.prefPane |
| 输入法 | Library/Input Methods/{name}.app |
| 屏幕保护 | Library/Screen Savers/{name}.saver |
| 框架 | Library/Frameworks/{name}.framework |
| 菜单插件 | Library/Contextual Menu Items/{name}.plugin |
| Spotlight | Library/Spotlight/{name}.mdimporter |
| 颜色面板 | Library/ColorPickers/{name}.colorPicker |
| 工作流 | Library/Workflows/{name}.workflow |
| 通讯录插件 | Library/Address Book Plug-Ins/{name}.bundle |
| 辅助功能组件 | Library/Accessibility/{name}.bundle |
| 邮件插件 | Library/Mail/Bundles/{name}.mailbundle |

以上新增 18 项来自同一原始名称 user_patterns 块中所有剩余的 Library 模板，块内的四个 XDG/小写位置及空格变体另见下文。保留原大小写与后缀，仅在名称对应位置检查元数据；不读取组件内部文件，不加载/执行/注册插件，不查询注册、启用或使用状态。普通文件/目录都可以是该模板的存在位置，不能把后缀当作有效组件类型。行内明确「名称线索，注册、使用状态和归属未确认，保留」。

固定为 C 字节语义的至少 2 字节，另将输入限制在 240 UTF-8 字节内的单个正常组件；拒绝点/双点、斜杠与控制字符，缺失/不支持保持未检查，不伪装为没有数据。空格、中文、glob 或 Shell 字样全部作为名称的字面部分，不插值或扩展。24 个模板在原函数的标准 user_patterns 中按 -e 判断；当前普通文件/目录都只观察元数据，不将名称种类当作实际文件类型或内容认证。

按原 common-root deny arm 排除 Library/Preferences/ByHost 本身，并按 _mole_is_shared_home_state_root 排除 Library/Application Support/Epic 共享根；与 21 个精确标识位置重合时保留原精确来源、不重复增加行。名称等于 ID 时共有七项重合（含输入法 .app），额外查询 20 项（含去重后的 3 个 XDG 位置）；ByHost 名称仍排除公共设置根本身，查询 27 项。names.candidates 表示本次额外查询的位置数（0..73），不是发现/可回收数量。名称不可用、不支持或预算失败不取消原精确标识、组/派生和设备设置结果；新行同样附加 Mole 路径保留与最新客户端保护，未知和受保护的未发现位置继续可见。

名称查询/返回前复核各有 500ms 检查点，复用 Probe 的无链接相对打开、父句柄池、缺失证据及返回前验证；复核超限整组撤下名称路径，只留 unknown/limitReached。选中 App 的最后身份验证在名称结果结束后进行，App 变化仍整份拒绝。阻塞内核 I/O 不承诺硬中断，原工作器超时和占用槽继续生效。

names 的 version=8/source=moleAppNameUserPaths，code 为 observed/notChecked/unknown，另含 issue、原清单 name 或 null、candidates、cliExcluded、baseName、leafNames、vscodeChannel、vendor。前端检查原名 24 类、适用 Library 空格变体 12 类与标准/复合 XDG 13 类、适用渠道基础名 9 类及包名派生 10 项、适用 VS Code 5 项扣除去重/根排除/独立 CLI 后的完整路径集合、名称与所选条目一致、来源/版本、状态和计数；旧协议或伪造来源/名称不发布结果。未检查在折叠摘要可见，展开说明原因；新行明确「名称线索，不证明归属，保留」。

当前已适配下述严格包名末段推导、VS Code 字面位置与有界用户厂商嵌套发现，其余产品专属规则仍待适配，不扩大到上级公共目录，不宣称完整 find_app_files。全部数据保留，无修改入口或执行资格。

## 空格名称变体

复用固定 Mole compound naming 块中的全部 12 个 Library 模板。原名称长度按 C locale 的 UTF-8 字节计数，超过 3 字节且含普通 ASCII 空格时适用；删除空格的形式查询 Application Support、Caches、Logs、Preferences（目录及 .plist）、Saved Application State（.savedState）共 6 项，将空格替换为下划线或连字符的形式分别查询 Application Support、Preferences（目录及 .plist）共 6 项。不折叠连续空格，不做小写、Unicode 空白替换或 Shell/glob 展开；例如 Suite  Studio 保留 Suite__Studio / Suite--Studio。

仍要求原名为 2..240 UTF-8 字节的正常单组件；控制空白不支持。若适用的去空格结果为空、点或双点，整组名称检查 notChecked/nameUnsupported，保留原 ID 等其他来源；这是客户端保守子集，不扩大到公共根或返回部分名称通过。生成 ByHost/Epic 的变体同样排除，路径与精确 ID 重合时保留精确来源，名称来源内也去重。加入下文 XDG 后，A B（3 字节）为 28 项，中 X（5 字节）为 49 项，By Host / Ep ic 为 48 项，org.example. app 对 org.example.app 去重后为 42 项。

名称变体和原名共用同一组开始/结束各 500ms 检查点、父句柄池、身份撤回与保护预算。只观察元数据，可能是跨渠道或共享数据；行内显示「空格变体线索，可能共用，不证明归属，保留」。默认只显示发现、未知及受保护的未发现位置；不增加页面或选择/修改入口。前端按绑定原名重新生成完整路径集合，拒绝缺少、重复、越界、小写伪造和错误种类，旧 v23/associations 9/names 4 也拒绝。

## XDG 位置与独立 CLI 排除

标准名称块增加 .config/{name}、.cache/{name}、.cache/{lowercase_name}、.local/share/{name}；适用空格门槛时再增加去空格/连字符/下划线的三种 ASCII 小写形式在上述三个父目录下的 9 个模板。保持固定 Mole 的 HOME 相对根，不读取 XDG_CONFIG_HOME 等自定义环境目录，不创建 ~/.{name}、任意 dotdir 或 rc 文件候选。普通小写名称的两个 cache 模板完全同路径时先保留原名来源；仅做字面路径去重，不推断大小写别名为同一对象。加上下文适用渠道模板后加上严格包名派生后的最大为 24+12+13+9+10+5=73 项，含厂商发现的主关联上限为 286 项。

小写与固定 C locale 的 tr 一致，仅转换 ASCII A-Z，Ü/İ 等字符保持。原 echo 单参数为 -n/-e/-E 的组合时输出为空；Rust 只复现这个确定转换，不执行 Shell，空派生按原公共根规则排除。源模板、实际转换赋值、公共根 case 和两个排除 helper 都有固定源码对照，未把自己的小写算法说成上游代码调用。

Rust 按固定 _mole_is_shared_home_state_root 与 _path_belongs_to_independent_cli 的本次可达规则在查询前过滤：共享 .cache/.config/.local 及其 bin/lib/share/state 根（ASCII 大小写不敏感）不纳入；独立 CLI 只在 .config/.cache/.local/share 直属判断，将叶名去一个前导点、ASCII 小写后精确匹配 claude/opencode/codex/gemini。Library 同名路径不受这个 CLI 排除，codex-beta、..codex 等不被前缀误排除。路径去重后，cliExcluded 记录因 CLI 规则排除的字面位置数，不表示扫描到文件，也不认证实际归属；Claude 为 4、claude 为 3、Co dex 的紧凑小写形式为 3。

独立 CLI 排除先于 Probe，不打开被排除的 XDG 根；names 失败/未检查/超限时清空此计数，前端按完整来源重算并校验，不能靠伪造排除数隐藏位置或注入 CLI 数据候选。折叠摘要提示 CLI 同名位置保留，展开说明数量与不纳入候选。XDG 行明确「名称线索，可能含独立工具配置或共享数据，归属未确认，保留」。所有未排除位置继续附加 Mole 路径保留与最新客户端保护，符号链接/权限/变化/超限不转为不存在或通过；无需提权或读取配置内容。

## 厂商嵌套目录

适配固定 find_vendor_nested_app_paths：仅在清单名称至少 4 个 C 字节且不属于原 40 个通用名称时，使用有效 reverse-DNS ID 的倒数第二段/末段作为厂商/产品 token；两段都至少 3 个 ASCII 字节。保留原 ASCII 小写和字面名称边界：父目录完整名称等于厂商 token，下一层目录名称等于原名、去空格、连字符、下划线变体或产品 token，或紧接 ASCII 空格、连字符、下划线、点。Sibelius.old 会命中，SibeliusBackup、Sibelius2026 不命中；通用名称排除不 trim，不擅自增删词表，也不将 token 当作发布者认证。

仅检查 Application Support、Caches、Logs 三个 Library 根的下一层厂商目录及再下一层产品目录。先按原厂商条件筛选父名称，仅进入匹配父目录，候选集合等价于原两层 find 的名称筛选；不扫描其他厂商的子树，不将厂商根本身作为候选，不递归产品内容。缺失根保留缺失证据；网络/链接/dataless/普通文件父布局等保守不支持，不跟随链接或隐式加载云数据。

与名称查询共享开始/结束各 500ms 检查点，并限制所有已枚举名称合计 1024 条、最多 64 个候选。三个根和匹配父目录使用无链接句柄/返回前元数据复核，候选要求目录类型。错误、变化、不支持名称/布局或超限将 vendor 标为 partial 并撤下该组全部候选，保留原因、计数和相关 Library 根；不返回不完整前缀来证明未发现。原精确、名称、ByHost 等来源继续独立，未知主身份仍整份撤回。

names.vendor 为 version=1/source=moleVendorNestedUserPaths，包含 observed/partial/notChecked、issue、vendorToken/productToken、rootCount/entries/candidates 和 firstUncertainRoot；未检查时 token/相关根为 null、数量为零。适用且完整时 rootCount=3、返回候选只含 present 目录；partial 的 candidates=0 且不返回厂商行。名称缺失/不支持或整体名称预算撤回时同样不返回厂商证据，通用名称/短 token 等为 notApplicable。静态 names.candidates 仍只计最多 73 个名称/产品字面位置，厂商候选另计、主关联上限为 286。

前端按绑定 ID/清单名称复算 token、通用名称排除、完整父名称/子名称边界、层级、种类/根、类型/状态及计数，拒绝额外层级、宽松前缀、伪造来源/旧协议和 partial 残留路径。新行附加最新客户端与 Mole 保护，显示「厂商与产品名称线索，可能共用，不证明归属，保留」；不完整在原折叠摘要直接可见，展开说明撤下和重查，不增加页面、控件或执行资格。目录存在/名称规则命中不证明共享归属或内容安全，所有数据保留。

## VS Code 专属位置

适配固定 find_app_files 的 VS Code 字面目录块：两类 ShipIt 更新缓存，以及所选渠道的扩展/配置 dotdir、Application Support 用户数据、Caches 目录，共 5 项。稳定版使用 .vscode、Code、com.microsoft.VSCode；Insiders 使用 .vscode-insiders、Code - Insiders、com.microsoft.VSCodeInsiders。两种渠道都会补查 com.microsoft.VSCode.ShipIt 和 com.microsoft.VSCodeInsiders.ShipIt，不能将另一渠道更新缓存说成当前应用独占。

保留原大小写敏感、未锚定正则 microsoft.*[vV][sS][cC]ode；渠道判断只允许 [iI]nsiders，并非任意 ASCII 大小写归一。Microsoft/VSCODE 不适用，VSCodeINSIDERS 仍按原规则进入稳定版；包含该子串也不是微软签名或产品身份认证。此块在原 collect_toolchain_leftovers 控制之外，不擅自套用其他工具链的 sibling guard；当前只读结果均保留，不能据此解除多副本保护。

本轮作为现有名称范围的保守扩展，要求本地 ID/规则及原清单名称可用；名称缺失或不支持时整个名称范围明确未检查，不返回 VS Code 通过。与原名、空格、渠道、包名位置共用有界元数据、去重和撤回；原精确/名称来源优先，如 Code/com.microsoft.VSCode 的用户数据和 ID 缓存不重复，新补 3 项、名称范围合计 31 项。Visual Studio Code 对同 ID 新补 4 项、合计 53 项。

新增位置遵循原 -d 类型条件：目录只读观察；普通文件、链接或变化保持 unknown/unsupported 或 changed，不误标为不存在，也不读取其内容。原精确/名称去重来源保持原类型语义。names.vscodeChannel 为已检查且适用的 stable/insiders，其余为 null；前端从绑定 ID 复算渠道与全部路径，校验完整去重集合，超限与来源失败撤下产品字段及路径。原关联详情展开解释扩展/用户数据可能含个人配置、两类更新缓存可能共用，每行保留归属未确认提示；没有新增页面、选择或修改入口。

## 严格包名派生

适配固定 find_app_files 中 bundle_leaf 分支的全部 5 个 Library 字面模板，每个派生名分别补查 Application Support、Caches、Logs、Preferences 的 .plist 与 Saved Application State 的 .savedState，共最多 10 项。仅用本次应用本地身份核对后的主 Bundle ID，不接受前端传入 ID，也不根据显示路径反推 ID。

保留原 C 字节条件：清单名称至少 3 字节、去 ASCII 空格后至少 3 字节；有效 reverse-DNS ID 的末段至少 8 字节，与原名不同、包含 [a-z][A-Z]，ASCII 小写后以去空格名称为完整字面前缀且至少多一个字符；后缀首字节必须为大写字母或数字。Wrapper/com.wrapper.GoogleChrome、64Gram/org.fork.TelegramDesktop 不适用，不能因为 ID 内有产品名就生成另一产品的数据路径。

只生成原末段和「原清单名称 + 空格 + 原后缀分词」两个名字。分词保留原 sed 的两步 acronym/小写或数字接大写边界；只拆后缀、不重新分词原名，如 AyuGramDesktop → AyuGram Desktop、Ayu Gram + AyuGramDesktop → Ayu Gram Desktop、AyuGram2FAThing → AyuGram 2 FA Thing。不截断较长派生组件或扩大到公共父目录；超长组件等元数据错误保持未知。

names.leafNames 为适用且已检查的两个有序原大小写名字，其余为空数组；前端从绑定的本地 ID 和原名复算完整名字、路径、去重及数量，拒绝伪造/缺失/顺序错误和旧协议。包名派生与其他名称位置共用父句柄池、开始/结束各 500ms 检查点、最后应用身份复核与最新保护；超限撤下全部名称证据。行内显示「包名派生线索，不证明归属，可能共用，保留」，补查说明只在原关联详情展开显示，无新增页面或执行入口。

## 渠道基础名称

接入固定 find_app_files 的 base_name 分支，复用共享保留谓词已有的同一 C locale 渠道后缀提取逻辑。后缀为 Nightly/Beta/Alpha/Dev/Canary/Preview/Insider/Edge/Stable/Release/RC/LTS/Developer Edition/Technology Preview，大小写敏感，必须有前置空白与非空基础名，只移除一层。共享保留的 .app 扩展名处理及后续 ASCII 小写行为保持原样，名称候选则绑定 Mole 原清单名称，不剥离其中的 .app。

遵循原 POSIX ERE 的贪婪捕获：Zed  Nightly → Zed（含一个尾随空格），Zed Beta Nightly → Zed Beta，Safari Technology Preview → Safari Technology，Firefox Developer Edition → Firefox；不自行去空格、反复剥离或优先认定长后缀。只有派生结果不同且超过 2 个 UTF-8 字节才适用，AB Beta 不增加渠道位置、中 Beta 增加。控制空白仍受整体名称的保守输入限制，Unicode 非 C 空白不作为分隔符。

适用时补查基础名的 Application Support/Caches/Logs/Preferences 目录及 .plist/Saved Application State 共 6 项，以及基础名 ASCII 小写的 .config/.cache/.local/share 共 3 项。与原名、空格/XDG 变体共用 500ms 开始/结束检查点、父句柄池、缺失/变化证据和保护预算；不递归为基础名生成更多变体。ByHost/Epic 共享根、空小写公共根、独立 CLI、精确 ID 来源去重继续在访问前生效：Codex Nightly 为 55 个名称位置、3 个 CLI 排除；org.example.app Nightly 对原主 ID 的 6 个基础名 Library 位置去重后为 52 个。

baseName 只在已检查且规则适用时为原大小写基础名，其余为 null；与路径、cliExcluded 一同按绑定原名在前端重算，未知/超限时整组撤下。渠道行显示「渠道名称线索，可能共用，归属未确认，保留」，展开说明按 Mole 渠道规则补查的实际基础名；这不是渠道类型或发布者认证。例如 Microsoft Edge 按同一原规则得到 Microsoft，界面明确可能与其他版本或应用共用，全部保留，不把宽泛基础名说成独占数据。

## 文件系统与结果边界

从根目录以相对句柄逐段查询；中间目录不跟随符号链接，打开使用 O_NOFOLLOW、O_DIRECTORY、O_NONBLOCK 和 O_CLOEXEC。只继续本地、非已知 dataless 的目录。允许系统正常本地目录跨设备映射；候选终点跨设备、链接、非普通文件/目录或普通文件多重硬链接均保持未知。候选终点只做 fstatat，不打开内容；权限读取失败不折成不存在。标准路径的链接布局也保守显示未知。

同一轮查询按绝对组件前缀共享已验证的父目录句柄，避免 21 份查询重复打开共同祖先；每次复用仍核对当前目录映射、类型/元数据和本地条件。每个查询独立保留逐段元数据和缺失位置；返回前重新验证路径映射、设备/inode、类型/链接数/flags、uid/gid、大小和 mtime/ctime，持有目录同时复核本地文件系统条件。缺失位置出现、对象替换或元数据变化时撤回为 changed；重新查询失败为 unavailable。未支持布局为 unsupported。HOME 及其内目录的其他条目变动也可能保守地使结果未知；HOME 以上祖先仅复核对象、类型/权限、uid/gid 和 flags，避免公共临时目录中无关条目变动干扰结果。

读取不是文件系统事务，返回后仍可能变化；这些观察不构成修改身份凭据。没有基于此查询同标识所有安装副本、判断共享数据、检查个人数据内容或适配完整 Mole 数据保护。前端不提供以候选路径为参数的修改入口。

沿用两个实际工作器、5 秒停止等待和真实工作器完成前持有名额的约束；阻塞 I/O 不因超时而被声称已经终止。关闭详情和迟到响应不保留正面结论，一分钟后连同原观察标为较早。重新检查先撤下旧观察。没有自动轮询、数据库写入、额外任务历史、权限修改、服务控制或提权。

## 逐项客户端保护

异步文件观察结束后，从 SQLite 读取最新保护目录，再对所选 App、21 个精确关联路径与本轮组/派生候选附加客户端保护提示。目录规则仍按组件比较父/子/自身重叠，使用已保存路径的意图，不访问、改写或要求其当前存在。所选标识与客户端相同时，相关候选受自身保留规则约束，系统目录位置也保留额外客户端限制。

每项 protectionReasons 可包含 systemLocation/currentApplication/clientDirectory；不回传重复的绝对保护目录。命中独立于 present/notFound/unknown，不把路径保护当作存在或独占归属。摘要提示受保护位置数；默认展示发现/未知与受保护的未发现位置。未命中也全部保留，没有安全、可清理或可执行状态。

保持原设置变更后的重查和迟到响应隔离；再次附加新规则会替换旧原因。设置读取失败、规则异常仍整份拒绝，不能把未知规则当作空规则。当前仅适配客户端保护，不把 Mole cleanup 的 should_protect_data 当作卸载资格；完整 should_protect_path、白名单和归属检查仍待完成；已支持的卸载模式字面保留子集见 [路径保留](application-uninstall-paths.md)。路径别名、大小写与挂载同对象关系不由这份字面规则证明。

## 协议与后续

应用检查协议 v27 的 associations 包含 version=13、固定 revision、observed/unknown、issue、entries、byHost 与 names。观测成功时先按固定顺序返回 21 项，后接最多 64 个动态目录候选、最多 64 个设备设置文件与最多 73 个 Library/XDG 名称及 VS Code 专属位置、最多 64 个厂商嵌套目录；每项 kind、relativePath、present/notFound/unknown、issue 、唯一/有界的 protectionReasons 及 molePathProtection 需互相一致。discovery 记录组/派生范围的 observed/partial/notChecked、原因、尝试根数、条目与候选数和优先问题根；缺失根也计入尝试数，未知主身份/规则时不检查且计数为零。partial 会在折叠摘要显示，动态目录必须位于 4 个声明根直属且符合名称边界，设备文件必须来自 ByHost 直属并符合主 ID 前缀及 .plist 后缀；独立 byHost 记录 observed/partial/notChecked、issue、entries/candidates，未知身份不扫描且计数为零。不接受任意路径或重复位置。前端校验精确修订、身份、完整数量、精确 ID 路径和状态组合，未知版本/矛盾结果整体拒绝。只向界面返回相对路径，不接受前端 HOME、路径或 ID 覆盖。

observed 只表示进行了有界查询，21 项仍可能全部未知。共享或个人数据未确认时全部保留，既有客户端、系统组件、后台条件和官方卸载器限制继续显示。完整发布仍需多副本/共享归属、完整候选与保护规则、个人数据默认保留的计划交互、退出协调、执行前验证、逐项结果及真实权限/云/网络/卷变化矩阵。路线图中的完整关联文件项保持未完成。

关联数据旁新增 [安装副本](application-copies.md) 的系统登记位置观察；范围有限，完整扫描及其他标识共享关系仍待核实，全部保留策略不变。

[共享关联](application-copies.md) 现使用固定 Mole 保留谓词核对同一清单与系统登记候选，命中解释保留原因，未命中仍不证明独占归属。

共享关联已增加 [常用安装根的有界发现](application-discovery.md)，补查未登记/被清单省略的位置；目录问题、变化和上限保持不完整。命中仍持有证据，未命中释放句柄并在返回前重新核对元数据，不提供执行资格。本地卷和收据也已有有界补查，完整语义/权限/故障矩阵仍待验收。

[扩展数据位置](application-extension-data.md) 按已取得标识查询八类精确位置和一层 ByHost 文件，单独记录范围与身份撤回；没有扩大本节主 ID 四根的匹配规则。全部新位置同样在异步结束后附加最新客户端保护。

第六十二阶段 associations version=4 在每项返回独立的 molePathProtection，绑定固定资源的卸载模式字面路径规则。正面保留不取决于位置存在；非命中且位置未知时保持 locationUnverified，不从未知位置推出保护已通过，详见 [路径保留](application-uninstall-paths.md)。

第六十五阶段已为取得的扩展 ID 增加 [客户端应用组名称补查](application-extension-data.md)，只在 Group Containers 直属名称上复用同一 Mole 点分边界；不认证组成员，也不将结果称为原 CLI 已有的扩展组发现。主 ID 四根规则仍独立，全部保留。
