import { useEffect, useRef, useState } from 'react'
import { Modal } from './Modal'
import { bytes } from '../lib/format'
import { useNow } from '../lib/use-now'
import {
  mountedVolumesTransport,
  type MountedVolumesReport,
  type MountedVolume,
} from '../lib/mounted-volumes'
const volumeName = (v: MountedVolume) =>
  v.kind === 'account' ? '当前账户目录' : v.kind === 'startup' ? '启动卷根目录' : v.name
export function MountedVolumePicker({
  openSignal,
  enabled,
  onAnalyze,
}: {
  openSignal: number
  enabled: boolean
  onAnalyze: (path: string) => void
}) {
  const [open, setOpen] = useState(false),
    [loading, setLoading] = useState(false),
    [report, setReport] = useState<MountedVolumesReport | null>(null),
    [error, setError] = useState('')
  const busy = useRef(false),
    generation = useRef(0),
    now = useNow()
  async function read() {
    if (busy.current) return
    const token = ++generation.current
    busy.current = true
    setLoading(true)
    setError('')
    setReport(null)
    try {
      const r = await mountedVolumesTransport.read()
      if (token === generation.current) setReport(r)
    } catch (e) {
      if (token === generation.current) setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      if (token === generation.current) setLoading(false)
    }
  }
  useEffect(() => {
    if (openSignal && !busy.current) {
      setOpen(true)
      void read()
    }
  }, [openSignal])
  useEffect(
    () => () => {
      generation.current++
    },
    [],
  )
  function close() {
    if (busy.current) return
    generation.current++
    setOpen(false)
  }
  async function select(volume: MountedVolume) {
    if (busy.current || !enabled || !report || now >= report.expiresAt || now < report.createdAt)
      return
    busy.current = true
    const token = ++generation.current
    setLoading(true)
    setError('')
    try {
      const path = await mountedVolumesTransport.resolve(volume)
      if (token !== generation.current) return
      setOpen(false)
      onAnalyze(path)
    } catch (e) {
      if (token === generation.current) {
        setError(String(e).replace(/^Error: /, ''))
        setReport(null)
      }
    } finally {
      busy.current = false
      if (token === generation.current) setLoading(false)
    }
  }
  if (!open) return null
  const fresh = report && now >= report.createdAt && now < report.expiresAt
  return (
    <Modal
      title="选择磁盘分析"
      className="mounted-volume-modal"
      onClose={loading ? undefined : close}
    >
      <h2>选择磁盘分析</h2>
      <p className="notice">
        从启动卷根目录、账户目录或已挂载的本地卷开始分析；列表也可能包含磁盘映像。容量是本次读取的观察值，APFS
        卷可能共享空间，不能直接相加。分析后需重新扫描、选择并确认，才会移动或清理文件。
      </p>
      {loading && <p role="status">正在读取或核对磁盘…</p>}
      {report && !report.complete && (
        <p role="status" className="notice warning">
          磁盘列表不完整，离线、权限受限或超出读取上限的卷可能未列出；请刷新或选择具体文件夹。
        </p>
      )}
      {report && !fresh && <p role="status">磁盘观察已过期，请刷新列表后选择。</p>}
      <ul className="mounted-volume-list" aria-label="本地磁盘与账户目录">
        {report?.volumes.map((v) => (
          <li key={v.id}>
            <div>
              <strong data-i18n={v.kind === 'mounted' ? 'off' : undefined}>{volumeName(v)}</strong>
              <p className="mono full-path" data-i18n="off">
                {v.path}
              </p>
              <p className="muted">
                可用 {bytes(v.availableBytes)} / 总量 {bytes(v.totalBytes)} ·{' '}
                <span data-i18n="off">{v.fileSystem ?? '—'}</span>
                {v.readOnly === true && ' · 只读卷'}
              </p>
              {!v.canAnalyze && <p className="muted">当前位置无法核对，暂不能从这里分析。</p>}
            </div>
            <button
              className="button secondary compact"
              aria-label={`分析 ${volumeName(v)}`}
              disabled={loading || !enabled || !fresh || !v.canAnalyze}
              onClick={() => void select(v)}
            >
              开始分析
            </button>
          </li>
        ))}
      </ul>
      {report && !report.volumes.length && <p>未读到可列出的磁盘，请刷新或选择文件夹。</p>}
      {error && (
        <p role="alert" className="notice warning">
          {error}
        </p>
      )}
      <div className="modal-actions">
        <button className="button secondary" disabled={loading} onClick={close}>
          返回空间分析
        </button>
        <button
          className="button primary"
          disabled={loading || !enabled}
          onClick={() => void read()}
        >
          刷新磁盘列表
        </button>
      </div>
    </Modal>
  )
}
