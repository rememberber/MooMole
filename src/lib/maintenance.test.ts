import { describe, expect, it } from 'vitest'
import { maintenanceCatalog, maintenanceItems, maintenanceSourceMatches } from './maintenance'

describe('source-bound maintenance reference', () => {
  it('searches literal IDs, effects and permissions, retaining source order', () => {
    expect(maintenanceItems(' dns ').map((item) => item.id)).toEqual([
      'network_optimization',
      'network_stack_optimize',
    ])
    expect(maintenanceItems('VPN').map((item) => item.id)).toEqual([
      'network_optimization',
      'network_stack_optimize',
    ])
    expect(maintenanceItems('[')).toEqual([])
    expect(maintenanceItems('sqlite_vacuum')[0].name).toBe('邮件、Safari 与信息数据库')
    expect(maintenanceItems('')).toEqual(maintenanceCatalog.items)
  })
  it('does not equate unknown or changed engine identity with the reference baseline', () => {
    expect(maintenanceSourceMatches(maintenanceCatalog.revision, maintenanceCatalog.version)).toBe(
      true,
    )
    expect(maintenanceSourceMatches(maintenanceCatalog.revision, null)).toBe(false)
    expect(maintenanceSourceMatches('other', maintenanceCatalog.version)).toBe(false)
    expect(maintenanceSourceMatches(maintenanceCatalog.revision, '2.0.0')).toBe(false)
  })
})
