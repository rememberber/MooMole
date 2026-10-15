import { useState } from 'react'
import { RefreshCw, ShieldCheck } from 'lucide-react'
import type { ReviewPlan } from '../types'
import { bytes, dateTime } from '../lib/format'
import { Modal } from './Modal'

const states: Record<string, string> = {
  unchanged: '元数据未发现变化',
  changed: '目标已变化',
  protected: '当前已保护',
  unavailable: '无法确认身份',
  expired: '计划已过期',
  versionMismatch: '版本已变化',
}

export function ReviewPlanDialog({
  initialPlan,
  onRefresh,
  onClose,
}: {
  initialPlan: ReviewPlan
  onRefresh: (id: string) => Promise<ReviewPlan>
  onClose: () => void
}) {
  const [plan, setPlan] = useState(initialPlan)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState('')
  const [onlyProblems, setOnlyProblems] = useState(false)
  const [limit, setLimit] = useState(100)
  const problems = plan.items.filter((item) => item.state !== 'unchanged').length
  const items = plan.items.filter((item) => !onlyProblems || item.state !== 'unchanged')
  async function refresh() {
    setPending(true)
    setError('')
    try {
      setPlan(await onRefresh(plan.id))
    } catch (error) {
      setError(String(error).replace(/^Error: /, ''))
    } finally {
      setPending(false)
    }
  }
  return (
    <Modal title="审阅计划" onClose={pending ? undefined : onClose} className="review-plan-modal">
      <h2>审阅计划</h2>
      <p className="muted">选中范围已保存在本机。复核只读取文件元数据，不执行清理。</p>
      <div className="notice">
        <ShieldCheck size={18} />
        <span>
          尚需复查 Mole
          规则、活动应用和每项操作影响。元数据未变化不代表可以安全删除，当前未开放执行。
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
          disabled={pending}
          onClick={() => void refresh()}
        >
          <RefreshCw size={13} />
          {pending ? '正在复核…' : '重新复核'}
        </button>
      </div>
      {error && (
        <p className="error-text" role="alert">
          {error}
        </p>
      )}
      <div className="plan-items" aria-label="计划复核结果" aria-busy={pending}>
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
