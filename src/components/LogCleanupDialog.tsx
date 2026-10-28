import { useEffect, useRef, useState } from 'react'
import type { CleanupConfirmation, ExecutionDetail } from '../types'
import { Modal } from './Modal'
import { bytes } from '../lib/format'
import { useNow } from '../lib/use-now'
import { ExecutionLocation, type RevealExecutionItem } from './ExecutionLocation'
import { ExecutionReason } from './ExecutionReason'

const states: Record<string, string> = {
  moved: '已移入废纸篓',
  skipped: '已保留',
  failed: '未作修改',
  cancelled: '开始前取消',
  notStarted: '尚未开始',
  uncertain: '结果待核实',
  pending: '尚未开始',
  intent: '等待完成证据',
}
export function LogCleanupDialog({
  confirmation,
  simple = false,
  onExecute,
  onDiscard,
  onBack,
  onClose,
  onRevealReceipt,
}: {
  simple?: boolean
  confirmation: CleanupConfirmation
  onExecute: (id: string, signal: AbortSignal) => Promise<ExecutionDetail>
  onDiscard?: (id: string) => Promise<void>
  onBack: () => void
  onClose: () => void
  onRevealReceipt?: RevealExecutionItem
}) {
  useNow()
  const expired = Date.now() < confirmation.createdAt || Date.now() >= confirmation.expiresAt
  const [acknowledged, setAcknowledged] = useState(simple)
  const [busy, setBusy] = useState(false)
  const [consumed, setConsumed] = useState(false)
  const [cancelling, setCancelling] = useState(false)
  const [result, setResult] = useState<ExecutionDetail | null>(null)
  const [error, setError] = useState('')
  const controller = useRef<AbortController | null>(null)
  const discard = useRef(onDiscard)
  discard.current = onDiscard
  useEffect(
    () => () => {
      controller.current?.abort()
      void discard.current?.(confirmation.id).catch(() => {})
    },
    [confirmation.id],
  )
  async function execute() {
    if (!acknowledged || busy || consumed) return
    if (Date.now() < confirmation.createdAt || Date.now() >= confirmation.expiresAt) {
      setError('确认已过期，请返回复查后再处理。')
      return
    }
    const abort = new AbortController()
    controller.current = abort
    setBusy(true)
    setConsumed(true)
    setError('')
    try {
      setResult(await onExecute(confirmation.id, abort.signal))
    } catch (error) {
      setError(`${String(error).replace(/^Error: /, '')} 请在历史中核对实际结果，不要重复处理。`)
    } finally {
      setBusy(false)
      controller.current = null
    }
  }
  const close = () => {
    controller.current?.abort()
    onClose()
  }
  return (
    <Modal
      title={result ? '清理结果' : busy ? '正在清理' : '确认移入废纸篓'}
      onClose={close}
      className="review-plan-modal"
    >
      <h2>{result ? '清理结果' : busy ? '正在清理' : '确认移入废纸篓'}</h2>
      {result ? (
        <>
          <p role="status">
            已移入 {result.movedCount} 项 · 其余 {result.itemCount - result.movedCount} 项未确认移入
          </p>
          {(result.uncertainCount > 0 || result.state === 'interrupted') && (
            <p className="notice warning">
              本批结果需要核实，已停止后续处理。请查看历史中的实际收据，不要重复执行。
            </p>
          )}
          <p className="muted">
            移入废纸篓不等于释放空间；当前没有应用内恢复。完整记录已保存在历史中，再次处理需重新扫描并生成计划。
          </p>
          <div className="plan-items">
            {result.items.map((item) => (
              <article className="plan-item" key={item.id}>
                <strong>{states[item.state] ?? '状态待核实'}</strong>
                <p className="mono full-path">{item.path}</p>
                <ExecutionReason reason={item.reason} />
                {item.receipt && (
                  <details>
                    <summary>实际移入位置</summary>
                    <p className="mono full-path">{item.receipt.destination}</p>
                    {onRevealReceipt && (
                      <ExecutionLocation
                        runId={result.id}
                        itemId={item.id}
                        onReveal={onRevealReceipt}
                      />
                    )}
                  </details>
                )}
              </article>
            ))}
          </div>
          <button className="button primary" onClick={close}>
            完成
          </button>
        </>
      ) : (
        <>
          <p>
            {confirmation.items.length} 个清理项目 · 文件大小合计{' '}
            {bytes(confirmation.items.reduce((total, item) => total + item.sizeBytes, 0))}
          </p>
          <p className="muted">
            仅处理下面列出的文件，另有 {confirmation.retainedCount}{' '}
            项保留。缓存可能需要重新生成或下载，日志可能用于排查问题，请确认不再需要。
          </p>
          <p className="notice warning">
            移入废纸篓不会立即释放空间，当前没有应用内恢复。处理期间若外部应用移动或改写文件，结果可能需要核实；不会自动重试。
          </p>
          {confirmation.requiresAuthorization && (
            <p className="notice">
              确认后将请求系统授权，只读检查所列文件的占用。随后重新核对 Mole
              规则，符合条件的文件由当前账户移入废纸篓；未知、变化或检查过期的文件保留。
            </p>
          )}
          <div className="plan-items">
            {confirmation.items.map((item) => (
              <article className="plan-item" key={item.id}>
                <strong>{item.path.split('/').at(-1)}</strong>
                <p className="mono full-path">{item.path}</p>
                <span>{bytes(item.sizeBytes)}</span>
              </article>
            ))}
          </div>
          {busy ? (
            <>
              <p role="status">
                {cancelling
                  ? '正在停止后续项目，等待当前项的实际结果。'
                  : confirmation.requiresAuthorization
                    ? '正在授权检查并逐项复核处理；系统确认窗口需要你本人操作。'
                    : '正在逐项重新检查并处理，请等待实际结果。'}
              </p>
              {confirmation.requiresAuthorization && (
                <p className="muted">
                  停止后续项目不保证系统授权窗口同步关闭；若窗口仍在，请亲自取消。
                </p>
              )}
              <button
                className="button secondary"
                disabled={cancelling}
                onClick={() => {
                  setCancelling(true)
                  controller.current?.abort()
                }}
              >
                {cancelling ? '正在停止…' : '停止后续项目'}
              </button>
            </>
          ) : (
            <>
              {(expired || consumed) && (
                <p role="status">
                  {consumed
                    ? '本次确认已使用，请核对结果或返回重新复查。'
                    : '确认已过期，请返回复查后再处理。'}
                </p>
              )}
              {!simple && (
                <label className="review-selection">
                  <input
                    type="checkbox"
                    checked={acknowledged}
                    disabled={expired || consumed}
                    onChange={(event) => setAcknowledged(event.target.checked)}
                  />
                  已核对所列项目及处理影响
                </label>
              )}
              <div className="plan-toolbar">
                <button className="button secondary" onClick={onBack}>
                  返回审阅
                </button>
                <button
                  className="button primary"
                  disabled={!acknowledged || expired || consumed}
                  onClick={() => void execute()}
                >
                  {confirmation.requiresAuthorization
                    ? '授权检查并移入废纸篓'
                    : simple
                      ? '确认清理'
                      : '移入废纸篓'}
                </button>
              </div>
            </>
          )}
          {error && (
            <p className="error-text" role="alert">
              {error}
            </p>
          )}
        </>
      )}
    </Modal>
  )
}
