import { useEffect, useMemo, useRef, useState } from 'react'
import { Search } from 'lucide-react'
import { ApplicationRow } from './ApplicationRow'
import type { AppEntry } from '../types'
import { appEstimateLabel, appSizeEstimate, compareApps } from '../lib/application-library'
import type { UninstallTarget } from '../lib/app-uninstall'

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
}: {
  apps: AppEntry[]
  taskId?: string | null
  protectedPaths?: string[]
  home: string
  refreshing?: boolean
  canReveal: boolean
  onReveal: (path: string) => void
  canUninstall?: boolean
  onUninstall?: (target: UninstallTarget) => void
}) {
  const [query, setQuery] = useState('')
  const [source, setSource] = useState('all')
  const [sort, setSort] = useState<'size' | 'name'>('size')
  const [onlySelected, setOnlySelected] = useState(false)
  const [limit, setLimit] = useState(100)
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
            (!onlySelected || selected.has(app.path)) &&
            `${app.name}\n${app.bundle_id}\n${app.path}\n${app.source}`
              .toLocaleLowerCase('zh-CN')
              .includes(query.trim().toLocaleLowerCase('zh-CN'))
          )
        })
        .sort((a, b) => compareApps(a, b, sort)),
    [apps, query, source, sort, onlySelected, selected],
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
  const hasFilters = Boolean(query || source !== 'all' || onlySelected)
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
            taskId={taskId}
            protectedPaths={protectedPaths}
            index={indices.get(app.path)!}
            home={home}
            selected={selected.has(app.path)}
            onSelect={(checked) => toggle([app.path], checked)}
            canReveal={canReveal}
            onReveal={onReveal}
            canUninstall={canUninstall && !refreshing}
            onUninstall={onUninstall}
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
