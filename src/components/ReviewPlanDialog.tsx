import { useEffect, useRef, useState } from 'react'
import { RefreshCw, ShieldCheck } from 'lucide-react'
import type { ReviewPlan } from '../types'
import { dateTime } from '../lib/format'
import { Modal } from './Modal'
import { useNow } from '../lib/use-now'
import { hasPermissionProblem } from './PermissionEvidence'
import { ReviewPlanItem } from './ReviewPlanItem'

export function ReviewPlanDialog({
  initialPlan,
  onRefresh,
  onClose,
  onCheckRules,
}: {
  initialPlan: ReviewPlan
  onRefresh: (id: string) => Promise<ReviewPlan>
  onClose: () => void
  onCheckRules?: (id: string, signal: AbortSignal) => Promise<ReviewPlan>
}) {
  const [plan, setPlan] = useState(initialPlan)
  const now = useNow()
  const expired = now >= plan.expiresAt || now < plan.createdAt
  const [pending, setPending] = useState<'local' | 'rules' | null>(null)
  const [cancelling, setCancelling] = useState(false)
  const controller = useRef<AbortController | null>(null)
  useEffect(() => () => controller.current?.abort(), [])
  const [error, setError] = useState('')
  const [onlyProblems, setOnlyProblems] = useState(false)
  const [limit, setLimit] = useState(100)
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
    item.state !== 'unchanged' || hasPermissionProblem(item.permissions)
  const problems = effectiveItems.filter(needsReview).length
  const items = effectiveItems.filter((item) => !onlyProblems || needsReview(item))
  async function refresh() {
    setPending('local')
    setError('')
    setPlan((current) => ({
      ...current,
      ruleCheckedAt: null,
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
      setPlan(await onCheckRules(plan.id, abort.signal))
    } catch (error) {
      setError(String(error).replace(/^Error: /, ''))
      setPlan((current) => ({
        ...current,
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
  return (
    <Modal
      title="审阅计划"
      onClose={() => {
        controller.current?.abort()
        onClose()
      }}
      className="review-plan-modal"
    >
      <h2>审阅计划</h2>
      <p className="muted">已保存所选范围，可逐项查看检查结果。</p>
      <div className="notice">
        <ShieldCheck size={18} />
        <span>当前未开放执行，复核不会清理文件。检查结果不代表已获准清理。</span>
      </div>
      <div className="plan-summary">
        <strong>{problems ? `${problems} 项需要重新确认` : '本次文件信息未见变化'}</strong>
        <span>{plan.itemCount} 个项目</span>
      </div>
      <p className="plan-next" role="status">
        {expired
          ? '此计划已失效，之前的复查结果不再适用。'
          : pending
            ? '正在检查所选项目，请等待结果。'
            : problems
              ? '下一步：先查看需确认的项目，展开检查详情了解原因。'
              : !plan.ruleCheckedAt
                ? '下一步：复查 Mole 规则，再逐项了解用途与影响。'
                : '下一步：查看逐项检查结果与可能影响。'}
      </p>
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
          <ReviewPlanItem item={item} key={item.id} />
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
