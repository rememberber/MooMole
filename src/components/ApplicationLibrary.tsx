import { getLocale } from '../i18n/locale'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Search } from 'lucide-react'
import { ApplicationRow } from './ApplicationRow'
import type { AppEntry } from '../types'
import { appEstimateLabel, appSizeEstimate, compareApps } from '../lib/application-library'
import type { AppControlTarget } from '../lib/app-control'
import type { UninstallTarget } from '../lib/app-uninstall'
import { useApplicationUsage } from '../lib/use-application-usage'
import { matchesUsage, type UsageFilter } from '../lib/application-usage'

export function ApplicationLibrary({
  apps,
  taskId,
  protectedPaths = [],
  home,
  refreshing = false,
  canReveal,
  onReveal,
  canUninstall = false,
  onUninstall,
  onManagedUninstall,
  onControl,
  canControl = false,
  onUpdate,
  onDiscoverUpdate,
}: {
  apps: AppEntry[]
  taskId?: string | null
  protectedPaths?: string[]
  home: string
  refreshing?: boolean
  canReveal: boolean
  onReveal: (path: string) => void
  canControl?: boolean
  onControl?: (target: AppControlTarget) => void
  onUpdate?: (target: UninstallTarget) => void
  onDiscoverUpdate?: (target: UninstallTarget) => void
  canUninstall?: boolean
  onUninstall?: (target: UninstallTarget) => void
  onManagedUninstall?: (target: UninstallTarget) => void
}) {
  const [query, setQuery] = useState('')
  const [source, setSource] = useState('all')
  const [sort, setSort] = useState<'size' | 'name'>('size')
  const [onlySelected, setOnlySelected] = useState(false)
  const [limit, setLimit] = useState(100)
  const usage = useApplicationUsage(apps, taskId, refreshing, JSON.stringify(protectedPaths))
  const [usageFilter, setUsageFilter] = useState<UsageFilter>('all')
  const usageRows = useMemo(
    () => new Map(usage.fresh ? usage.report!.rows.map((row) => [row.index, row]) : []),
    [usage.fresh, usage.report],
  )
  const effectiveUsageFilter = usage.fresh ? usageFilter : 'all'
  // Bind a comparison to this received list. A refresh must not transfer a
  // selection to a new observation, even when names or paths are unchanged.
  const [selection, setSelection] = useState({ apps, paths: new Set<string>() })
  const selected = useMemo(
    () => (selection.apps === apps ? selection.paths : new Set<string>()),
    [apps, selection],
  )
  const indices = useMemo(() => new Map(apps.map((app, index) => [app.path, index])), [apps])
  const pageCheck = useRef<HTMLInputElement>(null)
  const matching = useMemo(
    () =>
      apps
        .filter((app) => {
          const kind = ['App', 'Homebrew'].includes(app.source) ? app.source : 'unknown'
          return (
            (source === 'all' || source === kind) &&
            matchesUsage(
              usageRows.get(indices.get(app.path)!),
              effectiveUsageFilter,
              usage.report?.checkedAt ?? 0,
            ) &&
            (!onlySelected || selected.has(app.path)) &&
            `${app.name}\n${app.bundle_id}\n${app.path}\n${app.source}`
              .toLocaleLowerCase(getLocale())
              .includes(query.trim().toLocaleLowerCase(getLocale()))
          )
        })
        .sort((a, b) => compareApps(a, b, sort)),
    [
      apps,
      query,
      source,
      sort,
      onlySelected,
      selected,
      getLocale(),
      usageRows,
      effectiveUsageFilter,
      usage.report?.checkedAt,
      indices,
    ],
  )
  const visible = matching.slice(0, limit)
  const pageSelected = visible.filter((app) => selected.has(app.path)).length
  useEffect(() => {
    if (pageCheck.current)
      pageCheck.current.indeterminate = pageSelected > 0 && pageSelected < visible.length
  }, [pageSelected, visible.length])
  const chosen = apps.filter((app) => selected.has(app.path))
  const unknown = chosen.filter((app) => appSizeEstimate(app.size) == null).length
  const estimate = chosen.reduce((total, app) => total + (appSizeEstimate(app.size) ?? 0), 0)
  const hasFilters = Boolean(
    query || source !== 'all' || onlySelected || effectiveUsageFilter !== 'all',
  )
  function toggle(paths: string[], checked: boolean) {
    const next = new Set(selected)
    for (const path of paths) {
      if (checked) next.add(path)
      else next.delete(path)
    }
    setSelection({ apps, paths: next })
  }
  return (
    <section className="card application-library" aria-label="Mole 应用清单">
      <div className="list-toolbar">
        <h2>{apps.length} 个应用</h2>
        <label className="search-field">
          <Search size={14} aria-hidden="true" />
          <input
            aria-label="搜索应用"
            placeholder="名称、Bundle ID 或路径…"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value)
              setLimit(100)
            }}
          />
        </label>
        <select
          aria-label="应用来源"
          value={source}
          onChange={(event) => {
            setSource(event.target.value)
            setLimit(100)
          }}
        >
          <option value="all">全部来源</option>
          <option value="App">普通应用</option>
          <option value="Homebrew">Homebrew</option>
          <option value="unknown">来源待确认</option>
        </select>
        <select
          aria-label="应用排序"
          value={sort}
          onChange={(event) => {
            setSort(event.target.value as 'size' | 'name')
            setLimit(100)
          }}
        >
          <option value="size">占用从大到小</option>
          <option value="name">名称顺序</option>
        </select>
      </div>
      {taskId && (
        <div className="list-toolbar">
          <button
            className="button secondary compact"
            disabled={!canReveal || refreshing || usage.pending || !apps.length}
            onClick={() => void usage.read()}
          >
            {usage.report ? '重新检查使用线索' : '检查使用线索'}
          </button>
          {usage.pending && (
            <button
              className="button secondary compact"
              disabled={usage.stopping}
              onClick={() => void usage.cancel()}
            >
              {usage.stopping ? '已请求停止' : '停止检查使用线索'}
            </button>
          )}
          <select
            aria-label="应用使用线索"
            disabled={!usage.fresh || usage.pending}
            value={effectiveUsageFilter}
            onChange={(e) => {
              setUsageFilter(e.target.value as UsageFilter)
              setLimit(100)
            }}
          >
            <option value="all">全部使用线索</option>
            <option value="90">90 天前的打开记录</option>
            <option value="180">180 天前的打开记录</option>
            <option value="365">365 天前的打开记录</option>
            <option value="unknown">使用记录未知</option>
          </select>
          <small>
            {usage.pending
              ? '正在读取所列应用的系统打开记录…'
              : usage.report
                ? usage.fresh
                  ? `本轮已检查 ${usage.report.rows.length} / ${apps.length} 项`
                  : '使用线索已过期，请重新检查。'
                : '按需检查，不自动选择或卸载。'}
          </small>
        </div>
      )}
      {usage.error && (
        <p className="notice warning" role="alert">
          {usage.error}
        </p>
      )}
      {usage.report?.cancelled && (
        <p className="application-scope">本轮使用线索检查已停止，已返回结果保留。</p>
      )}
      {usage.report && (
        <p className="application-scope">
          系统打开记录可能缺失或滞后，也不覆盖后台运行；较早记录不证明一直未用。正在运行的应用不进入较早记录筛选，未知不按未用处理。卸载仍需另行检查和确认。
        </p>
      )}
      {usage.report && (!usage.report.complete || apps.length > 512) && (
        <p className="notice warning">
          使用线索检查未完整覆盖，未检查范围保持未知；每轮最多 512 项。
        </p>
      )}
      <details className="application-scope">
        <summary>列表范围与大小说明</summary>
        <p>
          来自内置 Mole 的 uninstall --list。搜索范围包含系统与当前账户的 Applications、Input
          Methods，已有的 PlayCover 目录、可读取的外接卷
          Applications，以及安装收据识别的非标准位置；上游会跳过部分应用并去重，清单不代表所有安装副本。
        </p>
        <p>
          大小沿用 Mole
          的应用包估算，可能来自扫描缓存，不包含全部关联数据。排序和对比按上游十进制单位换算，保留舍入误差；不是可释放空间。普通应用仅表示未被识别为
          Homebrew，不代表 App Store 来源。未知大小不计为零。
        </p>
      </details>
      {refreshing && (
        <p className="application-refresh" role="status">
          正在重新读取，当前显示上次结果。
        </p>
      )}
      <div className="application-selection">
        <label>
          <input
            ref={pageCheck}
            type="checkbox"
            aria-label="勾选当前显示的应用"
            checked={visible.length > 0 && pageSelected === visible.length}
            disabled={!visible.length}
            onChange={(event) =>
              toggle(
                visible.map((app) => app.path),
                event.target.checked,
              )
            }
          />
          当前显示
        </label>
        <p role="status">
          {chosen.length ? (
            <>
              已勾选 {chosen.length} 项 ·{' '}
              {unknown === chosen.length
                ? '所选大小均未知'
                : Number.isSafeInteger(Math.ceil(estimate))
                  ? `已知估算约 ${appEstimateLabel(estimate)}`
                  : '估算超出汇总范围'}
              {unknown > 0 && unknown < chosen.length && ` · ${unknown} 项大小未知`}
              {chosen.length > pageSelected && ` · ${chosen.length - pageSelected} 项未显示`}
            </>
          ) : (
            '勾选可对比占用，不会卸载。'
          )}
        </p>
        <label>
          <input
            type="checkbox"
            checked={onlySelected}
            onChange={(event) => {
              setOnlySelected(event.target.checked)
              setLimit(100)
            }}
          />
          只看已勾选
        </label>
        {chosen.length > 0 && (
          <button
            className="button text compact"
            onClick={() => setSelection({ apps, paths: new Set() })}
          >
            清空勾选
          </button>
        )}
      </div>
      <div className="application-results" aria-label="应用结果">
        {visible.map((app) => (
          <ApplicationRow
            key={app.path}
            app={app}
            usage={usageRows.get(indices.get(app.path)!)}
            taskId={taskId}
            protectedPaths={protectedPaths}
            index={indices.get(app.path)!}
            home={home}
            selected={selected.has(app.path)}
            onSelect={(checked) => toggle([app.path], checked)}
            canReveal={canReveal}
            onReveal={onReveal}
            canControl={canControl && !refreshing}
            onControl={onControl}
            onUpdate={onUpdate}
            onDiscoverUpdate={onDiscoverUpdate}
            canUninstall={canUninstall && !refreshing}
            onUninstall={onUninstall}
            onManagedUninstall={onManagedUninstall}
          />
        ))}
        {!visible.length && (
          <div className="empty-inline">
            <p>{apps.length ? '没有符合条件的应用。' : 'Mole 没有返回应用。'}</p>
            {hasFilters && (
              <button
                className="button text"
                onClick={() => {
                  setQuery('')
                  setSource('all')
                  setOnlySelected(false)
                  setUsageFilter('all')
                  setLimit(100)
                }}
              >
                清除筛选
              </button>
            )}
          </div>
        )}
      </div>
      <div className="application-footer">
        <span>
          显示 {visible.length} / {matching.length} 个匹配应用
        </span>
        {matching.length > limit && (
          <button
            className="button secondary compact"
            onClick={() => setLimit((current) => current + 100)}
          >
            再显示 100 项
          </button>
        )}
        <small>勾选仅用于本页对比，列表刷新成功或离开本页后清空。</small>
      </div>
    </section>
  )
}
