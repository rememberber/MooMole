# MooMole 开发约束

## 产品宗旨

**对 Mole CLI 进行客户端可视化，以及在此基础上提供更多的人性化功能。**

- 所有后续设计与开发都围绕 Mole CLI 的能力、语义和保护规则展开。
- 人性化扩展优先改善结果解释、逐项审阅、目录选择、保护、权限提示、任务控制、历史与恢复体验。
- 开发缓存测量只是补充能力，不将产品缩减为独立缓存工具。
- 不用解析终端文案来决定删除对象；修改操作应使用结构化计划、明确选择和执行前验证。

## 技术与参考

- 使用 React + Vite + TypeScript + Tauri + SQLite，以及适配 Mole CLI 所需技术。
- 需求与界面参考见 `docs/guidance.md`；产品方向见 `docs/product-direction.md`。
- README 和 App 保留学习研究用途、使用风险、上游致谢及 Mole 官方产品入口。

## 免费分发与更新

- 用户没有付费 Apple 开发者签名能力。Developer ID、Apple 公证、Mac App Store 或付费发布服务都不是当前正式发布的前置条件。
- 默认采用免费分发：架构匹配的 DMG，通过 GitHub Releases 提供下载；Tauri 的 ad-hoc 签名与 Apple 身份认证/公证应明确区分。
- 安装升级参考 `/Volumes/SanDisk-SSD-128G/IdeaProjects/MooTool/next/` 的 Electron 版：检查版本、下载 DMG、校验大小和 SHA-512、用户打开 DMG 并手动替换应用。
- 保持本项目 Tauri 技术栈，用 Rust 实现对应能力；参考 Electron 的协议与交互，不引入 Electron 运行时。
- 免费分发不等于无须验证：发布物仍需验证架构、嵌套可执行文件完整性、下载校验、首次打开和升级后数据/权限行为。
- 不自动关闭 Gatekeeper、不移除系统安全属性、不静默替换运行中的应用；需要用户进行的系统确认应如实说明。
- 特权操作必须在无付费证书前提下验证实现路线，不默认承诺依赖稳定 Developer ID 身份的常驻 helper。
- 安装升级细节、参考证据和验收要求见 `docs/distribution.md`；发布待办见 `docs/release-roadmap.md`。
