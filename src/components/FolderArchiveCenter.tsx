import { useEffect, useRef, useState } from 'react'
import { isTauri } from '@tauri-apps/api/core'
import * as api from '../lib/api'
import {
  folderArchive as transport,
  folderArchiveState,
  type FolderArchiveDraft,
  type FolderArchiveRecord,
  type FolderArchiveView,
} from '../lib/folder-archive'
import { bytes, dateTime } from '../lib/format'
import { useNow } from '../lib/use-now'
import { Modal } from './Modal'
const empty: FolderArchiveView = { records: [], activeRequests: [] }
export function FolderArchiveCenter({
  openSignal,
  home,
  onActiveChange,
  onOutcome,
  onHistory,
}: {
  openSignal: number
  home: string
  onActiveChange: (v: boolean) => void
  onOutcome: () => void
  onHistory: () => void
}) {
  const [open, setOpen] = useState(false),
    [pending, setPending] = useState(false),
    [view, setView] = useState(empty),
    [epoch, setEpoch] = useState(0)
  useEffect(() => {
    if (openSignal > 0) setOpen(true)
  }, [openSignal])
  useEffect(() => {
    if (!isTauri()) return
    let live = true,
      timer: ReturnType<typeof setTimeout> | undefined,
      tracking = view.activeRequests.length > 0
    async function load() {
      try {
        const next = await transport.read()
        if (!live) return
        setView(next)
        tracking = next.activeRequests.length > 0
        if (pending || tracking) timer = setTimeout(() => void load(), 700)
      } catch {
        if (live && (pending || tracking)) timer = setTimeout(() => void load(), 1000)
      }
    }
    void load()
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [pending, epoch, open])
  useEffect(
    () => onActiveChange(pending || view.activeRequests.length > 0),
    [pending, view.activeRequests, onActiveChange],
  )
  return (
    <>
      {view.activeRequests.length > 0 && !open && (
        <section className="card" role="status">
          <p>整份文件夹归档仍在进行，请在历史中核对结果。</p>
          <button className="button secondary compact" onClick={onHistory}>
            查看文件夹归档记录
          </button>
          <button
            className="button secondary compact"
            onClick={() => {
              for (const request of view.activeRequests)
                void transport.cancel(request).catch(() => setEpoch((v) => v + 1))
            }}
          >
            停止文件夹归档
          </button>
        </section>
      )}
      {open && (
        <Create
          home={home}
          onClose={() => setOpen(false)}
          onActiveChange={setPending}
          onOutcome={() => {
            setEpoch((v) => v + 1)
            onOutcome()
          }}
        />
      )}
    </>
  )
}
function Scope({ draft: d }: { draft: FolderArchiveDraft }) {
  return (
    <section aria-label="整份文件夹归档核对">
      <dl className="application-update-facts">
        <dt>来源文件夹</dt>
        <dd className="mono full-path">{d.root}</dd>
        <dt>ZIP 目标</dt>
        <dd className="mono full-path">{d.outputPath}</dd>
        <dt>完整范围</dt>
        <dd>
          {d.files} 个文件 · {d.directories} 个目录（含来源） · {d.hidden} 个隐藏项目
        </dd>
        <dt>正文 / 元数据预估</dt>
        <dd>
          {bytes(d.bytes)} / {bytes(d.metadataBytes)}
        </dd>
      </dl>
      <details>
        <summary>查看前 {d.samples.length} 项名称</summary>
        <ul className="folder-archive-samples">
          {d.samples.map((s) => (
            <li key={s.name} className="mono full-path" data-i18n="off">
              {s.name}
              {s.directory ? '/' : ''}
            </li>
          ))}
        </ul>
        <p>这只是名称预览；确认范围是上述整个文件夹，包含未在预览中列出的支持项目。</p>
      </details>
    </section>
  )
}
function Create({
  home,
  onClose,
  onActiveChange,
  onOutcome,
}: {
  home: string
  onClose: () => void
  onActiveChange: (v: boolean) => void
  onOutcome: () => void
}) {
  const [root, setRoot] = useState(`${home}/Downloads`),
    [directory, setDirectory] = useState(`${home}/Documents`),
    [filename, setFilename] = useState('MooMole-folder.zip'),
    [draft, setDraft] = useState<FolderArchiveDraft | null>(null),
    [pending, setPending] = useState<'checking' | 'creating' | 'choosing' | null>(null),
    [error, setError] = useState(''),
    [ack, setAck] = useState(false),
    [submitted, setSubmitted] = useState(false),
    [record, setRecord] = useState<FolderArchiveRecord | null>(null),
    [stopRequested, setStopRequested] = useState(false)
  const request = useRef<string | null>(null),
    owned = useRef<FolderArchiveDraft | null>(null),
    busy = useRef(false),
    alive = useRef(true),
    now = useNow()
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
      if (owned.current) void transport.discard(owned.current.id).catch(() => {})
    }
  }, [])
  function revoke() {
    if (owned.current) void transport.discard(owned.current.id).catch(() => {})
    owned.current = null
    setDraft(null)
    setAck(false)
  }
  async function choose(source: boolean) {
    if (busy.current) return
    busy.current = true
    setPending('choosing')
    onActiveChange(true)
    setError('')
    try {
      const selected = await api.chooseDirectory(
        source ? '选择要整份归档的文件夹' : '选择整份归档目标目录',
      )
      if (selected && alive.current) {
        revoke()
        if (source) setRoot(selected)
        else setDirectory(selected)
      }
    } catch (e) {
      setError(String(e))
    } finally {
      busy.current = false
      if (alive.current) {
        setPending(null)
        onActiveChange(false)
      }
    }
  }
  async function prepare() {
    if (busy.current) return
    busy.current = true
    revoke()
    setPending('checking')
    onActiveChange(true)
    setStopRequested(false)
    setError('')
    const id = crypto.randomUUID()
    request.current = id
    try {
      const d = await transport.prepare(
        root.trim().replace(/\/+$/, ''),
        directory.trim().replace(/\/+$/, ''),
        filename.trim(),
        id,
      )
      if (alive.current) {
        owned.current = d
        setDraft(d)
      } else void transport.discard(d.id).catch(() => {})
    } catch (e) {
      if (alive.current) setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      request.current = null
      if (alive.current) {
        setPending(null)
        onActiveChange(false)
      }
    }
  }
  async function execute() {
    if (
      busy.current ||
      !draft ||
      !ack ||
      submitted ||
      now < draft.createdAt ||
      now >= draft.expiresAt
    )
      return
    busy.current = true
    setSubmitted(true)
    setPending('creating')
    setError('')
    setStopRequested(false)
    onActiveChange(true)
    owned.current = null
    const id = crypto.randomUUID()
    request.current = id
    try {
      const r = await transport.execute(draft, id)
      if (alive.current) setRecord(r)
    } catch (e) {
      if (alive.current) setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      request.current = null
      if (alive.current) {
        setPending(null)
        onActiveChange(false)
        onOutcome()
      }
    }
  }
  const expired = !!draft && (now < draft.createdAt || now >= draft.expiresAt)
  return (
    <Modal
      title="归档整个文件夹"
      className="application-update-modal"
      onClose={pending ? undefined : onClose}
    >
      <h2>归档整个文件夹</h2>
      <p>
        创建包含目录、空目录、普通文件和 Mac 元数据的 ZIP，原文件保留；归档本身不释放原文件占用。
      </p>
      <p className="notice warning">
        ZIP
        不加密，整份范围包含隐藏内容，可能含有私密资料。请核对来源与保存位置，不把归档作为唯一备份或完整迁移替代品。
      </p>
      {!draft && !submitted && (
        <>
          <label className="personal-path">
            <span>来源文件夹</span>
            <input
              aria-label="来源文件夹"
              value={root}
              disabled={!!pending}
              onChange={(e) => setRoot(e.target.value)}
            />
          </label>
          <button
            className="button secondary compact"
            disabled={!!pending}
            onClick={() => void choose(true)}
          >
            选择来源文件夹…
          </button>
          <label className="personal-path">
            <span>归档目标目录</span>
            <input
              aria-label="归档目标目录"
              value={directory}
              disabled={!!pending}
              onChange={(e) => setDirectory(e.target.value)}
            />
          </label>
          <button
            className="button secondary compact"
            disabled={!!pending}
            onClick={() => void choose(false)}
          >
            选择目标目录…
          </button>
          <label className="personal-path">
            <span>ZIP 文件名</span>
            <input
              aria-label="ZIP 文件名"
              value={filename}
              disabled={!!pending}
              onChange={(e) => setFilename(e.target.value)}
            />
          </label>
        </>
      )}
      {draft && <Scope draft={draft} />}
      <details>
        <summary>完整范围、元数据与取消边界</summary>
        <p>
          最多 10000 项、64 层，正文与元数据合计 8
          GiB。拒绝任何受保护、云占位、链接、应用资料库或无法核对的项目，不生成缺项的整份归档；目标必须已存在且位于来源之外，同名不覆盖。
        </p>
        <p>
          保留普通权限、秒级修改时间、Finder
          标签、扩展属性、资源分支和隔离属性。创建时间、纳秒时间、所有者、ACL
          和文件标记不作为完整保留承诺；Mac 元数据的恢复取决于解压工具支持。
        </p>
        <p>
          发布前重新核对整份来源。停止只阻止后续操作，已发布的 ZIP
          不自动删除；结果未知时核对目标与临时路径，不自动继续或重试。系统调用返回前仍占用任务资源。
        </p>
      </details>
      {pending && (
        <p role="status">
          {pending === 'choosing'
            ? '正在等待目录选择…'
            : pending === 'checking'
              ? '正在完整核对目录、内容和元数据，尚未创建 ZIP。'
              : '正在创建并核验整份 ZIP，请等待发布结果。'}
        </p>
      )}
      {record && <Outcome record={record} onError={setError} />}{' '}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {submitted && !pending && !record && (
        <p>文件夹归档请求未返回完整结果，请在历史中检查实际位置；不会自动重试。</p>
      )}
      {draft && !submitted && (
        <label className="application-update-consent">
          <input
            type="checkbox"
            checked={ack}
            disabled={!!pending || expired}
            onChange={(e) => setAck(e.target.checked)}
          />
          我已核对整份范围、隐藏内容与目标，同意创建未加密 ZIP 并保留全部原文件。
        </label>
      )}
      {expired && !submitted && <p role="status">整份归档确认已过期，请重新检查。</p>}
      <div className="modal-actions">
        {pending ? (
          <button
            className="button secondary"
            disabled={stopRequested || !request.current}
            onClick={() => {
              if (request.current) {
                setStopRequested(true)
                void transport.cancel(request.current).catch((e) => {
                  setError(String(e))
                  setStopRequested(false)
                })
              }
            }}
          >
            {stopRequested ? '已请求停止' : '停止文件夹归档'}
          </button>
        ) : (
          <button className="button secondary" onClick={onClose}>
            关闭归档窗口
          </button>
        )}
        {!submitted && !pending && (
          <button className="button secondary" onClick={() => void prepare()}>
            {draft ? '重新检查整份归档' : '检查整个文件夹'}
          </button>
        )}
        {draft && !submitted && !pending && (
          <button className="button secondary" onClick={revoke}>
            更改归档范围…
          </button>
        )}
        {draft && !submitted && (
          <button
            className="button primary"
            disabled={!!pending || !ack || expired}
            onClick={() => void execute()}
          >
            确认归档整份文件夹
          </button>
        )}
      </div>
    </Modal>
  )
}
function Outcome({
  record: r,
  onError,
}: {
  record: FolderArchiveRecord
  onError: (s: string) => void
}) {
  return (
    <section aria-label="文件夹归档结果">
      <strong>{folderArchiveState[r.state]}</strong>
      <p>{r.reason}</p>
      <p className="mono full-path">{r.draft.outputPath}</p>
      {r.state === 'completed' && r.artifact && <p>实际 ZIP 大小：{bytes(r.artifact.bytes)}</p>}
      {['unknown', 'interrupted'].includes(r.state) && (
        <p className="mono full-path">{r.temporaryPath}</p>
      )}
      <div className="modal-actions">
        <button
          className="button secondary compact"
          onClick={() => void transport.reveal(r.draft.outputPath).catch((e) => onError(String(e)))}
        >
          查看文件夹归档
        </button>
        <button
          className="button secondary compact"
          onClick={() => void transport.reveal(r.draft.root).catch((e) => onError(String(e)))}
        >
          查看原文件夹
        </button>
      </div>
    </section>
  )
}
export function FolderArchiveHistory() {
  const [view, setView] = useState(empty),
    [selected, setSelected] = useState<string | null>(null),
    [ack, setAck] = useState(false),
    [pending, setPending] = useState(false),
    [error, setError] = useState(''),
    [epoch, setEpoch] = useState(0)
  useEffect(() => {
    if (!isTauri()) return
    let live = true,
      timer: ReturnType<typeof setTimeout> | undefined,
      tracking = view.activeRequests.length > 0
    async function load() {
      try {
        const v = await transport.read()
        if (!live) return
        setView(v)
        tracking = v.activeRequests.length > 0
        if (tracking) timer = setTimeout(() => void load(), 700)
      } catch (e) {
        if (live) {
          setError(String(e))
          if (tracking) timer = setTimeout(() => void load(), 1000)
        }
      }
    }
    void load()
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [epoch])
  const current = view.records.find((r) => r.id === selected)
  return (
    <section className="card" aria-label="整份文件夹归档记录">
      <div className="list-toolbar">
        <h2>整份文件夹归档记录</h2>
        <button className="button secondary compact" onClick={() => setEpoch((v) => v + 1)}>
          刷新文件夹归档
        </button>
      </div>
      {!view.records.length && !error && <p className="muted">暂无整份文件夹归档。</p>}
      {view.records.map((r) => (
        <button
          key={r.id}
          className="saved-plan-row"
          onClick={() => {
            setSelected(r.id)
            setAck(false)
            setError('')
          }}
        >
          <span>
            <strong>{folderArchiveState[r.state]}</strong>
            <small data-i18n="off">{r.draft.outputPath}</small>
            <small>{dateTime(r.createdAt)}</small>
          </span>
        </button>
      ))}
      {error && !current && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {current && (
        <Modal
          title="整份文件夹归档记录"
          className="application-update-modal"
          onClose={pending ? undefined : () => setSelected(null)}
        >
          <h2>整份文件夹归档记录</h2>
          <Outcome record={current} onError={setError} />
          <Scope draft={current.draft} />
          {error && (
            <p className="notice warning" role="alert">
              {error}
            </p>
          )}
          {view.activeRequests.includes(current.requestId) && (
            <button
              className="button secondary"
              onClick={() =>
                void transport.cancel(current.requestId).catch((e) => setError(String(e)))
              }
            >
              停止文件夹归档
            </button>
          )}
          {['unknown', 'interrupted'].includes(current.state) && current.reviewedAt === null && (
            <>
              <p>请核对实际 ZIP 与临时路径。这里只记录核查，不补做归档、删除文件或更改原结果。</p>
              <label className="application-update-consent">
                <input
                  type="checkbox"
                  checked={ack}
                  disabled={pending}
                  onChange={(e) => setAck(e.target.checked)}
                />
                我已检查目标和临时位置，了解未知结果保持且不会自动重试。
              </label>
              <button
                className="button secondary"
                disabled={!ack || pending}
                onClick={() => {
                  setPending(true)
                  void transport
                    .acknowledge(current.id)
                    .then(() => {
                      setAck(false)
                      setEpoch((v) => v + 1)
                    })
                    .catch((e) => setError(String(e)))
                    .finally(() => setPending(false))
                }}
              >
                记录文件夹归档已核对
              </button>
            </>
          )}
        </Modal>
      )}
    </section>
  )
}
