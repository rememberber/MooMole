# 嵌入扩展的只读标识观察

应用详情在原位置与来源内提供折叠的「嵌入扩展」，先展示发现位置数与检查不完整状态，展开查看标识、相对包路径和原因。观察只读取选中 App 内的身份元数据，不证明发布者、签名、运行状态或数据所有权；全部关联数据仍保留。

## 固定 Mole 语义

适配固定修订 ba3a0f57f78e4c22818fda701219cc3dd4d6953f 的 _mole_uninstall_embedded_bundle_ids，源文件 app_protection.sh 和 base.sh 沿用现有固定摘要校验，生产不调用 Shell、find 或 plutil。

从选中 App 的 Contents 内查找最深 12 层的 */Contents/Info.plist：

- bundle 扩展名按 ASCII 小写核对 appex、xpc；app 仅接受在 Contents/Library/LoginItems/ 下的包（包含其后代）。
- Bundle ID 使用原 reverse-DNS 分段语义，另限制 255 字节；主标识精确相等及 org.sparkle-project. 精确前缀排除，保留大小写差异。
- XML/binary plist 复用已锁定 Foundation 结构化读取，不运行包内代码；厂商名、文件名和终端文案不作为 ID。
- 同一标识的不同包路径各自保留元数据依据，没有按 ID 合并成一个安装/归属证明。按原相对路径排序展示。

主身份与 Mole 清单不同、不可读、格式不支持或固定规则资源不可用时不扫描，明确 notChecked 及原因。本地标识有效但 Mole 未报告时可以读取，同时保留主身份未比对说明。IPC 仍只接受原 taskId/index，前端不能传入 Bundle 路径、ID、HOME 或命令。

## 文件系统与预算

选中 App 仍受原规范路径、本地目录、无链接与返回前身份检查约束。扫描从持有的根句柄相对打开 Contents 和后代，沿用 O_NOFOLLOW/O_DIRECTORY/O_NONBLOCK/O_CLOEXEC、fstatat 及类型/设备/单硬链接检查；每个目录验证本地、非已知 dataless。链接、异常名称、特殊文件、不可读、非本地或不支持布局均保持不完整，不跟随到包外。

目录及读取过的合格身份文件保存元数据，返回前从同一根逐段重新无链接打开，复核设备/inode、大小与 mtime/ctime、mode、uid/gid、flags。目录变化、身份文件替换或读取期间变化会撤下本次全部扩展标识；最终复核失败/超时同样撤下，避免保留不能重新核对的正面结果。扫描中某个不可读取/不支持的组件可以与其他已复核候选并存，始终带 partial。原主应用身份变化仍整份观察拒绝。

每次扫描最多 2048 条目、128 个目录、128 个匹配身份文件位置、32 个扩展位置；单身份文件 1MiB，合格文件累计读取 4MiB，另允许一个字节判断超限。主应用自身及不属于支持类型的 Info.plist 位置也计入 128 份匹配位置，但不作为扩展内容读取。枚举检查点 500ms，返回前复核另有 500ms 检查点；到达数量、深度、字节或时间边界均保持不完整。不会因时间预算声称阻塞内核 I/O 已终止，继续受原两工作器及 5 秒停止等待约束。

深度是从选中 App 的 Contents 起算，目录边界达到 12 层时保守提示预算不完整。普通资源条目计入枚举预算，但不读取内容。不制造/写入真实扩展、安装位置或权限设置。

## 协议与界面

应用协议 v24 的 embedded 包含 version=1、固定 revision、observed/partial/notChecked、issue、entries/directories/plists 计数及 bundles。每个 bundle 包含 relativePath、bundleId 与 appex/xpc/loginItem 类型；不包含绝对数据路径、执行参数或可删除状态。

前端校验版本/修订、身份/状态关系、计数与上限、相对 Contents 路径组件、深度、类型对应后缀、登录项范围、ID 语法与排除、路径唯一。changed 不允许残留扩展标识。未知协议或矛盾结果整份拒绝；重查撤旧结果、迟到响应隔离和一分钟较早状态沿用原流程。

observed 只表示此固定有界范围未发现已知中断，空结果不代表无其他组件。身份观察只提供包内标识元数据；对应数据位置已支持 [扩展数据位置](application-extension-data.md) 的八类精确位置、有界 ByHost 及客户端应用组名称补查；仍未检查签名/entitlements、所有共享组、辅助进程或文件占用。完整关联候选、保护、共享与个人数据归属、计划及执行前验证仍按发布路线图推进。

第六十一阶段在数据位置观察完成后才结束扩展身份复核；扩展标识撤回或变化会一并撤下 extensionData。部分扫描已取得且复核有效的标识可以继续观察数据位置，结果保持不完整，并不认证完整扩展集合。
