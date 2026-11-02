import { useEffect, useRef, useState } from 'react'
import { Modal } from './Modal'
import { useNow } from '../lib/use-now'
import { bytes, dateTime } from '../lib/format'
import type { UninstallTarget } from '../lib/app-uninstall'
import {
  applicationUpdate as api,
  appUpdateState,
  type UpdateDraft,
  type AppUpdateRecord,
} from '../lib/application-update'
import { retainedApplication, type RetainedAppView } from '../lib/retained-application'
import { RetainedApplicationCenter, RetainedApplicationHistory } from './RetainedApplicationCenter'
import { applicationRestoration, type AppRestoreView } from '../lib/application-restoration'
import {
  ApplicationRestorationCenter,
  ApplicationRestorationHistory,
} from './ApplicationRestorationCenter'

export function ApplicationUpdateCenter({
  target,
  historyOpen,
  onClose,
  onHistoryClose,
  onActiveChange,
  onOutcome,
  canRestore = true,
}: {
  target: UninstallTarget | null
  historyOpen: boolean
  onClose: () => void
  onHistoryClose: () => void
  onActiveChange: (v: boolean) => void
  onOutcome: () => void
  canRestore?: boolean
}) {
  const [restore, setRestore] = useState<AppUpdateRecord | null>(null),
    [retire, setRetire] = useState<AppUpdateRecord | null>(null),
    [retireActive, setRetireActive] = useState(false),
    [updateActive, setUpdateActive] = useState(false),
    [restoreActive, setRestoreActive] = useState(false),
    [localHistory, setLocalHistory] = useState(false)
  useEffect(
    () => onActiveChange(updateActive || restoreActive || retireActive),
    [updateActive, restoreActive, retireActive, onActiveChange],
  )
  return (
    <>
      <RetainedApplicationCenter
        source={retire}
        historyVisible={historyOpen || localHistory}
        onClose={() => setRetire(null)}
        onActiveChange={setRetireActive}
        onOutcome={onOutcome}
        onHistory={() => setLocalHistory(true)}
      />
      <ApplicationRestorationCenter
        source={restore}
        historyVisible={historyOpen || localHistory}
        onClose={() => setRestore(null)}
        onActiveChange={setRestoreActive}
        onOutcome={onOutcome}
        onHistory={() => setLocalHistory(true)}
      />
      {target && (
        <Update
          key={`${target.taskId}:${target.index}`}
          target={target}
          onClose={onClose}
          onActiveChange={setUpdateActive}
          onOutcome={onOutcome}
        />
      )}
      {(historyOpen || localHistory) && !target && !restore && !retire && (
        <History
          canRestore={canRestore && !updateActive && !restoreActive && !retireActive}
          onRestore={setRestore}
          onRetire={setRetire}
          onClose={() => {
            setLocalHistory(false)
            onHistoryClose()
          }}
        />
      )}
    </>
  )
}
function Update({
  target,
  onClose,
  onActiveChange,
  onOutcome,
}: {
  target: UninstallTarget
  onClose: () => void
  onActiveChange: (v: boolean) => void
  onOutcome: () => void
}) {
  const [draft, setDraft] = useState<UpdateDraft | null>(null),
    [pending, setPending] = useState<'checking' | 'updating' | null>(null),
    [ack, setAck] = useState(false),
    [error, setError] = useState(''),
    [record, setRecord] = useState<AppUpdateRecord | null>(null),
    [submitted, setSubmitted] = useState(false),
    [cancelRequested, setCancelRequested] = useState(false),
    [canCancel, setCanCancel] = useState(false)
  const busy = useRef(false),
    request = useRef<string | null>(null),
    owned = useRef<UpdateDraft | null>(null),
    alive = useRef(true)
  const now = useNow()
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
      if (owned.current) void api.discard(owned.current.id).catch(() => {})
    }
  }, [])
  function revoke() {
    const old = owned.current
    owned.current = null
    setDraft(null)
    setAck(false)
    if (old) void api.discard(old.id).catch(() => {})
  }
  async function choose() {
    if (busy.current) return
    busy.current = true
    setError('')
    revoke()
    // Treat the system chooser as active too, so another task cannot start while
    // this selection is being made.
    setPending('checking')
    onActiveChange(true)
    setCancelRequested(false)
    try {
      const path = await api.choose()
      if (typeof path !== 'string' || !alive.current) return
      const id = crypto.randomUUID()
      request.current = id
      setCanCancel(true)
      const next = await api.prepare(target, path, id)
      if (!alive.current) {
        void api.discard(next.id).catch(() => {})
        return
      }
      owned.current = next
      setDraft(next)
    } catch (e) {
      if (alive.current) setError(String(e).replace(/^Error: /, ''))
    } finally {
      request.current = null
      setCanCancel(false)
      busy.current = false
      if (alive.current) {
        setPending(null)
        onActiveChange(false)
      }
    }
  }
  async function update() {
    if (
      busy.current ||
      !draft ||
      !ack ||
      submitted ||
      now >= draft.expiresAt ||
      now < draft.createdAt
    )
      return
    busy.current = true
    setSubmitted(true)
    setPending('updating')
    setCancelRequested(false)
    setError('')
    onActiveChange(true)
    const id = crypto.randomUUID()
    request.current = id
    setCanCancel(true)
    owned.current = null
    try {
      const result = await api.execute(draft, id)
      if (alive.current) setRecord(result)
    } catch (e) {
      if (alive.current) setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      request.current = null
      setCanCancel(false)
      if (alive.current) {
        setPending(null)
        onActiveChange(false)
        onOutcome()
      }
    }
  }
  const expired = !!draft && (now >= draft.expiresAt || now < draft.createdAt)
  return (
    <Modal
      title="更新此应用"
      className="application-update-modal"
      onClose={pending ? undefined : onClose}
    >
      <h2>更新此应用</h2>
      <strong data-i18n="off">{target.name}</strong>
      <p className="mono full-path">{target.path}</p>
      <p>选择从可信来源下载并解压的新版 .app。请先自行退出应用；核对完成后才会替换。</p>
      <details>
        <summary>更新范围与恢复边界</summary>
        <p>
          仅替换此安装位置的完整应用包。旧包保留在独立位置，不修改应用数据，不自动打开应用。有可靠双端收据时可在更新记录中另行确认恢复旧版；新版写入的数据可能不兼容旧版。
        </p>
        <p>
          本轮支持当前账户拥有、未发现运行进程的普通应用与 APFS 安装卷。App
          Store、Homebrew、后台或特权组件请使用原有管理工具。不会绕过 macOS 安全检查或自动删除旧包。
        </p>
      </details>
      {draft && !submitted && (
        <section aria-label="应用更新核对">
          <dl className="application-update-facts">
            <dt>版本</dt>
            <dd data-i18n="off">
              {draft.oldVersion} → {draft.newVersion}
            </dd>
            <dt>新版位置</dt>
            <dd className="mono full-path">{draft.replacement}</dd>
            <dt>新版逻辑大小</dt>
            <dd>{bytes(draft.bytes)}</dd>
            <dt>签名发布者</dt>
            <dd>
              {draft.publisher === 'adHoc' ? (
                '临时签名，无法确认发布者身份'
              ) : (
                <span data-i18n="off">{draft.publisher}</span>
              )}
            </dd>
          </dl>
          <p className="notice">
            签名完整性核对不等于 Apple 公证或来源可信。首次打开仍可能需要系统确认。
          </p>
          <label className="application-update-consent">
            <input
              type="checkbox"
              checked={ack}
              disabled={!!pending || expired}
              onChange={(e) => setAck(e.target.checked)}
            />
            我已核对新版来源，同意替换应用并保留旧包，了解应用数据不随旧包恢复。
          </label>
          {expired && <p role="status">本次核对已过期，请重新选择新版。</p>}
        </section>
      )}
      {pending && (
        <p role="status">
          {pending === 'checking'
            ? '正在核对身份、版本、签名与内容，尚未替换。'
            : '正在复制、复查并替换；系统调用可能需要等待。停止不能撤销已经完成的替换。'}
        </p>
      )}
      {record && (
        <section aria-label="应用更新结果">
          <strong>{appUpdateState[record.state]}</strong>
          <p>{record.reason}</p>
          <p className="mono full-path">{record.backup}</p>
          <p>请从更新记录核对安装位置与旧包。这里保留的是应用包，不是应用数据备份。</p>
        </section>
      )}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {submitted && !pending && !record && (
        <p>请求结果未完整返回，请在应用更新记录中核对；不会自动重复执行。</p>
      )}
      <div className="modal-actions">
        {pending ? (
          <button
            className="button secondary"
            disabled={cancelRequested || !canCancel}
            onClick={() => {
              if (request.current) {
                setCancelRequested(true)
                void api.cancel(request.current).catch((e) => {
                  setError(String(e))
                  setCancelRequested(false)
                })
              }
            }}
          >
            {cancelRequested ? '已请求停止' : '停止更新'}
          </button>
        ) : (
          <button className="button secondary" onClick={onClose}>
            返回应用列表
          </button>
        )}
        {!submitted && !pending && (
          <button className="button secondary" onClick={() => void choose()}>
            {draft ? '重新选择新版…' : '选择新版应用…'}
          </button>
        )}
        {!submitted && draft && (
          <button
            className="button primary"
            disabled={!!pending || !ack || expired}
            onClick={() => void update()}
          >
            确认替换并保留旧版
          </button>
        )}
      </div>
    </Modal>
  )
}
function History({
  onClose,
  onRestore,
  onRetire,
  canRestore,
}: {
  onClose: () => void
  onRestore: (r: AppUpdateRecord) => void
  onRetire: (r: AppUpdateRecord) => void
  canRestore: boolean
}) {
  const [rows, setRows] = useState<AppUpdateRecord[]>([]),
    [error, setError] = useState(''),
    [pending, setPending] = useState(false),
    [checked, setChecked] = useState(new Set<string>())
  const [restorations, setRestorations] = useState<AppRestoreView>({
    records: [],
    activeRequests: [],
  })
  const [retained, setRetained] = useState<RetainedAppView>({ records: [], activeRequests: [] })
  const generation = useRef(0)
  async function load() {
    const token = ++generation.current
    setPending(true)
    setError('')
    try {
      const [next, restored, retired] = await Promise.all([
        api.list(),
        applicationRestoration.read(),
        retainedApplication.read(),
      ])
      if (token === generation.current) {
        setRows(next)
        setRestorations(restored)
        setRetained(retired)
      }
    } catch (e) {
      if (token === generation.current) setError(String(e))
    } finally {
      if (token === generation.current) setPending(false)
    }
  }
  useEffect(() => {
    void load()
    return () => {
      generation.current++
    }
  }, [])
  useEffect(() => {
    if (!restorations.activeRequests.length && !retained.activeRequests.length) return
    const timer = setTimeout(() => void load(), 700)
    return () => clearTimeout(timer)
  }, [restorations, retained])
  return (
    <Modal
      title="应用更新记录"
      className="application-update-modal"
      onClose={pending ? undefined : onClose}
    >
      <h2>应用更新记录</h2>
      <p>旧包与未完成的暂存包不会自动删除。请先核对实际位置，再决定如何恢复或保留。</p>
      {error && (
        <p role="alert" className="notice warning">
          {error}
        </p>
      )}
      {rows.map((r) => (
        <article key={r.id} className="application-update-record">
          <strong data-i18n="off">
            {r.draft.name} · {r.draft.oldVersion} → {r.draft.newVersion}
          </strong>
          <p>
            {appUpdateState[r.state]} · {dateTime(r.createdAt)}
          </p>
          <p>{r.reason}</p>
          <p className="mono full-path">{r.draft.path}</p>
          <p className="mono full-path">{r.backup}</p>
          <div className="modal-actions">
            {r.state === 'updated' && r.retainedCopyAvailable && (
              <button
                className="button secondary compact"
                disabled={
                  pending ||
                  !!error ||
                  !canRestore ||
                  restorations.records.some(
                    (x) =>
                      x.state === 'running' ||
                      (['unknown', 'interrupted'].includes(x.state) && x.reviewedAt === null),
                  ) ||
                  retained.records.some(
                    (x) =>
                      x.state === 'running' || (x.state === 'unknown' && x.reviewedAt === null),
                  )
                }
                onClick={() => onRetire(r)}
              >
                检查保留版本与占用…
              </button>
            )}
            {r.state === 'updated' && r.recoveryAvailable && (
              <button
                className="button secondary compact"
                disabled={
                  pending ||
                  !!error ||
                  !canRestore ||
                  restorations.records.some(
                    (x) => x.draft.sourceId === r.id && x.state === 'restored',
                  ) ||
                  restorations.records.some(
                    (x) =>
                      x.state === 'running' ||
                      (['unknown', 'interrupted'].includes(x.state) && x.reviewedAt === null),
                  )
                }
                onClick={() => onRestore(r)}
              >
                检查并恢复此前版本…
              </button>
            )}
            <button
              className="button secondary compact"
              disabled={pending}
              onClick={() => void api.reveal(r.draft.path).catch((e) => setError(String(e)))}
            >
              显示安装位置
            </button>
            <button
              className="button secondary compact"
              disabled={pending}
              onClick={() =>
                void api
                  .reveal(r.backup.slice(0, r.backup.lastIndexOf('/')))
                  .catch((e) => setError(String(e)))
              }
            >
              显示旧版或暂存目录
            </button>
          </div>
          {['unknown', 'interrupted'].includes(r.state) && r.reviewedAt === null && (
            <>
              <label className="application-update-consent">
                <input
                  type="checkbox"
                  checked={checked.has(r.id)}
                  onChange={(e) =>
                    setChecked((old) => {
                      const next = new Set(old)
                      if (e.target.checked) next.add(r.id)
                      else next.delete(r.id)
                      return next
                    })
                  }
                />
                我已检查安装位置、旧版或暂存目录，了解此确认不会修复或回滚。
              </label>
              <button
                className="button secondary compact"
                disabled={pending || !checked.has(r.id)}
                onClick={() => {
                  setPending(true)
                  void api
                    .acknowledge(r.id)
                    .then(load)
                    .catch((e) => {
                      setError(String(e))
                      setPending(false)
                    })
                }}
              >
                记录已人工核对
              </button>
            </>
          )}
        </article>
      ))}
      {!pending && !rows.length && !error && <p>暂无应用更新记录。</p>}
      <ApplicationRestorationHistory view={restorations} pending={pending} onRefresh={load} />
      <RetainedApplicationHistory view={retained} pending={pending} onRefresh={load} />
      <div className="modal-actions">
        <button className="button secondary" disabled={pending} onClick={() => void load()}>
          刷新记录
        </button>
      </div>
    </Modal>
  )
}
