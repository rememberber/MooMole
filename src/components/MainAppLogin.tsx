import { useEffect, useRef, useState } from 'react'
import {
  loginStatus,
  loginError,
  type LoginView,
  type MainAppLoginTransport,
} from '../lib/main-app-login'
import { dateTime } from '../lib/format'
export function MainAppLogin({
  enabled,
  transport,
}: {
  enabled: boolean
  transport: MainAppLoginTransport
}) {
  const [view, setView] = useState<LoginView | null>(null),
    [pending, setPending] = useState(false),
    [ack, setAck] = useState(false),
    [error, setError] = useState('')
  const alive = useRef(true),
    busy = useRef(false),
    generation = useRef(0)
  async function refresh() {
    const ticket = ++generation.current
    try {
      const r = await transport.read()
      if (alive.current && ticket === generation.current) setView(r)
    } catch (e) {
      if (alive.current && ticket === generation.current) setError(String(e))
    }
  }
  useEffect(() => {
    alive.current = true
    if (enabled) void refresh()
    return () => {
      alive.current = false
    }
  }, [enabled, transport])
  useEffect(() => {
    if (!enabled || (!pending && !view?.activeRequests.length)) return
    const timer = setTimeout(() => void refresh(), 750)
    return () => clearTimeout(timer)
  }, [view, pending, enabled])
  const system = view?.system,
    registered = !!system && ['enabled', 'requiresApproval'].includes(system.status),
    active = !!view?.activeRequests.length,
    unresolved = view?.records.some((r) => r.state === 'unknown' && r.reviewedAt === null) ?? false
  async function change() {
    if (!enabled || !system || !ack || busy.current || active || unresolved) return
    busy.current = true
    generation.current++
    setPending(true)
    setAck(false)
    setError('')
    try {
      const r = await transport.change(!registered, crypto.randomUUID(), system)
      generation.current++
      if (alive.current)
        setView((v) =>
          v
            ? {
                ...v,
                system: r.after,
                records: [r, ...v.records.filter((v) => v.id !== r.id)].slice(0, 50),
                activeRequests: [],
                error: null,
              }
            : v,
        )
    } catch (e) {
      if (alive.current) setError(String(e))
    } finally {
      busy.current = false
      if (alive.current) {
        setPending(false)
        await refresh()
      }
    }
  }
  async function review(id: string) {
    if (busy.current || active) return
    busy.current = true
    setPending(true)
    setError('')
    try {
      await transport.acknowledge(id)
      await refresh()
    } catch (e) {
      if (alive.current) setError(String(e))
    } finally {
      busy.current = false
      if (alive.current) setPending(false)
    }
  }
  return (
    <section className="card main-app-login" aria-label="MooMole 登录启动">
      <strong>登录时打开 MooMole</strong>
      <p>
        仅设置当前运行的 MooMole
        副本。系统允许后，后续登录会按正常方式打开窗口；菜单栏、定期采集和清理偏好保持独立，不会默认处理文件。
      </p>
      <p role="status">
        {pending || active
          ? '正在等待系统登录设置结果…'
          : system
            ? loginStatus[system.status]
            : '尚未取得系统登录状态'}
      </p>
      {view?.error && <p className="notice warning">{loginError(view.error)}</p>}
      {error && (
        <p role="alert" className="notice warning">
          {error}
        </p>
      )}
      {system?.status === 'notFound' && (
        <p className="notice">
          系统尚未确认此副本的登录服务；可明确请求注册，是否生效以返回状态为准。
        </p>
      )}
      <label className="application-update-consent">
        <input
          type="checkbox"
          checked={ack}
          disabled={!enabled || pending || active || unresolved || !system}
          onChange={(e) => setAck(e.target.checked)}
        />
        我已核对当前应用的登录启动设置。
      </label>
      <div className="modal-actions">
        <button
          className="button secondary"
          disabled={!enabled || pending}
          onClick={() => void refresh()}
        >
          刷新登录启动状态
        </button>
        <button
          className="button secondary"
          disabled={!enabled || pending || active || unresolved || !ack || !system}
          onClick={() => void change()}
        >
          {registered ? '关闭登录启动' : '开启登录启动'}
        </button>
        <button
          className="button secondary"
          disabled={!enabled || pending}
          onClick={() => void transport.settings().catch((e) => setError(String(e)))}
        >
          在系统登录项中核对
        </button>
      </div>
      <details>
        <summary>当前副本与最近 50 次登录设置记录</summary>
        {system && (
          <p className="mono full-path" data-i18n="off">
            {system.bundlePath}
          </p>
        )}
        {view?.records.map((r) => (
          <article key={r.id} className="card">
            <strong>
              {r.state === 'accepted'
                ? '系统已接受登录设置'
                : r.state === 'noChange'
                  ? '系统已是所选登录状态'
                  : r.state === 'notStarted'
                    ? '登录设置未提交'
                    : r.state === 'unknown'
                      ? '登录设置结果待核对'
                      : '登录设置请求处理中'}
            </strong>
            <p>
              {r.enabled ? '开启登录启动' : '关闭登录启动'} · {dateTime(r.createdAt)}
            </p>
            {r.after && <p>{loginStatus[r.after.status]}</p>}
            {r.error && <p>{loginError(r.error)}</p>}
            {r.state === 'unknown' && r.reviewedAt === null && (
              <button
                className="button secondary compact"
                disabled={pending || active || !system || system.status === 'notFound'}
                onClick={() => void review(r.id)}
              >
                已核对系统登录状态，保留原结果
              </button>
            )}
            {r.reviewedAt !== null && <p>已记录人工核查，原结果未更改。</p>}
          </article>
        ))}
      </details>
    </section>
  )
}
