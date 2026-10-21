import type { MaintenanceReport } from '../../../src/lib/maintenance'
import {
  maintenanceReviewOutcome,
  maintenanceLifetime,
  type MaintenanceId,
  type MaintenanceReview,
} from '../../../src/lib/maintenance-review'
export const reviewSelection: MaintenanceId[] = [
  'prevent_network_dsstore',
  'legacy_overrides_audit',
]
export function maintenanceReview(
  report: MaintenanceReport,
  selected: MaintenanceId[],
  now = report.checkedAt,
): MaintenanceReview {
  return {
    protocol: 2,
    revision: report.revision,
    observationId: report.observationId,
    checkedAt: report.checkedAt,
    preparedAt: now,
    expiresAt: report.checkedAt + maintenanceLifetime,
    mode: 'reviewOnly',
    canExecute: false,
    originalsCaptured: false,
    concurrencyResolved: false,
    items: selected.map((id) => {
      const item = report.items.find((item) => item.id === id)!
      return {
        id,
        keys: item.findings.map((finding) => ({
          key: finding.key,
          label: finding.label,
          current: finding.value,
          originalEvidence: structuredClone(finding.originalEvidence),
          intent: id === 'prevent_network_dsstore' ? 'enableSetting' : 'removeOverride',
          outcome: maintenanceReviewOutcome(item, finding),
        })),
      }
    }),
  }
}
