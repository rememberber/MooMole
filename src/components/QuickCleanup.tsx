import { getLocale } from '../i18n/locale'
import { useEffect, useMemo, useRef, useState } from 'react'
import type { Analysis, AppEntry, CleanPreview, Page, ReviewPlan } from '../types'
import type { UninstallTarget } from '../lib/app-uninstall'
import { pathOverlaps, previewTotal, protectionFor } from '../lib/clean-preview'
import { bytes, shortPath } from '../lib/format'
import { useNow } from '../lib/use-now'
import {
  quickGroup,
  quickGroups,
  quickInspectionIds,
  quickSupported,
  recommendedQuickIds,
} from '../lib/quick-cleanup'
import { cleanupPreparation } from '../lib/cleanup-preparation'
import { appSizeEstimate, compareApps } from '../lib/application-library'
import { largeFileReport, largeFileInScope } from '../lib/large-files'

export function QuickCleanup({
  preview,
  home,
  protectedPaths,
  desktop,
  enabled,
  onCreatePlan,
  onNavigate,
  onInspect,
  onProtect,
  onReveal,
  scanRunning = false,
  apps = null,
  appsTaskId,
  appsStale = false,
  analysis = null,
  onUninstall,
}: {
  preview: CleanPreview | null
  home: string
  protectedPaths: string[]
  desktop: boolean
  enabled: boolean
  onCreatePlan: (scanId: string, ids: string[]) => Promise<void>
  onNavigate: (page: Page) => void
  scanRunning?: boolean
  onInspect?: (scanId: string, ids: string[], signal: AbortSignal) => Promise<ReviewPlan>
  onProtect?: (path: string) => Promise<void>
  onReveal?: (path: string) => void
  apps?: AppEntry[] | null
  appsTaskId?: string | null
  appsStale?: boolean
  analysis?: Analysis | null
  onUninstall?: (target: UninstallTarget) => void
}) {
  const [selected, setSelected] = useState<string[]>([])
  const [pending, setPending] = useState(false)
  const [checking, setChecking] = useState(false)
  const [inspection, setInspection] = useState<ReviewPlan[]>([])
  const [error, setError] = useState('')
  const [inspectionErrors, setInspectionErrors] = useState<Record<string, string>>({})
  const [query, setQuery] = useState('')
  const [onlySelected, setOnlySelected] = useState(false)
  const [limit, setLimit] = useState(100)
  const touched = useRef(false)
  const inspector = useRef(onInspect)
  inspector.current = onInspect
  const now = useNow()
  const fresh = !!(
    desktop &&
    preview?.protocol === 2 &&
    preview.scanId &&
    preview.scannedAt != null &&
    preview.expiresAt != null &&
    now >= preview.scannedAt &&
    now < preview.expiresAt
  )
  const items = useMemo(
    () =>
      preview?.items.map((item) => ({
        item,
        protected: protectionFor(item, protectedPaths).length > 0,
        supported: quickSupported(item, home),
      })) ?? [],
    [preview, protectedPaths, home],
  )
  const protectionsKey = JSON.stringify(protectedPaths)
  useEffect(() => {
    const abort = new AbortController()
    touched.current = false
    setSelected([])
    setInspection([])
    setInspectionErrors({})
    setError('')
    setChecking(false)
    if (fresh && !scanRunning && preview?.scanId && inspector.current) {
      const ids = quickInspectionIds(preview.items, home, protectedPaths)
      if (ids.length) {
        setChecking(true)
        const inspect = inspector.current
        const scanId = preview.scanId
        void (async () => {
          try {
            for (const id of ids) {
              if (abort.signal.aborted) return
              try {
                const checked = await inspect(scanId, [id], abort.signal)
                if (abort.signal.aborted) return
                setInspection((previous) => [...previous, checked])
                if (!touched.current)
                  setSelected((previous) => [
                    ...new Set([...previous, ...recommendedQuickIds(checked, Date.now())]),
                  ])
              } catch (reason) {
                if (abort.signal.aborted) return
                setInspectionErrors((previous) => ({
                  ...previous,
                  [id]: String(reason).replace(/^Error: /, ''),
                }))
              }
            }
          } finally {
            if (!abort.signal.aborted) setChecking(false)
          }
        })()
      }
    }
    return () => abort.abort()
  }, [preview, home, protectionsKey, fresh, scanRunning])
  const chosen = items.filter(
    ({ item, protected: blocked, supported }) =>
      selected.includes(item.id) && !blocked && supported && fresh,
  )
  const total = previewTotal(chosen.map(({ item }) => item))
  const preparations = new Map(
    inspection.flatMap((plan) => {
      const report = cleanupPreparation(plan, now)
      return report?.valid ? [...report.items] : []
    }),
  )
  const recommended = inspection.flatMap((plan) => recommendedQuickIds(plan, now))
  const needle = query.trim().toLocaleLowerCase(getLocale())
  const matches = (path: string) => path.toLocaleLowerCase(getLocale()).includes(needle)
  const protectedPath = (path: string) => protectedPaths.some((rule) => pathOverlaps(rule, path))
  const largeApps = useMemo(
    () =>
      apps
        ?.filter((app) => (appSizeEstimate(app.size) ?? 0) >= 500_000_000)
        .sort((a, b) => compareApps(a, b, 'size')) ?? [],
    [apps],
  )
  const largeFiles = analysis
    ? largeFileReport(analysis)
        .files.filter((file) => largeFileInScope(file.path, analysis.path))
        .sort((a, b) => b.size - a.size)
    : []
  function select(ids: string[], checked: boolean) {
    touched.current = true
    setSelected((current) =>
      checked
        ? [...new Set([...current, ...ids])].slice(0, 20)
        : current.filter((id) => !ids.includes(id)),
    )
  }
  async function prepare() {
    if (
      !fresh ||
      !enabled ||
      !preview?.scanId ||
      Date.now() < preview.scannedAt! ||
      Date.now() >= preview.expiresAt! ||
      !chosen.length ||
      chosen.length > 20 ||
      pending
    )
      return
    setPending(true)
    setError('')
    try {
      await onCreatePlan(
        preview.scanId,
        chosen.map(({ item }) => item.id),
      )
    } catch (reason) {
      setError(String(reason).replace(/^Error: /, ''))
    } finally {
      setPending(false)
    }
  }
  const protect = async (path: string) => {
    select(
      items.filter(({ item }) => pathOverlaps(item.path, path)).map(({ item }) => item.id),
      false,
    )
    try {
      await onProtect?.(path)
    } catch (reason) {
      setError(String(reason))
    }
  }
  return (
    <div className="quick-cleanup">
      <div className="quick-action card quick-selection">
        <div>
          <strong>
            已选 {chosen.length} 项 · {bytes(total.size)}
            {total.unknown ? `，另有 ${total.unknown} 项大小未知` : ''}
          </strong>
          <p className="quick-intro">
            {checking
              ? '正在检查默认推荐项，可继续浏览。'
              : recommended.length
                ? `${recommended.length} 项低风险缓存通过本轮检查；清理前仍会复核。`
                : '本轮暂无通过默认推荐检查的缓存，可自行审阅选择。'}
          </p>
        </div>
        <button
          className="button primary"
          disabled={!fresh || !enabled || !chosen.length || pending || checking}
          onClick={() => void prepare()}
        >
          {pending ? '正在准备…' : '一键清理'}
        </button>
      </div>
      <p className="quick-intro">
        默认只推荐已检查的帮助、图标等可重建缓存。其他缓存、日志和诊断需自行选择；大文件和大应用需单独审阅。清理移入废纸篓，逐项结果保存到历史。
      </p>
      {!preview ? (
        <div className="card quick-empty">
          点击顶部“扫描可清理项”，汇总缓存、日志、大文件和大应用。
        </div>
      ) : (
        <>
          {!fresh && (
            <p className="notice warning">
              扫描已过期、缺少有效凭据或当前为浏览器示例。请在桌面 App 中重新扫描。
            </p>
          )}
          <div className="quick-action">
            <input
              className="quick-search"
              type="search"
              aria-label="搜索清理项目"
              placeholder="搜索名称或路径…"
              value={query}
              onChange={(event) => {
                setQuery(event.target.value)
                setLimit(100)
              }}
            />
            <label>
              <input
                type="checkbox"
                checked={onlySelected}
                onChange={(event) => setOnlySelected(event.target.checked)}
              />{' '}
              只看已选
            </label>
            <button
              className="button secondary compact"
              disabled={!enabled || checking || !recommended.length}
              onClick={() => {
                touched.current = true
                setSelected(recommended)
              }}
            >
              恢复推荐
            </button>
            <button
              className="button secondary compact"
              disabled={!selected.length || pending}
              onClick={() => {
                touched.current = true
                setSelected([])
              }}
            >
              清空选择
            </button>
          </div>
          {quickGroups.map(([id, label]) => {
            const all = items.filter(({ item }) => quickGroup(item) === id)
            const members = all
              .filter(
                ({ item }) =>
                  matches(item.path) &&
                  (!onlySelected || chosen.some((entry) => entry.item.id === item.id)),
              )
              .sort((a, b) => (b.item.size ?? -1) - (a.item.size ?? -1))
            const summary = previewTotal(all.map(({ item }) => item))
            const eligible = members
              .filter((entry) => entry.supported && !entry.protected)
              .map(({ item }) => item.id)
            return (
              <details className="card quick-group" key={id} open={id === 'cache' || id === 'logs'}>
                <summary>
                  <strong>{label}</strong>
                  <span>
                    {all.length} 项 · {bytes(summary.size)}
                    {summary.unknown ? ` + ${summary.unknown} 项未知` : ''}
                  </span>
                </summary>
                {id === 'orphan' && (
                  <p className="quick-group-empty">
                    来自 Mole
                    卸载残留扫描的结构化候选。归属与共享保护仍需专项复查，本轮仅展示并保留，不自动删除。
                  </p>
                )}
                {members.length ? (
                  <>
                    <div className="quick-group-tools">
                      <span>
                        {
                          all.filter(({ item }) =>
                            chosen.some((entry) => entry.item.id === item.id),
                          ).length
                        }{' '}
                        项已选
                      </span>
                      <button
                        className="button secondary compact"
                        disabled={!fresh || !enabled || pending || checking || !eligible.length}
                        onClick={() =>
                          select(eligible, !eligible.every((value) => selected.includes(value)))
                        }
                      >
                        {eligible.length && eligible.every((value) => selected.includes(value))
                          ? '取消本类选择'
                          : '选择本类可处理项'}
                      </button>
                    </div>
                    {members.slice(0, limit).map(({ item, protected: blocked, supported }) => {
                      const checked = chosen.some((entry) => entry.item.id === item.id)
                      const observed = preparations.get(item.id)
                      return (
                        <div className="quick-item" key={item.id}>
                          <input
                            type="checkbox"
                            aria-label={`选择 ${item.path}`}
                            checked={checked}
                            disabled={
                              !fresh ||
                              !supported ||
                              blocked ||
                              !enabled ||
                              pending ||
                              checking ||
                              (!checked && chosen.length >= 20)
                            }
                            onChange={(event) => select([item.id], event.target.checked)}
                          />
                          <div>
                            <strong data-i18n="off" title={item.path}>
                              {item.path.split('/').at(-1)}
                            </strong>
                            <span className="mono full-path">{shortPath(item.path, home)}</span>
                            <small>
                              {blocked
                                ? '已保护'
                                : !supported
                                  ? '当前执行范围未覆盖，保留'
                                  : recommended.includes(item.id)
                                    ? '低风险推荐 · 本轮检查通过'
                                    : inspectionErrors[item.id]
                                      ? '推荐检查未完成，保留并查看详情'
                                      : observed?.code === 'authorizationRequired'
                                        ? '需本人授权检查占用，未确认前保留'
                                        : observed?.code === 'retained' ||
                                            observed?.code === 'notChecked'
                                          ? observed.reason
                                          : '需自行确认用途，清理前逐项复查'}
                            </small>
                            <details>
                              <summary>影响与检查详情</summary>
                              <p>{item.impact?.consequence || '用途和影响尚未确认，请先核对。'}</p>
                              <p>{item.impact?.recovery}</p>
                              {observed && <p>{observed.reason}</p>}
                              {inspectionErrors[item.id] && <p>{inspectionErrors[item.id]}</p>}
                            </details>
                          </div>
                          <span>{item.size == null ? '大小未知' : bytes(item.size)}</span>
                          <div className="quick-row-actions">
                            {onReveal && (
                              <button
                                className="button secondary compact"
                                disabled={!desktop}
                                onClick={() => onReveal(item.path)}
                              >
                                定位
                              </button>
                            )}
                            {onProtect && (
                              <button
                                className="button secondary compact"
                                disabled={!desktop || blocked || pending}
                                onClick={() => void protect(item.path)}
                              >
                                保护
                              </button>
                            )}
                          </div>
                        </div>
                      )
                    })}
                    {members.length > limit && (
                      <button
                        className="button secondary compact"
                        onClick={() => setLimit((value) => value + 100)}
                      >
                        再显示 100 项
                      </button>
                    )}
                  </>
                ) : (
                  <p className="quick-group-empty">
                    {all.length ? '没有匹配项目。' : '本次未发现候选，扫描范围可能不完整。'}
                  </p>
                )}
              </details>
            )
          })}
          <details className="card quick-group">
            <summary>
              <strong>大文件</strong>
              <span>
                {analysis
                  ? `${largeFiles.length} 项 · ${shortPath(analysis.path, home)}`
                  : '尚未扫描'}
              </span>
            </summary>
            <p className="quick-group-empty">
              {analysis
                ? `来自空间分析${analysis.scan_status !== 'complete' ? '（结果不完整）' : ''}；大小不代表可以删除，个人文件默认保留。`
                : '扫描后汇总空间分析报告的大文件。'}
            </p>
            {!onlySelected &&
              largeFiles
                .filter((file) => matches(file.path))
                .slice(0, limit)
                .map((file) => (
                  <div className="quick-item quick-readonly" key={file.path}>
                    <div>
                      <strong data-i18n="off">{file.name}</strong>
                      <span className="mono full-path">{shortPath(file.path, home)}</span>
                      <small>{protectedPath(file.path) ? '已保护' : '个人文件 · 需自行确认'}</small>
                    </div>
                    <span>{bytes(file.size)}</span>
                    <div className="quick-row-actions">
                      {onReveal && (
                        <button
                          className="button secondary compact"
                          disabled={!desktop}
                          onClick={() => onReveal(file.path)}
                        >
                          定位
                        </button>
                      )}
                      {onProtect && (
                        <button
                          className="button secondary compact"
                          disabled={!desktop || protectedPath(file.path)}
                          onClick={() => void protect(file.path)}
                        >
                          保护
                        </button>
                      )}
                    </div>
                  </div>
                ))}
            {largeFiles.length > limit && (
              <button
                className="button secondary compact"
                onClick={() => setLimit((value) => value + 100)}
              >
                再显示 100 项
              </button>
            )}
            <button className="button secondary compact" onClick={() => onNavigate('analyze')}>
              查看空间分析
            </button>
          </details>
          <details className="card quick-group">
            <summary>
              <strong>大应用</strong>
              <span>{apps ? `${largeApps.length} 项 · ≥ 500 MB（估算）` : '尚未扫描'}</span>
            </summary>
            <p className="quick-group-empty">
              {appsStale
                ? '应用清单已过期，请重新扫描。'
                : '应用默认保留，点击“审阅卸载”核对后处理；包外个人数据继续保留。'}
            </p>
            {!onlySelected &&
              largeApps
                .filter((app) => matches(`${app.name}\n${app.path}`))
                .slice(0, limit)
                .map((app) => (
                  <div className="quick-item quick-readonly" key={app.path}>
                    <div>
                      <strong data-i18n="off">{app.name}</strong>
                      <span className="mono full-path">{shortPath(app.path, home)}</span>
                      <small>{protectedPath(app.path) ? '已保护' : '需确认不再使用'}</small>
                    </div>
                    <span>{app.size}</span>
                    <div className="quick-row-actions">
                      {onUninstall && (
                        <button
                          className="button secondary compact"
                          disabled={
                            !desktop ||
                            !enabled ||
                            appsStale ||
                            !appsTaskId ||
                            protectedPath(app.path)
                          }
                          onClick={() =>
                            appsTaskId &&
                            onUninstall({
                              taskId: appsTaskId,
                              index: apps!.indexOf(app),
                              name: app.name,
                              path: app.path,
                              bundleId: app.bundle_id,
                            })
                          }
                        >
                          审阅卸载
                        </button>
                      )}
                      {onProtect && (
                        <button
                          className="button secondary compact"
                          disabled={!desktop || protectedPath(app.path)}
                          onClick={() => void protect(app.path)}
                        >
                          保护
                        </button>
                      )}
                    </div>
                  </div>
                ))}
            {largeApps.length > limit && (
              <button
                className="button secondary compact"
                onClick={() => setLimit((value) => value + 100)}
              >
                再显示 100 项
              </button>
            )}
          </details>
          <p className="quick-intro">
            单批最多 20 项，超过时请分批处理。移入废纸篓不会立即释放空间；恢复需在 Finder
            中核对。未支持的对象和检查未知项保留。
          </p>
        </>
      )}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      <div className="quick-more">
        <button className="button secondary compact" onClick={() => onNavigate('history')}>
          清理历史
        </button>
        <button className="button secondary compact" onClick={() => onNavigate('projects')}>
          工程产物
        </button>
        <button className="button secondary compact" onClick={() => onNavigate('installers')}>
          安装包
        </button>
        <button className="button secondary compact" onClick={() => onNavigate('cleanup')}>
          全部候选与诊断
        </button>
      </div>
    </div>
  )
}
