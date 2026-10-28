import { getLocale } from '../i18n/locale'
import { useImperativeHandle, useLayoutEffect, useMemo, useRef, useState, type Ref } from 'react'
import type { LargeFileContext, LargeFileHandle } from '../lib/analysis-context'
import { ExternalLink, Folder, Search } from 'lucide-react'
import type { Analysis } from '../types'
import { bytes } from '../lib/format'
import { parentDirectory } from '../lib/analysis'
import {
  fileKinds,
  filterLargeFiles,
  largeFileBytes,
  largeFileInScope,
  largeFileReport,
  type FileKind,
} from '../lib/large-files'

const PAGE_SIZE = 20

export function LargeFileLibrary({
  ref,
  initialContext,
  analysis,
  canAnalyze,
  canReveal,
  onAnalyze,
  onReveal,
}: {
  ref?: Ref<LargeFileHandle>
  initialContext?: LargeFileContext
  analysis: Analysis
  canAnalyze: boolean
  canReveal: boolean
  onAnalyze: (path: string) => void
  onReveal: (path: string) => void
}) {
  const [query, setQuery] = useState(initialContext?.query ?? '')
  const [kind, setKind] = useState<FileKind>(initialContext?.kind ?? 'all')
  const [minimum, setMinimum] = useState(initialContext?.minimum ?? 0)
  const [sort, setSort] = useState<'size' | 'name'>(initialContext?.sort ?? 'size')
  const [page, setPage] = useState(initialContext?.page ?? 0)
  const rows = useRef<HTMLDivElement>(null)
  const report = useMemo(() => largeFileReport(analysis), [analysis])
  const files = useMemo(
    () => filterLargeFiles(report.files, query, kind, minimum, sort),
    [report, query, kind, minimum, sort, getLocale()],
  )
  const pages = Math.max(1, Math.ceil(files.length / PAGE_SIZE))
  const currentPage = Math.min(page, pages - 1)
  useImperativeHandle(ref, () => ({
    capture: () => ({
      query,
      kind,
      minimum,
      sort,
      page: currentPage,
      scrollTop: rows.current?.scrollTop ?? 0,
    }),
  }))
  useLayoutEffect(() => {
    if (rows.current) rows.current.scrollTop = initialContext?.scrollTop ?? 0
  }, [])
  const filtered = !!query || kind !== 'all' || minimum !== 0
  function goToPage(next: number) {
    setPage(next)
    if (rows.current) rows.current.scrollTop = 0
    rows.current?.focus({ preventScroll: true })
  }
  return (
    <section className="card large-file-library" aria-label="Mole 大文件列表">
      <div className="list-toolbar analysis-filters">
        <label className="search-field">
          <Search size={14} />
          <input
            aria-label="搜索大文件"
            placeholder="名称或完整路径…"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value)
              setPage(0)
            }}
          />
        </label>
        <select
          aria-label="大文件类型"
          value={kind}
          onChange={(event) => {
            setKind(event.target.value as FileKind)
            setPage(0)
          }}
        >
          {Object.entries(fileKinds).map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
        <select
          aria-label="大文件最小大小"
          value={minimum}
          onChange={(event) => {
            setMinimum(Number(event.target.value))
            setPage(0)
          }}
        >
          <option value={0}>全部返回项</option>
          <option value={100 * 1024 ** 2}>至少 100 MiB</option>
          <option value={1024 ** 3}>至少 1 GiB</option>
        </select>
        <select
          aria-label="大文件排序"
          value={sort}
          onChange={(event) => {
            setSort(event.target.value as 'size' | 'name')
            setPage(0)
          }}
        >
          <option value="size">大小从大到小</option>
          <option value="name">名称顺序</option>
        </select>
      </div>
      <div className="large-file-intro">
        <p>Mole 返回的较大文件，可能来自子目录；列表并不完整，筛选只作用于已返回的项目。</p>
        {analysis.scan_status !== 'complete' && (
          <p>当前目录未完整测量，大文件列表也可能遗漏内容。</p>
        )}
        {report.unreadable > 0 && <p>{report.unreadable} 条记录格式无法识别，未提供路径操作。</p>}
      </div>
      <div
        ref={rows}
        className="analysis-rows"
        role="region"
        aria-label="大文件分页列表"
        tabIndex={-1}
      >
        {files.slice(currentPage * PAGE_SIZE, (currentPage + 1) * PAGE_SIZE).map((file, index) => {
          const inScope = largeFileInScope(file.path, analysis.path)
          return (
            <article className="large-file-row" key={`${file.path}:${index}`}>
              <div className="large-file-info">
                <strong data-i18n={file.name ? 'off' : undefined}>
                  {file.name || '未命名文件'}
                </strong>
                <p className="mono">{file.path || '路径未提供'}</p>
                {!inScope && <small>路径不在本次目录范围内，无法定位或继续分析。</small>}
              </div>
              <span className="large-file-size">
                {largeFileBytes(file) == null ? '大小未知' : bytes(file.size)}
              </span>
              <div className="large-file-actions">
                <button
                  className="button secondary compact"
                  disabled={!canAnalyze || !inScope}
                  onClick={() => onAnalyze(parentDirectory(file.path))}
                  aria-label={`分析 ${file.path} 所在目录`}
                >
                  <Folder size={13} />
                  分析所在目录
                </button>
                <button
                  className="icon-button"
                  disabled={!canReveal || !inScope}
                  onClick={() => onReveal(file.path)}
                  aria-label={`在 Finder 中显示大文件 ${file.path}`}
                >
                  <ExternalLink size={15} />
                </button>
              </div>
            </article>
          )
        })}
        {!files.length && (
          <div className="empty-inline">
            <p>
              {!report.reported
                ? '本次结果没有提供大文件列表，不代表目录里没有大文件。'
                : filtered
                  ? '没有匹配的大文件。'
                  : report.unreadable
                    ? '没有可展示的大文件记录，请重新分析。'
                    : '本次返回的大文件列表为空，不代表目录里没有其他大文件。'}
            </p>
            {filtered && (
              <button
                className="button text"
                onClick={() => {
                  setQuery('')
                  setKind('all')
                  setMinimum(0)
                  setPage(0)
                }}
              >
                清除大文件筛选
              </button>
            )}
          </div>
        )}
      </div>
      <div className="analysis-pagination">
        <span role="status">
          显示 {files.length ? currentPage * PAGE_SIZE + 1 : 0}–
          {Math.min((currentPage + 1) * PAGE_SIZE, files.length)} / {files.length} 条匹配记录 ·{' '}
          {report.reported
            ? `Mole 返回 ${report.files.length + report.unreadable} 条记录`
            : '大文件返回数量未知'}
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
        <summary>大文件范围与来源</summary>
        <p>
          固定 Mole 版本通常最多返回 20 条，受扫描范围、上游跳过规则、缓存及 Spotlight
          索引影响，不保证列出所有大文件。没有独立的逐文件完整性或来源标记；目录扫描完成也不代表这份列表完整。
        </p>
        <p>
          类型仅按路径扩展名分类，不读取文件内容。大小沿用上游报告值，不汇总为可释放空间，也不与目录总量相加；硬链接、APFS
          共享块及云端占位等度量边界仍适用。
        </p>
        <p>
          “分析所在目录”会重新调用 Mole，“在 Finder
          中显示”只定位文件。此处不提供删除或清理，文件可能已变化或不存在；结果时间和缓存边界见上方摘要。
        </p>
      </details>
    </section>
  )
}
