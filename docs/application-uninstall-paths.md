# 卸载模式的路径保留子集

应用详情的关联位置及扩展数据，增加固定 Mole 路径规则的只读保留提示。正面命中显示原因，并参与主「Mole 要求保留」结论；全部位置继续保留，不产生可卸载或安全清理状态。

## 与清理保护的区别

固定 Mole v1.59.1 / ba3a0f57f78e4c22818fda701219cc3dd4d6953f 的 batch.sh 在相关文件预览/执行过滤时，以 MOLE_UNINSTALL_MODE=1 调用 should_protect_path。卸载模式并不简单照搬 should_protect_data 或清理白名单：部分开发工具、VPN 等在清理时保留，但不能据此解释成卸载模式已命中同一规则。

当前适配这个上下文中的字面路径及终端安全缓存祖先身份子集。没有读取或改写 ~/.config/mole/whitelist，没有把 clean/optimize 的白名单错误地作为卸载函数的既有条件；完整白名单是否作为客户端额外保护、执行前规则和用户交互，继续单独验收。未调用卸载、safe_clean、safe_remove、服务控制或任何修改入口。

## 固定资源及匹配

沿用 app_protection.sh、app_protection_data.sh、file_ops.sh 和其他已审核资源的固定 SHA-256 与引擎清单校验。路径规则从已验证逻辑提取，不能仅因可变资源清单一致而接受新语义；语法变化或资源不可得保持未知。

支持的固定分支：

- _mole_is_shared_home_state_root：共享 HOME 状态根，按原字符类及一次末尾斜杠移除处理，子目录不因根名称而一概保留。
- is_orbstack_runtime_path：组容器和 .orbstack 运行数据，按原 ASCII 大小写不敏感匹配。
- should_protect_path 的系统关键词、关键系统缓存/容器及硬编码系统路径分支。
- 字面 /private/var/folders/ 与 /var/folders/ 下终端安全组件前缀，按固定 ENDPOINT_SECURITY_BUNDLE_PREFIXES 大小写不敏感匹配。
- 关键偏好、持久数据、Mole 日志、Codex 状态、网络/云、账号与音频/插件/系统缓存等固定 case 分支，保留原先匹配顺序。例如 Codex Crashpad pending 的直属子项分支只跳过该组其余模式，深层子项仍保留；不将例外解释为执行许可。

当前 SYSTEM_CRITICAL_BUNDLES 及 Apple 例外数组仅包含无前导斜杠/星号的标识或标识前缀，不能命中本接口的绝对路径，因此其全路径比较不增加正面结果。卸载模式跳过的清理数据规则、恢复副本规则、文件名数据保护和相关缓存例外不拿来扩大本次保护。

生产不运行 Shell。编译器只接受固定 case 模式的字面字节、受支持的星号/问号、ASCII 字符类/范围、反斜杠转义和带引号的 HOME；引号中的星号仍是字面值，HOME 本身的 glob 字符也不成为通配符。分支动作只接受已审核的保留或空分支，其他命令/变量/插值/字符类或异常结构拒绝，匹配使用明确的 LC_ALL=C 字节语义。

## 状态与保护独立

每个已返回数据位置新增 molePathProtection：

- protected：固定规则要求保留，reason 为 sharedRoot/containerRuntime/systemComponent/criticalData/endpointSecurity；祖先身份正面命中为 endpointSecurityIdentity。存在、未发现或未知都可保留该意图，不认证实际数据归属。
- notMatched：在已观察的存在/缺失状态下，本次已适配规则没有命中。不会显示通过或可删除，也不代替完整保护。
- unknown：规则不可得、路径形式不支持，位置未知且没有正面命中，或所需祖先身份检查未知/超限。原位置的 present/notFound/unknown、原因与客户端保护不改变。

source 固定为 moleUninstallPathSubset，version=2、revision 固定，前端要求字段、原因/状态与位置相互一致；不接受 cleanup 来源、未知版本或规则/存在矛盾。关联字段 associations 为 version=6，extensionData 为 version=3，整份应用协议为 v20；其他观察语义保持原样。

Mole 路径保留、客户端自身/系统/保存目录保护分别展示。受保护或规则未知的未发现位置仍可展开查看；新查询先撤下旧结果，最新客户端设置仍在异步结束后附加，一分钟较早及迟到响应隔离继续有效。路径规则失败不取消已有客户端保留。

## 终端安全缓存的现存祖先身份

上游 is_endpoint_security_cache_path 在两个字面 Darwin 缓存范围之外，调用 _mole_path_is_within_existing_root(path, /private/var/folders)：逐级比较现存祖先与保护根的设备号/inode。当前补入这个回调的保守子集，仅在路径包含固定终端安全前缀、且尚未有其他字面保留结论时读取元数据；不将此规则扩大为所有目录的通用保护解析。

生产不执行 Shell，不读取目标内容。复用 AnchoredPath 保留从 / 开始的相对目录/目标句柄，拒绝符号链接、特殊文件、普通文件多硬链接；额外核对所有已打开节点的本地卷与 dataless 状态。大小写/Unicode 同对象关系由实际文件系统的设备号/inode 决定，不通过字符串转小写推断。缺失后代仍可用现存祖先证明正面保留；无法完整打开原路径且没有正面祖先时，不能发布未命中。根、路径绑定或内容属性在返回前变化时撤回身份结果。

主 ID/组/派生与扩展数据共享 500ms 检查点、64 次打开尝试预算，超限为 identityLimitReached；缺失保护根、链接、不可读或不支持布局为 identityUnverified。预算是检查点限制，单次文件系统调用不是硬实时中止；原 5 秒工作线程超时和占用槽继续约束响应及重复准入。字面已命中或无终端安全前缀不新增身份 I/O，也不会因身份预算耗尽而取消原字面保留。已有位置状态和客户端保护规则独立。

## 未覆盖的条件

链接解析、跨挂载映射和所有别名布局仍未完成；遇到它们保留未知，不能称为完整 should_protect_path。客户端已保存目录的保护仍按字面组件关系，不据此宣称补齐其对象/别名身份。观察不是原子快照，结果产生后的变化窗口仍存在，执行前必须重新验证。

未适配清理模式 should_protect_data、完整用户白名单、compiled-model/live-handle/SQLite 或所有模块条件，也不证明文件所有者、签名、应用组成员、共享与个人数据归属。原函数隔离对照分别验证 1,801 条字面路径（有意不提供回调）和受支持本地布局的现存祖先回调；不声称完整别名、全函数或真实权限/卷矩阵已经完成。
