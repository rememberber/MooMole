import { useEffect, useRef, useState } from 'react'
import { Modal } from './Modal'
import { useNow } from '../lib/use-now'
import type { ExecutionDetail } from '../types'
import type {
  RestorationTarget,
  RestorationDraft,
  RestorationTransport,
} from '../lib/file-restoration'

export function FileRestorationCenter({
  target,
  transport,
  onClose,
  onActiveChange,
  onOutcome,
}: {
  target: RestorationTarget | null
  transport: RestorationTransport
  onClose: () => void
  onActiveChange: (active: boolean) => void
  onOutcome: () => void
}) {
  return (
    target && (
      <Restoration
        key={`${target.runId}:${target.itemId}`}
        target={target}
        transport={transport}
        onClose={onClose}
        onActiveChange={onActiveChange}
        onOutcome={onOutcome}
      />
    )
  )
}
function Restoration({
  target,
  transport,
  onClose,
  onActiveChange,
  onOutcome,
}: {
  target: RestorationTarget
  transport: RestorationTransport
  onClose: () => void
  onActiveChange: (active: boolean) => void
  onOutcome: () => void
}) {
  const [draft, setDraft] = useState<RestorationDraft | null>(null),
    [checking, setChecking] = useState(true)
  const [pending, setPending] = useState(false),
    [ack, setAck] = useState(false)
  const [result, setResult] = useState<ExecutionDetail | null>(null),
    [error, setError] = useState('')
  const busy = useRef(false),
    request = useRef<string | null>(null)
  const now = useNow()
  useEffect(() => {
    let live = true,
      prepared: RestorationDraft | null = null
    void transport
      .prepare(target)
      .then((d) => {
        prepared = d
        if (live) setDraft(d)
        else void transport.discard(d.id).catch(() => {})
      })
      .catch((e) => {
        if (live) setError(String(e).replace(/^Error: /, ''))
      })
      .finally(() => {
        if (live) setChecking(false)
      })
    return () => {
      live = false
      if (prepared) void transport.discard(prepared.id).catch(() => {})
    }
  }, [target, transport])
  async function run() {
    if (busy.current || !ack || !draft || now >= draft.expiresAt || request.current) return
    busy.current = true
    request.current = crypto.randomUUID()
    setPending(true)
    onActiveChange(true)
    try {
      setResult(await transport.execute(draft, request.current))
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    } finally {
      setPending(false)
      busy.current = false
      onActiveChange(false)
      onOutcome()
    }
  }
  const item = result?.items[0]
  return (
    <Modal title={result ? '文件恢复结果' : '恢复原位置'} onClose={pending ? undefined : onClose}>
      <h2>{result ? '文件恢复结果' : '恢复原位置'}</h2>
      <dl>
        <dt>当前记录位置</dt>
        <dd className="mono full-path" data-i18n="off">
          {target.currentPath}
        </dd>
        <dt>恢复到原位置</dt>
        <dd className="mono full-path" data-i18n="off">
          {target.originalPath}
        </dd>
      </dl>
      <p className="notice">
        仅恢复这一份已记录的普通文件，重新核对身份和保护范围。原位置必须存在且没有同名项目；不会覆盖、自动改名或创建目录。当前文件若已被移走、替换或权限受限，将保留并说明原因。
      </p>
      {checking && <p role="status">正在核对当前文件与原位置…</p>}
      {draft && !result && !pending && now >= draft.expiresAt && (
        <p role="status">恢复确认已过期，请返回后重新检查。</p>
      )}
      {draft && !result && !request.current && (
        <label className="confirm-check">
          <input
            type="checkbox"
            checked={ack}
            disabled={pending || now >= draft.expiresAt}
            onChange={(e) => setAck(e.target.checked)}
          />
          我已核对当前文件和原位置，确认将这一份文件移回。
        </label>
      )}
      {pending && (
        <p role="status">正在恢复。取消只会阻止尚未开始的系统调用，已经移回的文件会保留。</p>
      )}
      {item && (
        <section aria-label="文件恢复结果">
          <strong>
            {item.state === 'moved'
              ? '已恢复到原位置'
              : item.state === 'uncertain'
                ? '恢复结果待核实'
                : '本项未恢复'}
          </strong>
          <p>{item.reason}</p>
          <p className="muted">恢复另存为一条执行记录，原处理结果保持不变。请在原位置核对文件。</p>
        </section>
      )}
      {error && (
        <p role="alert" className="notice warning">
          {error}
        </p>
      )}
      <div className="modal-actions">
        {pending ? (
          <button
            className="button secondary"
            onClick={() => {
              if (request.current)
                void transport.cancel(request.current).catch((e) => setError(String(e)))
            }}
          >
            取消后续恢复
          </button>
        ) : (
          <button className="button secondary" onClick={onClose}>
            返回历史
          </button>
        )}
        {!result && !request.current && (
          <button
            className="button primary"
            disabled={checking || !draft || !ack || now >= draft.expiresAt}
            onClick={() => void run()}
          >
            确认恢复这一项
          </button>
        )}
      </div>
    </Modal>
  )
}
