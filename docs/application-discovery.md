# 安装目录与本地卷的有界只读发现

共享关联在原 Mole 清单和系统登记之外，增加常用安装目录扫描，以发现未登记或被清单去重的安装位置。命中仍只提示保留数据，不能证明全机扫描完整、数据独占或可卸载。

## 固定范围

依据固定 Mole lib/uninstall/batch.sh 的 _MOLE_UNINSTALL_LIVE_APP_ROOTS，探查 9 个根：

- /Applications
- 后端 HOME/Applications
- /System/Applications
- /Library/Input Methods
- 后端 HOME/Library/Input Methods
- 后端 HOME/Library/Application Support/Setapp/Applications
- 后端 HOME/Library/Containers/io.playcover.PlayCover/Applications
- /opt/homebrew/Caskroom
- /usr/local/Caskroom

沿用原安装根三层深度、忽略隐藏的非 .app 目录、大小写不敏感 .app 后缀与嵌套辅助应用排除。找到 .app 后不继续深入其中；候选实际身份在原 Bundle/Evidence 检查器核对。应用链接作为待核对候选交给原拒绝链接的检查器，不跟随链接遍历。平面 PlayCover、Wrapper、异常 .APP 布局仍可能不能读取。

固定资源继续受完整清单及 batch.sh 固定摘要保护。显式对照在临时目录调用原 find 表达式和提取的 _uninstall_live_candidate_is_nested_app，核对独立 App、三层深度、过深目录、隐藏项、嵌套辅助应用、大小写及应用链接；不进入卸载生命周期。

## 本地卷范围

另用独立预算枚举固定 /Volumes（含隐藏入口），读取每个本地目录的直属 App，以及恰名 Applications 目录下最多 3 层的独立 App。其他任意目录不递归；例如外接盘项目目录下的构建物不会因此被全盘搜索。已在清单/系统登记中的精确本地路径仍可以补充该范围。

使用拥有独立缓冲区的 getfsstat(MNT_NOWAIT) 获取结构化挂载快照，最多接受 255 项系统挂载；表不可取得、不可表示或达到容量时保持不完整。打开子路径前按快照拒绝非本地挂载及 autofs，不探测网络服务、不运行 mount/Perl/Shell。返回前在目录复核前后再次核对相关挂载的路径、文件系统 ID、flags、类型及来源；挂载消失、同名重挂载或新挂载等差异保持 changed。

保留 Mole 的确证内部快照例外：仅当精确名称 com.apple.TimeMachine.localsnapshots、父目录与子目录均为 root 所有、同设备且不是链接时跳过内容；保存该目录证据并重新核对身份、所有者和元数据。未知情况不能按名字排除。同名不同设备卷仍按正常范围处理。

这是保守的支持子集：网络卷、自动挂载、卷入口链接、不支持/不可读布局保持 partial；尚未实现 Mole 对已不可达网络共享和未挂载不可读普通目录的精确排除。不会把它们忽略后宣称完整。原函数以 App 自身为扫描起点时，其相对嵌套谓词可能列出 Contents 下的辅助 App；客户端继续拒绝所有祖先包含 .app 的候选，不进入直属 App 内部。源码对照测试显式记录这项差异，在独立 App 范围比较结果。

安装收据已补充有界非标准位置发现，见 [收据补查](application-receipts.md)；完整卸载资格仍未实现。

## 数量、时间与路径

常用目录与卷各自最多枚举 2,048 个条目、保存 128 份目录/缺失位置证据、取得 128 个根扫描候选。枚举按检查点使用 1 秒预算，返回前复核另有 1 秒预算；系统 I/O 可能阻塞，期限不是内核调用硬中断。上限边界无法证明目录已经列尽时保守标为不完整。

仅接受有界绝对路径，逐段 openat/O_NOFOLLOW/O_DIRECTORY/O_NONBLOCK/O_CLOEXEC，不跟随根或祖先目录链接。每段核对目录类型、设备/inode、mode/flags、uid/gid，拒绝已知 dataless、非本地文件系统与不支持布局；允许正常的本地系统目录跨设备映射。目录枚举使用目录描述符和 NUL 终止文件名，不解析终端文案，不调用生产 Shell。非 UTF-8、控制字符、失效名称和访问失败保持不完整。

扫描递归只保留当前浅层目录句柄，完成后关闭；保存路径与元数据证据，返回前重新用无链接路径遍历核对。已扫描目录比较设备/inode、mode/flags、uid/gid、大小和 mtime/ctime；缺失位置保存最近父目录对象和缺失名称，再核对父目录映射及该名称仍缺失。目录替换、出现、元数据或访问状态变化标为 changed。只显示一个优先原因/相关位置，不声称列出了全部问题。

卷入口最多尝试 64 个；卷发现独立使用 128 份目录证据、128 个候选及枚举/复核各 1 秒检查点预算。两个目录来源最多 256 个候选，与原 Mole 清单前 64 项、系统登记前 32 项合并，最多 352 个输入候选，再按路径/目录身份去重和重新读取包内 ID。最终最多保留显示 32 个共享命中，更多命中保持 partial。没有测量大小、读取应用个人数据或创建删除候选。

## 文件证据与资源占用

命中应用继续持有原目录、Contents、Info.plist 证据直到返回前验证。未命中候选改为保存三个元数据快照并释放句柄，返回前重新无链接打开并复核原快照；不重新解析文件内容。未命中项随后变更同样使结果不完整，不让较大候选集合长期占用每项三个句柄。

目录枚举变化不直接撤回仍有独立有效 Bundle/Evidence 的命中，但扫描范围保持不完整；实际候选包/身份变化则撤下该命中。当前所选包变化仍整份拒绝。读取和复核不是文件系统事务，不能为未来修改提供身份凭据。

所有工作沿用两个实际应用检查工作器及 5 秒停止等待，实际工作结束前持有名额。只在本地身份与固定资源有效时扫描，不在折叠行轮询，不缓存到磁盘或写额外历史，没有权限变更、提权或应用启停。

## 界面与协议

原「共享关联」摘要继续区分命中、未发现与检查不完整。展开显示探查根数量、原因及相关目录，保留共享数据的下一步；未命中也要求保留关联数据。

应用检查协议 v16，installations version=5/source=moleListRegistryRootsVolumesAndReceipts，包含 roots 与 volumes：observed/partial/notChecked、issue、rootCount、entries、candidates、firstUncertainPath。observed 只表示固定有界目录集合本次未发现已知中断，其中 roots 的根数量应为 9；不代表完整安装发现。volumes 的 rootCount 是本次尝试的本地卷入口数（0..64），不是机器全部磁盘数；网络入口可在尝试打开前拒绝。未知身份/资源时两项均为 notChecked 且计数为 0。目录 partial 时总体必须 partial，既有其他来源/候选错误可继续作为总体原因；rootsIncomplete/volumesIncomplete 仅在没有其他总体错误时使用；每项的专属原因与路径独立保留。

前端校验精确协议/来源/修订、计数上限、路径形态与状态一致性，矛盾结果整体拒绝。共享命中继续受既有 Mole 保留谓词、客户端/系统保护、原运行观察与过期/迟到响应机制约束。

后续仍需完整卷语义、收据完整性矩阵、真实网络/云/权限/重挂载矩阵及完整关联/执行前保护，见 [发布路线图](release-roadmap.md) 和 [共享关联](application-copies.md)。


收据来源另有最多 128 个候选，所有来源合计最多 480 个输入；此处 352 是不含收据的目录/清单/登记集合。协议 v16 的 receipts 字段与变化检测见 [收据补查](application-receipts.md)。
