import { useEffect, useMemo, useState } from 'react'
import { ArrowUpRight, Folder, Search, ShieldCheck } from 'lucide-react'
import type { CleanPreview } from '../types'
import { bytes, dateTime, shortPath } from '../lib/format'
import { previewTotal, protectionFor } from '../lib/clean-preview'
import { ImpactDetails } from './ImpactDetails'
import { useNow } from '../lib/use-now'
import { OriginDetails } from './OriginDetails'

export function CleanReview({
  preview,
  protectedPaths,
  home,
  desktop,
  onProtect,
  onReveal,
  onCreatePlan,
}: {
  preview: CleanPreview
  protectedPaths: string[]
  home: string
  desktop: boolean
  onProtect: (path: string) => Promise<void>
  onReveal: (path: string) => void
  onCreatePlan?: (scanId: string, ids: string[]) => Promise<void>
}) {
  const [query, setQuery] = useState('')
  const [section, setSection] = useState('all')
  const [filter, setFilter] = useState('all')
  const [impactKind, setImpactKind] = useState('all')
  const now = useNow()
  const [sort, setSort] = useState('size')
  const [selected, setSelected] = useState<string[]>([])
  const [page, setPage] = useState(0)
  const [pending, setPending] = useState<string | null>(null)
  const [error, setError] = useState('')
  const items = useMemo(
    () =>
      preview.items.map((item) => ({ ...item, protections: protectionFor(item, protectedPaths) })),
    [preview, protectedPaths],
  )
  useEffect(() => {
    setSelected([])
    setPage(0)
  }, [preview])
  useEffect(() => {
    setSelected((current) =>
      current.filter((id) => items.some((item) => item.id === id && !item.protections.length)),
    )
  }, [items])
  const chosen = items.filter((item) => selected.includes(item.id) && !item.protections.length)
  const total = previewTotal(items)
  const chosenTotal = previewTotal(chosen)
  const visible = items
    .filter(
      (item) =>
        (section === 'all' || item.section === section) &&
        `${item.path} ${item.section}`.toLowerCase().includes(query.toLowerCase()) &&
        (impactKind === 'all' || (item.impact?.kind ?? 'unknown') === impactKind) &&
        (filter === 'all' ||
          (filter === 'protected' && item.protections.length > 0) ||
          (filter === 'unknown' && item.size == null) ||
          (filter === 'selected' && chosen.some((i) => i.id === item.id))),
    )
    .sort((a, b) =>
      sort === 'size' ? (b.size ?? -1) - (a.size ?? -1) : a.path.localeCompare(b.path, 'zh-CN'),
    )
  const pages = Math.max(1, Math.ceil(visible.length / 100))
  const currentPage = Math.min(page, pages - 1)
  const pageItems = visible.slice(currentPage * 100, (currentPage + 1) * 100)
  const sections = [...new Set(items.map((item) => item.section))]
  const pageSections = [...new Set(pageItems.map((item) => item.section))]
  const selectable = pageItems.filter((item) => !item.protections.length)
  const allChecked = selectable.length > 0 && selectable.every((item) => selected.includes(item.id))
  const toggle = (id: string) =>
    setSelected((current) =>
      current.includes(id) ? current.filter((i) => i !== id) : [...current, id],
    )
  async function protect(path: string) {
    setPending(path)
    try {
      await onProtect(path)
    } finally {
      setPending(null)
    }
  }
  async function savePlan() {
    if (preview.protocol !== 2 || !preview.scanId || !onCreatePlan) return
    setError('')
    setPending('plan')
    try {
      await onCreatePlan(
        preview.scanId,
        chosen.map((item) => item.id),
      )
    } catch (error) {
      setError(String(error).replace(/^Error: /, ''))
    } finally {
      setPending(null)
    }
  }
  return (
    <section className="card clean-review" aria-label="Mole 清理项目审阅">
      <div className="review-heading">
        <div>
          <h2>逐项审阅</h2>
          <p>
            {items.length} 个候选路径 · 已测量 {bytes(total.size)}
            {total.unknown > 0 ? ' 起' : ''}
          </p>
        </div>
        <span className="tag">dry-run · 未执行删除</span>
      </div>
      <p className="review-note">
        大小为 Mole 预览估算，父子目录不重复累计；未知大小不计入。
        {!preview.systemIncluded && '本次未包含需要管理员权限的系统项目。'}
        受权限、活动应用及规则限制，本次结果可能不完整。
      </p>
      {preview.scannedAt && (
        <p className="review-note">
          扫描于 {dateTime(preview.scannedAt)} · 计划需在 {dateTime(preview.expiresAt!)}{' '}
          前保存与复核。
          {(now >= preview.expiresAt! || now < preview.scannedAt) &&
            '此扫描已过期或系统时间回退，请重新扫描。'}
        </p>
      )}
      {preview.protocol !== 2 && (
        <p className="notice warning">此旧扫描没有绑定调用来源，请重新扫描后保存新计划。</p>
      )}
      <div className="review-filters">
        <label className="review-search">
          <Search size={14} />
          <input
            aria-label="搜索清理项目"
            placeholder="搜索名称或路径"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value)
              setPage(0)
            }}
          />
        </label>
        <select
          aria-label="清理项目用途"
          value={impactKind}
          onChange={(e) => {
            setImpactKind(e.target.value)
            setPage(0)
          }}
        >
          <option value="all">全部用途</option>
          <option value="downloadCache">下载缓存</option>
          <option value="appCache">应用缓存</option>
          <option value="logs">运行日志</option>
          <option value="diagnostics">诊断报告</option>
          <option value="unknown">用途未确认</option>
        </select>
        <select
          aria-label="清理分类"
          value={section}
          onChange={(e) => {
            setSection(e.target.value)
            setPage(0)
          }}
        >
          <option value="all">全部分类</option>
          {sections.map((s) => (
            <option key={s}>{s}</option>
          ))}
        </select>
        <select
          aria-label="清理项目排序"
          value={sort}
          onChange={(e) => {
            setSort(e.target.value)
            setPage(0)
          }}
        >
          <option value="size">大小优先</option>
          <option value="name">路径名称</option>
        </select>
        <select
          aria-label="清理项目状态"
          value={filter}
          onChange={(e) => {
            setFilter(e.target.value)
            setPage(0)
          }}
        >
          <option value="all">全部项目</option>
          <option value="selected">已勾选</option>
          <option value="protected">已保护</option>
          <option value="unknown">大小未知</option>
        </select>
      </div>
      <div className="review-selection">
        <label>
          <input
            type="checkbox"
            aria-label="勾选本页可审阅项目"
            checked={allChecked}
            disabled={!selectable.length}
            onChange={() =>
              setSelected((current) =>
                allChecked
                  ? current.filter((id) => !selectable.some((item) => item.id === id))
                  : [...new Set([...current, ...selectable.map((item) => item.id)])],
              )
            }
          />
          本页
        </label>
        <span role="status">
          已勾选 {chosen.length} 项 · {bytes(chosenTotal.size)}
          {chosenTotal.unknown > 0 ? `，另有 ${chosenTotal.unknown} 项大小未知` : ''}
        </span>
        <button className="button text" disabled={!selected.length} onClick={() => setSelected([])}>
          清空勾选
        </button>
        <button
          className="button secondary compact"
          disabled={
            !desktop ||
            preview.protocol !== 2 ||
            !preview.scanId ||
            !onCreatePlan ||
            !chosen.length ||
            chosen.length > 500 ||
            pending != null ||
            (preview.expiresAt != null && now >= preview.expiresAt) ||
            (preview.scannedAt != null && now < preview.scannedAt)
          }
          onClick={() => void savePlan()}
        >
          {pending === 'plan' ? '正在核对并保存…' : '保存审阅计划'}
        </button>
      </div>
      <p className="review-note">
        勾选不执行清理，离开此页面后清空；可保存最多 500
        项的审阅计划，在历史中重新复核。保护目录与其父子项目均不可勾选。
      </p>
      {error && (
        <p className="error-text review-note" role="alert">
          {error}
        </p>
      )}
      {pageSections.map((group) => (
        <details className="review-group" key={group} open>
          <summary>
            {group}
            <span>
              {pages > 1
                ? `本页 ${pageItems.filter((item) => item.section === group).length} / `
                : ''}
              {visible.filter((item) => item.section === group).length} 项
            </span>
          </summary>
          {pageItems
            .filter((item) => item.section === group)
            .map((item) => (
              <article
                className={`review-item ${item.protections.length ? 'is-protected' : ''}`}
                key={item.id}
              >
                <input
                  type="checkbox"
                  aria-label={`审阅 ${item.path}`}
                  checked={chosen.some((i) => i.id === item.id)}
                  disabled={item.protections.length > 0}
                  onChange={() => toggle(item.id)}
                />
                <details className="review-detail">
                  <summary>
                    <span className="review-name">
                      {item.path.split('/').filter(Boolean).at(-1) || '/'}
                      {item.protections.length > 0 && (
                        <span className="tag">
                          <ShieldCheck size={11} />
                          已保护
                        </span>
                      )}
                    </span>
                    <strong>{item.size == null ? '大小未知' : bytes(item.size)}</strong>
                    <span className="review-path mono">{shortPath(item.path, home)}</span>
                  </summary>
                  <div className="review-explanation">
                    <p className="mono full-path">{item.path}</p>
                    <p>
                      来源：Mole clean dry-run · {item.section} · 引擎记录 {item.itemCount}{' '}
                      项。是否适合清理仍需结合用途确认，正在使用的数据应保留。
                    </p>
                    <ImpactDetails impact={item.impact} />
                    <OriginDetails value={item.provenance} />
                    {item.coveredBy && (
                      <p>
                        完整预览中，大小已计入父目录：<span className="mono">{item.coveredBy}</span>
                        。单独勾选子项时按子项重新计量。
                      </p>
                    )}
                    {item.protections.length > 0 && (
                      <p>
                        客户端保护目录：<span className="mono">{item.protections.join('、')}</span>
                        。预览仍保留记录以供核对，不修改 Mole 自有白名单。
                      </p>
                    )}
                    {!item.resolvedPath && <p>路径已不存在或无法解析；请重新扫描确认。</p>}
                    <div className="review-actions">
                      <button
                        className="button secondary compact"
                        disabled={!desktop}
                        onClick={() => onReveal(item.path)}
                      >
                        Finder
                        <ArrowUpRight size={12} />
                      </button>
                      {item.isDirectory && !item.protections.length && (
                        <button
                          className="button secondary compact"
                          disabled={!desktop || pending != null}
                          onClick={() => void protect(item.path)}
                        >
                          <ShieldCheck size={13} />
                          {pending === item.path ? '正在保护…' : '保护此目录'}
                        </button>
                      )}
                      <span className="muted">
                        <Folder size={12} />
                        {item.isDirectory ? '目录' : '文件或当前无法确认的路径'}
                      </span>
                    </div>
                  </div>
                </details>
              </article>
            ))}
        </details>
      ))}
      {!visible.length && (
        <div className="empty-inline">
          {items.length
            ? '没有符合条件的项目，试试其他筛选。'
            : 'Mole 本次没有列出候选路径；可展开原始日志了解跳过情况。'}
        </div>
      )}
      {pages > 1 && (
        <div className="review-pagination">
          <span>
            筛选后 {visible.length} 项 · 第 {currentPage + 1} / {pages} 页
          </span>
          <button
            className="button secondary compact"
            disabled={currentPage === 0}
            onClick={() => setPage(currentPage - 1)}
          >
            上一页
          </button>
          <button
            className="button secondary compact"
            disabled={currentPage + 1 >= pages}
            onClick={() => setPage(currentPage + 1)}
          >
            下一页
          </button>
        </div>
      )}
      <details className="clean-preview review-log">
        <summary>查看 Mole 原始日志</summary>
        <pre tabIndex={0} aria-label="Mole 清理预览日志">
          {preview.output || '没有日志。'}
        </pre>
        <p className="fine-print">
          预览协议 v{preview.protocol} · Mole {preview.revision.slice(0, 12)} ·
          原始日志不用于决定清理目标
        </p>
      </details>
    </section>
  )
}
