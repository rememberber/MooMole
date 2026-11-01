# 应用使用线索到卸载

2026-11-01：在保留的 Mole 应用清单中按需读取系统上次打开线索，帮助寻找值得复查的长期未用副本；不自动检查、选择或卸载。沿用原始 task/index，按确切安装副本绑定，不能由前端传任意应用路径或 Bundle ID。没有有效身份的清单行保持未知，不影响其他可检查副本。

来源为公开 [MDItemCreateWithURL / MDItem](https://developer.apple.com/documentation/coreservices/file_metadata/mditem)、[MDItemCopyAttribute](https://developer.apple.com/documentation/coreservices/1427080-mditemcopyattribute) 和 [kMDItemLastUsedDate](https://developer.apple.com/documentation/coreservices/kmditemlastuseddate)。只查询该路径及上次使用日期，不运行 mdls、不读取私有使用数据库或扫描用户文档。Apple 将该日期描述为由 Launch Services 打开行为更新；它不是完整活动日志，也不是连续闲置天数的证明。实际自建 App 未启动也可能立即得到系统时间戳，因此不据新建或记录缺失声称从未使用。

每行先核对应用主程序、Info.plist、Bundle ID、本地物理身份及祖先，查询后再次核对；路径与元数据返回路径不一致、身份变化、日期类型/值不可信保持未知。用户保护与系统保护不读取；不能核对运行状态不进入较早记录筛选。当前运行副本单列，其他同标识副本不合并。日期转换从 CFDate 的 2001 纪元到 Unix 毫秒，拒绝非有限、负值及未来时间。没有日期、未索引或不可读取不推算为长期未用。

一次最多 512 个确切索引、20 秒循环预算；25 秒接口等待上限、可停止，已返回条目可保留，未检查范围保持未知。系统调用不能被强制终止；超时/观察离开后停止后续读取，原线程继续持有单个读取槽、活动请求和安装互斥直至返回，不积累后台查询。返回前再核对保留清单行与保护配置；结果不是历史存档或操作授权。

后端节点：日期/未知/运行状态、确切副本、保护/身份变化、取消前不启动读取及成功清单索引/过期来源已有自动验证。显式原生查询读取本机已安装 App 的系统日期（确实观察到有值和未知两类），自建临时 App 正文保持；没有启动、退出或删除用户 App。另在原生产卸载验收中，从真实 Mole 清单的自建副本读取同一使用线索，再走既有完整计划、实时检查、独立确认、FULL 意图及真实系统废纸篓，核对确切收据后只取回该自建 App，包外样例保留。该样例的系统日期不被伪造成长期未用；较早筛选的日期分支由独立数据测试和页面用例验证，不能把此测试称为真实长期未用用户 App 的端到端窗口验收。

所有样例在系统 TempDir 内；原生卸载样例成功后也只保留精简证据并自动清理，失败且尚未核对取回时保留明确的救援记录，不按前缀清理未知文件。原生精简证据在 `.cache/verification/native-evidence.json` 的 `app-usage-native.json` 与 `app-uninstall-native.json` 条目。实际 App 窗口、更多系统/卷/使用方式、真实旧记录兼容性与完整取消/中断矩阵仍在完整目标内。
