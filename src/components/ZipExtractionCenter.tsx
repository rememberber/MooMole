import { useEffect, useRef, useState } from 'react'
import { isTauri } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { translate } from '../i18n/translate'
async function chooseZip() {
  const selected = await open({
    title: translate('选择要解压的压缩包文件'),
    directory: false,
    multiple: false,
    filters: [{ name: translate('压缩包'), extensions: ['zip', 'tar', 'gz', 'tgz'] }],
  })
  return typeof selected === 'string' ? selected : null
}

import * as api from '../lib/api'
import {
  zipExtraction as transport,
  zipExtractionState,
  extractionFormat,
  type ZipExtractionDraft,
  type ZipExtractionRecord,
  type ZipExtractionView,
} from '../lib/zip-extraction'
import { bytes, dateTime } from '../lib/format'
import { useNow } from '../lib/use-now'
import { Modal } from './Modal'
const empty: ZipExtractionView = { records: [], activeRequests: [] }
export function ZipExtractionCenter({
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
          <p>压缩包解压仍在进行，请在历史中核对结果。</p>
          <button className="button secondary compact" onClick={onHistory}>
            查看解压记录
          </button>
          <button
            className="button secondary compact"
            onClick={() => {
              for (const request of view.activeRequests)
                void transport.cancel(request).catch(() => setEpoch((v) => v + 1))
            }}
          >
            停止解压
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
function Scope({ draft: d }: { draft: ZipExtractionDraft }) {
  return (
    <section aria-label="压缩包解压核对">
      {d.encrypted && (
        <p className="notice warning">
          这是密码保护的
          ZIP。解压结果为明文，文件名和历史范围仍可见；密码不写入历史，解压完成后请妥善保管输出文件。
        </p>
      )}
      <dl className="application-update-facts">
        <dt>格式</dt>
        <dd data-i18n="off">
          {d.format === 'tar' ? 'TAR' : d.format === 'tarGzip' ? 'TAR.GZ / TGZ' : 'ZIP'}
        </dd>
        <dt>压缩包来源</dt>
        <dd className="mono full-path">{d.root}</dd>
        <dt>解压目标</dt>
        <dd className="mono full-path">{d.outputPath}</dd>
        <dt>完整范围</dt>
        <dd>
          {d.files} 个文件 · {d.directories} 个目录（含目标根目录） · {d.hidden} 个隐藏项目
        </dd>
        <dt>{d.macMetadata ? '解压内容与属性预算 / 其中元数据' : '解压数据 / 其中元数据文件'}</dt>
        <dd>
          {bytes(d.bytes)} / {bytes(d.metadataBytes)}
        </dd>
      </dl>
      {d.macMetadata && (
        <p className="notice warning" data-i18n="off">
          {translate(
            `恢复 ${d.macMetadata.items} 项的普通权限与修改时间、${d.macMetadata.attributes} 个数据属性；包含 ${d.macMetadata.executable} 个有执行权限的文件和 ${d.macMetadata.finderHidden} 个 Finder 隐藏项目。${d.macMetadata.skippedQuarantine} 项历史隔离记录不应用，全部输出使用新的系统隔离标记。`,
          )}
        </p>
      )}
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
        <p>这只是名称预览；确认范围是整个压缩包，包含未在预览中列出的文件和隐藏内容。</p>
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
  const [root, setRoot] = useState(''),
    [directory, setDirectory] = useState(`${home}/Documents`),
    [filename, setFilename] = useState('Extracted'),
    [restoreMetadata, setRestoreMetadata] = useState(false),
    [password, setPassword] = useState(''),
    [draft, setDraft] = useState<ZipExtractionDraft | null>(null),
    [pending, setPending] = useState<'checking' | 'creating' | 'choosing' | null>(null),
    [error, setError] = useState(''),
    [ack, setAck] = useState(false),
    [submitted, setSubmitted] = useState(false),
    [record, setRecord] = useState<ZipExtractionRecord | null>(null),
    [stopRequested, setStopRequested] = useState(false)
  const request = useRef<string | null>(null),
    owned = useRef<ZipExtractionDraft | null>(null),
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
      const selected = await (source ? chooseZip() : api.chooseDirectory('选择解压目标目录'))
      if (selected && alive.current) {
        revoke()
        if (source) {
          setRoot(selected)
          setPassword('')
          if (extractionFormat(selected) !== 'zip') setRestoreMetadata(false)
        } else setDirectory(selected)
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
      const preparing = transport.prepare(
        root.trim().replace(/\/+$/, ''),
        directory.trim().replace(/\/+$/, ''),
        filename.trim(),
        id,
        restoreMetadata,
        password,
      )
      setPassword('')
      const d = await preparing
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
      title="解压压缩包"
      className="application-update-modal"
      onClose={pending ? undefined : onClose}
    >
      <h2>解压压缩包</h2>
      <p>
        将 ZIP、TAR 或 TAR.GZ/TGZ
        的完整内容解压到新文件夹，原压缩包保留；不覆盖已有文件，不自动打开内容。
      </p>
      <p className="notice warning">
        {restoreMetadata
          ? '恢复 ZIP 中配对的 Mac 数据属性、普通权限与修改时间，包含执行权限和 Finder 隐藏标志。全部输出使用新的隔离标记，历史安全批准不恢复；外层目标目录仅本账户可访问。'
          : '完整解压包含隐藏文件。Mac 元数据文件保留为独立文件，未恢复标签或资源分支；输出采用本账户专用权限，并保留原压缩包的隔离属性。'}
      </p>
      {!draft && !submitted && (
        <>
          <label className="personal-path">
            <span>压缩包来源</span>
            <input
              aria-label="压缩包来源"
              value={root}
              disabled={!!pending}
              onChange={(e) => {
                setRoot(e.target.value)
                setPassword('')
                if (extractionFormat(e.target.value.trim()) !== 'zip') setRestoreMetadata(false)
              }}
            />
          </label>
          <button
            className="button secondary compact"
            disabled={!!pending}
            onClick={() => void choose(true)}
          >
            选择压缩包文件…
          </button>
          {extractionFormat(root.trim()) === 'zip' && (
            <label className="personal-path">
              <span>ZIP 密码（如需要）</span>
              <input
                type="password"
                aria-label="ZIP 密码（如需要）"
                autoComplete="off"
                spellCheck={false}
                value={password}
                disabled={!!pending}
                onChange={(e) => setPassword(e.target.value)}
              />
            </label>
          )}
          <label className="personal-path">
            <span>解压目标目录</span>
            <input
              aria-label="解压目标目录"
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
          <label className="application-update-consent">
            <input
              type="checkbox"
              checked={restoreMetadata}
              disabled={!!pending || extractionFormat(root.trim()) !== 'zip'}
              onChange={(e) => {
                revoke()
                setRestoreMetadata(e.target.checked)
              }}
            />
            恢复 Mac 标签、资源分支及文件属性（ZIP）
          </label>
          <label className="personal-path">
            <span>新文件夹名称</span>
            <input
              aria-label="新文件夹名称"
              value={filename}
              disabled={!!pending}
              onChange={(e) => setFilename(e.target.value)}
            />
          </label>
        </>
      )}
      {draft && <Scope draft={draft} />}
      <details>
        <summary>解压范围、元数据与取消边界</summary>
        <p>
          压缩包最多 2 GiB，解压内容最多 8 GiB、10000 项和 64 层。ZIP 支持存储、Deflate
          和密码解压；密码只用于本次检查与执行，不验证文件来源。旧式 ZIP
          加密强度较低，输出为明文。另支持普通 TAR、TAR.GZ/TGZ（单个 Gzip 成员）。TAR
          的路径与大小扩展经核对后使用，原权限与嵌入元数据不应用；链接、特殊文件、稀疏布局、越界名称和无法核对的条目整份拒绝。目标必须是已有普通目录，新文件夹同名不覆盖。
        </p>
        <p>
          默认仅解压正文：文件权限为 600，目录为 700，Mac 元数据副本保留为独立文件。选中 Mac
          恢复时，核对配对的 AppleDouble
          后恢复标签、资源分支、支持的数据属性、普通权限和修改时间；每项元数据最多 32 MiB，总量最多
          256 MiB。缺少明确属性、包含
          ACL、别名或未支持属性时整份拒绝，不静默跳过。两种模式都不恢复所有者、不自动打开或执行内容。
        </p>
        <p>
          发布前重新核对原压缩包、目标及全部解压内容。停止后清理本次临时内容；已发布结果不自动删除。中断或结果未知时需核对实际位置，不自动继续或重试。
        </p>
      </details>
      {pending && (
        <p role="status">
          {pending === 'choosing'
            ? '正在等待目录选择…'
            : pending === 'checking'
              ? '正在完整核对压缩包条目和内容，尚未解压。'
              : '正在解压并读回核验，请等待发布结果。'}
        </p>
      )}
      {record && <Outcome record={record} onError={setError} />}{' '}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {submitted && !pending && !record && (
        <p>压缩包解压请求未返回完整结果，请在历史中检查实际位置；不会自动重试。</p>
      )}
      {draft && !submitted && (
        <label className="application-update-consent">
          <input
            type="checkbox"
            checked={ack}
            disabled={!!pending || expired}
            onChange={(e) => setAck(e.target.checked)}
          />
          我已核对全部内容、隐藏范围及元数据边界，同意解压到新文件夹并保留原压缩包。
        </label>
      )}
      {expired && !submitted && <p role="status">解压范围确认已过期，请重新检查。</p>}
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
            {stopRequested ? '已请求停止' : '停止解压'}
          </button>
        ) : (
          <button className="button secondary" onClick={onClose}>
            关闭解压窗口
          </button>
        )}
        {!submitted && !pending && (
          <button
            className="button secondary"
            disabled={!root.trim() || !directory.trim() || !filename.trim()}
            onClick={() => void prepare()}
          >
            {draft ? '重新检查解压范围' : '检查压缩包内容'}
          </button>
        )}
        {draft && !submitted && !pending && (
          <button className="button secondary" onClick={revoke}>
            更改解压范围…
          </button>
        )}
        {draft && !submitted && (
          <button
            className="button primary"
            disabled={!!pending || !ack || expired}
            onClick={() => void execute()}
          >
            确认解压压缩包
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
  record: ZipExtractionRecord
  onError: (s: string) => void
}) {
  return (
    <section aria-label="压缩包解压结果">
      <strong>{zipExtractionState[r.state]}</strong>
      <p>{r.reason}</p>
      <p className="mono full-path">{r.draft.outputPath}</p>
      {r.state === 'completed' && r.artifact && (
        <p>
          {r.draft.macMetadata ? '内容与属性容量预算：' : '解压数据大小：'}
          {bytes(r.artifact.bytes)}
        </p>
      )}
      {['unknown', 'interrupted'].includes(r.state) && (
        <p className="mono full-path">{r.temporaryPath}</p>
      )}
      <div className="modal-actions">
        <button
          className="button secondary compact"
          onClick={() => void transport.reveal(r.draft.outputPath).catch((e) => onError(String(e)))}
        >
          查看解压结果
        </button>
        <button
          className="button secondary compact"
          onClick={() => void transport.reveal(r.draft.root).catch((e) => onError(String(e)))}
        >
          查看原压缩包
        </button>
      </div>
    </section>
  )
}
export function ZipExtractionHistory() {
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
    <section className="card" aria-label="压缩包解压记录">
      <div className="list-toolbar">
        <h2>压缩包解压记录</h2>
        <button className="button secondary compact" onClick={() => setEpoch((v) => v + 1)}>
          刷新解压记录
        </button>
      </div>
      {!view.records.length && !error && <p className="muted">暂无压缩包解压记录。</p>}
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
            <strong>{zipExtractionState[r.state]}</strong>
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
          title="压缩包解压记录"
          className="application-update-modal"
          onClose={pending ? undefined : () => setSelected(null)}
        >
          <h2>压缩包解压记录</h2>
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
              停止解压
            </button>
          )}
          {['unknown', 'interrupted'].includes(current.state) && current.reviewedAt === null && (
            <>
              <p>
                请核对实际解压结果与临时路径。这里只记录核查，不补做解压、删除文件或更改原结果。
              </p>
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
                记录解压结果已核对
              </button>
            </>
          )}
        </Modal>
      )}
    </section>
  )
}
