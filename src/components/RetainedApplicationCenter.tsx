import { useEffect, useRef, useState } from 'react'
import { isTauri } from '@tauri-apps/api/core'
import { Modal } from './Modal'
import { bytes, dateTime } from '../lib/format'
import { useNow } from '../lib/use-now'
import type { AppUpdateRecord } from '../lib/application-update'
import {
  retainedApplication as api,
  retainedAppState,
  type RetainedAppDraft,
  type RetainedAppRecord,
  type RetainedAppView,
} from '../lib/retained-application'
const empty: RetainedAppView = { records: [], activeRequests: [] }
export function RetainedApplicationCenter({
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
          <p>保留版本整理仍在进行，请查看处理记录。</p>
          <button className="button secondary compact" onClick={onHistory}>
            查看保留版本整理记录
          </button>
          <button
            className="button secondary compact"
            onClick={() => {
              for (const request of view.activeRequests)
                void api.cancel(request).catch(() => setEpoch((v) => v + 1))
            }}
          >
            停止保留版本整理
          </button>
        </section>
      )}
      {source && (
        <Retire
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
function Retire({
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
  const [draft, setDraft] = useState<RetainedAppDraft | null>(null),
    [pending, setPending] = useState<'checking' | 'moving' | null>('checking'),
    [ack, setAck] = useState(false),
    [error, setError] = useState(''),
    [record, setRecord] = useState<RetainedAppRecord | null>(null),
    [submitted, setSubmitted] = useState(false),
    [stopping, setStopping] = useState(false)
  const request = useRef<string | null>(null),
    owned = useRef<RetainedAppDraft | null>(null),
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
  const expired = !!draft && (now < draft.createdAt || now >= draft.expiresAt)
  async function execute() {
    if (
      busy.current ||
      !draft ||
      !ack ||
      submitted ||
      Date.now() < draft.createdAt ||
      Date.now() >= draft.expiresAt
    )
      return
    busy.current = true
    setSubmitted(true)
    setPending('moving')
    setError('')
    setStopping(false)
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
  return (
    <Modal
      title="整理保留的应用版本"
      className="application-update-modal"
      onClose={pending ? undefined : onClose}
    >
      <h2>整理保留的应用版本</h2>
      <strong data-i18n="off">{source.draft.name}</strong>
      {draft && (
        <>
          <dl className="application-update-facts">
            <dt>当前安装版本</dt>
            <dd data-i18n="off">{draft.installedVersion}</dd>
            <dt>待整理的保留版本</dt>
            <dd data-i18n="off">{draft.retainedVersion}</dd>
            <dt>保留包逻辑大小</dt>
            <dd>{bytes(draft.bytes)}</dd>
            <dt>安装位置</dt>
            <dd className="mono full-path">{draft.installed}</dd>
            <dt>待处理的保留包</dt>
            <dd className="mono full-path">{draft.retained}</dd>
          </dl>
          <p>
            {draft.afterRestore
              ? '此前已恢复旧版，这里保留的是当时的新版包。'
              : '更新后保留的旧版包可用于恢复此前版本。'}
          </p>
        </>
      )}
      <p className="notice warning">
        保留包将移入废纸篓，此更新记录不能再用于恢复或重复整理。请先确认当前版本可用，并自行备份重要数据。
      </p>
      <p>
        只处理所列保留包；当前安装包和包外数据保留。专用目录为空且身份一致时一并移除。移入废纸篓不会立即释放磁盘空间。
      </p>
      <details>
        <summary>保留版本整理与停止边界</summary>
        <p>
          再次核对更新与恢复收据、两份包的内容、签名、运行状态和保护设置。未知结果、身份变化或目录中有其他内容时保留，不按名称认领旧包。
        </p>
        <p>
          系统移入前停止会保留原包；移入后停止不会自动撤回。结果不明时检查安装位置、保留目录和废纸篓，不会自动重试。
        </p>
      </details>
      {pending && (
        <p role="status">
          {pending === 'checking'
            ? '正在核对保留版本与占用，尚未处理文件。'
            : '正在复查并移入废纸篓，请等待实际结果。'}
        </p>
      )}
      {record && <Outcome record={record} onError={setError} />}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {submitted && !pending && !record && (
        <p>保留版本整理未返回完整结果，请查看处理记录并核查实际位置；不会自动重试。</p>
      )}
      {draft && !submitted && (
        <label className="application-update-consent">
          <input
            type="checkbox"
            checked={ack}
            disabled={!!pending || expired}
            onChange={(e) => setAck(e.target.checked)}
          />
          我已核对当前和保留版本，同意仅将所列保留包移入废纸篓，了解原恢复副本将不再可用。
        </label>
      )}
      {expired && !submitted && <p role="status">保留版本确认已过期，请返回后重新检查。</p>}
      <div className="modal-actions">
        {pending ? (
          <button
            className="button secondary"
            disabled={stopping}
            onClick={() => {
              if (request.current) {
                setStopping(true)
                void api.cancel(request.current).catch((e) => {
                  setError(String(e))
                  setStopping(false)
                })
              }
            }}
          >
            {stopping ? '已请求停止' : '停止保留版本整理'}
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
            确认将保留版本移入废纸篓
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
  record: RetainedAppRecord
  onError: (s: string) => void
}) {
  return (
    <section aria-label="保留版本整理结果">
      <strong>{retainedAppState[r.state]}</strong>
      <p>{r.reason}</p>
      <p className="mono full-path">{r.draft.installed}</p>
      <p className="mono full-path">{r.draft.retained}</p>
      <p>
        {r.stageRemoved
          ? '空的专用保留目录已移除。'
          : '专用保留目录的移除未获确认，请检查实际状态。'}
      </p>
      <div className="modal-actions">
        <button
          className="button secondary compact"
          onClick={() => void api.reveal(r.draft.installed).catch((e) => onError(String(e)))}
        >
          显示安装位置
        </button>
        <button
          className="button secondary compact"
          onClick={() => void api.reveal(r.draft.stage).catch((e) => onError(String(e)))}
        >
          显示保留目录
        </button>
        {r.receipt && (
          <button
            className="button secondary compact"
            onClick={() => void api.reveal(r.receipt!.destination).catch((e) => onError(String(e)))}
          >
            显示实际废纸篓位置
          </button>
        )}
      </div>
    </section>
  )
}
export function RetainedApplicationHistory({
  view,
  pending,
  onRefresh,
}: {
  view: RetainedAppView
  pending: boolean
  onRefresh: () => Promise<void>
}) {
  const [selected, setSelected] = useState<string | null>(null),
    [ack, setAck] = useState(false),
    [saving, setSaving] = useState(false),
    [error, setError] = useState('')
  return (
    <section aria-label="保留版本整理记录">
      <h3>保留版本整理记录</h3>
      {!view.records.length && <p className="muted">暂无保留版本整理记录。</p>}
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
              {r.draft.name} · {r.draft.retainedVersion}
            </span>{' '}
            · {retainedAppState[r.state]} · {dateTime(r.createdAt)}
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
                  停止保留版本整理
                </button>
              )}
              {r.state === 'unknown' && r.reviewedAt === null && (
                <>
                  <p>
                    请先核对安装位置、保留目录及实际废纸篓位置。记录核查不改变原结果，也不恢复原收据的操作权限。
                  </p>
                  <label className="application-update-consent">
                    <input
                      type="checkbox"
                      checked={ack}
                      disabled={pending || saving}
                      onChange={(e) => setAck(e.target.checked)}
                    />
                    我已检查实际位置，了解核查不会补做处理或恢复文件。
                  </label>
                  <button
                    className="button secondary compact"
                    disabled={pending || saving || !ack}
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
                    记录已人工核对
                  </button>
                </>
              )}
              {error && (
                <p role="alert" className="notice warning">
                  {error}
                </p>
              )}
            </>
          )}
        </article>
      ))}
    </section>
  )
}
