import catalog from './maintenance-catalog.json' with { type: 'json' }

// Source-bound reference information, never a machine assessment or execution plan.
export const maintenanceCatalog = catalog
export type MaintenanceItem = (typeof catalog.items)[number]

export function maintenanceItems(query: string): MaintenanceItem[] {
  const needle = query.trim().toLocaleLowerCase()
  return catalog.items.filter((item) =>
    Object.values(item).some((value) => value.toLocaleLowerCase().includes(needle)),
  )
}

export function maintenanceSourceMatches(revision: string, version: string | null): boolean {
  return revision === catalog.revision && version === catalog.version
}

export const maintenanceTargets = {
  prevent_network_dsstore: [
    ['com.apple.desktopservices', 'DSDontWriteNetworkStores', '共享目录不写入 Finder 记录'],
    ['com.apple.desktopservices', 'DSDontWriteUSBStores', 'USB 卷不写入 Finder 记录'],
  ],
  legacy_overrides_audit: [
    ['-g', 'NSAppSleepDisabled', '全局禁用 App Nap'],
    ['com.apple.frameworks.diskimages', 'skip-verify', '磁盘映像跳过验证'],
    ['com.apple.frameworks.diskimages', 'skip-verify-locked', '锁定映像跳过验证'],
    ['com.apple.frameworks.diskimages', 'skip-verify-remote', '远程映像跳过验证'],
  ],
} as const
export type MaintenanceValue = 'enabled' | 'disabled' | 'unknown'
export interface MaintenanceFinding {
  domain: string
  key: string
  label: string
  value: MaintenanceValue
  managed: boolean
}
export interface MaintenanceObservation {
  id: keyof typeof maintenanceTargets
  code: 'observed' | 'protected' | 'unknown'
  taskProtection: 'matched' | 'notMatched' | 'unknown'
  findings: MaintenanceFinding[]
}
export interface MaintenanceReport {
  protocol: 1
  revision: string
  checkedAt: number
  ruleSource: 'current' | 'legacy' | 'defaults' | 'unknown'
  items: MaintenanceObservation[]
}

export function readMaintenanceReport(
  value: unknown,
  began: number,
  now = Date.now(),
): MaintenanceReport {
  const report = value as MaintenanceReport | null
  const invalid = () => {
    throw new Error('维护检查结果无法确认，请重新检查。')
  }
  if (
    !report ||
    report.protocol !== 1 ||
    report.revision !== catalog.revision ||
    !Number.isSafeInteger(report.checkedAt) ||
    report.checkedAt < began - 5_000 ||
    report.checkedAt > now + 5_000 ||
    !['current', 'legacy', 'defaults', 'unknown'].includes(report.ruleSource) ||
    !Array.isArray(report.items) ||
    report.items.length !== 2
  )
    return invalid()
  const ids = new Set<string>()
  for (const item of report.items) {
    if (
      !item ||
      !Object.hasOwn(maintenanceTargets, item.id) ||
      ids.has(item.id) ||
      !['observed', 'protected', 'unknown'].includes(item.code) ||
      !['matched', 'notMatched', 'unknown'].includes(item.taskProtection)
    )
      return invalid()
    ids.add(item.id)
    const targets = maintenanceTargets[item.id]
    if (!Array.isArray(item.findings) || item.findings.length !== targets.length) return invalid()
    for (const [index, finding] of item.findings.entries()) {
      const target = targets[index]
      if (
        !finding ||
        finding.domain !== target[0] ||
        finding.key !== target[1] ||
        finding.label !== target[2] ||
        !['enabled', 'disabled', 'unknown'].includes(finding.value) ||
        typeof finding.managed !== 'boolean'
      )
        return invalid()
    }
    if ((report.ruleSource === 'unknown') !== (item.taskProtection === 'unknown')) return invalid()
    const code =
      item.taskProtection === 'matched' || item.findings.some((f) => f.managed)
        ? 'protected'
        : item.taskProtection === 'unknown' || item.findings.some((f) => f.value === 'unknown')
          ? 'unknown'
          : 'observed'
    if (item.code !== code) return invalid()
  }
  return report
}

export function maintenanceConclusion(item: MaintenanceObservation): string {
  if (item.code === 'protected') return '已受保护，保持现状'
  if (item.code === 'unknown')
    return item.id === 'legacy_overrides_audit' && item.findings.some((f) => f.value === 'enabled')
      ? '存在启用的覆盖设置，其余状态未确认'
      : '部分状态未知，暂不作维护判断'
  if (item.id === 'prevent_network_dsstore')
    return item.findings.every((f) => f.value === 'enabled')
      ? '两项设置已开启，无须重复设置'
      : '部分设置未开启，可先了解影响'
  return item.findings.some((f) => f.value === 'enabled')
    ? '存在启用的覆盖设置，建议审阅'
    : '这四项覆盖设置均未启用'
}
