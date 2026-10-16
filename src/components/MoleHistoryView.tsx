import { useState } from 'react'
import { RefreshCw } from 'lucide-react'
import { bytes, dateTime } from '../lib/format'
import type { MoleHistory, MoleHistorySource } from '../types'

const commands: Record<string, string> = {
  clean: '清理',
  uninstall: '卸载',
  optimize: '维护',
  purge: '工程产物',
  installer: '安装包',
}
const modes: Record<string, string> = { permanent: '永久删除', trash: '移入废纸篓' }
const statuses: Record<string, string> = {
  ok: '记录为成功',
  error: '记录为错误',
  'dry-run': '仅预览',
  interrupted: '记录为中断',
  'timed-out': '记录为超时',
  'privacy-denied': '隐私权限被拒绝',
  'trash-failed': '移入废纸篓失败',
  'mutable-parent': '父目录变化',
}
const attribution = { run: 'run_id 归属', command: '仅命令级归属', ambiguous: '归属不明确' }

function Source({ name, value }: { name: string; value: MoleHistorySource }) {
  return (
    <div>
      <strong>
        {name} ·{' '}
        {value.state === 'ready' ? '已读取' : value.state === 'missing' ? '未找到日志' : '无法读取'}
      </strong>
      <p className="mole-history-path">{value.path}</p>
      {value.reason && <p className="notice warning">{value.reason}</p>}
    </div>
  )
}

