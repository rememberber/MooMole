# 日志处理前检查接入审阅流程

更新于 2026-10-23。实现：`cleanup_preparation.rs`、原 `check_review_rules` 命令、`ReviewPlanDialog`。

当前点击审阅计划中的“复查 Mole 规则”，会在原有检查结束后自动补充单文件日志准备结果；不增加页面或新按钮。摘要直接说明多少日志完成本轮准备、多少项目保留，行内说明原因，筛选“只看需要确认的项目”包含保留和未检查项。**这份只读结果不代表用户确认或清理许可。** 后续独立的短期确认与受限执行已接入，见 [日志执行](log-cleanup-execution.md)。

## 接通的条件

准备来自本次真实 Mole 规则响应，不接受前端提交的 passed、路径列表或权限结论。计划和范围从 SQLite 读取，原 NUL rules v4 仍绑定路径、HOME、来源和固定资源版本；没有新 Shell 规则或产品目录适配。

只有以下条件全部满足才尝试原生只读准备：

- 当前计划/扫描版本、选中顺序/ID/路径、有效期与本次复查完整对应；最新本地复核未变化且没有客户端保护。
- 实际 ledger 来源契约为 userLogs，准确路径也在当前 HOME 的用户日志/诊断报告单层范围内；仅单个普通文件，目录、缓存及其他模块保留。
- Mole 路径检查为 passed，模块 v2 的 binding/scope 为 userLogs 且 checked，占用 v1 为 noOpenHandles。缺失、未知、保留、预算不足或不支持不折算为通过。
- 本机权限观察为当前版本且未发现限制，再通过 [原生对象准备](cleanup-concurrency.md) 核对本账户、单硬链接、本地已落地和完整原计划身份。引用准备结束后释放，不跨 IPC 传递原生句柄。

v2 补充一个明确分开的条件范围：占用恰为 unknown、状态为 activityUnknown，而上述其他条件全部成立时，也核对原生对象，但返回 `authorizationRequired`，增加 `authorizationCount`，绝不增加 `observedCount`。busy、检查失败、预算不足、不支持、保护或权限未知不会获得此条件资格。确认只固定待检查的范围，用户明确提交后才发起受限系统检查；未取得当次空闲正面观察仍不处理。见 [授权与普通用户执行交接](cleanup-authorization-probe.md)。

每轮最多尝试 20 个文件、共享 500ms 检查点预算。前面独立完成的观察与后面未检查项分别说明；慢调用超过预算不发布该项通过结论。准备期间取消撤下全部观察；返回前再读保护设置，保护改变或有效期变化则撤下准备结果并刷新本地状态。设置读取失败返回错误，不使用空保护列表代替。

## 报告和失效

原 `PlanView` 新增可空 `cleanupPreparation`：

| 字段 | 含义 |
| --- | --- |
| version=2、planId | 报告协议及对应计划 |
| scope=userLogFiles、method=trash、readOnly=true | 当前准备范围及拟采用方式；没有执行权 |
| checkedAt、expiresAt | 最长 30 秒，且不超过计划到期及本轮规则证据 30 秒边界；准备不能延长旧规则的有效期 |
| observedCount、authorizationCount、items | 分开计数已观察准备与需要授权检查；逐项 ID、observed/authorizationRequired/retained/notChecked 和原因，保持计划顺序 |

报告不写 SQLite、不成为执行记录或确认票据。重新读取计划只返回本地复核，报告为空；新的规则响应也清除旧报告。未来执行必须重新读取计划、确认选择、保护、规则、权限与原生身份，不能根据这份报告直接调用文件修改。

前端独立核对版本、计划、完整有序 ID/数量、计数、只读/方式/范围、时间边界，以及 observed 项对应的本地状态/权限和 Mole 模块/占用证据。未知协议、缺项、重复、矛盾或过期都撤下逐项正面标签。重新复核、新规则检查、取消和失败同步清空旧准备。时钟回退不作为有效结果；界面用实际渲染时刻比较新响应，避免把上一个定时 tick 误判为时间回退。

authorizationRequired 另核对 activityUnknown/unknown 与完整其他条件，确认协议同为 v2 且必须显式匹配 `requiresAuthorization=true`；缺少或矛盾的标志不能显示处理确认。条件资格和真正无占用分别展示，不把未知项目标成已准备通过。

拟采用移入废纸篓，不等于释放磁盘空间；当前没有应用内恢复。方法和有效期说明放在已有“计划信息与检查范围”内，逐项用途与影响仍在原位置，不另堆确认标题、装饰卡片或重复路径导航。

## 验证

普通 Rust 测试验证真实只读对象准备、计划不变、缺失/混合来源、未知规则/占用/权限、保护、错误绑定、旧规则、20 项预算、取消、对象错误与撤回。前端单元验证协议/选择/计数/时间边界，以及正面准备不得覆盖保护/未知状态。浏览器覆盖摘要、保留筛选、恢复说明、浅深色布局、失效、重查失败、重复检查取消和未知协议。

显式固定 Mole 源码联验只访问自建 HOME：

```bash
node scripts/cargo.mjs test cleanup_preparation::tests::pinned_log_ledger_rules_and_native_preparation_remain_read_only -- --ignored --exact --nocapture
```

联验调用原版 `clean_user_essentials` 和真实 append hook，解析实际 NUL ledger/来源，保存所选日志计划，再运行正式 `rules::run` 与准备函数；不替换上游函数、不执行修改，跳过测试 HOME 的废纸篓分支。实际本机结果为 path=passed、module=checked、activity=unknown，准备正确保留，而不是制造 noOpenHandles。再向专用 HOME 写入真实 Mole 白名单，复查返回 whitelist 并保留；日志正文保持原样。正面准备由普通隔离测试和浏览器 fixture 分别验证，不把它们描述为真实系统占用检查通过。

## 后续入口

受限日志的正式确认及执行已接入独立后端确认，不改变准备报告的只读语义。用户核对固定范围及影响后明确提交，后端再次运行当次验证，接入任务控制、逐项结果及实际收据。发布 App 权限和故障验收仍待完成；目录、系统范围、共享/个人数据与永久删除不随此入口放开。详见 [受限日志执行](log-cleanup-execution.md)。
