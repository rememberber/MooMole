import { useEffect, useRef, useState } from 'react'
import * as api from '../lib/api'
import {
  fileArchiveStates,
  fileArchiveTransport as transport,
  type FileArchiveTarget,
  type FileArchiveDraft,
  type FileArchiveRecord,
} from '../lib/file-archive'
import { bytes, dateTime } from '../lib/format'
import { useNow } from '../lib/use-now'
import { Modal } from './Modal'

export function FileArchiveCenter({
  target,
  home,
  onClose,
  onActiveChange,
  onOutcome,
}: {
  target: FileArchiveTarget | null
  home: string
  onClose: () => void
  onActiveChange: (active: boolean) => void
  onOutcome: () => void
}) {
  return (
    target && (
      <Archive
        key={`${target.scanId}:${target.items.map((i) => i.id).join(',')}`}
        target={target}
        home={home}
        onClose={onClose}
        onActiveChange={onActiveChange}
        onOutcome={onOutcome}
      />
    )
  )
}
function Archive({
  target,
  home,
  onClose,
  onActiveChange,
  onOutcome,
}: {
  target: FileArchiveTarget
  home: string
  onClose: () => void
  onActiveChange: (active: boolean) => void
  onOutcome: () => void
}) {
  const [directory, setDirectory] = useState(`${home}/Documents`),
    [filename, setFilename] = useState('MooMole-files.zip')
  const [draft, setDraft] = useState<FileArchiveDraft | null>(null),
    [record, setRecord] = useState<FileArchiveRecord | null>(null)
  const [error, setError] = useState(''),
    [checking, setChecking] = useState(false),
    [pending, setPending] = useState(false),
    [ack, setAck] = useState(false)
  const request = useRef<string | null>(null),
    busy = useRef(false),
    now = useNow()
  useEffect(
    () => () => {
      if (draft) void transport.discard(draft.id).catch(() => {})
    },
    [draft?.id],
  )
  async function prepare() {
    if (busy.current || request.current) return
    busy.current = true
    setChecking(true)
    setError('')
    try {
      setDraft(
        await transport.prepare(target, directory.trim().replace(/\/+$/, ''), filename.trim()),
      )
      setAck(false)
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      setChecking(false)
    }
  }
  async function execute() {
    if (busy.current || !draft || !ack || request.current || now >= draft.expiresAt) return
    busy.current = true
    request.current = crypto.randomUUID()
    setPending(true)
    onActiveChange(true)
    setError('')
    try {
      setRecord(await transport.execute(draft, request.current))
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      setPending(false)
      onActiveChange(false)
      onOutcome()
    }
  }
  return (
    <Modal title="归档所选文件" onClose={pending ? undefined : onClose}>
      <h2>归档所选文件</h2>
      <p className="notice">
        创建 ZIP 并逐项校验，原文件保留；归档本身不释放原文件占用。ZIP
        不加密，仅保存文件内容和相对路径，暂不保存 Finder 标签、扩展属性、资源分支或原始权限。
      </p>
      {!draft && !record && (
        <>
          <label className="personal-path">
            <span>归档目标目录</span>
            <input
              aria-label="归档目标目录"
              value={directory}
              disabled={checking}
              onChange={(e) => setDirectory(e.target.value)}
            />
          </label>
          <button
            className="button secondary compact"
            disabled={checking}
            onClick={async () => {
              try {
                const p = await api.chooseDirectory('选择文件归档目标')
                if (p) setDirectory(p)
              } catch (e) {
                setError(String(e))
              }
            }}
          >
            选择归档目录
          </button>
          <label className="personal-path">
            <span>ZIP 文件名</span>
            <input
              aria-label="ZIP 文件名"
              value={filename}
              disabled={checking}
              onChange={(e) => setFilename(e.target.value)}
            />
          </label>
          <p className="muted">
            每批最多 20 个普通文件、合计 16
            GiB，十五分钟预算；同名目标不覆盖。发布前原文件变化或取消时不发布完整 ZIP。
          </p>
        </>
      )}
      {draft && (
        <p className="mono full-path" data-i18n="off">
          {draft.outputPath}
        </p>
      )}
      <div className="card personal-list" role="region" aria-label="归档文件列表">
        {target.items.map((i) => (
          <article key={i.id}>
            <span className="mono full-path" data-i18n="off">
              {i.path}
            </span>
            <small>{bytes(i.size)}</small>
          </article>
        ))}
      </div>
      {draft && !request.current && now >= draft.expiresAt && (
        <p role="status">归档确认已过期，请返回后重新检查。</p>
      )}
      {draft && !request.current && (
        <label className="confirm-check">
          <input type="checkbox" checked={ack} onChange={(e) => setAck(e.target.checked)} />
          我已核对所选文件、归档位置和原文件保留方式。
        </label>
      )}
      {pending && (
        <p role="status">正在生成并核对 ZIP；停止会保留原文件，发布边界的未知结果须在历史核对。</p>
      )}
      {record && (
        <div role="region" aria-label="文件归档结果">
          <strong>{fileArchiveStates[record.state]}</strong>
          <p>{record.reason}</p>
          <p>
            {record.entries.filter((e) => e.state === 'archived').length} / {record.entries.length}{' '}
            项
          </p>
          {record.state === 'completed' && (
            <button
              className="button secondary"
              onClick={() => void api.reveal(record.outputPath).catch((e) => setError(String(e)))}
            >
              在 Finder 中显示归档
            </button>
          )}
        </div>
      )}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      <div className="dialog-actions">
        {!draft && !request.current && (
          <button
            className="button primary"
            disabled={checking || !directory.trim() || !filename.trim()}
            onClick={() => void prepare()}
          >
            {checking ? '正在检查归档…' : '检查归档计划'}
          </button>
        )}
        {draft && !request.current && (
          <button
            className="button primary"
            disabled={!ack || now >= draft.expiresAt}
            onClick={() => void execute()}
          >
            确认创建 ZIP
          </button>
        )}
        {pending && (
          <button
            className="button secondary"
            onClick={() =>
              void transport.cancel(request.current!).catch((e) => setError(String(e)))
            }
          >
            停止归档
          </button>
        )}
        {!pending && (
          <button className="button secondary" onClick={onClose}>
            返回文件列表
          </button>
        )}
      </div>
    </Modal>
  )
}
export function FileArchiveHistory() {
  const [records, setRecords] = useState<FileArchiveRecord[]>([]),
    [error, setError] = useState(''),
    [selected, setSelected] = useState<FileArchiveRecord | null>(null),
    [ack, setAck] = useState(false)
  async function load() {
    try {
      setRecords(await transport.list())
      setError('')
    } catch (e) {
      setError(String(e))
    }
  }
  useEffect(() => {
    if (api.desktop) void load()
  }, [])
  return (
    <section className="card" aria-label="文件归档记录">
      <div className="list-toolbar">
        <h2>文件归档记录</h2>
        <button
          className="button secondary compact"
          disabled={!api.desktop}
          onClick={() => void load()}
        >
          刷新归档记录
        </button>
      </div>
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {!records.length && <p className="muted">尚无文件归档记录。</p>}
      {records.map((r) => (
        <button
          className="saved-plan-row"
          key={r.id}
          onClick={() => {
            setSelected(r)
            setAck(false)
          }}
        >
          <span>
            <strong>{fileArchiveStates[r.state]}</strong>
            <small>
              {dateTime(r.createdAt)} · {r.entries.length} 项
            </small>
            <span className="mono full-path" data-i18n="off">
              {r.outputPath}
            </span>
          </span>
        </button>
      ))}
      {selected && (
        <Modal title="文件归档记录" onClose={() => setSelected(null)}>
          <h2>{fileArchiveStates[selected.state]}</h2>
          <p>{selected.reason}</p>
          <p className="mono full-path" data-i18n="off">
            {selected.outputPath}
          </p>
          {['uncertain', 'interrupted'].includes(selected.state) && (
            <>
              <p>
                请核对目标和以下临时路径。确认已检查只允许之后建立新计划，不会补发、删除文件或把本次结果标成成功。
              </p>
            </>
          )}
          {selected.state !== 'completed' && (
            <p>
              临时归档记录位置：
              <span className="mono full-path" data-i18n="off">
                {selected.temporaryPath}
              </span>
            </p>
          )}
          {selected.entries.map((e) => (
            <p key={e.id}>
              <span className="mono full-path" data-i18n="off">
                {e.path} → {e.name}
              </span>{' '}
              ·{' '}
              {e.state === 'archived'
                ? '已归档'
                : e.state === 'notArchived'
                  ? '未归档'
                  : e.state === 'uncertain'
                    ? '结果待核实'
                    : e.state === 'verified'
                      ? '临时归档内容已核对'
                      : '尚未开始'}
            </p>
          ))}
          <button
            className="button secondary"
            onClick={() =>
              void api
                .reveal(selected.outputPath.slice(0, selected.outputPath.lastIndexOf('/')))
                .catch((e) => setError(String(e)))
            }
          >
            在 Finder 中查看目标目录
          </button>
          {['uncertain', 'interrupted'].includes(selected.state) && !selected.reviewedAt && (
            <>
              <label className="confirm-check">
                <input type="checkbox" checked={ack} onChange={(e) => setAck(e.target.checked)} />
                我已核对目标和临时归档，了解原结果仍待核实。
              </label>
              <button
                className="button secondary"
                disabled={!ack}
                onClick={async () => {
                  try {
                    setSelected(await transport.acknowledge(selected.id))
                    await load()
                  } catch (e) {
                    setError(String(e))
                  }
                }}
              >
                记录已核对
              </button>
            </>
          )}
          <button className="button secondary" onClick={() => setSelected(null)}>
            关闭归档记录
          </button>
        </Modal>
      )}
    </section>
  )
}
