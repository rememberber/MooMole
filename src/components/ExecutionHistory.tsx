import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ChevronRight, RefreshCw } from 'lucide-react'
import type { ExecutionDetail, ExecutionSummary } from '../types'
import { dateTime } from '../lib/format'
import { Modal } from './Modal'
import { ExecutionLocation, type RevealExecutionItem } from './ExecutionLocation'
import { watchExecution } from '../lib/execution-watcher'
import { focusReturnTarget } from '../lib/focus-return'
import { ExecutionReason } from './ExecutionReason'

const stateNames: Record<string, string> = {
  running: '进行中',
  completed: '全部移入废纸篓',
  partial: '部分完成',
  noChanges: '未作修改',
  cancelled: '已取消',
  interrupted: '已中断',
  pending: '尚未开始',
  intent: '等待完成证据',
  moved: '已移入废纸篓',
  skipped: '已跳过',
  failed: '失败，未作修改',
  notStarted: '中断后未开始',
  uncertain: '结果待核实',
}
const stateName = (state: string) => stateNames[state] ?? `未知状态（${state}）`
const errorText = (error: unknown) => String(error).replace(/^Error: /, '')

export function ExecutionHistory({
  initialRecords,
  loadRecords,
  loadDetail,
  onRecordsChange,
  onCancel,
  onRevealReceipt,
  onReady,
}: {
  onReady?: (section: HTMLElement | null) => void
  initialRecords: ExecutionSummary[]
  loadRecords: () => Promise<ExecutionSummary[]>
  loadDetail: (id: string) => Promise<ExecutionDetail>
  onRecordsChange?: (records: ExecutionSummary[]) => void
  onCancel?: (id: string) => Promise<void>
  onRevealReceipt?: RevealExecutionItem
}) {
  const [records, setRecords] = useState(initialRecords)
  const [refreshing, setRefreshing] = useState(false)
  const [error, setError] = useState('')
  const [selected, setSelected] = useState<string | null>(null)
  const [detail, setDetail] = useState<ExecutionDetail | null>(null)
  const [detailError, setDetailError] = useState('')
  const [trackingError, setTrackingError] = useState('')
  const [cancelError, setCancelError] = useState('')
  const [cancelRequested, setCancelRequested] = useState(false)
  const refreshRequest = useRef(0)
  const detailRequest = useRef(0)
  const terminalDetailRequest = useRef<number | null>(null)
  const opener = useRef<HTMLButtonElement | null>(null)
  const refreshButton = useRef<HTMLButtonElement>(null)
  const recordArea = useRef<HTMLElement | null>(null)
  const setRecordArea = useCallback(
    (section: HTMLElement | null) => {
      recordArea.current = section
      onReady?.(section)
    },
    [onReady],
  )
  useEffect(() => setRecords(initialRecords), [initialRecords])
  useEffect(
    () => () => {
      refreshRequest.current++
      detailRequest.current++
    },
    [],
  )
  useEffect(() => {
    if (!selected || detail?.state !== 'running') return
    const controller = new AbortController()
    const stop = watchExecution({
      load: async () => {
        const next = await loadDetail(selected)
        if (next.id !== selected) throw new Error('返回的执行记录不匹配。')
        return next
      },
      onValue: (next) => {
        if (next.state !== 'running') {
          terminalDetailRequest.current = detailRequest.current
          setCancelError('')
        }
        setDetail((current) =>
          current && (current.state !== 'running' || current.events.length > next.events.length)
            ? current
            : next,
        )
      },
      onError: setTrackingError,
      signal: controller.signal,
    })
    return () => {
      controller.abort()
      stop()
    }
  }, [selected, detail?.state, loadDetail])
  async function cancel() {
    if (!selected || !onCancel || cancelRequested || detail?.state !== 'running') return
    const request = detailRequest.current
    const id = selected
    setCancelRequested(true)
    setCancelError('')
    try {
      await onCancel(id)
    } catch (error) {
      if (request === detailRequest.current && terminalDetailRequest.current !== request) {
        setCancelRequested(false)
        setCancelError(`${errorText(error)} 停止是否生效需以更新后的逐项结果为准。`)
      }
    }
  }
  async function refresh() {
    const request = ++refreshRequest.current
    setRefreshing(true)
    setError('')
    try {
      const next = await loadRecords()
      if (request === refreshRequest.current) {
        setRecords(next)
        onRecordsChange?.(next)
      }
    } catch (error) {
      if (request === refreshRequest.current) setError(errorText(error))
    } finally {
      if (request === refreshRequest.current) setRefreshing(false)
    }
  }
  async function open(id: string) {
    const request = ++detailRequest.current
    setSelected(id)
    setDetail(null)
    setDetailError('')
    setTrackingError('')
    setCancelError('')
    setCancelRequested(false)
    try {
      const next = await loadDetail(id)
      if (next.id !== id) throw new Error('返回的执行记录不匹配，请重新打开。')
      if (request === detailRequest.current) {
        if (next.state !== 'running') terminalDetailRequest.current = request
        setDetail(next)
      }
    } catch (error) {
      if (request === detailRequest.current) setDetailError(errorText(error))
    }
  }
  function close() {
    detailRequest.current++
    setSelected(null)
    setDetail(null)
  }
  return (
    <>
      <section
        ref={setRecordArea}
        id="execution-records"
        tabIndex={-1}
        className="card saved-plans execution-history"
        aria-label="执行记录"
      >
        <div className="list-toolbar">
          <h2>执行记录</h2>
          <button
            ref={refreshButton}
            className="button secondary compact"
            disabled={refreshing}
            onClick={() => void refresh()}
          >
            <RefreshCw size={14} className={refreshing ? 'spin' : ''} />
            刷新记录
          </button>
        </div>
        {error && (
          <p className="notice warning" role="alert">
            {error}
          </p>
        )}
        {records.length ? (
          <>
            <p className="execution-retention muted">
              含移入收据或中断证据的记录会继续保留，超出普通历史保留期限。
            </p>
            {records.map((record) => (
              <button
                className="saved-plan-row"
                key={record.id}
                onClick={(event) => {
                  opener.current = event.currentTarget
                  void open(record.id)
                }}
              >
                <span>
                  <strong>
                    {stateName(record.state)} · {record.itemCount} 项
                  </strong>
                  <small>
                    {dateTime(record.createdAt)} · 已移入 {record.movedCount} 项 · 待核实{' '}
                    {record.uncertainCount} 项
                  </small>
                </span>
                <span>查看逐项结果</span>
                <ChevronRight size={14} />
              </button>
            ))}
          </>
        ) : (
          <p className="execution-retention muted">
            暂无执行记录。保存计划不会执行清理；符合条件的日志需单独核对并确认处理。
          </p>
        )}
      </section>
      {selected && (
        <Modal
          title="逐项执行结果"
          onClose={close}
          returnFocus={() =>
            focusReturnTarget(opener.current, refreshButton.current, recordArea.current)
          }
          className="review-plan-modal execution-modal"
        >
          <h2>逐项执行结果</h2>
          {detailError && (
            <div className="notice warning" role="alert">
              <p>{detailError}</p>
              <button className="button secondary" onClick={() => void open(selected)}>
                重试读取
              </button>
            </div>
          )}
          {trackingError && (
            <p className="notice warning" role="alert">
              {trackingError}
            </p>
          )}
          {detail?.state === 'running' && (
            <div className="plan-toolbar">
              <p role="status">
                {cancelRequested
                  ? '已请求停止后续项目，等待当前项的实际结果。'
                  : '正在跟踪本批次，逐项结果自动更新。'}
              </p>
              {onCancel && (
                <button
                  className="button secondary"
                  disabled={cancelRequested}
                  onClick={() => void cancel()}
                >
                  {cancelRequested ? '等待实际结果…' : '停止后续项目'}
                </button>
              )}
            </div>
          )}
          {cancelError && (
            <p className="notice warning" role="alert">
              {cancelError}
            </p>
          )}
          {detail ? (
            <ExecutionResults key={detail.id} detail={detail} onRevealReceipt={onRevealReceipt} />
          ) : (
            !detailError && <p role="status">正在读取本地记录…</p>
          )}
        </Modal>
      )}
    </>
  )
}

