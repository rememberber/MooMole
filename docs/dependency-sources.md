# 依赖分发物与标准库源码归档

2026-10-24：新增独立依赖归档，与 [客户端/Mole 源码包](source-bundle.md)、[许可材料](dependency-materials.md) 配套使用。它按锁定版本收集公开分发物，不递归复制用户缓存。**分发包不必然等于首选修改源码，完整对应源码与许可审阅仍未完成。**

## 生成

```sh
npm run source:dependencies -- --output local-artifacts/dependency-review
# 第一次准备标准库源码缓存后可离线复验
npm run source:dependencies -- --offline --output local-artifacts/dependency-offline
# 可选：将依赖归档绑定到同一干净提交构建的实际 App ZIP
npm run source:dependencies -- --offline --output local-artifacts/dependency-bound \
  --app-report local-artifacts/MATCHING-APP/validation.json
npm run test:dependency-sources
# 可选发布审计工具，需要 Python 3.9+；运行 App 和正常构建不需要 Python
npm run test:rust-source
```

需要已准备的匹配架构引擎、Node/npm/Rust/Go 和当前锁定依赖缓存；输出目录必须不存在。首次允许从固定官方来源取得匹配 Go/Rust 源码，成功校验后放入项目专属缓存；离线缺失则明确停止，不临时使用其他版本。npm pack 始终使用 offline 和 ignore-scripts，不执行包中的生命周期脚本。

| 分发物 | 核对依据 |
| --- | --- |
| Cargo crate | 当前目标 release/build 解析图，对照 Cargo.lock 精确包/版本/公开 registry 及原 `.crate` SHA-256；不复制整个 registry |
| npm 分发 tarball | production lock graph 的精确公开 registry URL、名称/版本与 sha512 integrity；不复制 node_modules 或其他项目包 |
| Go module zip | 两个实际引擎 BuildInfo 的精确模块/版本/h1；复制到私有阶段目录后逐项核对排序文件名和内容摘要，校验与原始 ZIP 压缩编码/时间无关 |
| Go/Rust 标准库源码 | Go 固定版本官方分发元数据、当前 Rust 安装的版本化渠道元数据；下载来源受限且原归档 SHA-256 必须相符，不更新工具链或安装组件 |

Go h1 校验只读取 ZIP，不解包或执行。链接/特殊类型、越界/重复/控制字符路径、异常 CRC、单项或总量超预算拒绝；原查询的 h1 不由本地 ziphash 文件替代。Cargo 和 npm 则核对原压缩字节。校验失败不会留下被标为成功的包。

