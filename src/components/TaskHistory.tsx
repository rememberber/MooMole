import { useEffect, useMemo, useRef, useState } from 'react'
import { ChevronRight, RefreshCw, Search } from 'lucide-react'
import type { TaskRecord } from '../types'
import { dateTime, shortPath } from '../lib/format'
import {
  canFollowTask,
  filterTasks,
  taskDuration,
  taskKindName,
  taskStateName,
  taskStates,
  taskTime,
  taskTimeLabel,
} from '../lib/task-history'
import { Modal } from './Modal'
import { DiagnosticExport } from './DiagnosticExport'

const BATCH = 50
export function TaskHistory({
  records,
  home,
  canRefresh,
  canTrack,
  onRefresh,
  onTrack,
}: {
  records: TaskRecord[]
  home: string
  canRefresh: boolean
  canTrack: boolean
  onRefresh: () => Promise<void>
  onTrack: (task: TaskRecord) => void
}) {
  const [query, setQuery] = useState('')
  const [state, setState] = useState('all')
  const [limit, setLimit] = useState(BATCH)
  const [selected, setSelected] = useState<string | null>(null)
  const [refreshing, setRefreshing] = useState(false)
  const [error, setError] = useState('')
  const refreshRequest = useRef(0)
  const opener = useRef<HTMLButtonElement | null>(null)
  const search = useRef<HTMLInputElement>(null)
  useEffect(
    () => () => {
      ++refreshRequest.current
    },
    [],
  )
  const matching = useMemo(() => filterTasks(records, query, state), [records, query, state])
  const visible = matching.slice(0, limit)
  const detail = selected == null ? null : records.find((task) => task.id === selected)
  async function refresh() {
    const request = ++refreshRequest.current
    setRefreshing(true)
    setError('')
    try {
      await onRefresh()
    } catch (error) {
      if (request === refreshRequest.current) setError(String(error).replace(/^Error: /, ''))
    } finally {
      if (request === refreshRequest.current) setRefreshing(false)
    }
  }
  function reset() {
    setQuery('')
    setState('all')
    setLimit(BATCH)
  }
  function closeDetail() {
    setSelected(null)
    requestAnimationFrame(() => {
      const target = opener.current?.isConnected ? opener.current : search.current
      target?.focus({ preventScroll: true })
    })
  }
  return (
    <>
      <section className="card task-history" aria-label="客户端检查任务">
        <div className="list-toolbar task-history-toolbar">
          <h2>客户端检查任务</h2>
          <label className="search-field">
            <Search size={14} aria-hidden="true" />
            <input
              ref={search}
              aria-label="搜索检查任务"
              placeholder="类型、路径或诊断…"
              value={query}
              onChange={(event) => {
                setQuery(event.target.value)
                setLimit(BATCH)
              }}
            />
          </label>
          <select
            aria-label="检查任务状态"
            value={state}
            onChange={(event) => {
              setState(event.target.value)
              setLimit(BATCH)
            }}
          >
            <option value="all">全部状态</option>
            {taskStates.map((state) => (
              <option key={state} value={state}>
                {taskStateName(state)}
              </option>
            ))}
            <option value="unknown">未知状态</option>
          </select>
          <button
            className="button secondary compact"
            disabled={!canRefresh || refreshing}
            onClick={() => void refresh()}
          >
            <RefreshCw size={14} className={refreshing ? 'spin' : ''} />
            刷新检查任务
          </button>
        </div>
        <p className="task-history-summary">
          显示 {visible.length} / {matching.length} 个匹配任务 · 已读取 {records.length} 条（上限
          200 条）
        </p>
        {refreshing && (
          <p className="task-history-summary" role="status">
            正在刷新，当前仍显示已读取记录。
          </p>
        )}
        {error && (
          <p className="task-history-summary" role="alert">
            检查任务暂时无法刷新：{error}
          </p>
        )}
        {visible.map((task) => (
          <article className="history-row" key={task.id}>
            <button
              className="history-open"
              onClick={(event) => {
                opener.current = event.currentTarget
                setSelected(task.id)
              }}
            >
              <span className="history-info">
                <strong>{taskKindName(task.kind)}</strong>
                <span title={task.path ?? task.message ?? ''}>
                  {task.path ? shortPath(task.path, home) : (task.message ?? '未记录目录')}
                </span>
                {task.state !== 'succeeded' && task.path && task.message && (
                  <small>{task.message}</small>
                )}
              </span>
              <time title={taskTimeLabel(task.createdAt)}>
                {taskTime(task.createdAt) == null ? '时间未知' : dateTime(task.createdAt)}
              </time>
              <span
                className={`state-tag ${taskStates.some((state) => state === task.state) ? task.state : 'unknown'}`}
              >
                {taskStateName(task.state)}
              </span>
              <ChevronRight size={14} aria-hidden="true" />
            </button>
            {canFollowTask(task) && (
              <button
                className="button secondary compact"
                disabled={!canTrack}
                onClick={() => onTrack(task)}
              >
                继续跟踪
              </button>
            )}
          </article>
        ))}
        {!matching.length && (
          <div className="empty-inline">
            <p>
              {records.length
                ? '没有匹配的检查任务。'
                : '尚无客户端检查任务。完成一次检查后会显示在这里。'}
            </p>
            {(query || state !== 'all') && (
              <button className="button secondary compact" onClick={reset}>
                清除筛选
              </button>
            )}
          </div>
        )}
        {matching.length > visible.length && (
          <div className="task-history-more">
            <button
              className="button secondary compact"
              onClick={() => setLimit((current) => current + BATCH)}
            >
              再显示 50 条
            </button>
          </div>
        )}
        <details className="task-history-scope">
          <summary>记录范围与保留说明</summary>
          <p>
            这里只展示客户端 SQLite 检查记录，和 Mole
            原始历史、逐项执行记录分开。按开始时间倒序读取最多最近 200
            条；搜索与筛选只作用于这些记录，不代表所有历史。保留期限在设置中调整。
          </p>
          <p>
            记录保存状态和诊断，不包含旧扫描结果，不是清理、释放空间或恢复凭证。运行状态以接回后的实际查询为准。
          </p>
        </details>
      </section>
      {selected != null && (
        <Modal title="检查任务详情" onClose={closeDetail} className="task-detail-modal">
          {detail ? (
            <>
              <h2>{taskKindName(detail.kind)}</h2>
              <p className="task-detail-state">{taskStateName(detail.state)}</p>
              <dl className="task-detail-fields">
                <dt>记录目录</dt>
                <dd className="mono">{detail.path ?? '未记录；此类检查可能不使用目录。'}</dd>
                <dt>开始时间</dt>
                <dd>{taskTimeLabel(detail.createdAt)}</dd>
                <dt>结束时间</dt>
                <dd>
                  {detail.finishedAt == null && detail.state === 'running'
                    ? '尚未记录结束'
                    : taskTimeLabel(detail.finishedAt)}
                </dd>
                <dt>记录用时</dt>
                <dd>{taskDuration(detail)}</dd>
                <dt>诊断信息</dt>
                <dd>{detail.message ?? '未记录诊断信息。'}</dd>
              </dl>
              <p className="muted">
                起止时间差仅作参考，不等于引擎测量耗时。此记录不包含旧扫描结果，也不是清理或恢复凭证。
              </p>
              {canFollowTask(detail) && (
                <button
                  className="button secondary"
                  disabled={!canTrack}
                  onClick={() => {
                    onTrack(detail)
                    closeDetail()
                  }}
                >
                  继续跟踪
                </button>
              )}
              <details className="task-history-scope">
                <summary>记录信息</summary>
                <p>来源：客户端 SQLite 检查任务</p>
                <p className="mono">任务 ID：{detail.id}</p>
                <p>
                  原始类型：{detail.kind} · 原始状态：{detail.state}
                </p>
              </details>
              <DiagnosticExport key={detail.id} taskId={detail.id} />
            </>
          ) : (
            <p role="status">
              此任务已不在当前读取列表中，可能超过保留期限或最近 200
              条范围。请刷新列表核对当前记录。
            </p>
          )}
        </Modal>
      )}
    </>
  )
}
