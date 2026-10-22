import { useEffect, useState } from 'react'
import {
  installerCatalog,
  installerView,
  type InstallerSort,
  installerIncompleteReason,
  zipInspectionText,
  zipDataInspectionText,
  imageInspectionText,
  type ImageObservations,
  type ZipDataObservations,
  connectionInspectionText,
  usageInspectionText,
  type UsageObservations,
  type ConnectionObservations,
  type ZipObservations,
  type InstallersReport,
} from '../lib/installers'
import { bytes, dateTime } from '../lib/format'
export function Installers({
  root,
  report,
  reading,
  error,
  onReveal,
  checking,
  checkKind,
  usageObservations,
  onCheckUsage,
  zipObservations,
  imageObservations,
  onCheckImage,
  zipDataObservations,
  onCheckZipData,
  onCheckZip,
  connectionObservations,
  onCheckConnection,
}: {
  root: string
  report: InstallersReport | null
  reading: boolean
  error: string
  checking: string
  checkKind: string
  usageObservations: UsageObservations
  onCheckUsage: (path: string) => void
  connectionObservations: ConnectionObservations
  onCheckConnection: (path: string) => void
  imageObservations: ImageObservations
  onCheckImage: (path: string) => void
  zipDataObservations: ZipDataObservations
  onCheckZipData: (path: string) => void
  zipObservations: ZipObservations
  onCheckZip: (path: string) => void
  onReveal: (path: string) => void
}) {
  const [query, setQuery] = useState('')
  const [extension, setExtension] = useState('')
  const [sort, setSort] = useState<InstallerSort>('size')
  useEffect(() => {
    setQuery('')
    setExtension('')
  }, [root])
  const visible = installerView(report?.items ?? [], query, extension, sort)
  return (
    <section aria-label="安装包扫描结果">
      {error && (
        <p className="notice" role="alert">
          {error}
          {report && ' 以下保留上次结果，本次未更新。'}
        </p>
      )}
      {reading && (
        <p className="muted" role="status">
          正在所选目录中只读查找安装包文件名，不打开安装包或读取压缩内容…
        </p>
      )}
      {!report ? (
        <div className="empty-state">
          <h3>{root ? '目录已选择' : '选择要查找安装包的目录'}</h3>
          <p className="muted">
            {root
              ? '点击顶部“扫描目录”查看文件线索。'
              : '从顶部选择常用位置或其他目录，再点击扫描；选择位置不会读取目录。'}
          </p>
        </div>
      ) : (
        <>
          <p role="status">
            发现 {report.items.length} 个文件线索 ·{' '}
            {report.completion === 'partial' ? '扫描不完整' : '本次范围查找结束'}
          </p>
          <p className="muted">
            上次扫描 {dateTime(report.checkedAt)}
            。清单按扩展名列出，ZIP 可逐项检查条目；DMG/ISO
            可逐项检查连接；可逐项检查占用，清理未开放，文件大小不代表可释放空间。
          </p>
          {report.completion === 'partial' && (
            <p className="notice">
              本次未覆盖：{installerIncompleteReason(report)}
              。可缩小范围后重试，未显示不代表不存在。
            </p>
          )}
          {report.items.length > 0 && (
            <div className="installer-filters" role="group" aria-label="安装包列表筛选">
              <input
                disabled={reading || !!checking}
                aria-label="筛选安装包"
                placeholder="名称或路径…"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
              <select
                disabled={reading || !!checking}
                aria-label="安装包类型"
                value={extension}
                onChange={(event) => setExtension(event.target.value)}
              >
                <option value="">全部类型</option>
                {installerCatalog.extensions.map((value) => (
                  <option key={value} value={value}>
                    {value.toUpperCase()}
                  </option>
                ))}
              </select>
              <select
                disabled={reading || !!checking}
                aria-label="安装包排序"
                value={sort}
                onChange={(event) => setSort(event.target.value as InstallerSort)}
              >
                <option value="size">文件大小从大到小</option>
                <option value="name">按路径排序</option>
              </select>
              <span className="muted" role="status">
                显示 {visible.length} / {report.items.length} 项
              </span>
            </div>
          )}
          {report.items.length === 0 ? (
            <div className="empty-state">
              <h3>本次未发现匹配文件</h3>
              <p className="muted">只检查所选范围的固定扩展名，不代表没有安装包。</p>
            </div>
          ) : visible.length === 0 ? (
            <div className="empty-state">
              <h3>没有符合筛选的文件</h3>
              <p className="muted">可清除名称或类型筛选；原始扫描结果和未覆盖提示仍保留。</p>
              <button
                className="button secondary compact"
                onClick={() => {
                  setQuery('')
                  setExtension('')
                }}
              >
                清除筛选
              </button>
            </div>
          ) : (
            <div className="installer-list">
              {visible.map((item) => (
                <article className="installer-row" key={item.path} aria-label={item.path}>
                  <div>
                    <div className="installer-heading">
                      <strong>{item.name}</strong>
                      <span className="muted">
                        {item.extension.toUpperCase()} ·{' '}
                        {item.classification === 'archiveUnverified'
                          ? zipObservations[item.path]?.report?.state === 'installerHint'
                            ? '发现安装条目线索'
                            : zipObservations[item.path]?.report?.state === 'noHintInWindow'
                              ? '检查窗口内未见安装线索'
                              : 'ZIP 内容未确认'
                          : '仅文件名线索'}
                      </span>
                    </div>
                    <span className="installer-path">{item.path}</span>
                    <span>
                      {item.protection === 'matched'
                        ? '客户端保护命中，保持现状'
                        : item.protection === 'unknown'
                          ? '客户端保护无法确认，保持现状'
                          : '未命中客户端保护；其他清理条件未确认'}
                      {item.sharing === 'shared' && ' · 共享硬链接'}
                    </span>
                    {item.extension === 'dmg' && (
                      <details className="origin-details">
                        <summary>
                          {imageObservations[item.path]?.report
                            ? imageInspectionText(imageObservations[item.path].report!)
                            : imageObservations[item.path]?.error
                              ? '本次 DMG 校验未确认'
                              : 'DMG 数据尚未校验'}
                        </summary>
                        <p>
                          只读核对映像内部 CRC32，不挂载或安装。最多 128 MB、4
                          秒运行预算；内部校验和不覆盖所有文件字节，通过不代表签名可信、未被使用或可清理。
                        </p>
                        {imageObservations[item.path]?.report && (
                          <p className="muted">
                            上次 DMG 校验 {dateTime(imageObservations[item.path].report!.checkedAt)}
                          </p>
                        )}
                        {imageObservations[item.path]?.error && (
                          <p className="notice" role="alert">
                            {imageObservations[item.path].error} 先前 DMG 校验结果已清除。
                          </p>
                        )}
                        <button
                          className="button secondary compact"
                          disabled={reading || !!checking || item.sharing === 'shared'}
                          onClick={() => onCheckImage(item.path)}
                        >
                          {checking === item.path && checkKind === 'image'
                            ? '正在校验…'
                            : imageObservations[item.path]
                              ? '重新校验 DMG'
                              : '校验 DMG 数据'}
                        </button>
                      </details>
                    )}
                    {zipObservations[item.path]?.report && (
                      <span className="muted">
                        上次检查 {dateTime(zipObservations[item.path].report!.checkedAt)} ·{' '}
                        {zipInspectionText(zipObservations[item.path].report!)}
                      </span>
                    )}
                    {((zipObservations[item.path]?.report &&
                      zipObservations[item.path].report!.state !== 'unknown') ||
                      zipDataObservations[item.path]) && (
                      <details className="origin-details">
                        <summary>
                          {zipDataObservations[item.path]?.report
                            ? zipDataInspectionText(zipDataObservations[item.path].report!)
                            : zipDataObservations[item.path]?.error
                              ? '本次 ZIP 数据校验未确认'
                              : 'ZIP 数据尚未校验'}
                        </summary>
                        <p>
                          测试压缩数据与 CRC，不写出文件。最多 64 MB ZIP、2
                          秒运行预算；通过不代表签名可信、可安装或可清理。
                        </p>
                        {zipDataObservations[item.path]?.report && (
                          <p className="muted">
                            上次数据校验{' '}
                            {dateTime(zipDataObservations[item.path].report!.checkedAt)}
                          </p>
                        )}
                        {zipDataObservations[item.path]?.error && (
                          <p className="notice" role="alert">
                            {zipDataObservations[item.path].error}{' '}
                            旧条目和数据结果已清除，请重新检查 ZIP。
                          </p>
                        )}
                        <button
                          className="button secondary compact"
                          disabled={
                            reading ||
                            !!checking ||
                            item.sharing === 'shared' ||
                            !zipObservations[item.path]?.report
                          }
                          onClick={() => onCheckZipData(item.path)}
                        >
                          {checking === item.path && checkKind === 'zipData'
                            ? '正在校验…'
                            : zipDataObservations[item.path]
                              ? '重新校验数据'
                              : '校验 ZIP 数据'}
                        </button>
                      </details>
                    )}
                    {usageObservations[item.path]?.report && (
                      <span
                        className={
                          usageObservations[item.path].report!.state === 'busy' ? 'notice' : 'muted'
                        }
                      >
                        上次占用检查 {dateTime(usageObservations[item.path].report!.checkedAt)} ·{' '}
                        {usageInspectionText(usageObservations[item.path].report!)}
                      </span>
                    )}
                    {usageObservations[item.path]?.error && (
                      <span className="notice" role="alert">
                        {usageObservations[item.path].error} 本次未确认，先前占用观察已清除。
                      </span>
                    )}
                    {connectionObservations[item.path]?.report && (
                      <span
                        className={
                          connectionObservations[item.path].report!.state === 'pathReported'
                            ? 'notice'
                            : 'muted'
                        }
                      >
                        上次连接检查 {dateTime(connectionObservations[item.path].report!.checkedAt)}{' '}
                        · {connectionInspectionText(connectionObservations[item.path].report!)}
                      </span>
                    )}
                    {connectionObservations[item.path]?.error && (
                      <span className="notice" role="alert">
                        {connectionObservations[item.path].error} 本次未确认，先前连接观察已清除。
                      </span>
                    )}
                    {zipObservations[item.path]?.error && (
                      <span className="notice" role="alert">
                        {zipObservations[item.path].error} 本次未确认，先前线索已清除。
                      </span>
                    )}
                  </div>
                  <div className="installer-actions">
                    <span className="installer-size">
                      <strong>
                        {item.measurement === 'measured' ? bytes(item.sizeBytes) : '大小未知'}
                      </strong>
                      <span className="muted">文件逻辑大小</span>
                    </span>
                    <button
                      className="button secondary compact"
                      disabled={reading || !!checking || item.sharing === 'shared'}
                      title={
                        item.sharing === 'shared'
                          ? '共享硬链接暂不检查'
                          : '只读可见进程记录，不关闭进程或提权'
                      }
                      onClick={() => onCheckUsage(item.path)}
                    >
                      {checking === item.path && checkKind === 'usage'
                        ? '正在检查…'
                        : usageObservations[item.path]
                          ? '重新检查占用'
                          : '检查占用'}
                    </button>
                    {['dmg', 'iso'].includes(item.extension) && (
                      <button
                        className="button secondary compact"
                        disabled={reading || !!checking || item.sharing === 'shared'}
                        title={
                          item.sharing === 'shared'
                            ? '共享硬链接暂不检查'
                            : '只读系统连接列表，不挂载或推出映像'
                        }
                        onClick={() => onCheckConnection(item.path)}
                      >
                        {checking === item.path && checkKind === 'connection'
                          ? '正在检查…'
                          : connectionObservations[item.path]
                            ? '重新检查连接'
                            : '检查连接'}
                      </button>
                    )}
                    {item.extension === 'zip' && (
                      <button
                        className="button secondary compact"
                        disabled={reading || !!checking || item.sharing === 'shared'}
                        title={
                          item.sharing === 'shared'
                            ? '共享硬链接暂不检查'
                            : '只列出条目，不解压文件'
                        }
                        onClick={() => onCheckZip(item.path)}
                      >
                        {checking === item.path && checkKind === 'zip'
                          ? '正在检查…'
                          : zipObservations[item.path]
                            ? '重新检查'
                            : '检查 ZIP'}
                      </button>
                    )}
                    <button
                      className="button secondary compact"
                      disabled={reading || !!checking}
                      onClick={() => onReveal(item.path)}
                    >
                      在 Finder 中显示
                    </button>
                  </div>
                </article>
              ))}
            </div>
          )}
        </>
      )}
      <details className="origin-details">
        <summary>扫描范围与来源</summary>
        <p>
          固定 Mole {installerCatalog.version} 的 installer 扩展名：
          {installerCatalog.extensions.join('、')}
          。本客户端不区分扩展名大小写，仅查找用户所选目录最多 2 层、10000 个子项、256 个结果，约 3
          秒预算，包含隐藏目录。常用位置来自固定 Mole 的 12
          个默认路径，只提供选择，不检查存在性/权限，不自动扫描；不存在、受限或链接位置在手动扫描时如实报错。
        </p>
        <p>
          扫描只检查普通文件元数据；点击“检查 ZIP”后使用系统 ZipInfo 取得有界完整条目列表，再核对前
          50 条的固定 Mole 名称模式（区分大小写）。条目检查只展示名称线索；其后可手动测试 ZIP
          数据，不写出文件或验证签名，不执行 installer（含 dry-run）或挂载映像。ZIP
          可能是普通个人档案，其他扩展名也不证明文件格式或冗余。链接、包目录、特殊文件、未下载内容和跨文件系统位置保留跳过原因。
        </p>
        <p>
          ZIP 列表最多 256 KB、10000 条、2 秒进程运行预算；特殊名称、读取失败或超限保留未知。前 50
          条未匹配不代表后续没有安装内容；名称线索也不证明格式有效、冗余或可清理。共享硬链接不检查；检查失败、重新扫描或选择范围会清除旧条目线索。
        </p>
        <p>
          数据校验独立调用系统 UnZip
          测试模式，持有同一只读文件描述符；不解压到磁盘或运行归档内容，不接收密码。 只对最多 64 MB
          ZIP 执行，2 秒运行/CPU 预算；这是压缩文件大小门槛，不是解压数据量上限。
          加密、损坏、超时、异常或读取不完整均保留未知；测试通过不证明签名、用途、冗余或执行资格。
          重新校验、重查 ZIP 条目、重扫或换范围会撤回旧数据结果。
        </p>
        <p>
          DMG 数据校验单独使用系统 hdiutil verify 的 plist 结果，仅完整成功且 CRC32
          的实际/预期值相等才显示匹配。
          不读取或写入校验缓存，不挂载、不安装、不接收密码或验证签名；最多 128 MB 映像、4 秒运行/CPU
          预算。
          缺少或不支持校验和、加密、损坏、受限、超时与异常报告保留未知；内部校验和不覆盖所有文件字节，不是来源认证或可清理证明。
          重试、重扫或换范围撤回旧结果，检查后状态仍可能变化。
        </p>
        <p>
          DMG/ISO 的“检查连接”只读取系统 DiskImages
          连接列表，按当前路径精确匹配，包含已连接但未挂载的映像。
          它不是格式、身份或占用验证；路径别名、移动和检查后变化可能影响结果。未列出不能证明未连接、未使用或可清理。
          不读取其他映像内容或解析别名，不挂载、推出或请求管理员权限；重查、重扫或换范围撤回旧连接观察。
        </p>
        <p>
          “检查占用”只读可见进程的结构化打开记录，按设备与文件身份核对，排除 MooMole
          自己的观察句柄。
          发现其他进程打开时请保留；没有发现仍可能受权限和进程视图限制，不代表文件空闲或可清理。
          不关闭进程、不提权，不运行完整 Mole 删除保护流程。重查、重扫或换范围会撤回旧占用观察。
        </p>
        <p>
          文件大小沿用 Mole 的逻辑字节口径，不是独占物理占用或回收收益。共享硬链接、APFS
          共享/压缩/稀疏及扫描后的变化都可能影响含义；前后复核不是原子快照。Finder
          入口仅显示当前位置，不打开或安装文件。完整保护、完整占用/活动状态和连接身份验证、选择与处理仍待接入。
        </p>
      </details>
    </section>
  )
}
