import type { MaintenanceFinding } from '../lib/maintenance'

const stateText = {
  unconfirmed: '未返回原值，缺失与读取不可用仍无法区分',
  unsupported: '原始类型或长度暂不支持',
  changed: '读取期间观察到变化，需重新检查',
  notChecked: '本次未检查',
} as const
const kindText = { boolean: '布尔值', integer: '整数', string: '字符串' } as const

export function MaintenanceOriginalEvidence({
  findings,
}: {
  findings: Pick<MaintenanceFinding, 'key' | 'label' | 'originalEvidence'>[]
}) {
  return (
    <details className="maintenance-originals">
      <summary>原值读取证据</summary>
      <p className="muted">
        仅观察当前用户的明确偏好域；有效设置可能来自继承。原始内容未保存为恢复记录，重复读取也不保证没有并发变化。
      </p>
      {findings.map((finding) => (
        <div className="maintenance-original" key={finding.key}>
          <strong>{finding.label}</strong>
          {finding.originalEvidence.map((evidence) => (
            <p key={evidence.scope}>
              {evidence.scope === 'anyHost' ? '通用主机域' : '本机域'}：
              {evidence.state === 'observed'
                ? `两次读取一致（${kindText[evidence.kind!]}），尚不能据此恢复`
                : stateText[evidence.state]}
              。
            </p>
          ))}
        </div>
      ))}
    </details>
  )
}
