import { useEffect, useRef, useState } from 'react'
import { bytes, dateTime } from '../lib/format'
import {
  notificationAllowed,
  notificationReason,
  notificationState,
  readStorageNotifications,
  type StorageNotificationsTransport,
  type StorageNotificationsView,
} from '../lib/storage-notifications'
export function StorageNotifications({
  enabled,
  transport,
}: {
  enabled: boolean
  transport: StorageNotificationsTransport
}) {
  const [view, setView] = useState<StorageNotificationsView | null>(null),
    [pending, setPending] = useState(false),
    [error, setError] = useState(''),
    [agreed, setAgreed] = useState(false)
  const busy = useRef(false),
    epoch = useRef(0)
  useEffect(() => {
    if (!enabled) return
    let live = true
    async function read() {
      if (busy.current) return
      const generation = ++epoch.current
      try {
        const next = readStorageNotifications(await transport.read())
        if (live && generation === epoch.current) {
          setView(next)
          setError('')
        }
      } catch (e) {
        if (live && generation === epoch.current)
          setError(notificationReason(String(e).replace(/^Error: /, '')))
      }
    }
    void read()
    const timer = setInterval(() => void read(), 30_000)
    return () => {
      live = false
      epoch.current++
      clearInterval(timer)
    }
  }, [enabled, transport])
  async function run(action: 'read' | 'request' | 'configure' | 'test', value = false) {
    if (!enabled || busy.current || (action === 'test' && !agreed)) return
    busy.current = true
    setPending(true)
    setError('')
    const generation = ++epoch.current
    if (action === 'test') setAgreed(false)
    try {
      const raw =
        action === 'configure' ? await transport.configure(value) : await transport[action]()
      const next = readStorageNotifications(raw)
      if (generation !== epoch.current) return
      if (action === 'configure' && next.enabled !== value) throw Error('unverifiedResponse')
      setView(next)
    } catch (e) {
      if (generation === epoch.current)
        setError(notificationReason(String(e).replace(/^Error: /, '')))
    } finally {
      busy.current = false
      if (generation === epoch.current) setPending(false)
    }
  }
  const allowed = notificationAllowed(view?.permission ?? null)
  return (
    <section className="card" aria-label="低空间系统提醒">
      <div className="list-toolbar">
        <h2>低空间系统提醒</h2>
        <button
          className="button secondary compact"
          disabled={!enabled || pending}
          onClick={() => void run('read')}
        >
          刷新提醒状态
        </button>
      </div>
      <div className="storage-notification-body">
        <p className="muted">
          默认关闭。仅在 MooMole
          运行、定期空间观察已开启且最近样本可靠低于所选阈值时提醒。同一次低空间状态只提交一次，恢复到阈值以上
          1 GiB 后再触发；不处理文件。
        </p>
        {view && (
          <p role="status">
            {view.enabled ? '低空间系统提醒已开启' : '低空间系统提醒已关闭'} ·{' '}
            {view.permission?.authorization === 'notDetermined'
              ? '尚未申请系统通知权限'
              : allowed
                ? '系统已允许通知'
                : view.permission?.authorization === 'denied'
                  ? '系统已拒绝通知'
                  : '系统通知权限或显示方式待核对'}
          </p>
        )}
        {view?.enabled && !view.monitoringEnabled && (
          <p className="notice">
            定期空间观察当前关闭，自动提醒不会运行；请在上方单独保存观察设置。
          </p>
        )}
        {(error || view?.error) && (
          <p className="notice warning" role="alert">
            {error || notificationReason(view?.error)}
          </p>
        )}
        <div className="modal-actions">
          <button
            className="button secondary"
            disabled={
              !enabled || !view || pending || allowed || view.permission?.authorization === 'denied'
            }
            onClick={() => void run('request')}
          >
            申请系统通知权限
          </button>
          <button
            className="button secondary"
            disabled={!enabled || !view || pending || (!view.enabled && !allowed)}
            onClick={() => void run('configure', !view?.enabled)}
          >
            {view?.enabled ? '关闭低空间提醒' : '开启低空间提醒'}
          </button>
        </div>
        <p className="muted">
          申请权限不会开启提醒或定期采集。macOS 的专注模式、显示设置以及 App
          在前台等情况可能影响显示；“已提交”只表示系统接受请求。可在系统设置的“通知”中核对 MooMole。
        </p>
        <details>
          <summary>测试提醒与最近记录</summary>
          <label className="confirm-check">
            <input
              type="checkbox"
              checked={agreed}
              disabled={!enabled || !allowed || pending}
              onChange={(e) => setAgreed(e.target.checked)}
            />
            我确认向本机提交一条测试通知，不改变定期观察或提醒开关。
          </label>
          <button
            className="button secondary"
            disabled={!enabled || !allowed || !agreed || pending}
            onClick={() => void run('test')}
          >
            发送一条测试提醒
          </button>
          {view && !view.records.length && <p className="muted">暂无系统提醒记录。</p>}
          {view?.records.map((r) => (
            <article className="saved-plan-row" key={r.id}>
              <div>
                <strong>
                  {r.kind === 'test' ? '测试提醒' : '低空间提醒'} · {notificationState[r.state]}
                </strong>
                <small>{dateTime(r.createdAt)}</small>
                {r.availableBytes !== null && r.thresholdBytes !== null && (
                  <p>
                    当时可用空间 {bytes(r.availableBytes)} · 阈值 {bytes(r.thresholdBytes)}
                  </p>
                )}
                {r.error && <p>{notificationReason(r.error)}</p>}
              </div>
            </article>
          ))}
          <p className="muted">
            最多保留最近 50 条提交记录。中断或未知不会自动重发；关闭开关不会撤回已交给 macOS
            的通知。
          </p>
        </details>
      </div>
    </section>
  )
}
