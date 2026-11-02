import { useEffect, useRef, useState } from 'react'
import { Modal } from './Modal'
import { useNow } from '../lib/use-now'
import type { ExecutionDetail } from '../types'
import {
  restorationBatchTransport,
  type RestorationBatchTarget,
  type RestorationBatchDraft,
} from '../lib/file-restoration-batch'
export function FileRestorationBatchCenter({
  target,
  onClose,
  onActiveChange,
  onOutcome,
}: {
  target: RestorationBatchTarget | null
  onClose: () => void
  onActiveChange: (v: boolean) => void
  onOutcome: () => void
}) {
  return (
    target && (
      <Batch
        key={`${target.runId}:${target.items.map((i) => i.itemId).join(',')}`}
        target={target}
        onClose={onClose}
        onActiveChange={onActiveChange}
        onOutcome={onOutcome}
      />
    )
  )
}
function Batch({
  target,
  onClose,
  onActiveChange,
  onOutcome,
}: {
  target: RestorationBatchTarget
  onClose: () => void
  onActiveChange: (v: boolean) => void
  onOutcome: () => void
}) {
  const [draft, setDraft] = useState<RestorationBatchDraft | null>(null),
    [checking, setChecking] = useState(true),
    [pending, setPending] = useState(false),
    [ack, setAck] = useState(false),
    [error, setError] = useState(''),
    [result, setResult] = useState<ExecutionDetail | null>(null)
  const busy = useRef(false),
    request = useRef<string | null>(null)
  useNow()
  // Preparation can finish between timer ticks. Use the render's clock so a
  // freshly returned confirmation is not briefly labelled as expired.
  const now = Date.now()
  useEffect(() => {
    let live = true,
      prepared: RestorationBatchDraft | null = null
    void restorationBatchTransport
      .prepare(target)
      .then((d) => {
        prepared = d
        if (live) setDraft(d)
        else void restorationBatchTransport.discard(d.id).catch(() => {})
      })
      .catch((e) => {
        if (live) setError(String(e).replace(/^Error: /, ''))
      })
      .finally(() => {
        if (live) setChecking(false)
      })
    return () => {
      live = false
      if (prepared) void restorationBatchTransport.discard(prepared.id).catch(() => {})
    }
  }, [target])
  async function run() {
    if (
      busy.current ||
      !draft ||
      !ack ||
      now < draft.createdAt ||
      now >= draft.expiresAt ||
      request.current
    )
      return
    busy.current = true
    request.current = crypto.randomUUID()
    setPending(true)
    onActiveChange(true)
    try {
      setResult(await restorationBatchTransport.execute(draft, request.current))
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      setPending(false)
      onActiveChange(false)
      onOutcome()
    }
  }
  const valid = draft && now >= draft.createdAt && now < draft.expiresAt
  return (
    <Modal
      className="file-restoration-modal"
      title={result ? '批量恢复结果' : '批量恢复原位置'}
      onClose={pending ? undefined : onClose}
    >
      <h2>{result ? '批量恢复结果' : '批量恢复原位置'}</h2>
      <p className="notice">
        逐项核对收据和原位置，同卷移回普通文件；不覆盖、不改名、不创建目录。检查未通过时请返回调整选择；执行时遇到冲突或变化则保留该项，继续处理其余项。取消会保留已经恢复的文件，没有整批撤销。
      </p>
      {target.items.some((i) => i.softwareContent) && (
        <p className="notice">
          恢复文件会保留原处理记录，不修改浏览器下载记录、聊天导出清单或播放器资料库；应用是否立即识别恢复后的文件需要另行核对。
        </p>
      )}
      <ol className="execution-items" aria-label="批量恢复选择">
        {target.items.map((t) => {
          const d = draft?.items.find((i) => i.sourceItemId === t.itemId),
            item = result?.items.find((i) => i.id === d?.itemId)
          return (
            <li key={t.itemId}>
              <p className="mono full-path" data-i18n="off">
                {t.currentPath} → {t.originalPath}
              </p>
              {item && (
                <>
                  <strong>
                    {item.state === 'moved'
                      ? '已恢复到原位置'
                      : item.state === 'uncertain'
                        ? '恢复结果待核实'
                        : '本项未恢复'}
                  </strong>
                  <p>{item.reason}</p>
                </>
              )}
            </li>
          )
        })}
      </ol>
      {checking && <p role="status">正在逐项核对收据与原位置…</p>}
      {draft && !valid && !result && !pending && (
        <p role="status">恢复确认已过期，请返回后重新检查。</p>
      )}
      {draft && !result && !request.current && (
        <label className="confirm-check">
          <input
            type="checkbox"
            checked={ack}
            disabled={!valid || pending}
            onChange={(e) => setAck(e.target.checked)}
          />
          我已核对全部所选文件及原位置，确认批量移回。
        </label>
      )}
      {pending && <p role="status">正在逐项恢复；停止后不会移回已经完成的项目。</p>}
      {result && (
        <p role="status">
          已恢复 {result.movedCount} / {result.itemCount}{' '}
          项；逐项结果已保存在同一条恢复历史中，原处理记录保持不变。
        </p>
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
                void restorationBatchTransport
                  .cancel(request.current)
                  .catch((e) => setError(String(e)))
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
            disabled={checking || !valid || !ack}
            onClick={() => void run()}
          >
            确认恢复所选文件
          </button>
        )}
      </div>
    </Modal>
  )
}
