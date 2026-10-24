# 依赖许可与署名材料

2026-10-24：App 开始随包提供当前构建依赖的许可材料，生成和打包均核对内容摘要。此步骤是材料收集与完整性检查；兼容性、完整对应源码及发行审阅继续待完成，不能由 SPDX 名称或生成成功推断可发布。

## 收集范围

| 来源 | 当前 arm64 记录 | 依据与边界 |
| --- | --- | --- |
| 客户端与 Mole | 2 | 应用 GPL、上游许可证/品牌政策和适配说明，固定引擎版本/提交 |
| npm | 8 | production lock graph 与实际安装包的名称、版本、许可声明相符；包括类型依赖，不声称每项都进入运行代码 |
| Rust | 294 | 当前目标、custom-protocol 的 Cargo 解析图，排除纯 dev 边，保守包含 build 依赖；不是最终机器码的精确裁剪分析 |
| Go | 22 | 从包内两个实际引擎的 JSON BuildInfo 合并模块，核对缓存版本与 h1；替换模块、冲突或缺失拒绝 |
| 标准库 | 2 | 匹配引擎 Go 版本的原材料，以及所用 Rust 的 COPYRIGHT-library 和随附许可文本；不复制整套编译器材料 |

各包根目录的 LICENSE/LICENCE/COPYING/NOTICE/COPYRIGHT/AUTHORS/PATENTS、许可目录或明确 license_file 均保留原字节。读取要求物理路径、普通文件、完整 UTF-8 与有限大小，前后身份、大小或修改时间变化拒绝。只有 SPDX 元数据或署名、没有正文/固定补充时拒绝，不把它们当许可正文。Go 没有机器声明的许可证时保留 null，不能根据文件名猜 SPDX。

部分发布包没有正文，另有包仅含 SPDX 元数据。缺失补充见 [来源清单](../third-party/license-supplements.json)，按精确版本、crate VCS 或 npm integrity 绑定，文件逐份 SHA-256 固定；依赖变化或材料损坏须重新核对，构建不临时抓取不明新文本。该目录的 Git 属性保留原始字节和空白，不对上游正文作换行或格式化处理。

目前 18 个补充项使用 11 份文件，其中 14 项仍需核查：包括上游政策、许可声明及上下文和标准文本引用。原文中的占位符保持原样，不替包作者编造年份/版权人；原包声明作者单独保留。典型来源包括 [objc2 固定提交政策](https://github.com/madsmtm/objc2/blob/d7d2fa23ceaa5e6096c923b081040e5d81b3b9df/LICENSE.md)、[Tauri 固定提交许可](https://github.com/tauri-apps/tauri/blob/59b63ab500da9859f12d8888c3298bc29bba0b92/LICENSE-MIT) 和 [Mozilla MPL 2.0](https://www.mozilla.org/en-US/MPL/2.0/)。它们不会被标成已完成完整署名或对应源码审阅。

## 构建与包内查看

```sh
npm run engine:prepare
npm run notices:prepare
# 已有完整工具链/依赖缓存时
npm run notices:prepare -- --offline
npm run test:notices
```

Tauri 的 build/dev 包装入口会先准备材料；直接调用 Cargo 前应先完成上述准备。CI 在引擎准备后增加该步骤。首次需要 Cargo 锁定依赖和对应标准库材料；离线模式只使用已有缓存，不是完整离线依赖包。

生成目录为被 Git 忽略的 `src-tauri/resources/legal/`，只有：

- `THIRD_PARTY_NOTICES.txt`：依赖清单、原声明、来源与去重后的完整材料/已标识引用。
- `THIRD_PARTY_NOTICES.json`：依赖/材料对应、原字节大小/SHA-256、正文偏移、整本摘要与 pending 审阅状态。不记录本机安装目录。

准备完成后才发布新目录；已有目录须证明是完整的本工具输出，未知文件或损坏证据不删除覆盖。输出经过自校验，失败不把部分材料当成成功。打包检查再次核对版本/架构/引擎、每份材料的原始字节、引用覆盖及整本摘要；仅更新整本摘要不能掩盖内部材料变化。

实际 App 的 `Contents/Resources/legal/` 包含这两份文件，可通过 Finder“显示包内容”查看文本。它们随外层 App 签名封装，并在 ZIP 解压及 DMG 挂载检查中再次验证。产物报告增加 dependencyMaterials 计数、摘要和 pending 状态。早期未附这组材料的本地包不作为当前发行候选。

## 验证与尚未完成

6 组隔离测试覆盖范围/去重/原文保留和本机路径省略，缺失或链接材料拒绝，补充绑定/摘要及政策核查标记，npm 漂移、Go 替换/校验/标准库不符，内部材料变化与未知输出保护。原 App/DMG 的 7 组测试及源码归档 4 组回归也通过；夹具不作为真实许可正文。

真实最终 arm64 构建 `local-artifacts/MooMole-0.1.0-mac-arm64-kj8QLX/` 通过 ad-hoc、嵌套及 ZIP 解压核对：328 条记录、244 份去重材料、文本 2,500,674 字节、14 项政策/参考核查标记；App 约 35.26 MiB。构建来源为 `feedfad` 加本阶段工作区，dirty=true，不冒称冻结发行提交；没有启动 App 或改变用户数据库。

完整 npm/Rust/Go 及相关嵌入代码的源码供应、未捕获的文件级声明/例外、许可证组合、品牌和发布材料仍须审阅；本工具不自动选择 OR 分支或给出法律结论。三条真实执行、安装升级、干净机器与最终发布验收也保持原门槛，见 [发布路线图](release-roadmap.md)。

已增加独立的 [依赖归档](dependency-sources.md)，按 Cargo/npm/Go 原校验和取得分发物及匹配标准库源码；这补充材料供应，不将当前 pending 审阅改为完成。