export function MoleHistoryView({
  report,
  disabled,
  reading,
  onRead,
}: {
  report: MoleHistory | null
  disabled: boolean
  reading: boolean
  onRead: () => void
}) {
  const [query, setQuery] = useState('')
  const [command, setCommand] = useState('all')
  const [status, setStatus] = useState('all')
  const [sessionLimit, setSessionLimit] = useState(20)
  const [deletionLimit, setDeletionLimit] = useState(20)
  const needle = query.toLocaleLowerCase()
  const sessions =
    report?.sessions.filter(
      (s) =>
        (command === 'all' || s.command === command) &&
        [s.command, commands[s.command] ?? '', s.run_id, s.started_at, s.ended_at].some((v) =>
          v.toLocaleLowerCase().includes(needle),
        ),
    ) ?? []
  const deletions =
    report?.deletions.filter(
      (d) =>
        (status === 'all' || d.status === status) &&
        [d.path, d.timestamp, d.mode, d.status].some((v) => v.toLocaleLowerCase().includes(needle)),
    ) ?? []
  return (
    <section className="card mole-history" aria-label="Mole CLI 原始历史">
      <div className="list-toolbar">
        <h2>Mole CLI 原始历史</h2>
        <button className="button secondary compact" disabled={disabled} onClick={onRead}>
          <RefreshCw size={14} className={reading ? 'spin' : ''} />
          {reading ? '读取中…' : report ? '重新读取 Mole 历史' : '读取 Mole 历史'}
        </button>
      </div>
      <p className="muted mole-history-intro">
        从当前账户的 Mole 日志读取，不计入客户端执行记录。原日志由 Mole
        管理，不受客户端保留天数控制。
      </p>
      {report ? (
        <>
          <p className="muted">
            读取于 {dateTime(report.collectedAt)} · 每类最多 {report.limit} 条
            {reading ? ' · 正在更新，以下为上次结果' : ''}
          </p>
          <details className="execution-metadata">
            <summary>日志来源与读取范围</summary>
            <Source name="操作日志" value={report.operationsSource} />
            <Source name="删除日志" value={report.deletionsSource} />
            <p className="muted">
              仅读取默认位置的当前日志，不含轮转归档或其他账户。使用内置固定版 Mole 的 JSON
              解析结果；旧格式或无法识别的行可能不出现在列表中。
            </p>
          </details>
          {(report.operationsSource.state === 'unavailable' ||
            report.deletionsSource.state === 'unavailable') && (
            <p className="notice warning" role="alert">
              部分历史无法读取，请展开日志来源查看原因；空列表不代表没有发生过操作。
            </p>
          )}
          <div className="execution-filters">
            <input
              aria-label="搜索 Mole 历史"
              placeholder="搜索路径、命令、run_id 或时间"
              value={query}
              onChange={(e) => {
                setQuery(e.target.value)
                setSessionLimit(20)
                setDeletionLimit(20)
              }}
            />
          </div>
          <div className="mole-history-section">
            <div className="list-toolbar">
              <h3>CLI 会话</h3>
              <select
                aria-label="Mole 历史命令"
                value={command}
                onChange={(e) => {
                  setCommand(e.target.value)
                  setSessionLimit(20)
                }}
              >
                <option value="all">全部命令</option>
                {[...new Set(report.sessions.map((s) => s.command))].map((value) => (
                  <option key={value} value={value}>
                    {commands[value] ?? value}
                  </option>
                ))}
              </select>
            </div>
            <p className="muted" role="status">
              显示 {Math.min(sessionLimit, sessions.length)} / {sessions.length} 个会话
            </p>
            {sessions.slice(0, sessionLimit).map((s, i) => (
              <details
                className="execution-item"
                key={`${report.collectedAt}:${i}:${s.command}:${s.run_id}`}
              >
                <summary>
                  <span>
                    {commands[s.command] ?? s.command} · {s.started_at || '未记录开始时间'}
                  </span>
                  <span className="tag">{attribution[s.attribution]}</span>
                </summary>
                <div className="mole-history-detail">
                  <p>
                    命令：{s.command} · run_id：{s.run_id || '未记录'}
                  </p>
                  <p>结束时间：{s.ended_at || '未记录结束，无法判断是否仍在运行'}</p>
                  <p>
                    上游汇总：{s.items} 项 · {s.size} · {s.operation_count} 条操作记录 · 失败任务{' '}
                    {s.failed_tasks}
                  </p>
                  <p>
                    操作记录：永久删除 {s.actions.removed} · 移入废纸篓 {s.actions.trashed} · 跳过{' '}
                    {s.actions.skipped} · 失败 {s.actions.failed} · 重建 {s.actions.rebuilt} · 其他{' '}
                    {s.actions.other}
                  </p>
                  {s.attribution !== 'run' && (
                    <p className="notice warning">
                      {s.attribution === 'ambiguous'
                        ? '上游标记为归属不明确，不合并为某次确定运行。'
                        : '缺少 run_id，仅保留命令级汇总，不能确定对应哪次运行。'}
                    </p>
                  )}
                </div>
              </details>
            ))}
            {!sessions.length && (
              <p className="muted">
                {report.operationsSource.state === 'missing'
                  ? '尚未找到 Mole 操作日志。'
                  : report.operationsSource.state === 'unavailable'
                    ? '操作日志无法读取。'
                    : report.sessions.length
                      ? '没有匹配的会话。'
                      : '当前日志中没有可识别的会话。'}
              </p>
            )}
            {sessions.length > sessionLimit && (
              <button
                className="button secondary compact"
                onClick={() => setSessionLimit((n) => n + 20)}
              >
                再显示 20 个会话
              </button>
            )}
          </div>
          <div className="mole-history-section">
            <div className="list-toolbar">
              <h3>删除日志</h3>
              <select
                aria-label="Mole 删除日志状态"
                value={status}
                onChange={(e) => {
                  setStatus(e.target.value)
                  setDeletionLimit(20)
                }}
              >
                <option value="all">全部日志状态</option>
                {[...new Set(report.deletions.map((d) => d.status))].map((value) => (
                  <option key={value} value={value}>
                    {statuses[value] ?? `原始状态：${value}`}
                  </option>
                ))}
              </select>
            </div>
            <p className="notice">
              这些记录没有
              run_id，无法可靠对应到上面的会话。日志不证明目标当前状态，也不提供恢复所需的实际位置和身份；这里只查看。
            </p>
            <p className="muted" role="status">
              显示 {Math.min(deletionLimit, deletions.length)} / {deletions.length} 条删除日志
            </p>
            {deletions.slice(0, deletionLimit).map((d, i) => (
              <details
                className="execution-item"
                key={`${report.collectedAt}:${i}:${d.timestamp}:${d.path}`}
              >
                <summary>
                  <span>{d.path || '未记录路径'}</span>
                  <span className="tag">{statuses[d.status] ?? `原始状态：${d.status}`}</span>
                </summary>
                <div className="mole-history-detail">
                  <p>
                    {d.timestamp} · {modes[d.mode] ?? `原始方式：${d.mode}`} ·{' '}
                    {d.size_kb == null ? '大小未知' : bytes(d.size_kb * 1024)}
                  </p>
                  <p className="mole-history-path">{d.path}</p>
                </div>
              </details>
            ))}
            {!deletions.length && (
              <p className="muted">
                {report.deletionsSource.state === 'missing'
                  ? '尚未找到 Mole 删除日志。'
                  : report.deletionsSource.state === 'unavailable'
                    ? '删除日志无法读取。'
                    : report.deletions.length
                      ? '没有匹配的删除日志。'
                      : '当前日志中没有可识别的删除记录。'}
              </p>
            )}
            {deletions.length > deletionLimit && (
              <button
                className="button secondary compact"
                onClick={() => setDeletionLimit((n) => n + 20)}
              >
                再显示 20 条删除日志
              </button>
            )}
          </div>
        </>
      ) : (
        <p className="muted">
          {reading
            ? '正在读取本机日志…'
            : '点击读取查看 CLI 历史；此操作不会执行清理。浏览器预览无法读取本机。'}
        </p>
      )}
    </section>
  )
}
