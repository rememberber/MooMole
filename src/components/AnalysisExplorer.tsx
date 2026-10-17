import {
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type Ref,
} from 'react'
import { ArrowRight, ChevronRight, ExternalLink, File, Folder, Info, Search } from 'lucide-react'
import type { Analysis, Entry } from '../types'
import { LargeFileLibrary } from './LargeFileLibrary'
import type {
  AnalysisContext,
  AnalysisExplorerHandle,
  LargeFileHandle,
} from '../lib/analysis-context'
import { measuredBytes, shortPath } from '../lib/format'
import { directoryCrumbs } from '../lib/analysis-navigation'
import {
  analysisObservation,
  compareEntries,
  entryBytes,
  measurementCoverage,
  spaceTiles,
} from '../lib/analysis'

const PAGE_SIZE = 100
const sizeLabel = (entry: Entry) =>
  entryBytes(entry) == null ? '无法测量' : measuredBytes(entry.size, entry.scan_status)

export function AnalysisExplorer({
  ref,
  initialContext,
  analysis,
  home,
  canAnalyze,
  canReveal,
  refreshing,
  onAnalyze,
  onReveal,
}: {
  ref?: Ref<AnalysisExplorerHandle>
  initialContext?: AnalysisContext
  analysis: Analysis
  home: string
  canAnalyze: boolean
  canReveal: boolean
  refreshing: boolean
  onAnalyze: (path: string) => void
  onReveal: (path: string) => void
}) {
  const [view, setView] = useState<'directory' | 'large'>(initialContext?.view ?? 'directory')
  const [query, setQuery] = useState(initialContext?.directory.query ?? '')
  const [type, setType] = useState(initialContext?.directory.type ?? 'all')
  const [sort, setSort] = useState<'size' | 'name'>(initialContext?.directory.sort ?? 'size')
  const [selection, setSelection] = useState<{
    analysis: Analysis
    path: string
    restored?: boolean
  } | null>(() =>
    initialContext?.directory.selectedPath
      ? { analysis, path: initialContext.directory.selectedPath, restored: true }
      : null,
  )
  const inspector = useRef<HTMLDivElement>(null)
  const rows = useRef<HTMLDivElement>(null)
  const directoryScroll = useRef(initialContext?.directory.scrollTop ?? 0)
  const largeFiles = useRef<LargeFileHandle>(null)
  const largeContext = useRef(initialContext?.large)
  const entries = useMemo(
    () =>
      analysis.entries
        .filter(
          (entry) =>
            `${entry.name}\n${entry.path}`
              .toLocaleLowerCase('zh-CN')
              .includes(query.trim().toLocaleLowerCase('zh-CN')) &&
            (type === 'all' ||
              (type === 'directory' && entry.is_dir) ||
              (type === 'file' && !entry.is_dir) ||
              (type === 'unknown' && entryBytes(entry) == null)),
        )
        .sort((a, b) => compareEntries(a, b, sort)),
    [analysis, query, type, sort],
  )
  const [page, setPage] = useState(() => {
    const index = entries.findIndex(
      (entry) => entry.path === initialContext?.directory.selectedPath,
    )
    return index >= 0 ? Math.floor(index / PAGE_SIZE) : (initialContext?.directory.page ?? 0)
  })
  const tiles = useMemo(() => spaceTiles(entries), [entries])
  const selected =
    selection?.analysis === analysis ? entries.find((entry) => entry.path === selection.path) : null
  const pages = Math.max(1, Math.ceil(entries.length / PAGE_SIZE))
  const currentPage = Math.min(page, pages - 1)
  const visible = entries.slice(currentPage * PAGE_SIZE, (currentPage + 1) * PAGE_SIZE)
  const unknown = entries.filter((entry) => entryBytes(entry) == null).length
  const zero = entries.filter((entry) => entryBytes(entry) === 0).length
  const observation = analysisObservation(analysis)
  const coverage = useMemo(() => measurementCoverage(analysis.entries), [analysis])
  const crumbs = useMemo(() => directoryCrumbs(analysis.path), [analysis.path])
  useImperativeHandle(ref, () => ({
    capture: () => ({
      path: analysis.path,
      context: {
        view,
        directory: {
          query,
          type,
          sort,
          page: currentPage,
          selectedPath: selected?.path ?? null,
          scrollTop: rows.current?.scrollTop ?? directoryScroll.current,
        },
        large: largeFiles.current?.capture() ?? largeContext.current,
        windowScroll: window.scrollY,
      },
    }),
  }))
  useLayoutEffect(() => {
    if (rows.current) {
      const row =
        selected && selection?.restored && currentPage !== initialContext?.directory.page
          ? rows.current.querySelector<HTMLElement>('.analysis-row-selected')
          : null
      rows.current.scrollTop = row?.offsetTop ?? directoryScroll.current
    }
  }, [view])
  useLayoutEffect(() => {
    window.scrollTo({ top: initialContext?.windowScroll ?? 0, behavior: 'instant' })
  }, [])
  useEffect(() => {
    if (selection?.analysis !== analysis || selection.restored) return
    const row = rows.current?.querySelector<HTMLElement>('.analysis-row-selected')
    if (row && rows.current) rows.current.scrollTop = row.offsetTop
    inspector.current?.focus({ preventScroll: true })
    inspector.current?.scrollIntoView({ block: 'nearest' })
  }, [selection, analysis])
  function switchView(next: 'directory' | 'large') {
    if (rows.current) directoryScroll.current = rows.current.scrollTop
    if (largeFiles.current) largeContext.current = largeFiles.current.capture()
    setView(next)
  }
  function clearSelection() {
    setSelection(null)
  }
  function select(entry: Entry) {
    setSelection({ analysis, path: entry.path })
    const index = entries.findIndex((item) => item.path === entry.path)
    if (index >= 0) setPage(Math.floor(index / PAGE_SIZE))
  }
  function goToPage(next: number) {
    setPage(next)
    clearSelection()
    if (rows.current) rows.current.scrollTop = 0
    rows.current?.focus({ preventScroll: true })
  }
  return (
    <section className="analysis-explorer" aria-label="Mole 空间分析结果">
      <div className="analysis-summary">
        <div>
          <span className="section-label">已测量占用</span>
          <strong>{measuredBytes(analysis.total_size, analysis.scan_status)}</strong>
        </div>
        <div>
          <nav className="mono analysis-path" aria-label="分析目录路径" title={analysis.path}>
            {crumbs.length ? (
              <ol>
                {crumbs.map((crumb, index) => (
                  <li key={crumb.path}>
                    {index > 0 && <ChevronRight size={12} aria-hidden="true" />}
                    {index === crumbs.length - 1 ? (
                      <span aria-current="location" title={analysis.path}>
                        {crumb.name}
                      </span>
                    ) : (
                      <button
                        className="analysis-crumb"
                        disabled={!canAnalyze || refreshing}
                        title={crumb.path}
                        aria-label={`分析目录 ${crumb.path}`}
                        onClick={() => onAnalyze(crumb.path)}
                      >
                        {crumb.name}
                      </button>
                    )}
                  </li>
                ))}
              </ol>
            ) : (
              <span aria-current="location">{shortPath(analysis.path, home)}</span>
            )}
          </nav>
          <small>
            {analysis.entries.length} 个直接子项 ·{' '}
            {analysis.scan_status === 'complete'
              ? '扫描完成'
              : analysis.scan_status === 'partial'
                ? '部分目录未完整测量'
                : '无法测量'}
          </small>
        </div>
      </div>
      <div className="analysis-observation" aria-label="结果时间与完整性">
        <p>
          {observation ? (
            <>
              本次读取完成{' '}
              <time dateTime={new Date(observation.finishedAt).toISOString()}>
                {new Date(observation.finishedAt).toLocaleString('zh-CN', { hour12: false })}
              </time>
              {' · '}用时{' '}
              {(observation.elapsedMs / 1000).toLocaleString('zh-CN', { maximumFractionDigits: 1 })}{' '}
              秒
            </>
          ) : (
            '读取时间未记录'
          )}
          {' · '}缓存使用情况未知
        </p>
        <p>
          返回的直接子项：{coverage.complete} 项已完整测量 · {coverage.partial} 项部分测量 ·{' '}
          {coverage.unknown} 项无法测量或状态未知
        </p>
        {(coverage.partial > 0 || coverage.unknown > 0) && (
          <p className="analysis-coverage-warning">
            有子项未完整测量，请先查看对应项目；未知大小不代表零。
          </p>
        )}
      </div>
      {refreshing && (
        <p className="inline-note" role="status">
          正在分析，当前显示上次结果。完成后会更新此页。
        </p>
      )}
      {analysis.scan_status !== 'complete' && (
        <div className="inline-note">
          <Info size={14} />带 “+” 的数值只包含已测量部分；无法读取不代表目录为空。
        </div>
      )}
      <div className="analysis-view-switch" role="group" aria-label="分析结果视图">
        <button
          className="button secondary compact"
          aria-pressed={view === 'directory'}
          onClick={() => switchView('directory')}
        >
          目录内容
        </button>
        <button
          className="button secondary compact"
          aria-pressed={view === 'large'}
          onClick={() => switchView('large')}
        >
          大文件
        </button>
      </div>
      {view === 'large' ? (
        <LargeFileLibrary
          ref={largeFiles}
          initialContext={largeContext.current}
          analysis={analysis}
          canAnalyze={canAnalyze}
          canReveal={canReveal}
          onAnalyze={onAnalyze}
          onReveal={onReveal}
        />
      ) : (
        <section className="card analysis-content">
          {selection?.restored && selection.analysis === analysis && !selected && (
            <p className="large-file-intro" role="status">
              上次选中的项目未在本次结果或当前筛选中找到，已取消选中。
            </p>
          )}
          <div className="list-toolbar analysis-filters">
            <label className="search-field">
              <Search size={14} />
              <input
                aria-label="筛选目录内容"
                placeholder="名称或路径…"
                value={query}
                onChange={(event) => {
                  setQuery(event.target.value)
                  setPage(0)
                  clearSelection()
                }}
              />
            </label>
            <select
              aria-label="目录项目类型"
              value={type}
              onChange={(event) => {
                setType(event.target.value)
                setPage(0)
                clearSelection()
              }}
            >
              <option value="all">全部项目</option>
              <option value="directory">文件夹</option>
              <option value="file">文件</option>
              <option value="unknown">无法测量</option>
            </select>
            <select
              aria-label="目录项目排序"
              value={sort}
              onChange={(event) => {
                setSort(event.target.value as 'size' | 'name')
                setPage(0)
                clearSelection()
              }}
            >
              <option value="size">占用从大到小</option>
              <option value="name">名称顺序</option>
            </select>
          </div>
          <div className="analysis-map-section">
            <h3>空间分布</h3>
            <p>点击图块查看项目，文件夹可继续分析。面积比较当前筛选中的已测量占用。</p>
            {tiles.length ? (
              <div className="space-map" role="group" aria-label="当前筛选的空间图">
                {tiles.map((tile, index) => {
                  const style = {
                    left: `${tile.x / 10}%`,
                    top: `${tile.y / 3.6}%`,
                    width: `${tile.width / 10}%`,
                    height: `${tile.height / 3.6}%`,
                  } as CSSProperties
                  const label = tile.entry ? `${tile.label} · ${sizeLabel(tile.entry)}` : tile.label
                  const content = (
                    <span className={tile.width < 95 || tile.height < 50 ? 'tile-compact' : ''}>
                      <strong>{tile.label}</strong>
                      {tile.entry && <small>{sizeLabel(tile.entry)}</small>}
                    </span>
                  )
                  return tile.entry ? (
                    <button
                      key={tile.entry.path}
                      className={`space-tile color-${index % 4}`}
                      style={style}
                      aria-label={`查看 ${label}`}
                      aria-pressed={selected?.path === tile.entry.path}
                      title={`${label}\n${tile.entry.path}`}
                      onClick={() => select(tile.entry!)}
                    >
                      {content}
                    </button>
                  ) : (
                    <div
                      key="remainder"
                      className="space-tile space-remainder"
                      style={style}
                      role="img"
                      aria-label={`${tile.label}（合并显示）`}
                      title="其余较小项目合并绘制，可在分页列表中逐项查看。"
                    >
                      {content}
                    </div>
                  )
                })}
              </div>
            ) : (
              <div className="empty-inline">当前筛选中没有可绘制的已测量占用，请查看下方列表。</div>
            )}
            <p className="analysis-map-caption">
              {tiles.some((tile) => !tile.entry)
                ? `${tiles.find((tile) => !tile.entry)!.label}合并绘制。`
                : '最多单列 12 项，其余合并绘制。'}
              {unknown} 项无法测量、{zero} 项已测量为零，均不占图块，仍可在列表查看。
            </p>
          </div>
          {selected && (
            <div
              ref={inspector}
              tabIndex={-1}
              className="analysis-inspector"
              role="region"
              aria-label="选中项目详情"
            >
              <div role="status">
                <strong>{selected.name}</strong>
                <span>
                  {sizeLabel(selected)} · {selected.is_dir ? '文件夹' : '文件'}
                  {selected.scan_status === 'partial' ? ' · 尚未完整测量' : ''}
                </span>
              </div>
              <p className="mono">{selected.path}</p>
              <div className="analysis-inspector-actions">
                {selected.is_dir && (
                  <button
                    className="button secondary compact"
                    disabled={!canAnalyze}
                    onClick={() => onAnalyze(selected.path)}
                  >
                    分析此文件夹
                    <ArrowRight size={13} />
                  </button>
                )}
                <button
                  className="button secondary compact"
                  disabled={!canReveal}
                  onClick={() => onReveal(selected.path)}
                >
                  <ExternalLink size={13} />在 Finder 中显示
                </button>
                <button className="button text compact" onClick={clearSelection}>
                  取消选中
                </button>
              </div>
            </div>
          )}
          <div
            ref={rows}
            tabIndex={-1}
            role="region"
            className="analysis-rows"
            aria-label="目录分页列表"
          >
            {visible.map((entry, index) => (
              <article
                className={`file-row ${selected?.path === entry.path ? 'analysis-row-selected' : ''}`}
                key={entry.path}
              >
                <button
                  className="file-name"
                  disabled={!canAnalyze || !entry.is_dir}
                  onClick={() => onAnalyze(entry.path)}
                  title={entry.path}
                >
                  <span className={`file-icon color-${index % 4}`}>
                    {entry.is_dir ? <Folder size={19} /> : <File size={18} />}
                  </span>
                  <span className="truncate">
                    {entry.name}
                    <small>
                      {entryBytes(entry) == null
                        ? '无法访问或测量'
                        : entry.is_dir
                          ? '文件夹'
                          : '文件'}
                    </small>
                  </span>
                </button>
                <div className="file-size">
                  <strong>{sizeLabel(entry)}</strong>
                </div>
                <div className="analysis-row-actions">
                  <button
                    className="icon-button"
                    aria-label={`查看 ${entry.path} 详情`}
                    aria-pressed={selected?.path === entry.path}
                    onClick={() => select(entry)}
                  >
                    <Info size={15} />
                  </button>
                  <button
                    className="icon-button"
                    disabled={!canReveal}
                    onClick={() => onReveal(entry.path)}
                    aria-label={`在 Finder 中显示 ${entry.name}`}
                  >
                    <ExternalLink size={15} />
                  </button>
                </div>
              </article>
            ))}
            {!visible.length && (
              <div className="empty-inline">
                <p>
                  {query || type !== 'all'
                    ? '没有匹配的项目。'
                    : analysis.scan_status === 'complete'
                      ? '此范围内没有扫描到子项。'
                      : '没有可展示的子项；部分内容无法读取，请检查目录权限。'}
                </p>
                {(query || type !== 'all') && (
                  <button
                    className="button text"
                    onClick={() => {
                      setQuery('')
                      setType('all')
                      setPage(0)
                      clearSelection()
                    }}
                  >
                    清除筛选
                  </button>
                )}
              </div>
            )}
          </div>
          <div className="analysis-pagination">
            <span role="status">
              显示 {entries.length ? currentPage * PAGE_SIZE + 1 : 0}–
              {Math.min((currentPage + 1) * PAGE_SIZE, entries.length)} / {entries.length}{' '}
              个匹配项目 · 第 {currentPage + 1} / {pages} 页
            </span>
            <button
              className="button secondary compact"
              disabled={currentPage === 0}
              onClick={() => goToPage(currentPage - 1)}
            >
              上一页
            </button>
            <button
              className="button secondary compact"
              disabled={currentPage + 1 >= pages}
              onClick={() => goToPage(currentPage + 1)}
            >
              下一页
            </button>
          </div>
          <details className="analysis-boundaries">
            <summary>测量范围与图表说明</summary>
            {observation ? (
              <>
                <p>
                  来源：Mole CLI{' '}
                  {observation.engineVersion ? `v${observation.engineVersion}` : '版本未记录'}
                  {' · '}显式目录 JSON。读取开始{' '}
                  {new Date(observation.startedAt).toLocaleString('zh-CN', { hour12: false })}
                  （本机时间）；完成时间为客户端取得本次结果的时间，用时由单调时钟记录，包含引擎调用、输出读取和解析。
                </p>
                <p className="mono">引擎修订：{observation.revision || '未记录'}</p>
              </>
            ) : (
              <p>
                这份结果没有可识别的读取来源记录，无法确认读取时间和引擎版本；可重新分析获取本次记录。
              </p>
            )}
            <p>
              Mole
              没有报告缓存命中及原始测量时间，因此读取完成时间不等于每个项目的测量时间；重新分析也可能复用缓存。
              完整性数量基于整份返回结果，不随筛选变化，也不代表磁盘上所有内容都已列出。
            </p>
            <p>
              来自 Mole 对当前目录的结构化结果，不递归展开成所有文件的清单。图表最多单列 12
              个较大项目，其他正值项目合并；无法测量与已测量为零的项目仍保留在列表，部分测量为零不代表目录为空。
            </p>
            <p>
              图表按当前筛选中子项的已测量值比较，不以整盘空间或目录总量充当百分比分母。扫描可能使用上游缓存，内容也可能继续变化；不是实时快照、可释放空间或清理许可。完整
              APFS、云端占位与挂载行为仍需验证。
            </p>
          </details>
        </section>
      )}
    </section>
  )
}
