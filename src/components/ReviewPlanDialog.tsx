import { useEffect, useRef, useState } from 'react'
import { RefreshCw, ShieldCheck } from 'lucide-react'
import type { ReviewPlan } from '../types'
import { bytes, dateTime } from '../lib/format'
import { Modal } from './Modal'
import { ImpactDetails, unknownImpact } from './ImpactDetails'
import { useNow } from '../lib/use-now'

const states: Record<string, string> = {
  unchanged: '元数据未发现变化',
  changed: '目标已变化',
  protected: '当前已保护',
  unavailable: '无法确认身份',
  expired: '计划已过期',
  versionMismatch: '版本已变化',
  moleProtected: 'Mole 规则保护',
  active: '使用中或无法确认空闲',
  ruleUnavailable: '规则复查未完成',
  pendingRules: '规则复查中',
  contentsChanged: '目录内容已变化',
  inventoryUnavailable: '目录内容无法确认',
  inventoryMissing: '需要重新生成目录计划',
}

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
        reason: '计划已过期或系统时间发生回退，请重新扫描。',
      }))
    : plan.items
  const problems = effectiveItems.filter((item) => item.state !== 'unchanged').length
  const items = effectiveItems.filter((item) => !onlyProblems || item.state !== 'unchanged')
  async function refresh() {
    setPending('local')
    setError('')
    try {
      setPlan(await onRefresh(plan.id))
    } catch (error) {
      setError(String(error).replace(/^Error: /, ''))
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
      <p className="muted">
        选中范围已保存在本机。复核读取文件元数据、保护规则和占用状态，不执行清理。
      </p>
      <div className="notice">
        <ShieldCheck size={18} />
        <span>
          Mole
          复查覆盖当前路径级保护，尚不包含完整清理审计与每项操作影响。实际操作前仍需再次复核，当前未开放执行。
        </span>
      </div>
      <div className="plan-summary">
        <span>{plan.itemCount} 个项目</span>
        <span>{problems ? `${problems} 项需要重新确认` : '本次元数据复核未发现变化'}</span>
      </div>
      <p className="fine-print">
        保存于 {dateTime(plan.createdAt)} · 到期 {dateTime(plan.expiresAt)}
        <br />
        复核于 {dateTime(plan.checkedAt)} · 计划 {plan.id.slice(0, 8)}
        <br />
        {expired
          ? '此计划已失效，之前的复查结果不再适用。'
          : plan.ruleCheckedAt
            ? `Mole 路径规则复查于 ${dateTime(plan.ruleCheckedAt)}`
            : 'Mole 路径规则尚未完成本次复查'}
      </p>
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
          className="button secondary compact"
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
          <article className="plan-item" key={item.id}>
            <div>
              <span className={`tag ${item.state === 'unchanged' ? '' : 'plan-problem'}`}>
                {states[item.state] ?? '需要重新确认'}
              </span>
              <strong>{bytes(item.size)}</strong>
            </div>
            <p className="mono full-path">{item.path}</p>
            <p>{item.reason}</p>
            {item.inventoryEntries != null && (
              <p>
                已记录目录内 {item.inventoryEntries}{' '}
                个子项的元数据，用于复核深层内容变化；不读取文件正文。
              </p>
            )}
            <details className="impact-fold">
              <summary>用途与影响 · {(item.impact ?? unknownImpact).title}</summary>
              <ImpactDetails impact={item.impact} />
            </details>
            {item.moleRule && (
              <p className="rule-evidence">
                {item.moleRule.source}：{item.moleRule.reason}
              </p>
            )}
          </article>
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
