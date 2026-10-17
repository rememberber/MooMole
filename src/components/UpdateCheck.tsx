import { useEffect, useRef, useState } from 'react'
import { ArrowUpRight, RefreshCw } from 'lucide-react'
import {
  checkUpdates,
  desktop,
  openUpdateReleases,
  updateSnapshot,
  downloadUpdate,
  cancelUpdateDownload,
  revealUpdateDownload,
} from '../lib/api'
import { bytes } from '../lib/format'
import type { UpdateCheckResult, UpdateSnapshot } from '../types'

const idle: UpdateSnapshot['download'] = {
  state: 'idle',
  fileName: null,
  transferred: 0,
  total: 0,
  error: null,
}
const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error))
export function UpdateCheck() {
  const [result, setResult] = useState<UpdateCheckResult | null>(null)
  const [download, setDownload] = useState(idle)
  const [pending, setPending] = useState(false)
  const [action, setAction] = useState(false)
  const [error, setError] = useState('')
  const [openError, setOpenError] = useState('')
  const [observationError, setObservationError] = useState('')
  const [loaded, setLoaded] = useState(!desktop)
  const active = useRef(true)
  const checking = useRef(false)
  const generation = useRef(0)
  function apply(snapshot: UpdateSnapshot) {
    setResult(snapshot.check)
    setDownload(snapshot.download)
    setLoaded(true)
    setObservationError('')
  }
  useEffect(() => {
    active.current = true
    let timer: ReturnType<typeof setTimeout>
    async function poll() {
      const current = generation.current
      try {
        const snapshot = await updateSnapshot()
        if (active.current && current === generation.current && !checking.current) apply(snapshot)
      } catch {
        if (active.current && current === generation.current && !checking.current)
          setObservationError('更新状态暂时无法读取，恢复连接后会重新确认。')
      } finally {
        if (active.current) timer = setTimeout(() => void poll(), 700)
      }
    }
    if (desktop) void poll()
    return () => {
      active.current = false
      clearTimeout(timer)
    }
  }, [])
  const running = ['downloading', 'verifying', 'cancelling'].includes(download.state)
  async function check() {
    if (checking.current) return
    checking.current = true
    generation.current += 1
    setPending(true)
    setError('')
    setResult(null)
    setDownload(idle)
    try {
      const next = await checkUpdates()
      if (active.current) setResult(next)
    } catch (error) {
      if (active.current) setError(errorText(error))
    } finally {
      checking.current = false
      if (active.current) setPending(false)
    }
  }
  async function perform(kind: 'download' | 'cancel' | 'reveal') {
    generation.current += 1
    setAction(true)
    setOpenError('')
    try {
      if (kind === 'download') {
        const snapshot = await downloadUpdate()
        if (active.current) apply(snapshot)
      } else if (kind === 'cancel') {
        await cancelUpdateDownload()
        if (active.current) setDownload((previous) => ({ ...previous, state: 'cancelling' }))
      } else await revealUpdateDownload()
    } catch (error) {
      if (active.current) setOpenError(errorText(error))
    } finally {
      if (active.current) setAction(false)
    }
  }
  const message = pending
    ? '正在检查更新…'
    : observationError
      ? '更新状态尚未确认'
      : !loaded
        ? '正在读取更新状态…'
        : download.state === 'downloading'
          ? '正在下载更新包'
          : download.state === 'verifying'
            ? '正在校验已保存的更新包…'
            : download.state === 'cancelling'
              ? '正在取消并清理临时文件…'
              : download.state === 'ready'
                ? '更新包已下载并校验'
                : download.state === 'cancelled'
                  ? '下载已取消'
                  : download.state === 'failed'
                    ? '下载未完成'
                    : result?.status === 'available'
                      ? `发现新版本 ${result.latestVersion}`
                      : result?.status === 'current'
                        ? '当前版本无需更新'
                        : result?.status === 'unpublished'
                          ? '尚未发布可检查的版本'
                          : desktop
                            ? '手动检查 MooMole 客户端更新'
                            : '请在桌面 App 中检查更新'
  return (
    <section className="setting-group update-check" aria-label="MooMole 更新">
      <div className="setting-row">
        <span role="status" aria-live="polite">
          {message}
          <small>
            {result
              ? result.channel === 'preview'
                ? '测试版通道'
                : '稳定版通道'
              : '检查时连接 GitHub'}{' '}
            · 手动安装 DMG
          </small>
        </span>
        <button
          className="button secondary"
          disabled={!desktop || !loaded || pending || running || action}
          onClick={() => void check()}
        >
          <RefreshCw size={14} />
          {error ? '重试检查' : '检查更新'}
        </button>
      </div>
      {error && (
        <p className="error-text" role="alert">
          {error}
        </p>
      )}
      {openError && openError !== download.error && (
        <p className="error-text" role="alert">
          {openError}
        </p>
      )}
      {observationError && (
        <p className="error-text" role="alert">
          {observationError}
        </p>
      )}
      {download.error && (
        <p className="error-text" role="alert">
          {download.error}
        </p>
      )}
      {download.fileName && <p className="muted update-file">{download.fileName}</p>}
      {running && (
        <div className="update-progress">
          <progress
            aria-label="更新包下载进度"
            max={download.total || 1}
            value={download.transferred}
          />
          <p className="muted">
            {bytes(download.transferred)} / {bytes(download.total)} · 关闭设置后仍会继续下载
          </p>
          <button
            className="button secondary"
            disabled={action || download.state === 'cancelling'}
            onClick={() => void perform('cancel')}
          >
            取消下载
          </button>
        </div>
      )}
      {result?.status === 'unpublished' && <p className="muted">可前往发布页查看开发进展。</p>}
      {result?.status === 'available' && (
        <>
          {download.state === 'ready' ? (
            <>
              <p className="muted">
                请在 Finder 中打开 DMG，手动将应用拖入“应用程序”。首次打开请按系统提示确认。
              </p>
              <button
                className="button secondary"
                disabled={action || !!observationError}
                onClick={() => void perform('reveal')}
              >
                {action ? '正在复核文件…' : '在 Finder 中查看'}
              </button>
            </>
          ) : !running && result.asset ? (
            <>
              <p className="muted">下载后校验大小和 SHA-512，再由你手动安装。</p>
              <button
                className="button secondary"
                disabled={action || !!observationError || !loaded}
                onClick={() => void perform('download')}
              >
                {download.state === 'failed' ? '重新下载' : '下载更新包'}
              </button>
            </>
          ) : !result.asset ? (
            <p className="muted">
              新版尚未提供当前架构的安装包，请查看发布页；不会选择其他架构的文件。
            </p>
          ) : null}
          {result.notes && (
            <details>
              <summary>更新内容</summary>
              <pre className="update-notes">{result.notes}</pre>
            </details>
          )}
          {result.notesTruncated && (
            <p className="muted">这里只展示部分更新内容，完整说明请查看发布页。</p>
          )}
        </>
      )}
      <button
        className="button ghost"
        onClick={() => {
          setOpenError('')
          void openUpdateReleases().catch((error) => {
            if (active.current) setOpenError(errorText(error))
          })
        }}
      >
        查看发布页
        <ArrowUpRight size={14} />
      </button>
    </section>
  )
}
