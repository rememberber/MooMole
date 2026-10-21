import {
  maintenanceCatalog,
  maintenanceTargets,
  type MaintenanceReport,
} from '../../../src/lib/maintenance'

export function maintenanceReport(): MaintenanceReport {
  return {
    protocol: 1,
    revision: maintenanceCatalog.revision,
    checkedAt: Date.now(),
    ruleSource: 'current',
    items: (
      Object.entries(maintenanceTargets) as [
        keyof typeof maintenanceTargets,
        readonly (readonly [string, string, string])[],
      ][]
    ).map(([id, targets]) => ({
      id,
      code: id === 'prevent_network_dsstore' ? 'observed' : 'protected',
      taskProtection: id === 'legacy_overrides_audit' ? 'matched' : 'notMatched',
      findings: targets.map(([domain, key, label]) => ({
        domain,
        key,
        label,
        value: id === 'prevent_network_dsstore' ? 'enabled' : 'unknown',
        managed: false,
      })),
    })),
  }
}
