import { useEffect, useRef, useState } from 'react'
import { isTauri } from '@tauri-apps/api/core'
import { Modal } from './Modal'
import { bytes, dateTime } from '../lib/format'
import { useNow } from '../lib/use-now'
import type { AppUpdateRecord } from '../lib/application-update'
import {
  applicationRestoration as api,
  appRestoreState,
  type AppRestoreDraft,
  type AppRestoreRecord,
  type AppRestoreView,
} from '../lib/application-restoration'
const empty: AppRestoreView = { records: [], activeRequests: [] }
export function ApplicationRestorationCenter({
  source,
  historyVisible,
  onClose,
  onActiveChange,
  onOutcome,
  onHistory,
}: {
  source: AppUpdateRecord | null
  historyVisible: boolean
  onClose: () => void
  onActiveChange: (v: boolean) => void
  onOutcome: () => void
  onHistory: () => void
}) {
  const [pending, setPending] = useState(false),
    [view, setView] = useState(empty),
    [epoch, setEpoch] = useState(0)
  useEffect(() => {
    if (!isTauri()) return
    let live = true,
      timer: ReturnType<typeof setTimeout> | undefined,
      tracking = false
    async function read() {
      try {
        const next = await api.read()
        if (!live) return
        setView(next)
        tracking = next.activeRequests.length > 0
        if (pending || tracking) timer = setTimeout(() => void read(), 700)
      } catch {
        if (live && (pending || tracking)) timer = setTimeout(() => void read(), 1000)
      }
    }
    void read()
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [pending, epoch, historyVisible])
  useEffect(
    () => onActiveChange(pending || view.activeRequests.length > 0),
    [pending, view.activeRequests, onActiveChange],
  )
  return (
    <>
      {view.activeRequests.length > 0 && !source && (
        <section className="card" role="status">
          <p>旧版应用恢复仍在进行，请在更新记录中核对两份应用。</p>
          <button className="button secondary compact" onClick={onHistory}>
            查看应用恢复记录
          </button>
          <button
            className="button secondary compact"
            onClick={() => {
              for (const request of view.activeRequests)
                void api.cancel(request).catch(() => setEpoch((v) => v + 1))
            }}
          >
            停止旧版恢复
          </button>
        </section>
      )}
      {source && (
        <Restore
          key={source.id}
          source={source}
          onClose={onClose}
          onActiveChange={setPending}
          onOutcome={() => {
            setEpoch((v) => v + 1)
            onOutcome()
          }}
        />
      )}
    </>
  )
}
function Restore({
  source,
  onClose,
  onActiveChange,
  onOutcome,
}: {
  source: AppUpdateRecord
  onClose: () => void
  onActiveChange: (v: boolean) => void
  onOutcome: () => void
}) {
  const [draft, setDraft] = useState<AppRestoreDraft | null>(null),
    [pending, setPending] = useState<'checking' | 'restoring' | null>('checking'),
    [error, setError] = useState(''),
    [ack, setAck] = useState(false),
    [submitted, setSubmitted] = useState(false),
    [record, setRecord] = useState<AppRestoreRecord | null>(null),
    [stopRequested, setStopRequested] = useState(false)
  const request = useRef<string | null>(null),
    owned = useRef<AppRestoreDraft | null>(null),
    busy = useRef(false),
    now = useNow()
  useEffect(() => {
    let live = true
    const id = crypto.randomUUID()
    request.current = id
    onActiveChange(true)
    void Promise.resolve()
      .then(() => (live ? api.prepare(source, id) : null))
      .then((d) => {
        if (!d) return
        if (live) {
          owned.current = d
          setDraft(d)
        } else void api.discard(d.id).catch(() => {})
      })
      .catch((e) => {
        if (live) setError(String(e).replace(/^Error: /, ''))
      })
      .finally(() => {
        if (live) {
          setPending(null)
          request.current = null
          onActiveChange(false)
        }
      })
    return () => {
      live = false
      if (owned.current) void api.discard(owned.current.id).catch(() => {})
    }
  }, [source, onActiveChange])
  async function execute() {
    if (
      busy.current ||
      !draft ||
      !ack ||
      submitted ||
      now < draft.createdAt ||
      now >= draft.expiresAt
    )
      return
    busy.current = true
    setSubmitted(true)
    setPending('restoring')
    setError('')
    setStopRequested(false)
    onActiveChange(true)
    owned.current = null
    const id = crypto.randomUUID()
    request.current = id
    try {
      setRecord(await api.execute(draft, id))
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    } finally {
      setPending(null)
      request.current = null
      onActiveChange(false)
      onOutcome()
      busy.current = false
    }
  }
  const expired = !!draft && (now < draft.createdAt || now >= draft.expiresAt)
  return (
    <Modal
      title="恢复此前应用版本"
      className="application-update-modal"
      onClose={pending ? undefined : onClose}
    >
      <h2>恢复此前应用版本</h2>
      <strong data-i18n="off">{source.draft.name}</strong>
      <dl className="application-update-facts">
        <dt>版本</dt>
        <dd data-i18n="off">
          {source.draft.newVersion} → {source.draft.oldVersion}
        </dd>
        <dt>安装位置</dt>
        <dd className="mono full-path">{source.draft.path}</dd>
        <dt>旧版来源 / 新版保留位置</dt>
        <dd className="mono full-path">{source.backup}</dd>
        {draft && (
          <>
            <dt>旧版逻辑大小</dt>
            <dd>{bytes(draft.bytes)}</dd>
          </>
        )}
      </dl>
      <p className="notice warning">
        只恢复应用包，应用数据不回退。新版修改过的文档、偏好或数据库可能不兼容旧版，请先自行备份重要数据并正常退出应用。
      </p>
      <p>
        恢复时交换两份完整应用：旧版回到安装位置，新版留在原旧版目录。不会删除任何一份、自动退出或打开应用。
      </p>
      <details>
        <summary>旧版恢复与取消边界</summary>
        <p>
          再次核对原更新收据、完整内容、签名、运行状态和当前保护。缺少收据、版本或身份变化时保留；旧历史不能通过重新观察现有包来补造恢复权限。
        </p>
        <p>
          交换前取消停止后续操作；交换后取消不会自动撤回。结果未知时请核对两端，不重复执行或自动反向交换。
        </p>
        <p>临时签名只核对完整性，不能确认发布者；首次打开仍由 macOS 决定。</p>
      </details>
      {pending && (
        <p role="status">
          {pending === 'checking'
            ? '正在核对旧版恢复证据，尚未交换应用。'
            : '正在复查并交换应用包，请等待双端结果。'}
        </p>
      )}
      {record && <Outcome record={record} onError={setError} />}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {submitted && !pending && !record && (
        <p>旧版恢复请求未返回完整结果，请检查应用恢复记录；不会自动重试。</p>
      )}
      {draft && !submitted && (
        <label className="application-update-consent">
          <input
            type="checkbox"
            checked={ack}
            disabled={!!pending || expired}
            onChange={(e) => setAck(e.target.checked)}
          />
          我已核对两份应用并备份重要数据，同意恢复旧版且保留新版，了解数据不会回退。
        </label>
      )}
      {expired && !submitted && <p role="status">旧版恢复确认已过期，请关闭后重新检查。</p>}
      <div className="modal-actions">
        {pending ? (
          <button
            className="button secondary"
            disabled={stopRequested}
            onClick={() => {
              if (request.current) {
                setStopRequested(true)
                void api.cancel(request.current).catch((e) => {
                  setError(String(e))
                  setStopRequested(false)
                })
              }
            }}
          >
            {stopRequested ? '已请求停止' : '停止旧版恢复'}
          </button>
        ) : (
          <button className="button secondary" onClick={onClose}>
            返回更新记录
          </button>
        )}
        {draft && !submitted && (
          <button
            className="button primary"
            disabled={!!pending || !ack || expired}
            onClick={() => void execute()}
          >
            确认恢复旧版并保留新版
          </button>
        )}
      </div>
    </Modal>
  )
}
function Outcome({
  record: r,
  onError,
}: {
  record: AppRestoreRecord
  onError: (s: string) => void
}) {
  return (
    <section aria-label="应用旧版恢复结果">
      <strong>{appRestoreState[r.state]}</strong>
      <p>{r.reason}</p>
      <p className="mono full-path">{r.draft.path}</p>
      <p className="mono full-path">{r.draft.retained}</p>
      <div className="modal-actions">
        <button
          className="button secondary compact"
          onClick={() => void api.reveal(r.draft.path).catch((e) => onError(String(e)))}
        >
          显示安装位置
        </button>
        <button
          className="button secondary compact"
          onClick={() => void api.reveal(r.draft.retained).catch((e) => onError(String(e)))}
        >
          显示保留包
        </button>
      </div>
    </section>
  )
}
export function ApplicationRestorationHistory({
  view,
  pending,
  onRefresh,
}: {
  view: AppRestoreView
  pending: boolean
  onRefresh: () => Promise<void>
}) {
  const [selected, setSelected] = useState<string | null>(null),
    [ack, setAck] = useState(false),
    [error, setError] = useState(''),
    [saving, setSaving] = useState(false)
  return (
    <section aria-label="应用旧版恢复记录">
      <h3>旧版恢复记录</h3>
      {!view.records.length && <p className="muted">暂无旧版恢复记录。</p>}
      {view.records.map((r) => (
        <article className="application-update-record" key={r.id}>
          <button
            className="button text compact"
            onClick={() => {
              setSelected(selected === r.id ? null : r.id)
              setAck(false)
              setError('')
            }}
          >
            <span data-i18n="off">
              {r.draft.name} · {r.draft.fromVersion} → {r.draft.toVersion}
            </span>{' '}
            · {appRestoreState[r.state]} · {dateTime(r.createdAt)}
          </button>
          {selected === r.id && (
            <>
              <Outcome record={r} onError={setError} />
              {view.activeRequests.includes(r.requestId) && (
                <button
                  className="button secondary compact"
                  onClick={() =>
                    void api
                      .cancel(r.requestId)
                      .then(onRefresh)
                      .catch((e) => setError(String(e)))
                  }
                >
                  停止旧版恢复
                </button>
              )}
              {['unknown', 'interrupted'].includes(r.state) && r.reviewedAt === null && (
                <>
                  <p>先核对安装位置和保留包。记录核查不会补做恢复、删除文件或改变原结果。</p>
                  <label className="application-update-consent">
                    <input
                      type="checkbox"
                      checked={ack}
                      disabled={pending || saving}
                      onChange={(e) => setAck(e.target.checked)}
                    />
                    我已核对两份实际应用，了解这里只记录核查。
                  </label>
                  <button
                    className="button secondary compact"
                    disabled={!ack || saving || pending}
                    onClick={() => {
                      setSaving(true)
                      void api
                        .acknowledge(r.id)
                        .then(onRefresh)
                        .then(() => setAck(false))
                        .catch((e) => setError(String(e)))
                        .finally(() => setSaving(false))
                    }}
                  >
                    记录旧版恢复已核对
                  </button>
                </>
              )}
            </>
          )}
        </article>
      ))}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
    </section>
  )
}
