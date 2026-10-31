# 多浏览器、多配置下载整理

2026-10-31：从原先固定 Chrome 默认配置扩展到 Chrome、Chromium、Microsoft Edge 与 Brave 标准位置的下载记录。沿用原有软件内容来源标识 `chromeDownloads` 以保留历史兼容；新增可选的 `browserProfile` 明确绑定浏览器和配置目录，旧记录缺失时仅表示原 Chrome/Default，不自动采用其他位置。

来源依据 Chromium 的 [用户数据目录说明](https://raw.githubusercontent.com/chromium/chromium/main/docs/user_data_dir.md)、[普通/访客/系统配置常量](https://raw.githubusercontent.com/chromium/chromium/main/chrome/common/chrome_constants.h) 及 [下载数据库实现](https://raw.githubusercontent.com/chromium/chromium/main/components/history/core/browser/download_database.cc)。Edge 的标准 macOS 数据根参考 [微软文档](https://learn.microsoft.com/en-us/deployedge/initial-preferences-support-on-microsoft-edge-browser)，Brave 标准根参考 [官方组件说明](https://github.com/brave/brave-browser/wiki/Brave-Components)。这些资料与各浏览器共享 Chromium 的事实只是来源适配依据；实际读取仍要求识别到普通 `downloads` 表和支持的列，不能仅凭浏览器名称接受未知结构。

仅在用户选择来源/浏览器时检测其本账户固定数据根，列出 `Default` 与 `Profile 数字` 的普通本地配置。最多检查 2,048 个目录条目、返回 64 份配置，枚举预算两秒；未知、保护、链接、无可读取 History 和超限保持未完整。配置名称使用磁盘目录名，不读账户姓名、Preferences、Local State、Cookies 或凭据。只做目录及文件身份观察，不在检测阶段读取数据库正文；没有标准根不表示不存在其他下载位置。访客/无痕/系统配置、其他浏览器、测试/预览渠道与自定义根尚未支持。

扫描时只读取所选浏览器的确切配置，不合并其他配置、浏览器或整个账户。数据库与 WAL 使用原有有界私有副本方案，SQLite 查询仅针对副本，新增查询进度中断使取消/十秒预算也覆盖扫描和排序期间。回滚日志存在或状态未知时拒绝；保护 WAL 也拒绝读取。不修改浏览器数据库和下载记录，不读取/返回访问 URL；快照字节仍可能包含浏览记录，正常退出清理私有临时目录，异常退出可能残留。

报告和持久执行计划保存确切浏览器/配置来源及原 History 身份。执行计划读取时校验固定路径，执行边界再次核对配置目录/祖先身份；浏览器正常新增下载记录本身不授予新增处理范围。目标文件仍使用原有确切 ID、当前身份、普通本地单链接文件、保护、独立确认、正式 Mole 规则、FULL 同步意图、逐项真实收据、取消/中断与历史归档。文件内容是否仍与原下载相同、可再次下载及能否丢弃未知，移走文件可能令浏览器的原下载链接失效。

后端节点：自建四种标准根的 Profile 2 真实 WAL 数据库经扫描、独立确认计划、正式 Mole 与原生废纸篓处理，核对实际收据后仅取回自建文件用于清理样例；原 History/WAL 字节、未列入文件和历史归档读回一致。证据 `.cache/multi-browser-downloads-native.json`；这不是浏览器实际导出/窗口验收或应用内恢复。另验证跨浏览器/配置不串源、伪造/越界配置、目录替换、链接/未知/保护和枚举上限；原 Chrome 默认来源的显式原生往返继续通过。页面、实际 App 窗口与发行安装身份验收在后续节点记录；Safari/Firefox、其他渠道/位置、应用内恢复、外接卷和更大真实浏览器数据继续保留在完整目标中。