标准库下载只允许相关固定 HTTPS 主机并限制重定向、时间和大小，原文缓存变化也拒绝；不自动替换损坏缓存。Go 使用 [官方版本索引](https://go.dev/dl/?mode=json&include=all) 精确选择编译版本；Rust 使用本机已安装版本的渠道描述，不选择“最新”版本。

## 输出与使用边界

输出 `MooMole-{version}-mac-{arch}-dependencies.tar.gz` 和 validation.json。归档内有 README、DEPENDENCIES.json 与原始分发包；清单列出包标识、公开出处、原校验值、文件大小/SHA-512 和 Go 内容核对结果，不保存本机缓存路径。

生成时与许可清单核对集合：应用和固定 Mole 自身另由 Git 源码归档提供，其余依赖不能遗漏、重复或错配算法/来源。package/锁文件、Cargo 配置、Tauri 配置、引擎和许可清单在收集及封装前后核对摘要，变化则撤回本次输出。外层归档会重新解包，对照完整文件集合与逐份摘要，再记录最终包的大小/SHA-512。已有目录不覆盖。

此包记录当前工作区来源与 dirty 状态。默认 `binaryBinding=null`；指定 `--app-report` 时要求当前来源和 App 均为同一干净提交、release 构建、相同产品/版本/架构/引擎版本及提交，随包许可文本摘要与依赖数量也须一致。报告限 1 MiB，报告和实际 ZIP 必须为单链接普通文件，ZIP 固定文件名、大小不超过 128 MiB；核对实际字节/SHA-512，封装结束再次读取并确认绑定不变，源提交/工作区状态变化也撤回输出。已有目录保持，不因失败覆盖旧材料。

DEPENDENCIES.json 和外部 validation.json 均记录报告 SHA-512、实际 ZIP 名称/大小/SHA-512、源码提交、架构及许可摘要，不保存本机路径；这证明所声明资产的对应关系，不证明报告发布者身份、首选修改源码完整或许可证兼容性。正式发行仍须核对客户端/Mole 源码资产、依赖归档及完整发行审阅。不会自动上传、创建 Release 或修改更新清单，也不需要付费 Apple 证书。

## 当前证据与剩余工作

8 组测试通过：原 5 组覆盖 Cargo 锁格式/来源/重复校验、逐字节复制与外层解包、篡改/遗漏/错标/错误来源和路径拒绝、旧目录保留与 Go 证明绑定，以及真实 Go 校验器的两个独立测试。新增 3 组验证实际字节绑定的双清单/无本机路径、20 类 App 来源/产品/版本/架构/引擎/许可/数量/归档或文件类型错误拒绝，以及收集过程中 App 字节被改写时撤回输出。测试内 App 字节是独立夹具，不声称它是可执行 App。Go 用固定预期向量验证排序/UTF-8 和错 hash，拒绝重复及越界路径。Linux 跳过需 Go 工具链的原生组，macOS CI 在已有工具链后执行全部组；没有把这些夹具当作公开依赖来源。

本机实际复验产物 `local-artifacts/MooMole-dependencies-137-final/`：326 项（Cargo 294、npm 8、Go 22、标准库源码 2），归档 78,378,054 字节，外层及全部内层核对通过。首次获取官方源码后，最终调用使用 --offline。来源为 `7a9b774` 加本阶段工作区、dirty=true；资料被 Git 忽略，未公开发布，没有运行 App 或处理个人文件。

后续仍须补齐/审阅 npm 及生成代码的首选修改源码、标准库的外部依赖闭包、文件级许可/例外和完整对应关系。清单的 preferredSourceReview、standardLibraryDependencyClosure、licenseReview 与 releaseAcceptance 均保持 pending。该归档不能代替三模块实机执行、干净机器安装升级或正式发布验收。

## Rust 标准库内置依赖核对

新增独立只读审计，不改变归档内容，也不安装 rust-src 组件。输入必须使用已由官方版本描述核实的归档与 SHA-256；传入一个自行计算的摘要本身不能证明官方来源。先核对完整压缩字节，再以流方式读取 tar/xz；不把成员解包到磁盘，不运行源码、构建脚本或 Cargo。

```sh
# 输出父目录需已存在，报告文件必须不存在
npm run source:rust-audit -- \
  --archive .cache/source-distribution/3f1f9b7ed48f4596fc87889b7b3c61747336a55c9c22db1ab0c697e0aadb77aa.archive \
  --version 1.99.0 \
  --sha256 3f1f9b7ed48f4596fc87889b7b3c61747336a55c9c22db1ab0c697e0aadb77aa \
  --output local-artifacts/rust-source-audit-138/validation.json
```

以归档内 library/Cargo.lock 的全部 registry 包为核对范围（含其他目标及测试的超集），检查 vendor 包集合完全相同、每包 `.cargo-checksum.json` 的 package 值与锁文件一致、文件集合无多无少，并核对每个文件的 SHA-256。拒绝链接/特殊类型、重复/越界路径、不支持的锁来源、重复 JSON 键和读取中替换；限制压缩大小、成员数、单文件与总展开大小。校验清单不是独立信任根，包内源码与清单由此前的完整官方归档摘要一起约束。

本机 Rust 1.99.0 官方归档实测通过：30 个 registry 包、1,215 个文件、16,022,018 字节，报告位于上例忽略目录。7 组测试覆盖正常核对、归档/版本错配、文件变更/遗漏/多出、包校验值错配、重复成员/路径/链接/特殊权限、歧义元数据/来源、预算/输入类型、旧报告保留及审计失败不写报告。测试已加入仓库 CI；没有据本机结果声称远端 CI 已运行。

该结果是 Rust 库 workspace 锁定 registry 包的源文件覆盖证据，不是当前 macOS 标准库的精确目标构建图：路径依赖、生成/本地代码、原生依赖与文件级许可仍须核对，Go 标准库的闭包也未据此完成。报告的 targetBuildClosure、licenseReview 和依赖总清单原有待审状态保持 pending。该独立报告尚未自动绑定最终 App 或并入依赖资产。

同期按固定 npm 版本查询 registry 元数据，并逐项核对 dist.integrity 与 package-lock：plugin-dialog、csstype、react、react-dom、scheduler 有完整 gitHead；api、@types/react、lucide-react 未提供。React/React DOM 与 scheduler 的发布提交不同，不能仅凭同一仓库合并成一个提交。未把版本标签、Cargo 同仓库提交或这次查询当作已完成的首选修改源码；完整仓库快照与分发内容对应仍待补齐。
