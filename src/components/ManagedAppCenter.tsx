import { useEffect, useRef, useState } from 'react'
import { Modal } from './Modal'
import type { UninstallTarget } from '../lib/app-uninstall'
import {
  managedTransport as t,
  readManagedDraft,
  readManagedRecord,
  managedState,
  type ManagedDraft,
  type ManagedRecord,
} from '../lib/managed-apps'
import { useNow } from '../lib/use-now'
export function ManagedAppCenter({
  target,
  onClose,
  historyVisible,
  refreshSignal = 0,
  desktop,
  enabled,
  onActiveChange,
  onChanged,
}: {
  target: UninstallTarget | null
  onClose: () => void
  historyVisible: boolean
  refreshSignal?: number
  desktop: boolean
  enabled: boolean
  onActiveChange: (v: boolean) => void
  onChanged: () => void
}) {
  const [draft, setDraft] = useState<ManagedDraft | null>(null),
    [result, setResult] = useState<ManagedRecord | null>(null),
    [rows, setRows] = useState<ManagedRecord[]>([]),
    [busy, setBusy] = useState(false),
    [executing, setExecuting] = useState(false),
    [error, setError] = useState(''),
    [ack, setAck] = useState(false)
  const request = useRef<string | null>(null),
    generation = useRef(0),
    observedRunning = useRef(false),
    changed = useRef(onChanged)
  changed.current = onChanged
  useNow()
  const now = Date.now()
  async function read() {
    const raw = await t.read()
    if (!Array.isArray(raw) || raw.length > 64) throw Error('Homebrew 操作历史返回异常。')
    setRows(raw.map(readManagedRecord))
  }
  useEffect(() => {
    if (!desktop) return
    let live = true,
      timer: ReturnType<typeof setTimeout> | undefined
    async function poll() {
      try {
        const raw = await t.read()
        if (!Array.isArray(raw) || raw.length > 64) throw Error('Homebrew 操作历史返回异常。')
        const next = raw.map(readManagedRecord)
        if (!live) return
        setRows(next)
        const active = next.some((r) => r.state === 'running')
        if (observedRunning.current && !active) changed.current()
        observedRunning.current = active
        onActiveChange(active || request.current !== null)
        if (active || request.current !== null) timer = setTimeout(() => void poll(), 1000)
      } catch (e) {
        if (live) {
          setError(String(e))
          timer = setTimeout(() => void poll(), 2000)
        }
      }
    }
    void poll()
    return () => {
      live = false
      if (timer) clearTimeout(timer)
    }
  }, [desktop, historyVisible, target, onActiveChange, busy, refreshSignal])
  useEffect(() => {
    const current = ++generation.current
    let live = true,
      issued = false,
      planId: string | null = null
    setDraft(null)
    setResult(null)
    setAck(false)
    setError('')
    if (!target) return
    const id = crypto.randomUUID()
    request.current = id
    setExecuting(false)
    setBusy(true)
    onActiveChange(true)
    void Promise.resolve()
      .then(() => {
        if (!live) return undefined
        issued = true
        return t.prepare(target, id)
      })
      .then((raw) => {
        if (!issued) return
        const d = readManagedDraft(raw, target)
        planId = d.id
        if (live) setDraft(d)
        else void t.discard(d.id).catch(() => {})
      })
      .catch((e) => {
        if (live) setError(String(e).replace(/^Error: /, ''))
      })
      .finally(() => {
        if (current === generation.current || request.current === id) setBusy(false)
        if (current === generation.current || request.current === id) onActiveChange(false)
        if (request.current === id) request.current = null
      })
    return () => {
      live = false
      if (issued && request.current === id) void t.cancel(id).catch(() => {})
      if (planId) void t.discard(planId).catch(() => {})
    }
  }, [target, onActiveChange])
  async function execute() {
    if (!draft || !ack || busy || !enabled || now < draft.createdAt || now >= draft.expiresAt)
      return
    const id = crypto.randomUUID()
    request.current = id
    setExecuting(true)
    setBusy(true)
    onActiveChange(true)
    setError('')
    try {
      const r = readManagedRecord(await t.execute(draft.id, id))
      if (r.id !== draft.id || r.requestId !== id || r.draft.path !== draft.path)
        throw Error('Homebrew 返回了其他操作的结果。')
      setDraft(null)
      setResult(r)
      await read()
      onChanged()
    } catch (e) {
      setDraft(null)
      setError(String(e))
      await read().catch(() => {})
    } finally {
      request.current = null
      setExecuting(false)
      setBusy(false)
      onActiveChange(false)
    }
  }
  return (
    <>
      {historyVisible && !target && error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
      {target && (
        <Modal
          title="通过 Homebrew 卸载"
          onClose={executing ? undefined : onClose}
          className="review-plan-modal"
        >
          <h2>通过 Homebrew 卸载</h2>
          <p className="mono full-path" data-i18n="off">
            {target.path}
          </p>
          <p className="notice warning">
            这会由 Homebrew 正常卸载所选
            Cask，不是移入废纸篓。没有应用内撤销，恢复通常需要重新安装。不会使用
            zap、强制卸载、忽略依赖或自动移除其他依赖；普通卸载与系统行为仍可能影响相关内容，请先自行保存并退出应用。
          </p>
          {busy && <p role="status">正在检查或等待 Homebrew 结束；停止后仍需核对已发生的结果。</p>}
          {error && (
            <p role="alert" className="error">
              {error}
            </p>
          )}
          {result && (
            <div role="status">
              <strong>{managedState[result.state]}</strong>
              <p>{result.reason}</p>
            </div>
          )}
          {draft && (
            <>
              <dl>
                <dt>Homebrew 包</dt>
                <dd className="mono" data-i18n="off">
                  {draft.token}
                </dd>
                <dt>已安装版本</dt>
                <dd data-i18n="off">{draft.installedVersion}</dd>
                <dt>管理命令位置</dt>
                <dd className="mono full-path" data-i18n="off">
                  {draft.managerPath}
                </dd>
              </dl>
              {(now < draft.createdAt || now >= draft.expiresAt) && (
                <p className="notice warning">确认时间无法核对或已过期，请关闭后重新检查。</p>
              )}
              <label className="confirmation-ack">
                <input
                  type="checkbox"
                  checked={ack}
                  disabled={busy}
                  onChange={(e) => setAck(e.target.checked)}
                />
                我已核对这个包和应用，了解 Homebrew 卸载及重新安装的恢复边界。
              </label>
              <button
                className="button primary"
                disabled={
                  busy || !enabled || !ack || now < draft.createdAt || now >= draft.expiresAt
                }
                onClick={() => void execute()}
              >
                确认通过 Homebrew 卸载
              </button>
            </>
          )}
          {busy && (
            <button
              className="button secondary"
              onClick={() => {
                if (request.current)
                  void t.cancel(request.current).catch((e) => setError(String(e)))
              }}
            >
              停止本次 Homebrew 任务
            </button>
          )}
          {!busy && (
            <button className="button secondary" onClick={onClose}>
              返回应用列表
            </button>
          )}
        </Modal>
      )}
      {(historyVisible || target) && rows.length > 0 && (
        <section className="personal-list" aria-label="Homebrew 操作历史">
          <h3>Homebrew 操作历史</h3>
          {rows.map((r) => (
            <article key={r.id}>
              <strong data-i18n="off">{r.draft.token}</strong>
              <p>{managedState[r.state]}</p>
              <p className="mono full-path" data-i18n="off">
                {r.draft.path}
              </p>
              <p>{r.reason}</p>
              {r.state === 'running' && (
                <button
                  className="button secondary compact"
                  onClick={() => {
                    void t.cancel(r.requestId).catch((e) => setError(String(e)))
                  }}
                >
                  停止本次 Homebrew 任务
                </button>
              )}
              {['unknown', 'interrupted'].includes(r.state) && r.reviewedAt === null && (
                <button
                  className="button secondary compact"
                  disabled={busy || !enabled}
                  onClick={() => {
                    void t
                      .review(r.id)
                      .then(() => read())
                      .catch((e) => setError(String(e)))
                  }}
                >
                  已核查实际 App 与包状态
                </button>
              )}
            </article>
          ))}
        </section>
      )}
    </>
  )
}
