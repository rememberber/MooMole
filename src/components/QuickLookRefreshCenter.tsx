import { useEffect, useRef, useState } from 'react'
import { Modal } from './Modal'
import { useNow } from '../lib/use-now'
import { dateTime } from '../lib/format'
import { watchExecution } from '../lib/execution-watcher'
import { focusReturnTarget } from '../lib/focus-return'
import {
  refreshSteps,
  refreshSessionKey,
  refreshId,
  readRefreshDraft,
  readRefreshDetail,
  readRefreshSnapshot,
  readRefreshList,
  preserveRefresh,
  refreshStepName,
  refreshStateName,
  refreshReason,
  type RefreshStep,
  type RefreshDraft,
  type RefreshSnapshot,
  type RefreshSummary,
  type RefreshTransport,
} from '../lib/quicklook-refresh'

const message = (e: unknown) => String(e).replace(/^Error: /, '')
export function QuickLookRefreshCenter({
  openSignal,
  enabled,
  historyVisible,
  transport,
  onActiveChange,
  initialRecords,
}: {
  openSignal: number
  enabled: boolean
  historyVisible: boolean
  transport: RefreshTransport
  onActiveChange: (active: boolean) => void
  initialRecords?: unknown
}) {
  useNow()
  const [open, setOpen] = useState(false),
    [mode, setMode] = useState<'prepare' | 'result'>('prepare')
  const [selected, setSelected] = useState<RefreshStep[]>([...refreshSteps]),
    [draft, setDraft] = useState<RefreshDraft | null>(null)
  const [checking, setChecking] = useState(false),
    [ack, setAck] = useState(false),
    [error, setError] = useState('')
  const [trackingError, setTrackingError] = useState('')
  const [snapshot, setSnapshot] = useState<RefreshSnapshot | null>(null),
    [active, setActive] = useState(false)
  const [records, setRecords] = useState<RefreshSummary[]>(() => {
    try {
      return readRefreshList(initialRecords ?? [])
    } catch {
      return []
    }
  })
  const [request, setRequest] = useState<string | null>(() => {
    try {
      return enabled
        ? (records.find((row) => row.retainedForReview)?.id ??
            refreshId(sessionStorage.getItem(refreshSessionKey)))
        : null
    } catch {
      return null
    }
  })
  const [epoch, setEpoch] = useState(0),
    [historyError, setHistoryError] = useState('')
  const [cancelRequested, setCancelRequested] = useState(false),
    [reviewing, setReviewing] = useState(false),
    [reviewAck, setReviewAck] = useState(false)
  const current = useRef(request),
    latest = useRef(snapshot),
    submitting = useRef(false),
    generation = useRef(0),
    fixed = useRef<RefreshDraft | null>(null)
  const opener = useRef<HTMLElement | null>(null),
    alive = useRef(true),
    observed = useRef(false)
  const currentSnapshot = () => latest.current
  const expected = useRef<{ planId: string; selected: RefreshStep[] } | null>(null)
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
      generation.current++
      if (fixed.current) void transport.discard(fixed.current.id).catch(() => {})
    }
  }, [transport])
  useEffect(() => {
    onActiveChange(active)
  }, [active, onActiveChange])
  function apply(next: RefreshSnapshot) {
    if (!alive.current || next.requestId !== current.current) return
    if (
      next.record &&
      expected.current &&
      (next.record.planId !== expected.current.planId ||
        JSON.stringify(next.record.items.map((i) => i.step)) !==
          JSON.stringify(expected.current.selected))
    )
      throw new Error('维护结果不属于原始计划或选择。')
    const value = preserveRefresh(latest.current, next)
    latest.current = value
    setSnapshot(value)
    setActive(value.active)
    if (!value.active && value.record && value.record.state !== 'running') {
      setError('')
      try {
        if (sessionStorage.getItem(refreshSessionKey) === value.requestId)
          sessionStorage.removeItem(refreshSessionKey)
      } catch {
        /* observation is still available in history */
      }
    }
  }
  useEffect(() => {
    if (!enabled || !request) return
    const controller = new AbortController()
    return watchExecution({
      signal: controller.signal,
      load: async () => readRefreshSnapshot(await transport.read(request), request),
      onError: setTrackingError,
      onValue: (value) => {
        if (value.active || value.record) observed.current = true
        if (!value.active && !value.record && !observed.current && submitting.current) return
        apply(value)
        if (!value.active && !value.record)
          setError('尚未读到此请求的维护记录。保留请求标识，不会自动重试。')
      },
    })
  }, [enabled, request, epoch, transport])
  async function loadHistory() {
    try {
      const next = readRefreshList(await transport.list())
      if (!alive.current) return
      const known = latest.current?.record
      setRecords((previous) => {
        const rows = next.map((row) => {
          const p = previous.find((p) => p.id === row.id)
          return p &&
            ((p.state !== 'running' && row.state === 'running') ||
              p.succeededCount > row.succeededCount ||
              (p.reviewedAt !== null && row.reviewedAt === null))
            ? p
            : row
        })
        if (known && !rows.some((row) => row.id === known.id)) rows.unshift(known)
        return rows
      })
      setHistoryError('')
    } catch (e) {
      if (alive.current) setHistoryError(message(e))
    }
  }
  useEffect(() => {
    if (enabled && historyVisible) void loadHistory()
  }, [enabled, historyVisible, snapshot?.record?.state, snapshot?.record?.reviewedAt])
  useEffect(() => {
    if (!openSignal) return
    opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
    generation.current++
    setChecking(false)
    setAck(false)
    setError('')
    setReviewAck(false)
    if (active || submitting.current) {
      setMode('result')
      setOpen(true)
      return
    }
    if (fixed.current) void transport.discard(fixed.current.id).catch(() => {})
    current.current = null
    expected.current = null
    latest.current = null
    setRequest(null)
    setSnapshot(null)
    setTrackingError('')
    fixed.current = null
    setDraft(null)
    setSelected([...refreshSteps])
    setMode('prepare')
    setOpen(true)
  }, [openSignal])
  function close() {
    generation.current++
    setOpen(false)
    if (mode === 'prepare' && fixed.current) {
      void transport.discard(fixed.current.id).catch(() => {})
      fixed.current = null
      setDraft(null)
    }
  }
  function choose(step: RefreshStep) {
    if (checking) return
    if (fixed.current) void transport.discard(fixed.current.id).catch(() => {})
    fixed.current = null
    setDraft(null)
    setAck(false)
    setError('')
    setSelected((previous) =>
      refreshSteps.filter((s) => (s === step ? !previous.includes(s) : previous.includes(s))),
    )
  }
  async function prepare() {
    if (checking || submitting.current || !selected.length) return
    const ticket = ++generation.current,
      chosen = [...selected],
      began = Date.now()
    setChecking(true)
    setError('')
    setAck(false)
    if (fixed.current) void transport.discard(fixed.current.id).catch(() => {})
    fixed.current = null
    setDraft(null)
    try {
      const raw = await transport.prepare(chosen),
        value = readRefreshDraft(raw, chosen, began)
      if (!alive.current || ticket !== generation.current) {
        void transport.discard(value.id).catch(() => {})
        return
      }
      fixed.current = value
      setDraft(value)
    } catch (e) {
      if (alive.current && ticket === generation.current) setError(message(e))
    } finally {
      if (alive.current && ticket === generation.current) setChecking(false)
    }
  }
  const expired = draft !== null && (Date.now() < draft.createdAt || Date.now() >= draft.expiresAt)
  async function execute() {
    const value = fixed.current
    if (
      !enabled ||
      !ack ||
      !value ||
      submitting.current ||
      Date.now() < value.createdAt ||
      Date.now() >= value.expiresAt
    )
      return
    submitting.current = true
    generation.current++
    fixed.current = null
    setDraft(null)
    const id = crypto.randomUUID()
    expected.current = { planId: value.planId, selected: value.selected }
    current.current = id
    latest.current = null
    observed.current = false
    setRequest(id)
    setEpoch((e) => e + 1)
    setSnapshot(null)
    setMode('result')
    setError('')
    setActive(true)
    setCancelRequested(false)
    setReviewAck(false)
    try {
      sessionStorage.setItem(refreshSessionKey, id)
    } catch {
      setError('无法保存本机接回标识；结果仍保存在操作历史中。')
    }
    try {
      const record = readRefreshDetail(await transport.execute(value, id))
      if (record.id !== id) throw new Error('维护结果不属于本次请求。')
      if (current.current === id && alive.current) {
        observed.current = true
        apply({ version: 1, requestId: id, active: false, record })
      }
    } catch (e) {
      if (
        alive.current &&
        current.current === id &&
        (!currentSnapshot()?.record || currentSnapshot()?.record?.state === 'running')
      )
        setError(`${message(e)} 请读取原请求结果，不要重复提交。`)
    } finally {
      if (current.current === id) submitting.current = false
    }
  }
  async function cancel() {
    if (!request || cancelRequested) return
    const id = request
    setCancelRequested(true)
    try {
      await transport.cancel(id)
    } catch (e) {
      // A late control response cannot change committed execution evidence or
      // the feedback for a different request opened in the meantime.
      if (!alive.current || current.current !== id) return
      const known = currentSnapshot()?.record
      if (known && known.state !== 'running') return
      setError(message(e))
      setCancelRequested(false)
    }
  }
  function showRecord(id: string) {
    if ((active || submitting.current) && current.current !== id) return
    opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
    if (current.current !== id) {
      expected.current = null
      observed.current = false
      latest.current = null
      setSnapshot(null)
    }
    current.current = id
    setRequest(id)
    setEpoch((e) => e + 1)
    setMode('result')
    setReviewAck(false)
    setError('')
    setTrackingError('')
    setOpen(true)
  }
  async function acknowledge() {
    const record = latest.current?.record
    if (!record || !reviewAck || reviewing || latest.current?.active) return
    setReviewing(true)
    try {
      const next = readRefreshDetail(await transport.acknowledge(record))
      if (next.id !== record.id || next.snapshotSha256 !== record.snapshotSha256)
        throw new Error('核查结果不属于原记录。')
      apply({ version: 1, requestId: record.id, active: false, record: next })
    } catch (e) {
      setError(message(e))
    } finally {
      setReviewing(false)
    }
  }
  const record = snapshot?.record
  const issue = error || trackingError
  return (
    <>
      {request &&
        !open &&
        (active ||
          issue ||
          record?.state === 'running' ||
          (record?.retainedForReview && !historyVisible)) && (
          <section className="notice execution-recovery" aria-label="维护状态">
            <div>
              <strong>{active ? '正在刷新预览' : '维护结果需要核对'}</strong>
              <p>{issue || '结果持续保存；关闭窗口不会丢弃已发生的结果。'}</p>
            </div>
            <button className="button secondary compact" onClick={() => showRecord(request)}>
              查看维护结果
            </button>
          </section>
        )}
      {historyVisible && (
        <section aria-label="维护记录">
          <div className="section-heading">
            <h3>预览刷新</h3>
            <button
              className="button secondary compact"
              disabled={!enabled}
              onClick={() => void loadHistory()}
            >
              刷新维护记录
            </button>
          </div>
          {historyError && (
            <p role="alert" className="error-text">
              {historyError}
            </p>
          )}
          {records.length === 0 ? (
            <p className="muted">尚无预览刷新记录。</p>
          ) : (
            <div className="maintenance-list">
              {records.map((row) => (
                <button
                  className="button secondary"
                  key={row.id}
                  disabled={(active || submitting.current) && row.id !== current.current}
                  onClick={() => showRecord(row.id)}
                >
                  {dateTime(row.createdAt)} · {refreshStateName(row.state)} · {row.succeededCount}/
                  {row.selectedCount} 项完成{row.reviewedAt !== null ? ' · 已人工核查' : ''}
                </button>
              ))}
            </div>
          )}
        </section>
      )}
      {open && (
        <Modal
          title={mode === 'prepare' ? '刷新预览' : '维护结果'}
          onClose={close}
          returnFocus={() => focusReturnTarget(opener.current)}
          className="maintenance-refresh-modal"
        >
          <h2>{mode === 'prepare' ? '刷新预览' : '维护结果'}</h2>
          {mode === 'prepare' ? (
            <>
              <p>预览异常时，可刷新 Quick Look 的缩略图缓存和生成器列表。</p>
              <p className="muted">
                可能影响正在进行的预览。缓存由系统重建，初次显示可能较慢；没有原缓存备份或一键撤销，不保证提速。
              </p>
              <div className="refresh-step-choices">
                {refreshSteps.map((step) => (
                  <label className="uninstall-ack" key={step}>
                    <input
                      type="checkbox"
                      checked={selected.includes(step)}
                      disabled={checking}
                      onChange={() => choose(step)}
                    />
                    {refreshStepName(step)}
                  </label>
                ))}
              </div>
              <p className="muted">
                首批仅刷新 Quick Look 服务，不直接删除 Finder 缓存目录、不修改偏好或重启
                Finder。服务无法按目录排除，保护规则不明确时保留。
              </p>
              {draft && (
                <>
                  <p role="status">已核对本次规则、账户和系统工具；提交时还会逐步复查。</p>
                  <label className="uninstall-ack">
                    <input
                      type="checkbox"
                      checked={ack}
                      onChange={(e) => setAck(e.target.checked)}
                    />
                    我已了解预览影响、缓存重建及无法撤销的边界。
                  </label>
                  {expired && (
                    <p className="error-text" role="status">
                      确认已过期，请重新检查；不会自动执行。
                    </p>
                  )}
                </>
              )}
              {issue && (
                <p role="alert" className="error-text">
                  {issue}
                </p>
              )}
              <div className="modal-actions">
                <button
                  className="button secondary"
                  disabled={checking || !selected.length}
                  onClick={() => void prepare()}
                >
                  {checking ? '正在检查…' : draft ? '重新检查' : '检查所选步骤'}
                </button>
                <button
                  className="button primary"
                  disabled={!draft || expired || !ack || checking}
                  onClick={() => void execute()}
                >
                  开始刷新
                </button>
              </div>
            </>
          ) : (
            <>
              <p role="status">
                {record
                  ? refreshStateName(record.state)
                  : active
                    ? '正在提交并读取记录…'
                    : '正在读取原请求结果…'}
              </p>
              {issue && (
                <p role="alert" className="error-text">
                  {issue}
                </p>
              )}
              {record && (
                <>
                  <div className="maintenance-list">
                    {record.items.map((item) => (
                      <article className="refresh-result-row" key={item.step}>
                        <strong>{refreshStepName(item.step)}</strong>
                        <span className="tag">{refreshStateName(item.state)}</span>
                        <p>{refreshReason(item.reason)}</p>
                      </article>
                    ))}
                  </div>
                  <p className="muted">
                    已完成 {record.succeededCount}/{record.selectedCount} 项 · 待核实{' '}
                    {record.uncertainCount}{' '}
                    项。缓存由系统重建，不代表预览已修复；已发生的结果不会因取消而撤销。
                  </p>
                  {record.state === 'running' && !snapshot?.active && (
                    <p className="notice warning">
                      当前没有对应 worker，结果尚未收齐。请重新启动应用对账；不会自动继续。
                    </p>
                  )}
                  {['needsReview', 'interrupted'].includes(record.state) &&
                    record.reviewedAt === null &&
                    !snapshot?.active && (
                      <>
                        <p className="notice warning">
                          请先核对预览表现和逐步结果。人工核查只解除新修改任务的阻塞，原来的未知/中断记录保持，不会重新执行。
                        </p>
                        <label className="uninstall-ack">
                          <input
                            type="checkbox"
                            checked={reviewAck}
                            onChange={(e) => setReviewAck(e.target.checked)}
                          />
                          我已核查结果，了解未知步骤不会自动重试。
                        </label>
                        <button
                          className="button secondary"
                          disabled={!reviewAck || reviewing}
                          onClick={() => void acknowledge()}
                        >
                          {reviewing ? '正在保存…' : '记录已核查'}
                        </button>
                      </>
                    )}
                  {record.reviewedAt !== null && (
                    <p className="muted">
                      已于 {dateTime(record.reviewedAt)} 人工核查，原结果保持。
                    </p>
                  )}
                </>
              )}
              <details>
                <summary>请求与记录</summary>
                <p className="mono full-path">{request}</p>
                {record && <p className="muted">开始于 {dateTime(record.createdAt)}</p>}
              </details>
              <div className="modal-actions">
                {snapshot?.active && (
                  <button
                    className="button secondary"
                    disabled={cancelRequested}
                    onClick={() => void cancel()}
                  >
                    {cancelRequested ? '已请求停止，正在收集结果…' : '取消后续步骤'}
                  </button>
                )}
                <button className="button secondary" onClick={() => setEpoch((e) => e + 1)}>
                  重新读取
                </button>
                <button
                  className="button primary"
                  onClick={(event) => {
                    if (event.detail < 2) close()
                  }}
                >
                  {active ? '后台继续' : '完成'}
                </button>
              </div>
            </>
          )}
        </Modal>
      )}
    </>
  )
}