function ExecutionResults({
  detail,
  onRevealReceipt,
}: {
  detail: ExecutionDetail
  onRevealReceipt?: RevealExecutionItem
}) {
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState('all')
  const [limit, setLimit] = useState(50)
  const events = useMemo(() => {
    const grouped = new Map<string, ExecutionDetail['events']>()
    for (const event of detail.events) {
      const values = grouped.get(event.itemId) ?? []
      values.push(event)
      grouped.set(event.itemId, values)
    }
    return grouped
  }, [detail.events])
  const items = detail.items.filter(
    (item) =>
      item.path.toLocaleLowerCase().includes(query.toLocaleLowerCase()) &&
      (filter === 'all' || (filter === 'moved' ? item.state === 'moved' : item.state !== 'moved')),
  )
  return (
    <>
      <p className="muted">
        {stateName(detail.state)} · 已移入 {detail.movedCount} / {detail.itemCount} 项 · 待核实{' '}
        {detail.uncertainCount} 项
      </p>
      <div className={`notice ${detail.uncertainCount ? 'warning' : ''}`}>
        {detail.uncertainCount > 0
          ? '存在只有意图、没有可靠完成记录的项目。不要据此自动重试或恢复；需要核查实际结果。'
          : '取消与中断不会撤销已经完成的项目。移入废纸篓不等于释放磁盘空间。'}{' '}
        这里仅查看记录，收据本身不能授权恢复操作。
      </div>
      <details className="execution-metadata">
        <summary>批次与计划信息</summary>
        <dl>
          <dt>批次 ID</dt>
          <dd>{detail.id}</dd>
          <dt>计划 ID</dt>
          <dd>{detail.planId}</dd>
          <dt>处理方式</dt>
          <dd>
            {detail.method === 'trash' ? '移入 macOS 废纸篓' : `未知方式（${detail.method}）`}
          </dd>
          <dt>开始 / 结束</dt>
          <dd>
            {dateTime(detail.createdAt)} /{' '}
            {detail.finishedAt == null ? '未结束' : dateTime(detail.finishedAt)}
          </dd>
          <dt>Mole 源码 / 协议</dt>
          <dd>
            {detail.revision} / {detail.protocol}
          </dd>
          <dt>计划 SHA-256</dt>
          <dd>{detail.snapshotSha256}</dd>
        </dl>
      </details>
      <div className="execution-filters">
        <input
          aria-label="搜索执行路径"
          placeholder="搜索路径"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value)
            setLimit(50)
          }}
        />
        <select
          aria-label="执行项目状态"
          value={filter}
          onChange={(e) => {
            setFilter(e.target.value)
            setLimit(50)
          }}
        >
          <option value="all">全部项目</option>
          <option value="moved">已移入</option>
          <option value="issues">未完成或异常</option>
        </select>
        <span className="muted" role="status">
          显示 {Math.min(limit, items.length)} / {items.length} 项
        </span>
      </div>
      <div className="execution-items">
        {items.slice(0, limit).map((item) => (
          <details key={item.id} className="execution-item">
            <summary>
              <span>{item.path}</span>
              <span
                className={`state-tag ${item.state === 'moved' ? 'succeeded' : item.state === 'uncertain' ? 'failed' : ''}`}
              >
                {stateName(item.state)}
              </span>
            </summary>
            <ExecutionReason reason={item.reason} />
            {item.receipt && (
              <div className="execution-receipt">
                <strong>记录的实际目标</strong>
                <p>{item.receipt.destination}</p>
                <small>
                  设备 {item.receipt.device} · inode {item.receipt.inode}
                </small>
                {onRevealReceipt && ['moved', 'uncertain'].includes(item.state) && (
                  <ExecutionLocation
                    runId={detail.id}
                    itemId={item.id}
                    onReveal={onRevealReceipt}
                  />
                )}
              </div>
            )}
            <ol className="execution-events" aria-label="项目记录顺序">
              {(events.get(item.id) ?? []).map((event, index) => (
                <li key={index}>
                  <time>{dateTime(event.createdAt)}</time> · {stateName(event.state)}
                  <ExecutionReason reason={event.reason} />
                </li>
              ))}
            </ol>
          </details>
        ))}
        {!items.length && <p className="muted">没有匹配的项目。</p>}
      </div>
      {items.length > limit && (
        <button className="button secondary" onClick={() => setLimit((n) => n + 50)}>
          再显示 50 项
        </button>
      )}
    </>
  )
}
