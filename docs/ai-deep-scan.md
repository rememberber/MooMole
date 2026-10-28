# AI 深度扫描

2026-10-28：新增侧栏“AI 深度扫描”。MooMole 收集证据，用户本机已有的 Agent 调用模型分析，用户再选择建议进入原有清理审阅。支持范围不等同于全盘任意文件删除。

## 用户流程

1. 检测本机 CLI。当前检测 Codex CLI、Claude Code、Gemini CLI、OpenCode；前两者有调用适配，后两者仅标注发现/未适配。搜索 GUI 进程 PATH、常用用户安装目录、最多 8 个 NVM 版本与已知 App 内 Codex CLI，不执行登录 shell，不扫描凭据。找到可执行文件不表示登录成功或有模型额度。
2. 选择 Agent 和普通物理目录。开始前确认本次路径、名称、类型、大小和修改时间可能交给 Agent 的模型服务，并使用用户自己的额度。改变 Agent 或目录撤回勾选。本机 Agent 不等于离线大模型。不发送文件正文，不自动安装或改写全局 Skill。
3. App 验证 CLI 所需参数，运行内置 Mole 结构化清理预览，保存绑定文件身份与有效期的扫描；同时在所选目录扩展观察文件元数据。Mole 失败时明确说明并继续扩展线索，不能提交清理审阅。引擎整体不可用时当前界面不开放扫描。
4. 将内置 [Skill](../skills/moomole-deep-scan/SKILL.md) 与本次有界清单通过 stdin 交给 Agent。模型返回用途、影响、建议保留/审阅/核实与不确定性。所有建议默认不选。
5. 只有本次 Mole 清单中被建议审阅、未保护且未过期的项目可提交原有 `create_review_plan`。AI 无法增加路径或扩大执行范围。随后由原有规则复查、受限准备、用户单独确认、逐项验证和真实移入废纸篓流程处理；现阶段仅支持该执行链已支持的用户缓存/普通日志，未知、残留和个人大文件继续保留。

扩展线索可以超出 Mole 的分类，模型可以解释归档、工程产物、个人文件等用途与保留风险，但不凭名称/年龄/大小证明可删，不能仅凭元数据确认内容重复、备份存在、独占归属、损坏或当前未使用。AI 分析不授予修改权限，不把“移入废纸篓”表述成已释放空间。

## 读取边界

- Mole 最多取按大小排序的前 200 个未保护候选，排除与当前保护规则重叠的原始/解析路径；模型不接收终端原文。
- 扩展扫描最多 3 层、2000 个访问条目、400 条总证据、5 秒预算。目录大小未知，不递归测量目录大小。重叠路径去重，不相加估算可释放空间。
- 使用已有 `AnchoredPath` 与 descriptor-relative/no-follow 目录读取；跳过隐藏位置、Library/Applications/System/Keychains、密钥扩展名、符号链接/特殊文件、其他挂载设备、云占位与不可读项目。根目录本身受保护则拒绝，子级保护在遍历时剪枝。仍可能有个人信息出现在普通文件名与路径中，需按界面说明确认。
- 保护设置在扫描完成、发送模型前再次核对；发生变化则不发送。发送后的信息无法撤回。模型报告逐项绑定本次证据 ID，拒绝陌生 ID、重复 ID、附加命令/路径字段、超限与控制字符；扩展元数据不能返回可执行审阅建议。
- prompt 不超过 256 KiB，模型输出不超过 1 MiB，最终报告不超过 256 KiB、100 个建议、20 条模型限制；无效回复整体拒绝，不从自然语言提取删除路径。

## Agent 调用

