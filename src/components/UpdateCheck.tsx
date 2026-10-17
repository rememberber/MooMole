import { useEffect, useRef, useState } from 'react'
import { ArrowUpRight, RefreshCw } from 'lucide-react'
import { checkUpdates, desktop, openUpdateReleases } from '../lib/api'
import type { UpdateCheckResult } from '../types'

export function UpdateCheck() {
  const [result, setResult] = useState<UpdateCheckResult | null>(null)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState('')
  const [openError, setOpenError] = useState('')
  const active = useRef(true)
  const checking = useRef(false)
  useEffect(() => {
    active.current = true
    return () => {
      active.current = false
    }
  }, [])
  async function check() {
    if (checking.current) return
    checking.current = true
    setPending(true)
    setError('')
    setResult(null)
    try {
      const next = await checkUpdates()
      if (active.current) setResult(next)
    } catch (error) {
      if (active.current) setError(error instanceof Error ? error.message : String(error))
    } finally {
      checking.current = false
      if (active.current) setPending(false)
    }
  }
  const message =
    result?.status === 'available'
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
          {pending ? '正在检查更新…' : message}
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
          disabled={!desktop || pending}
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
      {openError && (
        <p className="error-text" role="alert">
          {openError}
        </p>
      )}
      {result?.status === 'unpublished' && <p className="muted">可前往发布页查看开发进展。</p>}
      {result?.status === 'available' && (
        <>
          <p className="muted">
            {result.asset
              ? '请前往发布页查看说明并手动下载。应用内下载尚未开放。'
              : '新版尚未提供当前架构的安装包，请查看发布页；不会选择其他架构的文件。'}
          </p>
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
            if (active.current) setOpenError(error instanceof Error ? error.message : String(error))
          })
        }}
      >
        查看发布页
        <ArrowUpRight size={14} />
      </button>
    </section>
  )
}
