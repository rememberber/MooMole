# npm 原始源码输入打包

首批发布物需要把可修改的源码输入与编译分发包一起提供。`source:dependencies` 已收集 npm 发布包；其中包含编译产物，不能据此当作首选修改源码已齐。本工具把已固定来源的仓库快照和发布声明加入同一候选，并绑定实际 App/依赖资产。完整首选形式、生成步骤与构建依赖闭包、文件级许可及发行审阅仍为 pending。

## 输入与运行

仅开发/候选构建需要 Python 3.9+，发布 App 不依赖 Python。先准备干净 App、对应的依赖包与报告，再运行：

```sh
npm run source:npm -- \
  --app-report local-artifacts/BUILD/validation.json \
  --dependency-report local-artifacts/DEPS/validation.json \
  --output local-artifacts/npm-sources

# 已有全部固定来源缓存时
npm run source:npm -- --offline \
  --app-report local-artifacts/BUILD/validation.json \
  --dependency-report local-artifacts/DEPS/validation.json \
  --output local-artifacts/npm-sources-offline
npm run test:npm-source
```

BUILD/DEPS 为实际构建目录，输出目录必须全新且父目录已存在，不覆盖旧材料。来源使用 [npm-source-origins.json](../third-party/npm-source-origins.json) 的固定完整提交、原提供方大小/SHA-256/SHA-512；package-lock 的整体摘要与全部 production 包名称/版本/integrity 必须吻合。还从实际 App 的源提交读取 pins 和 lock，与当前文件逐字节核对，避免把变化后的依赖来源配给旧 App。

只从明确的 HTTPS GitHub codeload 和 npm registry 取得公开输入，不带认证信息、不运行提供方代码或生命周期脚本，不把提供方路径解包到文件系统。`.cache/npm-source-inputs` 按 SHA-256 保存原始下载字节；命中也重核摘要，链接/共享文件或变化拒绝。下载有期限和字节上限，失败不接受部分包；离线模式缺失时明确失败。

## 保存范围

目前覆盖 8 个锁定包、6 份仓库输入、21,199 个仓库文件及 @types/react 的 13 份声明：

- 五份仓库快照保存原始 gzip 字节。
- Lucide 原提供方包另有固定摘要；按既有明确 React/共享/生成工具工作区和根文件、icons/categories/scripts/发布元数据范围生成子集，保留 4,019 个原文件及模式。未选中的无关缓存不携带，不称为完整 Lucide 仓库。
- @types/react 保留匹配 npm integrity 的原发布归档，逐份核对全部 .d.ts 声明；没有已验证的仓库提交，不编造来源。

读取 tar 时拒绝路径越界、重复、链接/特殊文件、特殊权限、LFS 指针与超预算；仓库文件数量/展开字节数匹配 pins。子集及最终外层归档均读回对比原字节/模式；固定顺序、时间及 gzip 头，避免使用私人旧打包结果的时间戳。仅按路径和固定元数据选择文件，不执行源码中的指令。

输出为 `MooMole-版本-npm-source-inputs.tar.gz` 和 `validation.json`。归档内严格保存七份来源归档、NPM_SOURCES.json 和 README。报告和内部清单绑定原 App 报告 SHA-512、实际 App ZIP、源提交/架构/notices，以及对应依赖包大小/SHA-512；封装前后重核。pins 中的 registry/发布证明引用来自前次来源审阅，本工具不重新执行签名审计，也未自动携带阶段 146 的全部私有原始诊断。

## 实际验证与候选接入

2026-10-26 阶段 159：6 组可移植测试通过，覆盖原文件/模式、明确子集与声明、路径/链接/重复/LFS 拒绝、inventory/integrity/lock 漂移、离线缓存变化及跨 App/资产绑定；夹具不是实际许可或签名证明。另从官方 codeload 实际下载 csstype 原归档，209,040 字节及固定摘要匹配，网络入口得到实证。

已使用实际 `a4c553c` 的 arm64/x64 App 和对应依赖报告分别离线封装。两份均 44,454,290 字节，源码/声明数量如上，绑定各自 App；独立读回外层及各内层摘要通过。再与原实际 App/DMG、客户端/Mole 源码和依赖材料汇总，候选 schema 2 的 11 份有效载荷及 12 行 SHA512SUMS 均通过。证据在 `local-artifacts/npm-inputs-159-{arm64,x64}-final/`、`npm-candidate-159-{arm64,x64}-final/` 及 audit.json。它们复用旧实际构建检验新材料配对，不是新提交的远程构建。

候选工作流已接入此步骤，实际新流水线结果另行记录。完整生成代码/首选源码闭包、13 项许可核查、原生安装升级和三条执行剩余验收继续保留，不能只因源码归档成功就公开发布。
