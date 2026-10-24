# 依赖分发物与标准库源码归档

2026-10-24：新增独立依赖归档，与 [客户端/Mole 源码包](source-bundle.md)、[许可材料](dependency-materials.md) 配套使用。它按锁定版本收集公开分发物，不递归复制用户缓存。**分发包不必然等于首选修改源码，完整对应源码与许可审阅仍未完成。**

## 生成

```sh
npm run source:dependencies -- --output local-artifacts/dependency-review
# 第一次准备标准库源码缓存后可离线复验
npm run source:dependencies -- --offline --output local-artifacts/dependency-offline
npm run test:dependency-sources
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

此包记录当前工作区来源与 dirty 状态，当前没有自动 App 报告绑定；正式发行还须核对同一冻结提交、架构、引擎、许可摘要和源码资产。不会自动上传、创建 Release 或修改更新清单，也不需要付费 Apple 证书。

## 当前证据与剩余工作

5 组测试通过：Cargo 锁格式/来源/重复校验，逐字节复制与外层解包，篡改/遗漏/错标/错误来源和路径拒绝，旧目录保留与 Go 证明绑定，以及真实 Go 校验器的两个独立测试。Go 用固定预期向量验证排序/UTF-8 和错 hash，拒绝重复及越界路径。Linux 跳过需 Go 工具链的原生组，macOS CI 在已有工具链后执行全部组；没有把这些夹具当作公开依赖来源。

本机实际复验产物 `local-artifacts/MooMole-dependencies-137-final/`：326 项（Cargo 294、npm 8、Go 22、标准库源码 2），归档 78,378,054 字节，外层及全部内层核对通过。首次获取官方源码后，最终调用使用 --offline。来源为 `7a9b774` 加本阶段工作区、dirty=true；资料被 Git 忽略，未公开发布，没有运行 App 或处理个人文件。

后续仍须补齐/审阅 npm 及生成代码的首选修改源码、标准库的外部依赖闭包、文件级许可/例外和完整对应关系。清单的 preferredSourceReview、standardLibraryDependencyClosure、licenseReview 与 releaseAcceptance 均保持 pending。该归档不能代替三模块实机执行、干净机器安装升级或正式发布验收。
