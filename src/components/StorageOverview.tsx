import { useEffect, useState, type CSSProperties } from 'react'
import { HardDrive, LoaderCircle, Sparkles, Zap } from 'lucide-react'
import { mountedVolumesTransport, type MountedVolumesReport } from '../lib/mounted-volumes'
import { diskBytes } from '../lib/status-disks'
import { dateTime } from '../lib/format'
import { useNow } from '../lib/use-now'

const demoVolumes: MountedVolumesReport = {
  version: 1,
  createdAt: 0,
  expiresAt: 0,
  complete: true,
  volumes: [
    {
      id: 'demo-startup',
      name: '',
      path: '/',
      kind: 'startup',
      fileSystem: 'apfs',
      readOnly: true,
      totalBytes: 512 * 1024 ** 3,
      availableBytes: 128 * 1024 ** 3,
      canAnalyze: false,
    },
    {
      id: 'demo-external',
      name: 'External',
      path: '/Volumes/External',
      kind: 'mounted',
      fileSystem: 'exfat',
      readOnly: false,
      totalBytes: 1024 ** 4,
      availableBytes: 640 * 1024 ** 3,
      canAnalyze: false,
    },
  ],
}

export function StorageOverview({
  desktop,
  refreshSignal,
  canClean,
  onClean,
}: {
  desktop: boolean
  refreshSignal: number
  canClean: boolean
  onClean: () => void
}) {
  const [report, setReport] = useState<MountedVolumesReport | null>(desktop ? null : demoVolumes)
  const [loading, setLoading] = useState(desktop)
  const [error, setError] = useState('')
  useNow()
  const now = Date.now()
  useEffect(() => {
    if (!desktop) return
    let live = true
    setLoading(true)
    setError('')
    setReport(null)
    void mountedVolumesTransport
      .read()
      .then(
        (value) => {
          if (live) setReport(value)
        },
        (reason) => {
          if (live) setError(String(reason).replace(/^Error: /, ''))
        },
      )
      .finally(() => {
        if (live) setLoading(false)
      })
    return () => {
      live = false
    }
  }, [desktop, refreshSignal])
  const stale = desktop && report && (now < report.createdAt || now >= report.expiresAt)
  // The account directory is a second view into a volume, not another disk.
  const volumes = report?.volumes.filter((v) => v.kind !== 'account') ?? []
  return (
    <>
      <section className="overview-clean" aria-label="一键清理">
        <button
          className="overview-clean-button"
          disabled={!canClean}
          onClick={onClean}
          aria-label="开始一键清理"
        >
          <span className="overview-clean-mark" aria-hidden="true">
            m<span>·</span>
            <Sparkles size={18} />
          </span>
          <span>一键清理</span>
          <Zap size={14} aria-hidden="true" />
        </button>
        <div className="overview-clean-copy">
          <h2>让空间回到你手中</h2>
          <p>
            使用 Mole 完整清理规则，处理缓存、过期日志、废纸篓等项目。查看范围与影响，确认后执行。
          </p>
          <span className="muted">沿用 Mole 保护 · 可停止 · 逐项记录</span>
        </div>
      </section>
      <section className="overview-volumes" aria-label="所有磁盘空间" aria-busy={loading}>
        <header>
          <h2>
            <HardDrive size={17} />
            磁盘空间
          </h2>
          <span className="muted">
            {loading
              ? '正在读取磁盘…'
              : desktop && report
                ? `读取于 ${dateTime(report.createdAt)}`
                : desktop
                  ? '容量尚未读取'
                  : '示例磁盘'}
          </span>
        </header>
        {loading && (
          <p className="overview-volume-loading" role="status">
            <LoaderCircle className="spin" size={16} />
            正在读取已连接的磁盘…
          </p>
        )}
        {error && (
          <p className="notice warning" role="alert">
            {error}
          </p>
        )}
        {report && !report.complete && (
          <p className="notice warning" role="status">
            磁盘列表不完整，离线、权限受限或超出读取上限的卷可能未列出；请刷新或选择具体文件夹。
          </p>
        )}
        {stale && (
          <p className="notice warning" role="status">
            磁盘空间数据已过期，请刷新；连接或移除硬盘后也请刷新列表。
          </p>
        )}
        <div className="overview-volume-grid">
          {volumes.map((volume) => {
            const known = volume.totalBytes !== null && volume.availableBytes !== null
            const used = known ? volume.totalBytes! - volume.availableBytes! : null
            const usage = known ? (used! / volume.totalBytes!) * 100 : null
            return (
              <article className="card overview-volume" key={volume.id}>
                <div className="overview-volume-heading">
                  <HardDrive size={19} />
                  <div>
                    <h3 data-i18n={volume.kind === 'mounted' ? 'off' : undefined}>
                      {volume.kind === 'startup' ? '启动磁盘' : volume.name}
                    </h3>
                    <span className="mono full-path" data-i18n="off">
                      {volume.path}
                    </span>
                  </div>
                  <span className="tag">
                    {volume.kind === 'startup' ? '系统卷' : volume.readOnly ? '只读卷' : '已挂载卷'}
                  </span>
                </div>
                <div className="overview-volume-capacity">
                  <strong>{diskBytes(volume.availableBytes)}</strong>
                  <span>可用</span>
                  <span className="overview-volume-total">共 {diskBytes(volume.totalBytes)}</span>
                </div>
                <div
                  className="overview-volume-bar"
                  role="img"
                  data-unknown={!known}
                  aria-label={usage === null ? '空间占用未知' : `空间占用 ${usage.toFixed(1)}%`}
                  style={{ '--used': `${usage ?? 0}%` } as CSSProperties}
                >
                  <span />
                </div>
                <div className="overview-volume-meta">
                  <span>已占用 {diskBytes(used)}</span>
                  <span data-i18n="off">{usage === null ? '—' : `${usage.toFixed(1)}%`}</span>
                </div>
                {!volume.canAnalyze && desktop && (
                  <p className="muted">当前位置无法核对，请刷新后重试。</p>
                )}
              </article>
            )
          })}
        </div>
        {report && !volumes.length && (
          <p className="empty-inline">未读到可列出的磁盘，请刷新或选择文件夹。</p>
        )}
        <p className="overview-volume-note muted">
          展示启动磁盘及已挂载的本地卷，包含外置硬盘与磁盘映像。APFS
          卷可能共享空间，容量不相加；可用空间不代表可清理量。
        </p>
      </section>
    </>
  )
}
