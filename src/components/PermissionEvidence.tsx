import type { PermissionObservation } from '../types'

const names: Record<string, string> = {
  volume: '卷状态',
  parentAccess: '父目录访问',
  fileFlags: '锁定属性',
  stickyDirectory: '目录所有权保护',
  extendedAcl: '扩展访问控制',
  mountBoundary: '挂载边界',
}
const labels: Record<string, string> = {
  noRestrictionObserved: '未发现已检查范围中的限制',
  restricted: '发现文件系统访问限制',
  unknown: '部分权限无法确认',
  notChecked: '本次未检查权限',
}

function current(value: PermissionObservation | null | undefined): value is PermissionObservation {
  if (
    !value ||
    value.version !== 1 ||
    !Object.hasOwn(labels, value.code) ||
    !Array.isArray(value.findings)
  )
    return false
  if (
    typeof value.reason !== 'string' ||
    value.findings.some(
      (f) =>
        !f ||
        !Object.hasOwn(names, f.kind) ||
        !['clear', 'restricted', 'unknown'].includes(f.state) ||
        typeof f.reason !== 'string',
    )
  )
    return false
  if (value.code === 'noRestrictionObserved')
    return (
      value.findings.length === 6 &&
      new Set(value.findings.map((f) => f.kind)).size === 6 &&
      value.findings.every((f) => f.state === 'clear')
    )
  if (value.code === 'restricted') return value.findings.some((f) => f.state === 'restricted')
  return value.code !== 'notChecked' || value.findings.length === 0
}

export const hasPermissionProblem = (value: PermissionObservation | null | undefined) =>
  !current(value) || value.code !== 'noRestrictionObserved'

export function PermissionEvidence({ value }: { value: PermissionObservation | null | undefined }) {
  const known = current(value)
  return (
    <section className="module-evidence permission-evidence" aria-label="本机权限观察">
      <strong>本机权限观察 · {known ? labels[value.code] : '权限尚未确认'}</strong>
      <p>
        {known ? value.reason : '这份结果没有可识别的权限观察，请重新复核；不能视为已获准清理。'}
      </p>
      <details>
        <summary>观察项目与边界</summary>
        <p>
          仅观察所选目标及直接父目录，不读取正文或试写文件。未检查目录后代、废纸篓目标、完整隐私授权或实际移动结果；不会自动修改权限或申请管理员权限。
        </p>
        {known &&
          value.findings.map((finding, index) => (
            <div className="permission-finding" key={`${finding.kind}:${index}`}>
              <strong>
                {names[finding.kind]} ·{' '}
                {finding.state === 'clear'
                  ? '本次未发现限制'
                  : finding.state === 'restricted'
                    ? '受限'
                    : '无法确认'}
              </strong>
              <p>{finding.reason}</p>
              {finding.osError != null && <small>系统诊断代码：{finding.osError}</small>}
            </div>
          ))}
      </details>
    </section>
  )
}
