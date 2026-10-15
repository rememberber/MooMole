import type { Impact } from '../types'

export const unknownImpact: Impact = {
  version: 1,
  kind: 'unknown',
  title: '用途尚未确认',
  purpose: '目前没有与该位置对应的明确用途说明，不能仅凭名称或大小判断。',
  consequence: '对应用、项目或个人数据的影响尚不明确，建议先在 Finder 中核对。',
  recovery: '尚未确认可恢复性，应保留无法确认用途的重要内容。',
  requiresReview: true,
}

export function ImpactDetails({ impact }: { impact?: Impact | null }) {
  const info = impact ?? unknownImpact
  return (
    <div className="impact-details" aria-label="用途与影响说明">
      <strong>{info.title}</strong>
      <dl>
        <dt>通常用途</dt>
        <dd>{info.purpose}</dd>
        <dt>可能影响</dt>
        <dd>{info.consequence}</dd>
        <dt>可恢复性</dt>
        <dd>{info.recovery}</dd>
      </dl>
      <p>客户端按目录范围整理的说明，未检查文件正文，不代表该项目可安全删除。</p>
    </div>
  )
}
