import { useEffect, useMemo, useState } from 'react'
import { FolderOpen } from 'lucide-react'
import * as api from '../lib/api'
import type { PersonalController } from '../lib/use-personal-files'
import { filterPersonalFiles, type PersonalConfirmation } from '../lib/personal-files'
import { fileKinds, type FileKind } from '../lib/large-files'
import { bytes, dateTime, shortPath } from '../lib/format'
import { pathOverlaps } from '../lib/clean-preview'
import { useNow } from '../lib/use-now'
import { useLanguage } from '../i18n/locale'
import { Modal } from './Modal'
import { CleanupResultSummary } from './CleanupResultSummary'
import { ExecutionReason } from './ExecutionReason'
import { ExecutionLocation } from './ExecutionLocation'

export function PersonalFiles({
  controller: c,
  home,
  protectedPaths,
  enabled,
  onHistory,
}: {
  controller: PersonalController
  home: string
  protectedPaths: string[]
  enabled: boolean
  onHistory: () => void
}) {
  const language = useLanguage()
  const [query, setQuery] = useState(''),
    [kind, setKind] = useState<FileKind>('all')
  const [minimum, setMinimum] = useState(0),
    [older, setOlder] = useState(0),
    [sort, setSort] = useState<'size' | 'name' | 'modified'>('size')
  const [selected, setSelected] = useState<string[]>([]),
    [limit, setLimit] = useState(100)
  const [confirmation, setConfirmation] = useState<PersonalConfirmation | null>(null)
  const [pending, setPending] = useState(false),
    [ack, setAck] = useState(false),
    [error, setError] = useState('')
  const now = useNow(),
    report = c.snapshot.report
  const fresh =
    !!report &&
    c.root.trim().replace(/\/+$/, '') === report.root &&
    now >= report.createdAt &&
    now < report.expiresAt
  const blocked = (path: string) => protectedPaths.some((rule) => pathOverlaps(path, rule))
  const chosen =
    report?.items.filter((item) => selected.includes(item.id) && !blocked(item.path) && fresh) ?? []
  const items = useMemo(
    () => filterPersonalFiles(report?.items ?? [], query, kind, minimum, older, now, sort),
    [report, query, kind, minimum, older, now, sort, language],
  )
  useEffect(() => {
    setSelected([])
    setLimit(100)
  }, [report?.id])
  useEffect(() => setSelected([]), [c.root])
  useEffect(() => {
    setLimit(100)
  }, [query, kind, minimum, older, sort])
  useEffect(
    () => () => {
      if (confirmation) void c.discard(confirmation.id).catch(() => {})
    },
    [confirmation?.id],
  )
  async function prepare() {
    if (pending || !chosen.length || !fresh) return
    setPending(true)
    setError('')
    try {
      setConfirmation(await c.prepare(chosen.map((item) => item.id)))
      setAck(false)
    } catch (reason) {
      setError(String(reason).replace(/^Error: /, ''))
    } finally {
      setPending(false)
    }
  }
  const close = () => {
    setConfirmation(null)
    setAck(false)
  }
  return (
    <div className="personal-files">
      <form
        id="personal-files-form"
        className="personal-scope"
        onSubmit={(event) => {
          event.preventDefault()
          if (enabled && !c.running) void c.start()
        }}
      >
        <label className="personal-path">
          <span>整理目录</span>
          <input
            aria-label="个人文件目录"
            value={c.root}
            disabled={c.running}
            onChange={(event) => c.setRoot(event.target.value)}
          />
        </label>
        <button
          type="button"
          className="button secondary compact"
          disabled={!api.desktop || c.running}
          onClick={async () => {
            const root = await api.chooseDirectory('选择要整理的个人目录')
            if (root) c.setRoot(root)
          }}
        >
          <FolderOpen size={14} />
          选择目录
        </button>
        <button
          type="button"
          className="button text compact"
          disabled={c.running}
          onClick={() => c.setRoot(`${home}/Downloads`)}
        >
          下载目录
        </button>
      </form>
      <p className="muted">
        按内容需要自行选择个人文件，默认不选。文件较大或久未修改不代表可以删除；本轮处理普通文件，文件夹和应用资料库保留。
      </p>
      {c.error && (
        <p className="notice warning" role="alert">
          {c.error}
        </p>
      )}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {c.running && (
        <p role="status">
          {c.snapshot.phase === 'processing'
            ? '正在逐项处理个人文件，结果会保存到历史。'
            : '正在读取目录，可停止扫描并保留已取得的结果。'}
        </p>
      )}
      {report && (
        <>
          <div className="personal-summary">
            <span className="mono">{shortPath(report.root, home)}</span>
            <span>
              {report.items.length} 个普通文件 ·{' '}
              {bytes(report.items.reduce((sum, item) => sum + item.size, 0))} ·{' '}
              {dateTime(report.createdAt)}
            </span>
          </div>
          {(!report.complete || report.skipped > 0) && (
            <p className="notice">
              {report.cancelled ? '扫描已停止。' : '本次扫描范围有限。'} 已跳过 {report.skipped}{' '}
              项；隐藏、链接、受保护、共享、云占位、其他卷或无法读取的内容未纳入。已显示项目不代表目录全部内容。
            </p>
          )}
          {!fresh && (
            <p className="notice warning">扫描已过期或整理目录已变化，请重新扫描后选择。</p>
          )}
          <div className="personal-filters">
            <input
              type="search"
              aria-label="搜索个人文件"
              placeholder="搜索名称或路径…"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
            <select
              aria-label="个人文件类型"
              value={kind}
              onChange={(event) => setKind(event.target.value as FileKind)}
            >
              {Object.entries(fileKinds).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
            <select
              aria-label="个人文件最小大小"
              value={minimum}
              onChange={(event) => setMinimum(Number(event.target.value))}
            >
              <option value={0}>全部大小</option>
              <option value={100 * 1024 ** 2}>至少 100 MB</option>
              <option value={1024 ** 3}>至少 1 GB</option>
            </select>
            <select
              aria-label="个人文件修改时间"
              value={older}
              onChange={(event) => setOlder(Number(event.target.value))}
            >
              <option value={0}>全部修改时间</option>
              <option value={30}>30 天前修改</option>
              <option value={90}>90 天前修改</option>
              <option value={365}>一年前修改</option>
            </select>
            <select
              aria-label="个人文件排序"
              value={sort}
              onChange={(event) => setSort(event.target.value as typeof sort)}
            >
              <option value="size">大小从大到小</option>
              <option value="modified">最早修改优先</option>
              <option value="name">按名称</option>
            </select>
          </div>
          <div className="personal-selection">
            <strong>
              已选 {chosen.length} 项 · {bytes(chosen.reduce((sum, item) => sum + item.size, 0))}
            </strong>
            <span>每批最多 20 项</span>
            <button
              className="button secondary compact"
              disabled={!selected.length || pending || c.running}
              onClick={() => setSelected([])}
            >
              清空选择
            </button>
            <button
              className="button primary compact"
              disabled={!enabled || c.running || !fresh || !chosen.length || pending}
              onClick={() => void prepare()}
            >
              {pending ? '正在准备…' : '移入废纸篓…'}
            </button>
          </div>
          <div className="card personal-list" role="region" aria-label="个人文件列表">
            {items.slice(0, limit).map((item) => (
              <article className="personal-row" key={item.id}>
                <input
                  type="checkbox"
                  aria-label={`选择 ${item.path}`}
                  checked={chosen.some((value) => value.id === item.id)}
                  disabled={
                    !enabled ||
                    !fresh ||
                    blocked(item.path) ||
                    c.running ||
                    pending ||
                    (!selected.includes(item.id) && chosen.length >= 20)
                  }
                  onChange={(event) =>
                    setSelected((ids) =>
                      event.target.checked ? [...ids, item.id] : ids.filter((id) => id !== item.id),
                    )
                  }
                />
                <div>
                  <strong data-i18n="off">{item.name}</strong>
                  <p className="mono full-path">{shortPath(item.path, home)}</p>
                  <small>
                    {blocked(item.path) ? '已保护' : '个人文件 · 需自行确认'} ·{' '}
                    {dateTime(item.modifiedAt)}
                  </small>
                </div>
                <span>{bytes(item.size)}</span>
                <button
                  className="button secondary compact"
                  disabled={!api.desktop}
                  onClick={() =>
                    void api.reveal(item.path).catch((reason) => setError(String(reason)))
                  }
                >
                  在 Finder 中查看
                </button>
              </article>
            ))}
            {!items.length && (
              <p className="empty-inline">没有匹配的个人文件，可调整筛选或重新扫描。</p>
            )}
          </div>
          {items.length > limit && (
            <button
              className="button secondary compact"
              onClick={() => setLimit((value) => value + 100)}
            >
              再显示 100 项
            </button>
          )}
        </>
      )}
      {!report && !c.running && !c.result && (
        <p className="empty-inline">选择下载目录或其他个人目录，再点击顶部“扫描文件”。</p>
      )}
      {c.result && (
        <section className="card personal-result" aria-label="个人文件处理结果">
          <strong>
            已移入 {c.result.movedCount} / {c.result.itemCount} 项 · 待核实{' '}
            {c.result.uncertainCount} 项
          </strong>
          <CleanupResultSummary detail={c.result} />
          {c.result.items.map((item) => (
            <article className="personal-result-row" key={item.id}>
              <strong>
                {item.state === 'moved'
                  ? '已移入废纸篓'
                  : item.state === 'uncertain'
                    ? '结果待核实'
                    : item.state === 'cancelled'
                      ? '开始前取消'
                      : '本项未确认移入'}
              </strong>
              <p className="mono full-path">{item.path}</p>
              <ExecutionReason reason={item.reason} />
              {item.receipt && (
                <ExecutionLocation
                  runId={c.result!.id}
                  itemId={item.id}
                  onReveal={api.revealExecutionItem}
                />
              )}
            </article>
          ))}
        </section>
      )}
      <button className="button secondary compact" onClick={onHistory}>
        查看处理历史
      </button>
      {!api.desktop && <p className="notice">浏览器预览不能读取或处理本机文件，请使用桌面 App。</p>}
      {confirmation && (
        <Modal
          title="确认处理个人文件"
          onClose={pending ? undefined : close}
          className="review-plan-modal"
        >
          <h2>确认处理个人文件</h2>
          <p>
            {confirmation.items.length} 个文件 ·{' '}
            {bytes(confirmation.items.reduce((sum, item) => sum + item.size, 0))}
          </p>
          <p className="notice warning">
            这些是你选择的个人文件，可能包含唯一副本或未同步内容。请核对内容并关闭相关文件；本次不完整检查所有应用占用，不能保证文件空闲。
          </p>
          <p>
            仅将下面列出的普通文件移入废纸篓，保留文件夹。处理前重新检查路径保护、权限和文件变化；不清空废纸篓。恢复需在
            Finder 中核对实际收据，原位置有同名文件时不要覆盖。
          </p>
          <div className="plan-items">
            {confirmation.items.map((item) => (
              <article className="plan-item" key={item.id}>
                <strong data-i18n="off">{item.name}</strong>
                <p className="mono full-path">{item.path}</p>
                <span>{bytes(item.size)}</span>
              </article>
            ))}
          </div>
          <label className="uninstall-ack">
            <input
              type="checkbox"
              checked={ack}
              onChange={(event) => setAck(event.target.checked)}
            />
            我已核对所选个人文件，确认不再需要，并了解占用与恢复边界。
          </label>
          <div className="modal-actions">
            <button className="button secondary" disabled={pending} onClick={close}>
              返回选择
            </button>
            <button
              className="button primary"
              disabled={
                !ack || pending || now >= confirmation.expiresAt || now < confirmation.createdAt
              }
              onClick={async () => {
                setPending(true)
                try {
                  await c.execute(confirmation.id)
                  close()
                } finally {
                  setPending(false)
                }
              }}
            >
              确认移入废纸篓
            </button>
          </div>
          {now >= confirmation.expiresAt && (
            <p className="notice warning">个人文件确认已过期，请返回重新检查。</p>
          )}
        </Modal>
      )}
    </div>
  )
}
