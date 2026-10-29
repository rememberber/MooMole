import { useEffect, useRef, useState } from 'react'
import { RefreshCw, ShieldCheck } from 'lucide-react'
import type { CleanupConfirmation, ExecutionDetail, ExecutionSummary, ReviewPlan } from '../types'
import { dateTime } from '../lib/format'
import { Modal } from './Modal'
import { useNow } from '../lib/use-now'
import { hasPermissionProblem } from './PermissionEvidence'
import { ReviewPlanItem } from './ReviewPlanItem'
import { cleanupPreparation } from '../lib/cleanup-preparation'
import { cleanupConfirmation } from '../lib/cleanup-confirmation'
import type { RevealExecutionItem } from './ExecutionLocation'
import { LogCleanupDialog } from './LogCleanupDialog'

export function ReviewPlanDialog({
  initialPlan,
  autoCheck = false,
  onRefresh,
  onClose,
  onCheckRules,
  onExecute,
  onDiscardConfirmation,
  onRevealReceipt,
  executions = [],
}: {
  initialPlan: ReviewPlan
  autoCheck?: boolean
  onRefresh: (id: string) => Promise<ReviewPlan>
  onClose: () => void
  onCheckRules?: (id: string, signal: AbortSignal) => Promise<ReviewPlan>
  onExecute?: (id: string, signal: AbortSignal) => Promise<ExecutionDetail>
  onDiscardConfirmation?: (id: string) => Promise<void>
  onRevealReceipt?: RevealExecutionItem
  executions?: ExecutionSummary[]
}) {
  const [plan, setPlan] = useState(initialPlan)
  const [confirming, setConfirming] = useState<CleanupConfirmation | null>(null)
  // Tick expiry while idle, but compare new responses to the actual render time
  // rather than the previous tick (which can precede a fresh native response).
  useNow()
  const now = Date.now()
  const expired = now >= plan.expiresAt || now < plan.createdAt
  const [pending, setPending] = useState<'local' | 'rules' | null>(null)
  const [cancelling, setCancelling] = useState(false)
  const controller = useRef<AbortController | null>(null)
  useEffect(() => () => controller.current?.abort(), [])
  const [error, setError] = useState('')
  const [onlyProblems, setOnlyProblems] = useState(false)
  const [limit, setLimit] = useState(100)
  const preparation = cleanupPreparation(plan, now)
  const offer = cleanupConfirmation(plan, now)
  const effectiveItems = expired
    ? plan.items.map((item) => ({
        ...item,
        state: 'expired',
        moleRule: null,
        descendantRules: null,
        permissions: null,
        reason: '计划已过期或系统时间发生回退，请重新扫描。',
      }))
    : plan.items
  const needsReview = (item: ReviewPlan['items'][number]) =>
    item.state !== 'unchanged' ||
    hasPermissionProblem(item.permissions) ||
    (preparation != null &&
      (!preparation.valid || preparation.items.get(item.id)?.code !== 'observed'))
  const problems = effectiveItems.filter(needsReview).length
  const items = effectiveItems.filter((item) => !onlyProblems || needsReview(item))
  async function refresh() {
    setPending('local')
    setError('')
    setPlan((current) => ({
      ...current,
      ruleCheckedAt: null,
      cleanupPreparation: null,
      cleanupConfirmation: null,
      items: current.items.map((item) => ({
        ...item,
        moleRule: null,
        descendantRules: null,
        permissions: null,
        state: 'pendingLocal',
        reason: '正在重新读取本地身份与保护，之前的规则结果不再适用。',
      })),
    }))
    try {
      setPlan(await onRefresh(plan.id))
    } catch (error) {
      setError(String(error).replace(/^Error: /, ''))
      setPlan((current) => ({
        ...current,
        cleanupPreparation: null,
        cleanupConfirmation: null,
        items: current.items.map((item) => ({
          ...item,
          moleRule: null,
          descendantRules: null,
          permissions: null,
          state: 'unavailable',
          reason: '本次本地复核未完成，请重新检查；之前的规则结果不再适用。',
        })),
      }))
    } finally {
      setPending(null)
    }
  }
  async function checkRules() {
    if (!onCheckRules) return
    const abort = new AbortController()
    controller.current = abort
    setPending('rules')
    setError('')
    setCancelling(false)
    setPlan((current) => ({
      ...current,
      ruleCheckedAt: null,
      cleanupPreparation: null,
      cleanupConfirmation: null,
      items: current.items.map((item) => ({
        ...item,
        moleRule: null,
        descendantRules: null,
        permissions: null,
        state: 'pendingRules',
        reason: 'Mole 规则复查进行中，结果尚未确认。',
      })),
    }))
    try {
      const checked = await onCheckRules(plan.id, abort.signal)
      setPlan(checked)
      if (autoCheck && !abort.signal.aborted) {
        const confirmation = cleanupConfirmation(checked, Date.now())
        if (confirmation) setConfirming(confirmation)
      }
    } catch (error) {
      setError(String(error).replace(/^Error: /, ''))
      setPlan((current) => ({
        ...current,
        cleanupPreparation: null,
        cleanupConfirmation: null,
        items: current.items.map((item) => ({
          ...item,
          state: 'ruleUnavailable',
          reason: '本次规则复查未完成，请重试或重新扫描。',
        })),
      }))
    } finally {
      controller.current = null
      setPending(null)
      setCancelling(false)
    }
  }
  const autoStarted = useRef(false)
  useEffect(() => {
    if (autoCheck && onCheckRules && !autoStarted.current) {
      autoStarted.current = true
      void checkRules()
    }
  }, [])
  if (confirming && onExecute)
    return (
      <LogCleanupDialog
        confirmation={confirming}
        execution={executions.find(
          (execution) =>
            execution.planId === confirming.planId && execution.createdAt >= confirming.createdAt,
        )}
        simple={autoCheck}
        onExecute={onExecute}
        onDiscard={onDiscardConfirmation}
        onRevealReceipt={onRevealReceipt}
        onClose={onClose}
        onBack={() => {
          setConfirming(null)
          setPlan((current) => ({ ...current, cleanupConfirmation: null }))
        }}
      />
    )
  return (
    <Modal
      title="审阅计划"
      onClose={() => {
        controller.current?.abort()
        if (plan.cleanupConfirmation)
          void onDiscardConfirmation?.(plan.cleanupConfirmation.id).catch(() => {})
        onClose()
      }}
      className="review-plan-modal"
    >
      <h2>审阅计划</h2>
      <p className="muted">已保存所选范围，可逐项查看检查结果。</p>
      <div className="notice">
        <ShieldCheck size={18} />
        <span>
          {onExecute
            ? '复核不会清理文件。仅满足条件的用户缓存和日志可在单独确认后移入废纸篓。'
            : '当前未开放执行，复核不会清理文件。检查结果不代表已获准清理。'}
        </span>
      </div>
      <div className="plan-summary motion-scan" data-scanning={pending != null && !cancelling}>
        <strong>
          {preparation?.summary ??
            (problems ? `${problems} 项需要重新确认` : '本次文件信息未见变化')}
        </strong>
        <span>{plan.itemCount} 个项目</span>
      </div>
      <p className="plan-next" role="status">
        <span className="motion-text" data-active={pending != null && !cancelling && !expired}>
          {expired
            ? '此计划已失效，之前的复查结果不再适用。'
            : pending
              ? '正在检查所选项目，请等待结果。'
              : preparation
                ? onExecute && offer
                  ? '下一步：核对项目用途与影响，再查看后端固定的移入范围。未满足条件的项目保留。'
                  : preparation.next
                : problems
                  ? '下一步：先查看需确认的项目，展开检查详情了解原因。'
                  : !plan.ruleCheckedAt
                    ? '下一步：复查 Mole 规则，再逐项了解用途与影响。'
                    : '下一步：查看逐项检查结果与可能影响。'}
        </span>
      </p>
      {onExecute &&
        preparation?.valid &&
        plan.cleanupPreparation!.observedCount + plan.cleanupPreparation!.authorizationCount >
          0 && (
          <button
            className="button primary compact"
            disabled={!offer || pending != null}
            onClick={() => offer && setConfirming(offer)}
          >
            {offer
              ? offer.requiresAuthorization
                ? '查看授权后移入范围'
                : '查看移入废纸篓范围'
              : '重新复查后生成确认范围'}
          </button>
        )}
      <details className="plan-info">
        <summary>计划信息与检查范围</summary>
        <p>
          保存于 {dateTime(plan.createdAt)} · 到期 {dateTime(plan.expiresAt)}
          <br />
          复核于 {dateTime(plan.checkedAt)} · 计划 {plan.id.slice(0, 8)}
          <br />
          {expired
            ? '计划已失效，请重新扫描。'
            : plan.ruleCheckedAt
              ? `Mole 规则复查于 ${dateTime(plan.ruleCheckedAt)}`
              : 'Mole 规则尚未完成本次复查'}
        </p>
        <p>
          复核读取文件元数据、保护规则和占用状态。Mole 复查包含路径保护、已适配的用户缓存 Deno
          约束和用户日志目录约束，并补充占用观察与有界的目录后代路径保护检查。其他模块条件仍需单独核对。
        </p>
        {plan.cleanupPreparation && (
          <p>
            本轮准备有效至 {dateTime(plan.cleanupPreparation.expiresAt)}，最长 30 秒。
            日志准备仅限来源明确的本账户、本地普通文件，每轮最多准备 20 项。
            拟采用移入废纸篓，不等于释放磁盘空间；当前不提供应用内恢复。
            原生对象引用在本次检查结束后释放，实际执行前必须重新检查。
          </p>
        )}
      </details>
      <div className="plan-toolbar">
        <label>
          <input
            type="checkbox"
            checked={onlyProblems}
            onChange={(e) => {
              setOnlyProblems(e.target.checked)
              setLimit(100)
            }}
          />
          只看需要确认的项目
        </label>
        <button
          className="button secondary compact"
          disabled={pending != null}
          onClick={() => void refresh()}
        >
          <RefreshCw size={13} />
          {pending === 'local' ? '正在复核…' : '重新复核'}
        </button>
        <button
          className="button primary compact"
          disabled={pending != null || !onCheckRules || expired}
          onClick={() => void checkRules()}
        >
          {pending === 'rules' ? '正在复查 Mole…' : '复查 Mole 规则'}
        </button>
        {pending === 'rules' && (
          <button
            className="button text"
            disabled={cancelling}
            onClick={() => {
              setCancelling(true)
              controller.current?.abort()
            }}
          >
            {cancelling ? '正在取消…' : '取消复查'}
          </button>
        )}
      </div>
      {error && (
        <p className="error-text" role="alert">
          {error}
        </p>
      )}
      <div className="plan-items" aria-label="计划复核结果" aria-busy={pending != null}>
        {items.slice(0, limit).map((item) => (
          <ReviewPlanItem item={item} key={item.id} preparation={preparation?.items.get(item.id)} />
        ))}
        {!items.length && <div className="empty-inline">本次没有需要重新确认的项目。</div>}
        {items.length > limit && (
          <button className="button secondary" onClick={() => setLimit((current) => current + 100)}>
            再显示 100 项（剩余 {items.length - limit} 项）
          </button>
        )}
      </div>
    </Modal>
  )
}