Codex 使用 `exec --ignore-user-config --ignore-rules --ephemeral --skip-git-repo-check --sandbox read-only --output-schema ...`，禁用 shell、统一执行、hooks、plugins、apps、MCP、浏览器、图像、子 Agent 等工具能力；私有临时工作目录中加载内置 Skill 与 schema。认证沿用 Agent；隔离用户配置意味着使用 Codex 默认模型，非自定义 provider/profile。Claude 使用 `--bare --print --tools "" --disallowedTools "mcp__*" --strict-mcp-config --mcp-config '{"mcpServers":{}}' --setting-sources "" --no-session-persistence --output-format json --json-schema ...`，读取其 `structured_output`。Claude 的 bare 模式不复用订阅 OAuth/钥匙串，必须已有 API 环境凭据（如 `ANTHROPIC_API_KEY` 或对应云 provider 凭据），不能把本机订阅登录当作可调用证明；详情见 [Claude 程序调用说明](https://code.claude.com/docs/en/headless#start-faster-with-bare-mode)。缺少参数则停止，未调用模型；不回退不受限模式或自动授权。

CLI 参数依据 [OpenAI 非交互文档](https://learn.chatgpt.com/docs/non-interactive-mode)、本机 `codex exec --help` / `codex features list` 和 [Claude CLI 文档](https://code.claude.com/docs/en/cli-reference) 核对。CLI 与服务端版本可能变化；报错应在终端处理登录/模型/额度，不自动重发请求或开启权限。

## 生命周期与记录

一轮扫描登记为独立 `aiscan` 任务，注册到现有进程组监督、更新安装准入及退出取消链。AI 扫描期间不准入新的普通扫描/修改；已有其他任务时不能开始 AI 扫描。Mole 300 秒、Agent 180 秒、CLI 帮助 10 秒有界；取消终止当前自有进程组，不按名字杀其他 Agent。stdin 写入与 stdout/stderr 都有界，退出后清理私有临时目录。

前端轮询后端快照，页面切换不停止扫描，WebView 重载可接回当前任务，状态读取失败只重试读取。取消不自动删除文件，也无法撤回已发送给模型的数据。App 重启后原任务按现有历史规则标记中断，模型请求不自动恢复或重发。

AI 证据/报告只在当前 App 进程短期保留（完成后 15 分钟读取失效）；原 Mole 清单使用现有 15 分钟有效期及扫描保留规则，结果过期撤下选择。SQLite 记录任务类型、范围、时间及状态，不持久化 AI 报告/完整 prompt；Agent 自身日志与服务端数据政策仍由所选 Agent/服务管理。清理确认后继续使用原有持久化执行记录、逐项收据、取消/未知核查和 Finder 取回边界，未增加自动恢复承诺。

## 验收

- 前端单测验证原始 Mole ID、明确建议、保护、取消及有效期边界；Playwright 验证菜单、模型数据确认、默认不选、扩展线索不可选、精确原 ID 提交、保护变更及过期。
- Rust 验证陌生/重复/越权 ID、额外命令字段、工具限制参数、目录保护、秘密文件/链接跳过和取消。
- 本机 Codex CLI `0.164.0-alpha.11.1` 已通过可选 `live_codex_metadata_roundtrip`：仅发送 `/Users/synthetic/Downloads/archive.zip` 一条合成元数据，真实调用返回有效建议。不会读取这个假路径的内容。Claude 未安装，当前仅完成参数依据和适配单测，尚未真实登录/调用验收；Gemini/OpenCode 未开放调用。
- 独立 ad-hoc 签名调试 App `MooMole AI Check.app` 已原生打开并确认侧栏、Codex 本地检测/其余未发现、内置 Skill、勾选前不能开始、改目录后撤回数据确认。证据位于 `local-artifacts/ai-scan-acceptance-UBGPxk/`，未随源码分发。未向模型发送用户实际目录清单；真实模型调用仅使用上面的合成元数据。
- 本轮未据此宣称所有 AI 建议都能执行、整盘清理、Claude 实测、跨版本/Intel/发布环境或新一轮 AI 到真实清理全链路均已验收。既有缓存/日志的真实执行证据与限制仍见 [速战速决](quick-cleanup.md)、[日志执行](log-cleanup-execution.md) 和 [首发验收](first-release-execution.md)。
