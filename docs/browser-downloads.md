# 多浏览器、多配置下载整理

2026-11-01 Firefox 后端节点：新增 `~/Library/Application Support/Firefox/Profiles/<配置>/places.sqlite` 标准来源；只检测普通本地配置和数据库元数据，不读取账户名、`prefs.js`、`profiles.ini`、Cookies、登录或密钥文件。配置名使用磁盘目录名，禁止越界、隐藏、控制字符及超过 128 UTF-8 字节的名称；其他浏览器仍使用原 Default/Profile 数字规则。旧计划缺少 browserProfile 时仍仅解释为 Chrome/Default。

Firefox 适配依据 Mozilla 的 [DownloadHistory 实现](https://raw.githubusercontent.com/mozilla-firefox/firefox/main/toolkit/components/downloads/DownloadHistory.sys.mjs) 和 [下载访问类型定义](https://raw.githubusercontent.com/mozilla-firefox/firefox/main/toolkit/components/places/nsINavHistoryService.idl)：仅查询普通 `moz_places`、`moz_historyvisits`、`moz_annos`、`moz_anno_attributes` 表，以下载访问类型 7、唯一的 `downloads/destinationFileURI` 和 `downloads/metaData` 注释核对来源。只有 state=1 且未标记 deleted 的本地 file URI 可进入文件核对；失败、取消、暂停和已删除跳过，缺失、歧义、损坏及超限记录保持未完整。不返回普通浏览 URL/标题，不依靠 `downloads.json` 会话记录推断已完成状态。

沿用私有数据库/WAL 副本、有界查询、保护、确切文件 ID、当前身份和独立确认。原记录不作为可丢弃、内容仍等于下载时或可再次下载的证明。自建 Places WAL 样例经正式 Mole 规则和系统废纸篓实际处理一项，原数据库/WAL 和未列入文件保持不变；仅按真实收据取回自建样例后由 TempDir 自动清理，SQLite 关闭/重开与归档读回一致。精简证据位于 `.cache/verification/native-evidence.json` 的 `firefox-downloads-native.json` 条目。四项新增后端测试通过；这不代替真实 Firefox 数据兼容性或打包 App 窗口验收。

Firefox 界面节点：浏览器列表新增 Firefox，必须明确选择配置，展示确切 `places.sqlite` 路径；切换后撤销旧选择，重新扫描才可形成新确认。默认零文件选择，复用预览、筛选、独立确认、真实结果与历史。新增 WebKit 流程覆盖未选禁用、切换撤回、确切来源传参和仅一次确认执行；日文模式保持原配置和路径，其他浏览器及个人整理流程继续通过。960×780 确认窗目视检查通过，真实后端报告也由生产前端解析器读入通过。完整 Rust 回归 671 项通过、102 项显式忽略；本次 Firefox 与 Mail 的真实原生测试已另行显式执行通过，前端 240 项、相关页面 24 项及构建/翻译/格式检查通过。未为本节点重复打包 App 或 DMG；真实 Firefox 数据、实际 App 窗口、Safari、其他渠道/位置及完整故障矩阵仍未完成。

2026-10-31：从原先固定 Chrome 默认配置扩展到 Chrome、Chromium、Microsoft Edge 与 Brave 标准位置的下载记录。沿用原有软件内容来源标识 `chromeDownloads` 以保留历史兼容；新增可选的 `browserProfile` 明确绑定浏览器和配置目录，旧记录缺失时仅表示原 Chrome/Default，不自动采用其他位置。

来源依据 Chromium 的 [用户数据目录说明](https://raw.githubusercontent.com/chromium/chromium/main/docs/user_data_dir.md)、[普通/访客/系统配置常量](https://raw.githubusercontent.com/chromium/chromium/main/chrome/common/chrome_constants.h) 及 [下载数据库实现](https://raw.githubusercontent.com/chromium/chromium/main/components/history/core/browser/download_database.cc)。Edge 的标准 macOS 数据根参考 [微软文档](https://learn.microsoft.com/en-us/deployedge/initial-preferences-support-on-microsoft-edge-browser)，Brave 标准根参考 [官方数据目录说明](https://support.brave.app/hc/en-us/articles/4404876135565-How-do-I-uninstall-Brave)及[产品目录配置实现](https://raw.githubusercontent.com/brave/brave-core/master/build/mac/tweak_info_plist.py)。这些资料与各浏览器共享 Chromium 的事实只是来源适配依据；实际读取仍要求识别到普通 `downloads` 表和支持的列，不能仅凭浏览器名称接受未知结构。

仅在用户选择来源/浏览器时检测其本账户固定数据根，列出 `Default` 与 `Profile 数字` 的普通本地配置。最多检查 2,048 个目录条目、返回 64 份配置，枚举预算两秒；未知、保护、链接、无可读取 History 和超限保持未完整。配置名称使用磁盘目录名，不读账户姓名、Preferences、Local State、Cookies 或凭据。只做目录及文件身份观察，不在检测阶段读取数据库正文；没有标准根不表示不存在其他下载位置。访客/无痕/系统配置、其他浏览器、测试/预览渠道与自定义根尚未支持。

扫描时只读取所选浏览器的确切配置，不合并其他配置、浏览器或整个账户。数据库与 WAL 使用原有有界私有副本方案，SQLite 查询仅针对副本，新增查询进度中断使取消/十秒预算也覆盖扫描和排序期间。回滚日志存在或状态未知时拒绝；保护 WAL 也拒绝读取。不修改浏览器数据库和下载记录，不读取/返回访问 URL；快照字节仍可能包含浏览记录，正常退出清理私有临时目录，异常退出可能残留。

报告和持久执行计划保存确切浏览器/配置来源及原 History 身份。执行计划读取时校验固定路径，执行边界再次核对配置目录/祖先身份；浏览器正常新增下载记录本身不授予新增处理范围。目标文件仍使用原有确切 ID、当前身份、普通本地单链接文件、保护、独立确认、正式 Mole 规则、FULL 同步意图、逐项真实收据、取消/中断与历史归档。文件内容是否仍与原下载相同、可再次下载及能否丢弃未知，移走文件可能令浏览器的原下载链接失效。

后端节点：自建四种标准根的 Profile 2 真实 WAL 数据库经扫描、独立确认计划、正式 Mole 与原生废纸篓处理，核对实际收据后仅取回自建文件用于清理样例；原 History/WAL 字节、未列入文件和历史归档读回一致。证据 `.cache/multi-browser-downloads-native.json`；这不是浏览器实际导出/窗口验收或应用内恢复。另验证跨浏览器/配置不串源、伪造/越界配置、目录替换、链接/未知/保护和枚举上限；原 Chrome 默认来源的显式原生往返继续通过。页面、实际 App 窗口与发行安装身份验收在后续节点记录；Safari/Firefox、其他渠道/位置、应用内恢复、外接卷和更大真实浏览器数据继续保留在完整目标中。

界面节点：软件清理 → 附件、下载与离线 → 浏览器已完成下载，选择浏览器后仅检测其标准根。可重新检测、选择确切配置并查看 History 路径；检测失败/未选配置不能开始扫描，未知范围明确保留。Default 若存在沿用默认选择，其他配置须手动选取；文件仍默认不选。切换配置撤下旧文件选择，报告来源不匹配时不能准备处理；重新扫描后按新确切 ID 选择，确认窗展示浏览器与配置名称。首次使用、执行/取消、任务接回、保护和历史仍复用软件内容主流程，配置检测不授权文件操作。

验证记录：Rust 默认回归 660 项通过（97 项显式忽略），前端 233 项通过；软件清理/软件内容/个人整理 22 项页面回归通过，补空状态引导和语言后软件内容十项再次通过，合计 23 个不同页面用例。页面覆盖多配置确切传参、切换撤销、未选/未知/失败禁用、只重读不执行、独立确认、结果、英日文与原路径保留。960×780 选择界面及确认窗截图已目视检查，正常无横向溢出；这是 WebKit IPC 模拟，不能替代实际 App/浏览器窗口。构建、类型、翻译、格式检查通过；本地 ad-hoc App 按最终提交另行构建核验，不安装、推送或公开发布。
