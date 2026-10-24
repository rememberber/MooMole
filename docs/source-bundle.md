# 固定提交的源码包

2026-10-24：源码归档可与同一干净提交的 App 构建报告绑定。这是发布材料基础；依赖源码/署名汇总、实际安装升级与三个模块完整执行验收仍待完成。

## 生成与核对

先准备固定 Mole Git 对象，再从客户端指定提交导出到不存在的新目录：

```sh
npm run source:bundle -- --revision HEAD --output local-artifacts/source-review
```

HEAD 会先解析为完整提交；也可传完整 40 位提交 ID。版本来自该提交的 package.json，导出后核对锁文件、Cargo 和 Tauri 镜像。上游提交读取该客户端提交中的固定引擎声明，不由缓存 HEAD 或标签决定。缺少 Git 对象则停止，不自动换版本或下载。

输出材料需要一同保留：

- `MooMole-{version}-source.tar.gz`：完整客户端树，包含锁文件、构建脚本、测试与文档。
- `Mole-{version}-{commit前12位}-source.tar.gz`：完整对应上游树，包含 Go 源码/锁文件、许可证和品牌政策；保留内部符号链接。
- `SOURCE.json`：完整提交/版本、归档字节数及十六进制 SHA-512，逐项路径、Git 模式/blob、大小和 SHA-256；不写入本机绝对路径。
- `BUILD.md`：构建前提、归档与 checkout 的区别、联网/离线、免费签名和待完成事项。

使用 Git archive 导出，不递归复制工作区。未提交/未跟踪文件、缓存、用户数据库和私人验收资料不进入归档；已跟踪的常见数据库、凭据文件和构建目录也拒绝。名称检查不能识别任意源码里的秘密，发布仍须内容审阅。

条目和大小受限，越界路径/链接链、循环/缺失链接及子模块拒绝。导出后在私有目录解包，逐项核对原 Git blob、可执行位和链接；attributes 造成的漏项/替换失败。失败只清理本次新建目录，已有输出直接拒绝。

## 与 App 绑定

```sh
npm run source:bundle -- --revision HEAD --output local-artifacts/source-bound --app-report local-artifacts/本次App目录/validation.json
```

报告须为 release profile、dirty=false，客户端/引擎完整提交和版本均一致；还会在报告同目录复核确切名称的 App ZIP 普通文件、字节数与 SHA-512。变化后的包不产生成功绑定。这里只绑定本地产物与来源记录，不证明发布者身份或位级可重复编译。

不传报告时，`binaryBinding=null`，不能据此声明匹配某个 App。正式发布仍需核对最终验收提交、二进制、标签、源码与说明。

## 构建和验收边界

App 产物脚本记录 Git 提交，解包的客户端目录需先初始化独立 Git 仓库、加入文件并由使用者以自己的身份提交，或者从公开仓库检出原完整提交。本地重建提交会不同于原发行提交，不应冒称相同，也不能借用父目录的其他仓库。

安装 Node.js、Git、Rust、Go 与 Xcode Command Line Tools 后运行 npm ci，再使用 App/DMG 构建入口。当前引擎准备仍取得固定上游 Git 对象，不直接消费随包的 Mole tarball；后者提供完整对应源码供检查/修改。离线仍需确切 Git 对象与依赖缓存；没有完整离线依赖包。详见 [构建说明](local-app-build.md) 和 [引擎准备](engine-integrity.md)。

锁文件不等于依赖源码或署名清单；实际打包的 npm/Rust/Go 依赖材料仍待汇总审阅。本入口不勾选完整许可证/发布关口，不上传文件或创建 Release，不需要付费 Apple 证书。

四组隔离测试覆盖工作区变化/私人文件排除、Unicode/空格、可执行位/内部链接和重复摘要；脏报告/源提交/版本漂移及 ZIP 变化；数据库/凭据、链接链越界/循环、子模块、attributes 漏项/替换、版本镜像和已有目录保护。使用自建仓库和非可执行归档替身。

真实提交 `6dd97170be3636808fdd02e5ea2f6289a3eb964f` 和 Mole `ba3a0f57f78e4c22818fda701219cc3dd4d6953f` 首次完成归档/解包核对：客户端 457 项、压缩后 2,803,212 字节；上游 299 项、8,072,515 字节。材料留在被 Git 忽略的 `local-artifacts/MooMole-source-134-6dd9717/`；此份无二进制绑定。

工具提交后，从干净 `c50b6725dfeb717b6ea0c457b8c2b84fd70a4ec4` 离线构建 arm64 App，ad-hoc、嵌套程序与 ZIP 解压校验通过（`MooMole-0.1.0-mac-arm64-S5Ig3e`）。随后实际带报告导出成功：客户端 460 项、2,812,621 字节；固定上游归档不变。清单绑定该完整提交、报告摘要和实际 ZIP 的 13,236,589 字节及 SHA-512，`dirty=false`。配对材料位于 `local-artifacts/MooMole-source-134-c50b672-bound/`，没有上传。

随后在全新的 `local-artifacts/source-rebuild-135-F3agJ3/` 解包，460 项原始字节/模式核对后建立独立本地 Git 提交。重建树 `3cc4a0a456f15729b1aa73071e91e36005d83a9e` 与原提交完全一致；本地提交为 `962cc3f0fe0178574cb7a0cc9ad2151124655ac6`，构建报告如实记录它，没有冒用原提交身份。

以独立 node_modules/target、复用现有工具链、npm/Cargo/Go 缓存及确切 Mole Git 对象运行 npm ci --offline 和 build:local-app --offline，release 编译 48.19 秒后成功生成 arm64 ad-hoc App，嵌套/解压校验通过，来源 dirty=false。没有复制原 checkout 的源码、node_modules、target 或已生成引擎；上游仍按现有配方从确切 Git 对象导出。清理本次自有 node_modules/target 后，保留的最终 App 再次校验通过，证明没有依赖该构建输出目录完成包内验证。日志、来源/重建报告与最终包均在上述忽略目录。

这补齐本机客户端源码归档重建和干净 App/源包对应证据；不是无缓存、断网监测或无工具链的干净机器验收，也不是位级可重复编译证明。完整依赖材料、最终发行物配对和实际下载/安装仍待完成。没有启动新包改动用户库，没有公开发布。
