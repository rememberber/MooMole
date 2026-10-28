import { getLocale } from '../i18n/locale'
import { useMemo, useState } from 'react'
import { Search } from 'lucide-react'
import type { Snapshot } from '../types'
import { bytes, dateTime, percent } from '../lib/format'
import {
  matchingProcesses,
  processMetric,
  processSampleTime,
  type ProcessSort,
} from '../lib/process-snapshot'

export function ProcessSnapshot({
  snapshot,
  refreshing,
}: {
  snapshot: Snapshot
  refreshing: boolean
}) {
  const [query, setQuery] = useState('')
  const [sort, setSort] = useState<ProcessSort>('cpu')
  const samples = Array.isArray(snapshot.top_processes) ? snapshot.top_processes : null
  const matching = useMemo(
    () => matchingProcesses(samples ?? [], query, sort),
    [samples, query, sort, getLocale()],
  )
  const sampleTime = processSampleTime(snapshot.process_collected_at)
  return (
    <section className="card process-snapshot" aria-label="Mole 进程快照">
      <div className="list-toolbar process-toolbar">
        <h2>进程快照</h2>
        <label className="search-field">
          <Search size={14} aria-hidden="true" />
          <input
            aria-label="搜索进程"
            placeholder="名称或 PID…"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        <select
          aria-label="进程排序"
          value={sort}
          onChange={(event) => setSort(event.target.value as ProcessSort)}
        >
          <option value="cpu">CPU 从高到低</option>
          <option value="memory">驻留内存从大到小</option>
          <option value="name">名称顺序</option>
          <option value="pid">PID 顺序</option>
        </select>
      </div>
      <div className="process-summary">
        <p>
          {samples ? `显示 ${matching.length} / ${samples.length} 个已返回样本` : '进程样本数未知'}
          {' · '}
          {snapshot.process_stale === true
            ? '复用的较早样本'
            : snapshot.process_stale === false
              ? '采样时为新样本'
              : '样本新鲜度未知'}
        </p>
        <p>
          样本时间：
          {sampleTime ? <time dateTime={sampleTime}>{dateTime(sampleTime)}</time> : '未知'}
          {' · '}仅对返回样本筛选和排序，并非全部进程。
        </p>
        {refreshing && <p role="status">正在刷新，当前仍是上次快照。</p>}
      </div>
      {matching.length ? (
        <table className="process-table" aria-label="进程样本">
          <thead>
            <tr>
              <th scope="col">名称</th>
              <th scope="col">PID</th>
              <th scope="col">CPU</th>
              <th scope="col">驻留内存</th>
            </tr>
          </thead>
          <tbody>
            {matching.map((sample, index) => (
              <tr key={`${sample.pid}:${index}`}>
                <th scope="row" data-i18n="off">
                  {sample.name}
                </th>
                <td className="mono">{sample.pid}</td>
                <td>{processMetric(sample.cpu) == null ? '未知' : percent(sample.cpu!)}</td>
                <td>
                  {processMetric(sample.memory_bytes) == null ? '未知' : bytes(sample.memory_bytes)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <div className="empty-inline">
          <p>
            {samples == null
              ? '此快照未提供进程列表。'
              : samples.length === 0
                ? 'Mole 没有返回进程记录。'
                : '当前样本中没有匹配的进程。'}
          </p>
          {query && (
            <button className="button secondary compact" onClick={() => setQuery('')}>
              清除搜索
            </button>
          )}
        </div>
      )}
      <details className="process-scope">
        <summary>采样范围与指标说明</summary>
        <p>
          内置 Mole 按 CPU 优先选出最多 5
          个进程样本。搜索和内存排序仅作用于这些记录，不能据此判断全机内存排名。
        </p>
        <p>
          进程 CPU 沿用 ps 报告值，可超过 100%；与整机 CPU 采样口径不同，不能相加或据此计算剩余
          CPU。 驻留内存来自 RSS 换算，缺失时不根据内存百分比推算，也不代表可释放空间。
        </p>
        <p>样本时间可能早于整份快照；此处只供查阅，不会结束进程。</p>
      </details>
    </section>
  )
}
