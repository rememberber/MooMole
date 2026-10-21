import {
  installerCatalog,
  installerIncompleteReason,
  zipInspectionText,
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
  zipObservations,
  onCheckZip,
}: {
  root: string
  report: InstallersReport | null
  reading: boolean
  error: string
  checking: string
  zipObservations: ZipObservations
  onCheckZip: (path: string) => void
  onReveal: (path: string) => void
}) {
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
              : '从顶部选择下载目录或其他明确范围，不自动扫描个人目录。'}
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
            。清单按扩展名列出，ZIP
            可逐项检查条目；挂载与占用尚未检查，清理未开放，文件大小不代表可释放空间。
          </p>
          {report.completion === 'partial' && (
            <p className="notice">
              本次未覆盖：{installerIncompleteReason(report)}
              。可缩小范围后重试，未显示不代表不存在。
            </p>
          )}
          {report.items.length === 0 ? (
            <div className="empty-state">
              <h3>本次未发现匹配文件</h3>
              <p className="muted">只检查所选范围的固定扩展名，不代表没有安装包。</p>
            </div>
          ) : (
            <div className="installer-list">
              {report.items.map((item) => (
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
                    {zipObservations[item.path]?.report && (
                      <span className="muted">
                        上次检查 {dateTime(zipObservations[item.path].report!.checkedAt)} ·{' '}
                        {zipInspectionText(zipObservations[item.path].report!)}
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
                        {checking === item.path
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
          秒预算，包含隐藏目录。不自动读取 Mole 的其他默认位置。
        </p>
        <p>
          扫描只检查普通文件元数据；点击“检查 ZIP”后使用系统 ZipInfo 取得有界完整条目列表，再核对前
          50 条的固定 Mole 名称模式（区分大小写）。只展示名称线索，不解压或校验归档数据/签名，不执行
          installer（含 dry-run）或挂载映像。ZIP
          可能是普通个人档案，其他扩展名也不证明文件格式或冗余。链接、包目录、特殊文件、未下载内容和跨文件系统位置保留跳过原因。
        </p>
        <p>
          ZIP 列表最多 256 KB、10000 条、2 秒进程运行预算；特殊名称、读取失败或超限保留未知。前 50
          条未匹配不代表后续没有安装内容；名称线索也不证明格式有效、冗余或可清理。共享硬链接不检查；检查失败、重新扫描或选择范围会清除旧条目线索。
        </p>
        <p>
          文件大小沿用 Mole 的逻辑字节口径，不是独占物理占用或回收收益。共享硬链接、APFS
          共享/压缩/稀疏及扫描后的变化都可能影响含义；前后复核不是原子快照。Finder
          入口仅显示当前位置，不打开或安装文件。完整保护、占用/挂载检查、选择与处理仍待接入。
        </p>
      </details>
    </section>
  )
}
