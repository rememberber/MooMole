import { useEffect, useRef, useState } from 'react'
import { RefreshCw, Trash2 } from 'lucide-react'
import { Modal } from './Modal'
import { dateTime } from '../lib/format'
import { useNow } from '../lib/use-now'
import { watchExecution } from '../lib/execution-watcher'
import { focusReturnTarget } from '../lib/focus-return'
import {
  parseUninstallPlan,
  parseUninstallReview,
  parseUninstallDetail,
  parseUninstallSnapshot,
  parseUninstallList,
  preserveUninstall,
  uninstallId,
  uninstallSessionKey,
  uninstallStateName,
  type UninstallTarget,
  type UninstallPlan,
  type UninstallReview,
  type UninstallSnapshot,
  type UninstallSummary,
  type UninstallTransport,
} from '../lib/app-uninstall'

const message = (error: unknown) => String(error).replace(/^Error: /, '')
export function ApplicationUninstallCenter({
  target,
  enabled,
  historyVisible,
  transport,
  onTargetClose,
  onActiveChange,
  onOutcome,
}: {
  target: UninstallTarget | null
  enabled: boolean
  historyVisible: boolean
  transport: UninstallTransport
  onTargetClose: () => void
  onActiveChange: (active: boolean) => void
  onOutcome: () => void
}) {
  useNow()
  const now = Date.now()
  const [open, setOpen] = useState(false)
  const [plan, setPlan] = useState<UninstallPlan | null>(null)
  const [review, setReview] = useState<UninstallReview | null>(null)
  const [checking, setChecking] = useState(false)
  const [acknowledged, setAcknowledged] = useState(false)
  const [consumed, setConsumed] = useState(false)
  const [error, setError] = useState('')
  const [snapshot, setSnapshot] = useState<UninstallSnapshot | null>(null)
  const [requestId, setRequestId] = useState<string | null>(null)
  const [submitted, setSubmitted] = useState(false)
  const [trackingError, setTrackingError] = useState('')
  const [cancelRequested, setCancelRequested] = useState(false)
  const [records, setRecords] = useState<UninstallSummary[]>([])
  const [historyError, setHistoryError] = useState('')
  const [refreshing, setRefreshing] = useState(false)
  const [watchEpoch, setWatchEpoch] = useState(0)
  const opener = useRef<HTMLElement | null>(null)
  const generation = useRef(0)
  const mounted = useRef(false)
  const fixed = useRef<{ plan: UninstallPlan | null; confirmationId: string | null }>({
    plan: null,
    confirmationId: null,
  })
  const currentRequest = useRef<string | null>(null)
  const busySubmit = useRef(false)
  const executionOwnsPlan = useRef(false)
  const cancelling = useRef(false)
  const listGeneration = useRef(0)
  const outcomeNotified = useRef(new Set<string>())
  const trackChanges = useRef(new Set<string>())
  const settledRequests = useRef(new Set<string>())
  const observedRequests = useRef(new Set<string>())
  const activeCallback = useRef(onActiveChange),
    outcomeCallback = useRef(onOutcome)
  activeCallback.current = onActiveChange
  outcomeCallback.current = onOutcome
  const active = submitted || snapshot?.active === true
  useEffect(() => {
    activeCallback.current(active)
  }, [active])
  function revoke() {
    if (executionOwnsPlan.current) return
    const old = fixed.current
    fixed.current = { plan: null, confirmationId: null }
    if (old.confirmationId) void transport.discardConfirmation(old.confirmationId).catch(() => {})
    if (old.plan) void transport.discardPlan(old.plan.id).catch(() => {})
  }
  function adopt(next: UninstallSnapshot) {
    if (!mounted.current || currentRequest.current !== next.requestId) return false
    if (next.active || next.record) observedRequests.current.add(next.requestId)
    // A read can reach the native side before a newly sent execute IPC. Absence
    // alone does not terminate an unacknowledged submission or permit replay.
    if (
      !next.active &&
      !next.record &&
      busySubmit.current &&
      !settledRequests.current.has(next.requestId) &&
      !observedRequests.current.has(next.requestId)
    )
      return false
    setSnapshot((previous) => preserveUninstall(next, previous))
    if (!next.active) {
      executionOwnsPlan.current = false
      busySubmit.current = false
      setSubmitted(false)
      revoke()
      try {
        if (sessionStorage.getItem(uninstallSessionKey) === next.requestId)
          sessionStorage.removeItem(uninstallSessionKey)
      } catch {
        /* Durable records remain readable. */
      }
    }
    if (next.record && next.record.state !== 'running') {
      setRecords((previous) =>
        [next.record!, ...previous.filter((r) => r.id !== next.requestId)].slice(0, 64),
      )
      if (
        ['moved', 'uncertain'].includes(next.record.itemState) &&
        trackChanges.current.has(next.requestId) &&
        !outcomeNotified.current.has(next.requestId)
      ) {
        outcomeNotified.current.add(next.requestId)
        outcomeCallback.current()
      }
      if (!next.active) {
        try {
          if (sessionStorage.getItem(uninstallSessionKey) === next.requestId)
            sessionStorage.removeItem(uninstallSessionKey)
        } catch {
          /* History is still durable. */
        }
      }
    }
    return true
  }
  function follow(id: string, reveal = false, changed = false) {
    if (changed) trackChanges.current.add(id)
    setSubmitted(true)
    currentRequest.current = id
    setRequestId(id)
    setSnapshot(null)
    setTrackingError('')
    setCancelRequested(false)
    cancelling.current = false
    setWatchEpoch((v) => v + 1)
    if (reveal) {
      setOpen(true)
      setError('')
    }
  }
  async function refresh() {
    if (!enabled) return
    const g = ++listGeneration.current
    setRefreshing(true)
    setHistoryError('')
    try {
      const next = parseUninstallList(await transport.list())
      if (!mounted.current || g !== listGeneration.current) return
      setRecords((previous) =>
        next
          .map((item) => {
            const prior = previous.find((r) => r.id === item.id)
            return prior && prior.state !== 'running' && item.state === 'running' ? prior : item
          })
          .concat(previous.filter((r) => r.retainedForReview && !next.some((n) => n.id === r.id)))
          .slice(0, 64),
      )
      if (!currentRequest.current) {
        const running = next.find((r) => r.state === 'running')
        if (running) follow(running.id, false, true)
      }
    } catch (e) {
      if (mounted.current && g === listGeneration.current) setHistoryError(message(e))
    } finally {
      if (mounted.current && g === listGeneration.current) setRefreshing(false)
    }
  }
  useEffect(() => {
    mounted.current = true
    if (enabled) {
      try {
        const id = sessionStorage.getItem(uninstallSessionKey)
        if (id) follow(uninstallId(id), false, true)
      } catch {
        /* Ignore an invalid/unavailable observer ID. Never submit it. */
      }
      void refresh()
    }
    return () => {
      mounted.current = false
      generation.current++
      listGeneration.current++
      revoke()
    }
    // The transport is a stable per-App boundary, not a per-render callback.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, transport])
  useEffect(() => {
    if (historyVisible) void refresh()
  }, [historyVisible])
  useEffect(() => {
    if (!requestId || !enabled) return
    const controller = new AbortController()
    const stop = watchExecution({
      load: async () => parseUninstallSnapshot(await transport.read(requestId), requestId),
      onValue: (next) => {
        if (adopt(next) && !next.active) controller.abort()
      },
      onError: setTrackingError,
      signal: controller.signal,
    })
    return () => {
      controller.abort()
      stop()
    }
  }, [requestId, watchEpoch, enabled, transport])
  async function check(selected: UninstallTarget) {
    opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
    const g = ++generation.current
    revoke()
    setOpen(true)
    setPlan(null)
    setReview(null)
    setAcknowledged(false)
    setConsumed(false)
    setChecking(true)
    setError('')
    currentRequest.current = null
    setRequestId(null)
    setSnapshot(null)
    setTrackingError('')
    try {
      const next = parseUninstallPlan(
        await transport.prepare(selected.taskId, selected.index),
        selected,
      )
      if (!mounted.current || g !== generation.current) {
        void transport.discardPlan(next.id).catch(() => {})
        return
      }
      fixed.current.plan = next
      setPlan(next)
      const result = parseUninstallReview(await transport.review(next.id), next)
      if (!mounted.current || g !== generation.current) {
        if (result.confirmation)
          void transport.discardConfirmation(result.confirmation.id).catch(() => {})
        return
      }
      fixed.current.confirmationId = result.confirmation?.id ?? null
      setReview(result)
    } catch (e) {
      if (mounted.current && g === generation.current) setError(message(e))
    } finally {
      if (mounted.current && g === generation.current) setChecking(false)
    }
  }
  useEffect(() => {
    if (target && enabled && !active && !busySubmit.current) void check(target)
  }, [target])
  async function execute() {
    const c = review?.confirmation
    if (!c || !plan || !acknowledged || consumed || busySubmit.current) return
    if (Date.now() < c.createdAt || Date.now() >= c.expiresAt) {
      setError('确认已过期，请重新检查。')
      return
    }
    busySubmit.current = true
    setConsumed(true)
    setSubmitted(true)
    setError('')
    setCancelRequested(false)
    executionOwnsPlan.current = true
    const id = crypto.randomUUID()
    trackChanges.current.add(id)
    currentRequest.current = id
    setRequestId(id)
    setSnapshot(null)
    try {
      sessionStorage.setItem(uninstallSessionKey, id)
    } catch {
      setTrackingError('本次接回标识未能保存在窗口中；可从卸载记录读取实际结果。')
    }
    try {
      const detail = parseUninstallDetail(await transport.execute(plan.id, c.id, id), id)
      settledRequests.current.add(id)
      if (
        detail.planId !== plan.id ||
        detail.path !== plan.path ||
        detail.bundleId !== plan.bundleId
      )
        throw new Error('返回的 App 身份与确认不一致，请核对记录。')
      adopt({ version: 1, requestId: id, active: false, record: detail })
    } catch (e) {
      settledRequests.current.add(id)
      if (mounted.current && currentRequest.current === id) {
        setError(`${message(e)} 请读取实际记录，不要重复提交。`)
        void transport
          .read(id)
          .then((raw) => adopt(parseUninstallSnapshot(raw, id)))
          .catch(() => {})
      }
    } finally {
      if (currentRequest.current === id && mounted.current) {
        if (!executionOwnsPlan.current) setSubmitted(false)
        void refresh()
      }
    }
  }
  async function readAgain() {
    if (!requestId) return
    const id = requestId
    setTrackingError('')
    try {
      adopt(parseUninstallSnapshot(await transport.read(id), id))
    } catch (e) {
      if (currentRequest.current === id) setTrackingError(message(e))
    }
  }
  async function cancel() {
    if (!requestId || cancelling.current) return
    cancelling.current = true
    setCancelRequested(true)
    try {
      await transport.cancel(requestId)
    } catch (e) {
      cancelling.current = false
      setCancelRequested(false)
      setTrackingError(`${message(e)} 是否停止请以实际结果为准。`)
      void readAgain()
    }
  }
  function close() {
    generation.current++
    revoke()
    setChecking(false)
    setOpen(false)
    onTargetClose()
  }
  function openRecord(id: string) {
    if ((active || executionOwnsPlan.current) && currentRequest.current !== id) return
    generation.current++
    revoke()
    setPlan(null)
    setReview(null)
    setConsumed(true)
    setAcknowledged(false)
    follow(
      id,
      true,
      records.some((r) => r.id === id && r.state === 'running'),
    )
  }
  const detail = snapshot?.record
  const confirmation = review?.confirmation
  const expired = !!confirmation && (now < confirmation.createdAt || now >= confirmation.expiresAt)
  const title = requestId ? '卸载结果' : '确认卸载'
  return (
    <>
      {!open &&
        requestId &&
        (active || trackChanges.current.has(requestId)) &&
        (!historyVisible || active) && (
          <section className="notice uninstall-progress" aria-label="卸载状态">
            <div>
              <strong>
                {detail
                  ? `${detail.name} · ${uninstallStateName(detail.itemState)}`
                  : active
                    ? '正在核对卸载结果'
                    : '卸载结果待确认'}
              </strong>
              <p>
                {active
                  ? '关闭窗口不会中断系统操作，实际结果仍在收集中。'
                  : '实际记录已保留；移入废纸篓不等于释放空间。'}
              </p>
            </div>
            <button
              className="button secondary compact"
              onClick={(e) => {
                opener.current = e.currentTarget
                setOpen(true)
              }}
            >
              查看卸载结果
            </button>
            {!active && (
              <button
                className="button text compact"
                onClick={() => {
                  currentRequest.current = null
                  setRequestId(null)
                  setSnapshot(null)
                }}
              >
                关闭提示
              </button>
            )}
          </section>
        )}
      {historyVisible && (
        <section className="card saved-plans uninstall-history" aria-label="卸载记录">
          <div className="list-toolbar">
            <h2>卸载记录</h2>
            <button
              className="button secondary compact"
              disabled={!enabled || refreshing}
              onClick={() => void refresh()}
            >
              <RefreshCw size={14} className={refreshing ? 'spin' : ''} />
              刷新卸载记录
            </button>
          </div>
          {historyError && (
            <p className="notice warning" role="alert">
              {historyError}
            </p>
          )}
          {records.length ? (
            records.map((r) => (
              <button
                className="saved-plan-row"
                key={r.id}
                disabled={active && r.id !== requestId}
                onClick={(e) => {
                  opener.current = e.currentTarget
                  openRecord(r.id)
                }}
              >
                <span>
                  <strong>
                    {r.name} · {uninstallStateName(r.itemState)}
                  </strong>
                  <small>{dateTime(r.createdAt)} · 包外数据保留</small>
                </span>
                <span>查看结果</span>
              </button>
            ))
          ) : (
            <p className="execution-retention muted">
              暂无卸载记录。检查范围不会卸载，确认执行后才创建记录。
            </p>
          )}
        </section>
      )}
      {open && (
        <Modal
          title={title}
          onClose={close}
          returnFocus={() =>
            focusReturnTarget(
              opener.current,
              document.querySelector<HTMLElement>('main.main-content'),
            )
          }
          className="review-plan-modal uninstall-modal"
        >
          <h2>{title}</h2>
          {requestId ? (
            <>
              {detail ? (
                <>
                  <strong role="status">
                    {detail.name} · {uninstallStateName(detail.itemState)}
                  </strong>
                  <p className="mono full-path">{detail.path}</p>
                  <p>{detail.reason}</p>
                  {detail.itemState === 'uncertain' && (
                    <p className="notice warning">
                      系统可能已处理此 App，结果尚未核实。不要按原路径重复执行，也不要覆盖已有对象。
                    </p>
                  )}
                  {!snapshot?.active && detail.state === 'running' && (
                    <p className="notice warning">
                      当前没有此任务的运行句柄，但持久记录尚未结束。请重新启动 App
                      对账；不要重复提交。
                    </p>
                  )}
                  <p className="muted">
                    仅处理 App
                    包，包外设置、缓存和个人数据保留。移入废纸篓不会立即释放空间；当前没有应用内恢复。
                  </p>
                  {detail.receipt && (
                    <details>
                      <summary>当时记录的移入位置</summary>
                      <p className="mono full-path">{detail.receipt.actual.destination}</p>
                      <p className="muted">
                        这是当时的收据，当前位置可能变化。需要取回时，在 Finder 核对
                        App，优先使用“放回原处”；若无此项，可将此 App
                        移回上方原目录。原位置已有同名对象时取消，勿覆盖。
                      </p>
                    </details>
                  )}
                  <details>
                    <summary>执行记录</summary>
                    <p className="mono full-path">请求 {detail.id}</p>
                    {detail.events.map((e, i) => (
                      <p key={i}>
                        {uninstallStateName(e.state)} · {dateTime(e.createdAt)}
                        <br />
                        {e.reason}
                      </p>
                    ))}
                  </details>
                </>
              ) : (
                <p role="status">
                  {submitted || snapshot?.active
                    ? '正在取得当次复查和系统处理结果…'
                    : '尚未取得持久结果，请重新读取或在历史中核对。'}
                </p>
              )}
              {active && (
                <>
                  <p className="muted">
                    取消只停止尚未调用的操作；已经进入系统处理时仍会收集实际结果。关闭窗口后可从状态提示或历史返回。
                  </p>
                  <button
                    className="button secondary"
                    disabled={cancelRequested}
                    onClick={() => void cancel()}
                  >
                    {cancelRequested ? '已请求取消，等待实际结果…' : '取消卸载'}
                  </button>
                </>
              )}
              {error && (
                <p className="notice warning" role="alert">
                  {error}
                </p>
              )}
              {trackingError && (
                <p className="notice warning" role="alert">
                  {trackingError}
                </p>
              )}
              <div className="modal-actions">
                <button className="button secondary" onClick={() => void readAgain()}>
                  重新读取结果
                </button>
                <button className="button primary" onClick={close}>
                  {active ? '后台继续' : '完成'}
                </button>
              </div>
            </>
          ) : (
            <>
              <strong>{plan?.name ?? target?.name}</strong>
              <p className="mono full-path">{plan?.path ?? target?.path}</p>
              {checking && <p role="status">正在检查 App、运行状态与卸载范围；尚未执行。</p>}
              {review && <p role="status">{review.message}</p>}
              {error && (
                <p className="notice warning" role="alert">
                  {error}
                </p>
              )}
              {confirmation && (
                <>
                  <p className="muted">
                    首批仅支持普通 App。若通过其他管理工具安装或带专用后台服务，请使用对应卸载方式。
                  </p>
                  <p className="notice warning">
                    移入废纸篓不会立即释放空间，当前没有应用内恢复。执行前会再次检查；未知、过期或变化时保留。
                  </p>
                  <label className="uninstall-ack">
                    <input
                      type="checkbox"
                      checked={acknowledged}
                      disabled={expired || consumed}
                      onChange={(e) => setAcknowledged(e.target.checked)}
                    />
                    这是普通 App，我已核对范围并确认卸载此 App 包。
                  </label>
                  {expired && (
                    <p className="error-text" role="status">
                      确认已过期，请重新检查；不会自动执行。
                    </p>
                  )}
                </>
              )}
              <div className="modal-actions">
                <button
                  className="button secondary"
                  disabled={checking || !target}
                  onClick={() => {
                    if (target) void check(target)
                  }}
                >
                  重新检查
                </button>
                <button
                  className="button primary"
                  disabled={!confirmation || !acknowledged || expired || consumed || checking}
                  onClick={() => void execute()}
                >
                  <Trash2 size={14} />
                  移入废纸篓
                </button>
              </div>
            </>
          )}
        </Modal>
      )}
    </>
  )
}
