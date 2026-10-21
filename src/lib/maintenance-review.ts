import {
  maintenanceCatalog,
  validOriginalEvidence,
  type MaintenanceObservation,
  type MaintenanceReport,
  type MaintenanceFinding,
} from './maintenance'

export const maintenanceLifetime = 60_000
export type MaintenanceId = MaintenanceObservation['id']
export type ReviewOutcome =
  'preserveProtected' | 'preserveRestricted' | 'preserveUnknown' | 'unchanged' | 'needsEvidence'
export interface MaintenanceReviewKey {
  key: string
  label: string
  current: MaintenanceFinding['value']
  originalEvidence: MaintenanceFinding['originalEvidence']
  intent: 'enableSetting' | 'removeOverride'
  outcome: ReviewOutcome
}
export interface MaintenanceReview {
  protocol: 2
  revision: string
  observationId: string
  checkedAt: number
  preparedAt: number
  expiresAt: number
  mode: 'reviewOnly'
  canExecute: false
  originalsCaptured: false
  concurrencyResolved: false
  items: { id: MaintenanceId; keys: MaintenanceReviewKey[] }[]
}
export function maintenanceReviewOutcome(
  item: MaintenanceObservation,
  finding: MaintenanceFinding,
): ReviewOutcome {
  if (item.code === 'protected') return 'preserveProtected'
  if (item.code === 'restricted') return 'preserveRestricted'
  if (
    finding.value === 'unknown' ||
    finding.originalEvidence.some((e) => e.state === 'changed') ||
    item.taskProtection === 'unknown' ||
    item.paths.some(
      (path) =>
        path.state !== 'present' ||
        path.access !== 'noRestrictionObserved' ||
        path.clientProtection === 'unknown' ||
        path.moleProtection === 'unknown',
    )
  )
    return 'preserveUnknown'
  if (
    (item.id === 'prevent_network_dsstore' && finding.value === 'enabled') ||
    (item.id === 'legacy_overrides_audit' && finding.value === 'disabled')
  )
    return 'unchanged'
  return 'needsEvidence'
}
export const maintenanceOutcomeText: Record<ReviewOutcome, string> = {
  preserveProtected: '发现保护，保持现状',
  preserveRestricted: '存在文件访问限制，保持现状',
  preserveUnknown: '条件未知，保持现状',
  unchanged: '无需变更',
  needsEvidence: '缺少原值与恢复证据，暂不能变更',
}

export function readMaintenanceReview(
  value: unknown,
  report: MaintenanceReport,
  selected: MaintenanceId[],
  began: number,
  now = Date.now(),
): MaintenanceReview {
  const review = value as MaintenanceReview | null
  const invalid = () => {
    throw new Error('维护审阅结果无法确认，请重新检查后审阅。')
  }
  if (
    !review ||
    review.protocol !== 2 ||
    review.revision !== maintenanceCatalog.revision ||
    review.mode !== 'reviewOnly' ||
    review.canExecute !== false ||
    review.originalsCaptured !== false ||
    review.concurrencyResolved !== false ||
    review.observationId !== report.observationId ||
    review.checkedAt !== report.checkedAt ||
    !Number.isSafeInteger(review.preparedAt) ||
    review.preparedAt < began - 5_000 ||
    review.preparedAt > now + 5_000 ||
    review.preparedAt < report.checkedAt ||
    now < report.checkedAt ||
    review.expiresAt !== report.checkedAt + maintenanceLifetime ||
    now >= review.expiresAt ||
    !Array.isArray(review.items) ||
    review.items.length !== selected.length ||
    selected.length < 1 ||
    selected.length > 2 ||
    new Set(selected).size !== selected.length
  )
    return invalid()
  for (const [index, item] of review.items.entries()) {
    const source = report.items.find((source) => source.id === selected[index])
    if (
      !item ||
      !source ||
      item.id !== source.id ||
      !Array.isArray(item.keys) ||
      item.keys.length !== source.findings.length
    )
      return invalid()
    for (const [keyIndex, key] of item.keys.entries()) {
      const finding = source.findings[keyIndex]
      if (
        !key ||
        key.key !== finding.key ||
        key.label !== finding.label ||
        key.current !== finding.value ||
        !validOriginalEvidence(key.originalEvidence) ||
        key.originalEvidence.some(
          (entry, i) =>
            entry.scope !== finding.originalEvidence[i].scope ||
            entry.state !== finding.originalEvidence[i].state ||
            entry.kind !== finding.originalEvidence[i].kind,
        ) ||
        key.intent !==
          (item.id === 'prevent_network_dsstore' ? 'enableSetting' : 'removeOverride') ||
        key.outcome !== maintenanceReviewOutcome(source, finding)
      )
        return invalid()
    }
  }
  return review
}
