import { getLocale } from '../i18n/locale'
import { useEffect, useRef, useState } from 'react'
import {
  cancelCleanupVisibility,
  desktop,
  getCleanupVisibility,
  startCleanupVisibility,
} from '../lib/api'
import {
  cleanupVisibilityText,
  readCleanupVisibility,
  type CleanupVisibilitySnapshot,
} from '../lib/cleanup-visibility'
const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error))

export function CleanupVisibility({ saving = false }: { saving?: boolean }) {
  const [snapshot, setSnapshot] = useState<CleanupVisibilitySnapshot | null>(null)
  const [action, setAction] = useState(false)
  const [error, setError] = useState('')
  const [actionError, setActionError] = useState('')
  const [reload, setReload] = useState(0)
  const active = useRef(false)
  const acting = useRef(false)
  const generation = useRef(0)
  useEffect(() => {
    active.current = true
    return () => {
      active.current = false
      generation.current++
    }
  }, [])
  useEffect(() => {
    if (!desktop) return
    const token = generation.current
    let stopped = false
    let timer: ReturnType<typeof setTimeout> | undefined
    async function read() {
      try {
        const next = readCleanupVisibility(await getCleanupVisibility())
        if (stopped || !active.current || token !== generation.current) return
        setSnapshot(next)
        setError('')
        if (next.state === 'running' || next.state === 'cancelling')
          timer = setTimeout(() => void read(), 700)
      } catch (error) {
        if (!stopped && active.current && token === generation.current) {
          setSnapshot(null)
          setError(errorText(error))
        }
      }
    }
    void read()
    return () => {
      stopped = true
      clearTimeout(timer)
    }
  }, [reload])
  async function change(cancel = false) {
    if (acting.current || !snapshot || (!cancel && (saving || !!error))) return
    acting.current = true
    const token = ++generation.current
    setAction(true)
    setError('')
    setActionError('')
    if (!cancel) setSnapshot(null)
    try {
      const value =
        cancel && snapshot.requestId
          ? await cancelCleanupVisibility(snapshot.requestId)
          : await startCleanupVisibility()
      if (active.current && token === generation.current) setSnapshot(readCleanupVisibility(value))
    } catch (error) {
      if (active.current && token === generation.current) setActionError(errorText(error))
    } finally {
      if (active.current && token === generation.current) {
        acting.current = false
        setAction(false)
        setReload((value) => value + 1)
      }
    }
  }
  if (!desktop) return <p className="muted">请在桌面 App 中检查系统权限。</p>
  const busy = snapshot?.state === 'running' || snapshot?.state === 'cancelling'
  return (
    <div className="cleanup-visibility" role="group" aria-label="清理权限检查">
      <p>用于排查清理占用检查的权限问题。点击后请求系统管理员确认，只读取进程可见性。</p>
      <p className="muted">
        不处理文件；检查通过不代表所选项目可以清理。关闭设置不会取消已开始的检查。
      </p>
      <p role="status" aria-live="polite">
        {snapshot
          ? cleanupVisibilityText(snapshot)
          : error
            ? '检查状态暂时无法确认，请重新读取。'
            : '正在读取检查状态…'}
      </p>
      {snapshot?.finishedAt && (
        <p className="muted">
          检查时间：{new Date(snapshot.finishedAt).toLocaleString(getLocale())}
        </p>
      )}
      {snapshot?.diagnostic && (
        <details>
          <summary>检查细节</summary>
          <p>{snapshot.diagnostic}</p>
        </details>
      )}
      {error && <p role="alert">{error}</p>}
      {actionError && <p role="alert">{actionError}</p>}
      <div className="button-row">
        <button
          className="button secondary"
          disabled={saving || action || !snapshot || busy || !!error}
          onClick={() => void change()}
        >
          授权检查
        </button>
        {busy && (
          <button
            className="button secondary"
            disabled={action || snapshot.state === 'cancelling'}
            onClick={() => void change(true)}
          >
            停止等待
          </button>
        )}
        {error && (
          <button
            className="button secondary"
            disabled={action}
            onClick={() => {
              generation.current++
              setReload((value) => value + 1)
            }}
          >
            重新读取
          </button>
        )}
      </div>
    </div>
  )
}
