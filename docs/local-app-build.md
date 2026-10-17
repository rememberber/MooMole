# 本地优化 App 与免费签名验收

2026-10-17。提供本地验收用的优化 App、ZIP 和校验报告，先验证不依赖付费证书的签名顺序。本轮不是 DMG、安装升级或公开 Release；当前只有本机 Apple Silicon 验证，Intel 和干净机器仍待实测。

## 运行

安装 README 中的 macOS/Node/Rust/Go 工具链与 npm 依赖后：

```bash
npm run test:app-artifact
npm run build:local-app

# 固定 Mole 源码、Go 模块及 Cargo 依赖均已缓存时：
npm run build:local-app -- --offline
```

默认从固定 Mole 提交准备引擎，再执行锁定 Cargo 依赖的 release profile 构建。工具链仍由现有开发脚本查找（包含标准 PATH 回退），不要求预存个人 .cache；默认需要联网取得源码/依赖。离线模式要求已有缓存，不提供离线安装器。当前脚本记录实际 Node/npm/Rust/Go、SDK 和 macOS 版本，没有实现所有机器间的工具链强制一致或位级可重复构建。

成功后在新的 `local-artifacts/MooMole-版本-mac-架构-随机后缀/` 中生成：

- `MooMole.app`：完成内外层 ad-hoc 签名并通过验证的副本。
- `MooMole-版本-mac-架构.app.zip`：保留 App 结构与权限的验收归档。
- `validation.json`：App/引擎版本、架构、三处签名信息、实际工具链、源码提交与 dirty 标记，以及 ZIP 字节数和十六进制 SHA-512。

这些目录被 Git 忽略，不覆盖先前产物。构建或验收失败时可能保留中间文件供检查；只有完整校验通过后才写最终报告，不能把目录或 ZIP 的存在当作成功。多份相同 Bundle ID 的 App 共用既有本地数据与实例锁，原生验收前应正常退出旧实例。

## 签名与资源摘要顺序

1. 校验应用版本镜像及正式 CSP；重新准备固定源码引擎。Tauri 用 `--no-sign --bundles app` 生成优化中间包，子进程不接收 Apple 证书/认证相关环境变量。这个中间包不是最终产物。
2. 校验 App 布局、Bundle ID、主程序名称、包版本、三个薄 Mach-O 的实际架构，以及原引擎清单。新复制到独立目录后再验证，不修改源 App 或项目的原始引擎资源。
3. 使用系统 `codesign --sign - --timestamp=none`，先分别签两个内嵌 Go 可执行文件，绑定明确的引擎标识；不使用 `--deep` 批量签未知代码，不申请自定义 entitlements。
4. 在副本中重新生成 `UPSTREAM.json` 的最终字节摘要，保留源码、编译/协议信息，校验签名后的引擎。签名改变 Mach-O 字节，不能保留签名前摘要。
5. 最后签外层 App，封装包括上述清单在内的资源。逐个严格验证三个签名，确认 ad-hoc、对应标识和无自定义 entitlements，再做外层 `--deep --strict` 验证；签完外层不再修改资源。
6. 用 ditto 创建 ZIP，在独立临时目录解压后重新执行同样校验；移除自身验证临时目录，再核对原 App，并流式计算归档大小及 SHA-512 后写报告。

当前配方只支持这三个已知的薄 Mach-O。额外本机代码、链接、共享硬链接或特殊文件会停止；未来增加 framework/helper、Universal 或 entitlements 必须扩展并审阅配方。App 遍历有条目/深度/大小预算，内嵌引擎继续使用既有的更严格预算。签名不是运行前逐文件身份锁定或文件系统事务，仍假定构建工作区受开发者控制。

ad-hoc 提供本地代码/资源完整性封装，不提供 Apple 发布者身份认证，也不证明报告来自可信维护者。校验摘要同样不是独立发布者签名；有能力同时改写并重新签名包与报告的人不在该检查的认证能力内。报告明确 `notarized: false`，不会访问 Apple 公证服务、关闭 Gatekeeper 或移除安全属性。下载后的系统确认、DMG 首次打开及权限迁移继续按 [分发方案](distribution.md) 验证。

参考：[Tauri ad-hoc 签名](https://v2.tauri.app/distribute/sign/macos/#ad-hoc-signing)、[Apple 嵌套代码及资源封装说明](https://developer.apple.com/library/archive/technotes/tn2206/_index.html)。实际参数也已用当前项目固定 Tauri CLI 的帮助与本机 codesign 执行核对。

## 正式与开发 CSP

`tauri.conf.json` 的 `csp` 仅允许本地页面资源与 Tauri IPC；开发服务器 HTTP/WebSocket 来源移至仅开发使用的 `devCsp`。本地优化构建会拒绝生产 CSP 中的远端/开发连接和不在当前配方内的脚本来源。当前保留界面需要的内联样式，没有增加远程内容或本机命令权限。

这只是 CSP 配置与当前读操作的验证，不代表全部命令权限、窗口来源或未来联网更新功能已完成安全审查。开发热重载与未来新增模块仍需各自回归。

## 验证范围

5 组产物测试覆盖生产策略边界、流式大小/摘要、真实隔离 Mach-O 的签名顺序与源副本不变、ZIP 解压校验、原生字节/普通资源/重新伪造引擎清单后的外层封装拒绝，以及额外代码、链接、架构/版本和目标复用拒绝。Linux 运行前两组，原生三组只在 macOS 执行；均不运行 fixture 代码、不触及用户数据。CI 已加入这些测试，远端运行仍待验证。

本机优化 App 启动、Mole 真实读取及具体证据见开发记录第三十七阶段。最终源码包/许可证汇总、最小受支持系统/Intel、DMG、浏览器下载/Gatekeeper、升级后数据/隐私授权和最终发布仍在 [路线图](release-roadmap.md) 中，报告将安装与 Gatekeeper 验收标记为 pending。
