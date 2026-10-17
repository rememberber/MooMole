# 客户端更新检查

2026-10-17。设置 → 关于提供手动“检查更新”与发布页入口，先完成清单获取、版本判断和精确架构选包；应用内下载、下载完成校验、缓存和打开 DMG 安装仍待接入。不会用“有新版”代表安装包已下载或可执行。

## 界面与联网

不在启动、进入关于页或定时后台自动检查。用户点击后，由 Rust 访问固定来源：

`https://raw.githubusercontent.com/rememberber/MooMole/main/update-manifest.json`

来源对应本仓库默认 main 分支，非当前开发分支。只有经过发布流程核对的清单才应合入该分支；dev 内增加文件不代表线上已生效。初始清单的 releases 为空，不预填本地验收包或尚未上传的资产。

请求使用系统 TLS、证书验证及系统代理支持，Accept 为 application/json，User-Agent 为 MooMole/当前应用版本。GitHub/所用网络代理可以看到请求、IP 与版本标识；不向请求附加目录、任务历史、诊断、数据库、账号或设备标识，不使用 cookie、GitHub 登录凭据或遥测；代理按系统配置工作。前端不发起此 HTTP 请求，生产 WebView CSP 保持原配置；CSP 不约束 Rust 的固定来源联网能力。

清单请求仅允许 HTTPS，不跟随重定向；连接预算 5 秒、整体预算 15 秒，按 Content-Length 和实际分块同时限制 512 KiB。全应用同时只允许一次检查，完成、失败或 future 丢弃时释放占用。页面关闭不自动重发，仍在进行的检查最多保留至预算结束；再次检查可能提示正在检查，稍后可重试。

- HTTP 404、空 releases 或稳定通道只有预发布版本：显示“尚未发布可检查的版本”。不是“已是最新版本”。
- 存在可比较的发布版本但不高于当前版本：显示“当前版本无需更新”，不建议降级。
- 有新版：显示版本号、默认折叠的纯文本更新内容及发布页；缺少当前架构的包时明确提示，不选择另一架构或更旧版本。
- 断网、超时、异常 HTTP、格式或内容校验失败：显示错误并支持重试，不继续展示上次的新版结论。

“查看发布页”通过无参数 Rust 命令打开固定 `https://github.com/rememberber/MooMole/releases`；前端不能传任意 URL。打开失败保留 App 和检查结果，提示重试。浏览器预览禁用检查，不生成假检查结果；发布页仍可通过浏览器查看。

## 清单协议 v1

采用单产品协议，借鉴 MooTool Electron 的固定产品、SemVer、通道、架构匹配、说明和大小/摘要约定，不解析其多产品注册表，也不共用其产品 ID 或清单。必需字段如下；以下为格式示例，不能直接作为真实发布记录：

```json
{
  "schemaVersion": 1,
  "productId": "io.github.rememberber.moomole",
  "releases": [
    {
      "version": "0.2.0",
      "notes": "本版本功能、修复和已知限制。",
      "releaseUrl": "https://github.com/rememberber/MooMole/releases/tag/v0.2.0",
      "assets": [
        {
          "platform": "darwin",
          "architecture": "arm64",
          "packageType": "dmg",
          "fileName": "MooMole-0.2.0-mac-arm64.dmg",
          "url": "https://github.com/rememberber/MooMole/releases/download/v0.2.0/MooMole-0.2.0-mac-arm64.dmg",
          "size": 1,
          "sha512": "用真实产物的128位小写十六进制SHA-512替换此示例"
        }
      ]
    }
  ]
}
```

所有层级拒绝未知字段。产品 ID 必须完全一致；最多 64 个发布版本，每版本最多两个不同架构资产。严格 SemVer 长度最多 128 字节，同一版本优先级不能重复，build 元数据不产生新版；不接受 v 前缀、前导零或手写 prerelease 布尔标记。稳定版只接收正式版本，预发布客户端可以接收后续预发布及正式版本。当前不提供用户切换通道开关。

平台固定 darwin，仅支持 arm64/x64 薄架构 DMG，不支持 Universal 或其他包型。文件名必须与版本和架构完全对应；发布页及下载 URL 必须与本仓库的 v版本标签完整匹配，拒绝其他仓库、HTTP、查询/片段、凭据、路径穿越和其他来源。Rust 主机 aarch64/x86_64 归一为 arm64/x64；未知系统/架构拒绝检查。

大小为 1 至 1 GiB 的整数，SHA-512 为 128 位小写十六进制，与本地 DMG validation.json 一致；**不是 MooTool Electron 使用的 Base64**。当前只验证清单字段，尚未下载或验证实际资产，不能据此显示“已校验/安装就绪”。未来系统最低版本选择、下载重定向的独立来源策略、磁盘/下载预算和升级授权仍需补齐。

每版说明最多 16 KiB，只允许换行、回车及制表符这三种控制字符。仅汇总当前版本之后的可接收版本，新版在前，最多 32 KiB；不足以保留下一完整节时停止并提示仅展示部分内容。React 用纯文本渲染，不解析 HTML/Markdown，不从说明中生成可执行链接。

## 发布与信任边界

先验证并上传 Release 的真实 App/DMG、源码与许可证材料，确认公开下载、架构、大小、摘要与发布页，再更新 main 清单；当前没有实现自动生成/上传/合并清单的流程。不把本地验收报告转成已发布记录，也不提前发布指向不存在资产的清单。

HTTPS、固定仓库来源及同源摘要不等于独立维护者签名；ad-hoc 也不提供 Apple 发布者身份或公证。自有清单签名、密钥保管与验证仍在路线图内。App 不购买或导入付费证书，不自动替换应用，不关闭 Gatekeeper 或移除安全属性。

实现所用 API 核对 [reqwest 0.13.5](https://docs.rs/reqwest/0.13.5/reqwest/)、[ClientBuilder](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html) 与 [semver 1.0.28 Version](https://docs.rs/semver/1.0.28/semver/struct.Version.html)。依赖与锁文件固定；HTTP/TLS 行为仍需最低支持系统、Intel 和干净网络环境实测。

## 验证范围

8 组 Rust 测试覆盖空种子、通道/SemVer/正式版接替、降级与 build 元数据、精确架构/缺包、产品/字段/版本与数量拒绝、来源/名称/大小/摘要拒绝、纯文本说明及截断、重复检查，以及真实本地 HTTP fixture 的成功/404/重定向/503/声明或分块超限/中断和生产 HTTPS 限制。fixture 仅在测试中允许本地 HTTP，不提供运行时切换来源。

4 项浏览器定向测试覆盖点击才请求、等待防重复、无发布与无需更新区别、错误重试、清除旧结论、缺架构说明、说明作为文本、截断提示、960px 浅深色布局、发布页失败恢复、不传 URL、关闭后旧结果不串入新弹窗，以及预览禁用。原生与全量回归证据见开发记录第三十九阶段；这些验证不代替真实发布资产、下载校验、安装或升级验收。
