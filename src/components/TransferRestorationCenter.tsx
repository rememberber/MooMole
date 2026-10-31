import { useEffect, useRef, useState } from 'react'
import { isTauri } from '@tauri-apps/api/core'
import { Modal } from './Modal'
import { bytes, dateTime } from '../lib/format'
import { useNow } from '../lib/use-now'
import {
  transferRestoration as api,
  transferRestoreState,
  type TransferRestoreTarget,
  type TransferRestoreDraft,
  type TransferRestoreRecord,
  type TransferRestoreView,
} from '../lib/transfer-restoration'
const empty: TransferRestoreView = { records: [], activeRequests: [] }
export function TransferRestorationCenter({
  target,
  onClose,
  onActiveChange,
  onOutcome,
  onHistory,
  historyVisible = false,
}: {
  target: TransferRestoreTarget | null
  onClose: () => void
  onActiveChange: (v: boolean) => void
  onOutcome: () => void
  onHistory: () => void
  historyVisible?: boolean
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
        if (next.activeRequests.length || pending) timer = setTimeout(() => void read(), 700)
      } catch {
        if (live && (pending || tracking)) timer = setTimeout(() => void read(), 1000)
      }
    }
    void read()
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [epoch, pending, historyVisible])
  useEffect(() => {
    onActiveChange(pending || view.activeRequests.length > 0)
  }, [pending, view.activeRequests, onActiveChange])
  return (
    <>
      {view.activeRequests.length > 0 && !target && (
        <section className="card" role="status">
          <p>跨磁盘恢复仍在进行，原副本保持。请在恢复记录中核对结果。</p>
          <button className="button secondary compact" onClick={onHistory}>
            查看跨磁盘恢复记录
          </button>
          <button
            className="button secondary compact"
            onClick={() => {
              for (const request of view.activeRequests)
                void api.cancel(request).catch(() => setEpoch((v) => v + 1))
            }}
          >
            停止后续恢复
          </button>
        </section>
      )}
      {target && (
        <Restore
          key={`${target.runId}:${target.itemId}`}
          target={target}
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
  target,
  onClose,
  onActiveChange,
  onOutcome,
}: {
  target: TransferRestoreTarget
  onClose: () => void
  onActiveChange: (v: boolean) => void
  onOutcome: () => void
}) {
  const [draft, setDraft] = useState<TransferRestoreDraft | null>(null),
    [pending, setPending] = useState<'checking' | 'restoring' | null>('checking'),
    [error, setError] = useState(''),
    [ack, setAck] = useState(false),
    [record, setRecord] = useState<TransferRestoreRecord | null>(null),
    [submitted, setSubmitted] = useState(false),
    [stopRequested, setStopRequested] = useState(false)
  const request = useRef<string | null>(null),
    owned = useRef<TransferRestoreDraft | null>(null),
    busy = useRef(false),
    now = useNow()
  useEffect(() => {
    let live = true
    const id = crypto.randomUUID()
    request.current = id
    onActiveChange(true)
    // StrictMode may dispose an effect before its first microtask. Do not
    // start two exclusive native prepare jobs for one visible confirmation.
    void Promise.resolve()
      .then(() => (live ? api.prepare(target, id) : null))
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
  }, [target, onActiveChange])
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
    onActiveChange(true)
    setError('')
    setStopRequested(false)
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
      title="从跨磁盘副本恢复"
      className="transfer-restoration-modal"
      onClose={pending ? undefined : onClose}
    >
      <h2>从跨磁盘副本恢复</h2>
      <p>把已校验的副本复制回原文件名，不覆盖已有文件。另一磁盘的副本保留，原废纸篓项目不改动。</p>
      <dl>
        <dt>恢复来源</dt>
        <dd className="mono full-path">{target.copyPath}</dd>
        <dt>恢复到</dt>
        <dd className="mono full-path">{target.originalPath}</dd>
        {draft && (
          <>
            <dt>文件逻辑大小</dt>
            <dd>{bytes(draft.bytes)}</dd>
          </>
        )}
      </dl>
      <details>
        <summary>恢复与取消边界</summary>
        <p>
          重新核对完整内容、元数据、原记录、保护和原位置目录身份。原目录缺失、同名冲突、磁盘或副本变化时保留；不自动新建目录、编号、覆盖或删除副本。
        </p>
        <p>
          取消停止后续复制，已经发布的恢复文件不会自动撤回。结果未知时请核对原位置、副本和临时路径，不要重复执行；原移动记录保持不变。
        </p>
      </details>
      {pending && (
        <p role="status">
          {pending === 'checking'
            ? '正在核对原跨磁盘记录与完整副本，尚未恢复…'
            : '正在复制、核验并恢复原位置，请等待逐项结果。'}
        </p>
      )}
      {record && <Result record={record} onError={setError} />}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {submitted && !pending && !record && (
        <p>恢复请求未返回完整结果，请在跨磁盘恢复记录中核对；不会自动重试。</p>
      )}
      {!submitted && draft && (
        <label className="confirm-check">
          <input
            type="checkbox"
            checked={ack}
            disabled={!!pending || expired}
            onChange={(e) => setAck(e.target.checked)}
          />
          我已核对两端位置，同意从副本恢复，并保留副本与原废纸篓项目。
        </label>
      )}
      {expired && !submitted && <p role="status">恢复确认已过期，请关闭后重新检查。</p>}
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
            {stopRequested ? '已请求停止' : '停止本次恢复'}
          </button>
        ) : (
          <button className="button secondary" onClick={onClose}>
            关闭恢复窗口
          </button>
        )}
        {draft && !submitted && (
          <button
            className="button primary"
            disabled={!!pending || expired || !ack}
            onClick={() => void execute()}
          >
            确认恢复并保留副本
          </button>
        )}
      </div>
    </Modal>
  )
}
function Result({
  record: r,
  onError,
}: {
  record: TransferRestoreRecord
  onError: (s: string) => void
}) {
  return (
    <section aria-label="跨磁盘恢复结果">
      <strong>{transferRestoreState[r.state]}</strong>
      <p>{r.reason}</p>
      <div className="modal-actions">
        <button
          className="button secondary compact"
          onClick={() => void api.reveal(r.draft.originalPath).catch((e) => onError(String(e)))}
        >
          查看恢复原位置
        </button>
        <button
          className="button secondary compact"
          onClick={() => void api.reveal(r.draft.copyPath).catch((e) => onError(String(e)))}
        >
          查看保留副本
        </button>
      </div>
      {['unknown', 'interrupted'].includes(r.state) && (
        <p className="mono full-path">{r.temporaryPath}</p>
      )}
    </section>
  )
}
export function TransferRestorationHistory() {
  const [view, setView] = useState(empty),
    [error, setError] = useState(''),
    [selected, setSelected] = useState<TransferRestoreRecord | null>(null),
    [ack, setAck] = useState(false),
    [saving, setSaving] = useState(false),
    [epoch, setEpoch] = useState(0)
  useEffect(() => {
    if (!isTauri()) return
    let live = true,
      timer: ReturnType<typeof setTimeout> | undefined
    async function load() {
      try {
        const next = await api.read()
        if (!live) return
        setView(next)
        setSelected((old) => (old ? (next.records.find((r) => r.id === old.id) ?? old) : null))
        if (next.activeRequests.length) timer = setTimeout(() => void load(), 700)
      } catch (e) {
        if (live) setError(String(e))
      }
    }
    void load()
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [epoch])
  return (
    <section className="card" aria-label="跨磁盘恢复记录">
      <div className="list-toolbar">
        <h2>跨磁盘恢复记录</h2>
        <button className="button secondary compact" onClick={() => setEpoch((v) => v + 1)}>
          刷新恢复记录
        </button>
      </div>
      {view.records.length === 0 && !error && <p className="muted">尚无跨磁盘恢复记录。</p>}
      {view.records.map((r) => (
        <button
          className="saved-plan-row"
          key={r.id}
          onClick={() => {
            setSelected(r)
            setAck(false)
          }}
        >
          <span>
            <strong>{transferRestoreState[r.state]}</strong>
            <small data-i18n="off">{r.draft.originalPath}</small>
            <small>{dateTime(r.createdAt)}</small>
          </span>
        </button>
      ))}
      {error && !selected && (
        <p role="alert" className="notice warning">
          {error}
        </p>
      )}
      {selected && (
        <Modal
          title="跨磁盘恢复记录"
          className="transfer-restoration-modal"
          onClose={saving ? undefined : () => setSelected(null)}
        >
          <h2>跨磁盘恢复记录</h2>
          <p className="mono full-path">{selected.draft.originalPath}</p>
          <Result record={selected} onError={setError} />
          {error && (
            <p role="alert" className="notice warning">
              {error}
            </p>
          )}
          {view.activeRequests.includes(selected.requestId) && (
            <button
              className="button secondary"
              onClick={() => void api.cancel(selected.requestId).catch((e) => setError(String(e)))}
            >
              停止本次恢复
            </button>
          )}
          {['unknown', 'interrupted'].includes(selected.state) && selected.reviewedAt === null && (
            <>
              <p>先检查原位置、副本和临时路径。记录核对不会补做恢复、删除文件或更改原结果。</p>
              <label className="confirm-check">
                <input
                  type="checkbox"
                  checked={ack}
                  disabled={saving}
                  onChange={(e) => setAck(e.target.checked)}
                />
                我已核对实际文件，了解此操作只记录核查。
              </label>
              <button
                className="button secondary"
                disabled={!ack || saving}
                onClick={() => {
                  setSaving(true)
                  void api
                    .acknowledge(selected.id)
                    .then(() => {
                      setEpoch((v) => v + 1)
                      setAck(false)
                    })
                    .catch((e) => setError(String(e)))
                    .finally(() => setSaving(false))
                }}
              >
                记录恢复已核对
              </button>
            </>
          )}
        </Modal>
      )}
    </section>
  )
}
