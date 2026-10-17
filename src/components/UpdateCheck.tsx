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
  installUpdate,
  setUpdateAutoDownload,
} from '../lib/api'
import { bytes } from '../lib/format'
import type { UpdateCheckResult, UpdateSnapshot } from '../types'

const idle: UpdateSnapshot['download'] = {
  state: 'idle',
  restored: false,
  fileName: null,
  transferred: 0,
  total: 0,
  error: null,
}
const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error))
export function UpdateCheck({ saving = false }: { saving?: boolean }) {
  const [result, setResult] = useState<UpdateCheckResult | null>(null)
  const [download, setDownload] = useState(idle)
  const [pending, setPending] = useState(false)
  const [remoteChecking, setRemoteChecking] = useState(false)
  const [autoDownload, setAutoDownload] = useState(false)
  const [preferenceError, setPreferenceError] = useState<string | null>(null)
  const [recovering, setRecovering] = useState(false)
  const [recoveryError, setRecoveryError] = useState<string | null>(null)
  const [action, setAction] = useState<
    'download' | 'cancel' | 'reveal' | 'install' | 'preference' | null
  >(null)
  const [installing, setInstalling] = useState(false)
  const [confirmInstall, setConfirmInstall] = useState(false)
  const acting = useRef(false)
  const [error, setError] = useState('')
  const [openError, setOpenError] = useState('')
  const [installError, setInstallError] = useState<string | null>(null)
  const [observationError, setObservationError] = useState('')
  const [loaded, setLoaded] = useState(!desktop)
  const active = useRef(true)
  const checking = useRef(false)
  const generation = useRef(0)
  function apply(snapshot: UpdateSnapshot) {
    setRemoteChecking(snapshot.checking)
    setError(snapshot.checkError ?? '')
    setAutoDownload(snapshot.autoDownload)
    setPreferenceError(snapshot.preferenceError)
    setRecovering(snapshot.recovering)
    setRecoveryError(snapshot.recoveryError)
    setResult(snapshot.check)
    setDownload(snapshot.download)
    setInstalling(snapshot.installing)
    setInstallError(snapshot.installError)
    if (snapshot.download.state !== 'ready') setConfirmInstall(false)
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
        if (active.current && current === generation.current) {
          if (checking.current) {
            setRemoteChecking(snapshot.checking)
            setRecovering(snapshot.recovering)
            setObservationError('')
          } else apply(snapshot)
        }
      } catch {
        if (active.current && current === generation.current)
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
  const checkingNow = pending || remoteChecking
  const running = ['downloading', 'verifying', 'cancelling'].includes(download.state)
  const preparing = installing || action === 'install'
  async function check() {
    if (
      checking.current ||
      remoteChecking ||
      acting.current ||
      preparing ||
      recovering ||
      observationError
    )
      return
    setConfirmInstall(false)
    checking.current = true
    generation.current += 1
    setPending(true)
    setError('')
    setOpenError('')
    setInstallError(null)
    setRecoveryError(null)
    setRecovering(false)
    setResult(null)
    setDownload(idle)
    try {
      const next = await checkUpdates()
      if (active.current) apply(next)
    } catch (error) {
      if (active.current) setError(errorText(error))
    } finally {
      generation.current += 1
      checking.current = false
      if (active.current) setPending(false)
    }
  }
  async function perform(kind: 'download' | 'cancel' | 'reveal' | 'install') {
    if (acting.current) return
    acting.current = true
    setConfirmInstall(false)
    generation.current += 1
    setAction(kind)
    setOpenError('')
    try {
      if (kind === 'download') {
        const snapshot = await downloadUpdate()
        if (active.current) apply(snapshot)
      } else if (kind === 'cancel') {
        await cancelUpdateDownload()
        if (active.current) setDownload((previous) => ({ ...previous, state: 'cancelling' }))
      } else if (kind === 'install') await installUpdate()
      else await revealUpdateDownload()
    } catch (error) {
      if (active.current) {
        setOpenError(errorText(error))
        if (kind === 'install') setInstalling(false)
      }
    } finally {
      acting.current = false
      if (active.current) setAction(null)
    }
  }
  async function toggleAutoDownload(enabled: boolean) {
    if (acting.current) return
    acting.current = true
    generation.current += 1
    setAction('preference')
    setOpenError('')
    try {
      const next = await setUpdateAutoDownload(enabled)
      if (active.current) apply(next)
    } catch (error) {
      if (active.current) setOpenError(errorText(error))
    } finally {
      generation.current += 1
      acting.current = false
      if (active.current) setAction(null)
    }
  }
  const operationError =
    download.error || openError || installError || recoveryError || preferenceError
  const message = observationError
    ? '更新状态尚未确认'
    : recovering
      ? '正在复核本地更新包…'
      : checkingNow
        ? '正在检查更新…'
        : !loaded
          ? '正在读取更新状态…'
          : preparing
            ? '正在复核并打开安装包…'
            : download.state === 'downloading'
              ? '正在下载更新包'
              : download.state === 'verifying'
                ? '正在校验已保存的更新包…'
                : download.state === 'cancelling'
                  ? '正在取消并清理临时文件…'
                  : download.state === 'ready'
                    ? download.restored
                      ? '已找到并校验本地更新包'
                      : '更新包已下载并校验'
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
          disabled={
            !desktop ||
            !loaded ||
            checkingNow ||
            running ||
            !!action ||
            preparing ||
            recovering ||
            !!observationError
          }
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
      {operationError && (
        <p className="error-text" role="alert">
          {operationError}
        </p>
      )}
      {observationError && (
        <p className="error-text" role="alert">
          {observationError}
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
            disabled={!!action || download.state === 'cancelling'}
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
              {confirmInstall ? (
                <div role="group" aria-label="确认安装更新">
                  <p className="muted">
                    即将打开安装包并退出 MooMole。设置与历史会保留，请按系统提示手动替换应用。
                  </p>
                  <div className="update-actions">
                    <button
                      className="button primary"
                      disabled={
                        checkingNow ||
                        !!action ||
                        preparing ||
                        recovering ||
                        saving ||
                        !!observationError
                      }
                      onClick={() => void perform('install')}
                    >
                      打开并退出
                    </button>
                    <button className="button secondary" onClick={() => setConfirmInstall(false)}>
                      暂不安装
                    </button>
                  </div>
                </div>
              ) : (
                <>
                  <p className="muted">
                    安装时打开 DMG 并退出 MooMole，再由你手动将应用拖入“应用程序”替换。
                  </p>
                  <div className="update-actions">
                    <button
                      className="button primary"
                      disabled={
                        checkingNow ||
                        !!action ||
                        preparing ||
                        recovering ||
                        saving ||
                        !!observationError
                      }
                      onClick={() => {
                        setOpenError('')
                        setConfirmInstall(true)
                      }}
                    >
                      {preparing ? '正在准备安装…' : '打开 DMG 安装'}
                    </button>
                    <button
                      className="button secondary"
                      disabled={
                        checkingNow || !!action || preparing || recovering || !!observationError
                      }
                      onClick={() => void perform('reveal')}
                    >
                      {action === 'reveal' ? '正在复核文件…' : '在 Finder 中查看'}
                    </button>
                  </div>
                </>
              )}
            </>
          ) : !running && result.asset ? (
            <>
              <p className="muted">下载后校验大小和 SHA-512，再由你手动安装。</p>
              <button
                className="button secondary"
                disabled={
                  checkingNow ||
                  !!action ||
                  preparing ||
                  recovering ||
                  !!observationError ||
                  !loaded
                }
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
      <label className="update-preference">
        <input
          type="checkbox"
          aria-label="手动检查后自动下载"
          aria-describedby="update-auto-download-hint"
          checked={autoDownload}
          disabled={
            !desktop ||
            !loaded ||
            saving ||
            checkingNow ||
            recovering ||
            !!action ||
            preparing ||
            !!observationError
          }
          onChange={(event) => void toggleAutoDownload(event.target.checked)}
        />
        <span>
          手动检查后自动下载
          <small id="update-auto-download-hint">
            仅影响后续手动检查，当前下载可单独取消；安装仍需确认。
          </small>
        </span>
      </label>
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
