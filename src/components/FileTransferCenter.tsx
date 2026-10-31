import { useEffect, useRef, useState } from 'react'
import * as api from '../lib/api'
import {
  transferTransport as transport,
  transferState,
  type TransferTarget,
  type TransferDraft,
  type TransferRecord,
} from '../lib/file-transfer'
import { bytes, dateTime } from '../lib/format'
import { useNow } from '../lib/use-now'
import { Modal } from './Modal'
import type { TransferRestoreTarget } from '../lib/transfer-restoration'
function Items({
  record,
  onError,
  onRestore,
  canRestore = false,
}: {
  record: TransferRecord
  onError: (error: string) => void
  onRestore?: (target: TransferRestoreTarget) => void
  canRestore?: boolean
}) {
  return (
    <div className="card personal-list" role="region" aria-label="跨磁盘逐项结果">
      {record.items.map((i) => (
        <article key={i.id}>
          <div>
            <strong>{transferState[i.state] ?? '跨磁盘结果待核实'}</strong>
            <p>{i.reason}</p>
            <p className="mono full-path" data-i18n="off">
              {i.path}
            </p>
            <p>
              确认目标：
              <span className="mono full-path" data-i18n="off">
                {i.target ?? '—'}
              </span>
            </p>
            {i.copyReceipt && (
              <p>
                副本记录位置：
                <span className="mono full-path" data-i18n="off">
                  {i.copyReceipt.destination}
                </span>
                <button
                  className="button text compact"
                  onClick={() =>
                    void api.reveal(i.copyReceipt!.destination).catch((e) => onError(String(e)))
                  }
                >
                  查看副本记录位置
                </button>
              </p>
            )}
            {i.sourceTrashReceipt && (
              <p>
                原件废纸篓位置：
                <span className="mono full-path" data-i18n="off">
                  {i.sourceTrashReceipt.destination}
                </span>
                <button
                  className="button text compact"
                  onClick={() =>
                    void api
                      .reveal(i.sourceTrashReceipt!.destination)
                      .catch((e) => onError(String(e)))
                  }
                >
                  查看原件记录位置
                </button>
              </p>
            )}
            {onRestore &&
              record.state !== 'running' &&
              i.state === 'moved' &&
              i.copyReceipt &&
              i.sourceTrashReceipt && (
                <button
                  className="button secondary compact"
                  disabled={!canRestore || (record.state !== 'completed' && !record.reviewedAt)}
                  onClick={() =>
                    onRestore({
                      runId: record.id,
                      itemId: i.id,
                      originalPath: i.path,
                      copyPath: i.copyReceipt!.destination,
                    })
                  }
                >
                  从副本恢复原位置…
                </button>
              )}
            {!['moved', 'skipped'].includes(i.state) && i.temporaryPath && (
              <p>
                临时记录位置：
                <span className="mono full-path" data-i18n="off">
                  {i.temporaryPath}
                </span>
              </p>
            )}
          </div>
        </article>
      ))}
    </div>
  )
}
export function FileTransferCenter({
  target,
  onClose,
  onActiveChange,
  onOutcome,
}: {
  target: TransferTarget | null
  onClose: () => void
  onActiveChange: (active: boolean) => void
  onOutcome: () => void
}) {
  return (
    target && (
      <Transfer
        key={`${target.scanId}:${target.destination}:${target.items.map((i) => i.id).join(',')}`}
        target={target}
        onClose={onClose}
        onActiveChange={onActiveChange}
        onOutcome={onOutcome}
      />
    )
  )
}
function Transfer({
  target,
  onClose,
  onActiveChange,
  onOutcome,
}: {
  target: TransferTarget
  onClose: () => void
  onActiveChange: (active: boolean) => void
  onOutcome: () => void
}) {
  const [draft, setDraft] = useState<TransferDraft | null>(null),
    [record, setRecord] = useState<TransferRecord | null>(null),
    [checking, setChecking] = useState(true),
    [pending, setPending] = useState(false),
    [ack, setAck] = useState(false),
    [error, setError] = useState('')
  const request = useRef<string | null>(null),
    spent = useRef(false),
    now = useNow()
  useEffect(() => {
    let live = true,
      prepared: TransferDraft | null = null
    void transport
      .prepare(target)
      .then((d) => {
        prepared = d
        if (live) setDraft(d)
        else void transport.discard(d.id).catch(() => {})
      })
      .catch((e) => {
        if (live) setError(String(e))
      })
      .finally(() => {
        if (live) setChecking(false)
      })
    return () => {
      live = false
      if (prepared) void transport.discard(prepared.id).catch(() => {})
    }
  }, [target])
  useEffect(() => {
    onActiveChange(pending)
  }, [pending, onActiveChange])
  useEffect(() => {
    if (!pending || !draft || !request.current) return
    let live = true,
      timer: ReturnType<typeof setTimeout> | undefined
    const read = async () => {
      try {
        const view = await transport.read()
        if (!live) return
        const next = view.records.find((r) => r.id === draft.id && r.requestId === request.current)
        if (next) {
          setRecord(next)
          if (next.state !== 'running') {
            setPending(false)
            onOutcome()
            return
          }
        }
      } catch (e) {
        if (live) setError(String(e))
      }
      if (live) timer = setTimeout(() => void read(), 700)
    }
    void read()
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [pending, draft])
  async function execute() {
    if (!draft || !ack || spent.current || now >= draft.expiresAt) return
    spent.current = true
    request.current = crypto.randomUUID()
    setPending(true)
    setError('')
    try {
      setRecord(await transport.execute(draft, request.current))
      setPending(false)
    } catch (e) {
      setError(String(e))
      try {
        const v = await transport.read()
        const r = v.records.find((r) => r.id === draft.id)
        if (r) setRecord(r)
        setPending(v.activeRequests.includes(request.current))
      } catch {
        setPending(false)
      }
    } finally {
      onOutcome()
    }
  }
  const stop = () => {
    if (request.current) void transport.cancel(request.current).catch((e) => setError(String(e)))
  }
  return (
    <Modal title="跨磁盘移动确认与结果" onClose={pending ? undefined : onClose}>
      <h2>{record ? transferState[record.state] : '移动到另一磁盘'}</h2>
      <p className="notice">
        先复制并校验目标文件，再将原件移入原磁盘的废纸篓。不会覆盖同名目标、永久删除原件或清空废纸篓；原磁盘空间不会因此立即释放。停止后已生成的副本会保留，原件处理未完成时会分别记录。请先关闭相关文件；本轮不完整检查所有应用占用。
      </p>
      <details>
        <summary>复制校验与恢复边界</summary>
        <p>
          核对内容、权限、修改时间、扩展属性和资源分支；安全标记保留并按系统形式核对。复制失败、变化或未知时不继续处理原件。完成后可从跨磁盘移动记录选择可靠副本，另行确认恢复原位置；副本与原废纸篓项目不自动删除。
        </p>
      </details>
      {checking && <p role="status">正在核对跨磁盘计划…</p>}
      {!record && draft && (
        <div className="card personal-list" role="region" aria-label="跨磁盘确认文件">
          {draft.items.map((i) => (
            <article key={i.id}>
              <div>
                <p className="mono full-path" data-i18n="off">
                  {i.path}
                </p>
                <p className="mono full-path" data-i18n="off">
                  → {draft.targets.find((t) => t.id === i.id)?.destination ?? '—'}
                </p>
                <small>{bytes(i.size)}</small>
                {!draft.targets.find((t) => t.id === i.id)?.destination && (
                  <p>此项按重名规则保留。</p>
                )}
              </div>
            </article>
          ))}
        </div>
      )}
      {record && (
        <>
          <p>{record.reason}</p>
          <Items record={record} onError={setError} />
        </>
      )}
      {draft && !spent.current && (
        <>
          <label className="confirm-check">
            <input type="checkbox" checked={ack} onChange={(e) => setAck(e.target.checked)} />
            我已核对目标，确认校验后把原件移入废纸篓，并了解取消和恢复边界。
          </label>
          {now >= draft.expiresAt && <p role="status">跨磁盘确认已过期，请返回后重新检查。</p>}
        </>
      )}
      {pending && <p role="status">正在复制、校验或处理原件；请以逐项记录为准。</p>}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      <div className="dialog-actions">
        {draft && !spent.current && (
          <button
            className="button primary"
            disabled={!ack || now >= draft.expiresAt}
            onClick={() => void execute()}
          >
            确认跨磁盘移动
          </button>
        )}
        {pending ? (
          <button className="button secondary" onClick={stop}>
            停止跨磁盘移动
          </button>
        ) : (
          <button className="button secondary" onClick={onClose}>
            返回文件列表
          </button>
        )}
      </div>
    </Modal>
  )
}
export function FileTransferHistory({
  onRestore,
  canRestore = false,
}: {
  onRestore?: (target: TransferRestoreTarget) => void
  canRestore?: boolean
}) {
  const [records, setRecords] = useState<TransferRecord[]>([]),
    [active, setActive] = useState<string[]>([]),
    [selected, setSelected] = useState<TransferRecord | null>(null),
    [ack, setAck] = useState(false),
    [error, setError] = useState(''),
    [epoch, setEpoch] = useState(0)
  useEffect(() => {
    if (!api.desktop) return
    let live = true,
      timer: ReturnType<typeof setTimeout> | undefined
    const load = async () => {
      try {
        const v = await transport.read()
        if (!live) return
        setRecords(v.records)
        setActive(v.activeRequests)
        setSelected((old) => (old ? (v.records.find((r) => r.id === old.id) ?? old) : null))
        if (v.activeRequests.length) timer = setTimeout(() => void load(), 700)
      } catch (e) {
        if (live) setError(String(e))
      }
    }
    void load()
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [epoch])
  return (
    <section className="card" aria-label="跨磁盘移动记录">
      <div className="list-toolbar">
        <h2>跨磁盘移动记录</h2>
        <button
          className="button secondary compact"
          disabled={!api.desktop}
          onClick={() => setEpoch((v) => v + 1)}
        >
          刷新跨磁盘记录
        </button>
      </div>
      {records.length === 0 && <p className="muted">尚无跨磁盘移动记录。</p>}
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
            <strong>{transferState[r.state]}</strong>
            <small>
              {dateTime(r.createdAt)} · {r.items.length} 项
            </small>
          </span>
        </button>
      ))}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {selected && (
        <Modal title="跨磁盘移动记录" onClose={() => setSelected(null)}>
          <h2>{transferState[selected.state]}</h2>
          <p>{selected.reason}</p>
          <Items
            record={selected}
            onError={setError}
            canRestore={canRestore}
            onRestore={
              onRestore
                ? (target) => {
                    setSelected(null)
                    onRestore(target)
                  }
                : undefined
            }
          />
          {active.includes(selected.requestId) && (
            <button
              className="button secondary"
              onClick={() =>
                void transport.cancel(selected.requestId).catch((e) => setError(String(e)))
              }
            >
              停止跨磁盘移动
            </button>
          )}
          {['partial', 'uncertain', 'interrupted'].includes(selected.state) &&
            !selected.reviewedAt && (
              <>
                <p>
                  请核对副本、原件废纸篓及临时位置。记录已检查不会移动文件、补做步骤或把原结果改成成功。
                </p>
                <label className="confirm-check">
                  <input type="checkbox" checked={ack} onChange={(e) => setAck(e.target.checked)} />
                  我已核对两端和临时位置，了解原结果仍保留。
                </label>
                <button
                  className="button secondary"
                  disabled={!ack}
                  onClick={async () => {
                    try {
                      setSelected(await transport.acknowledge(selected.id))
                      setEpoch((v) => v + 1)
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
            关闭跨磁盘记录
          </button>
        </Modal>
      )}
    </section>
  )
}
