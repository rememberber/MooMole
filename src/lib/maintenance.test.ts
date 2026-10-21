import { describe, expect, it } from 'vitest'
import {
  maintenanceCatalog,
  maintenanceItems,
  maintenanceSourceMatches,
  readMaintenanceReport,
  maintenanceConclusion,
} from './maintenance'
import { maintenanceReport } from '../../tests/e2e/fixtures/maintenance'

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
  it('rejects stale, partial, mismatched and internally inconsistent observations', () => {
    const report = maintenanceReport()
    expect(readMaintenanceReport(report, report.checkedAt, report.checkedAt)).toEqual(report)
    for (const mutate of [
      (r: typeof report) => {
        r.protocol = 3 as 4
      },
      (r: typeof report) => {
        r.revision = 'other'
      },
      (r: typeof report) => {
        r.checkedAt -= 60_000
      },
      (r: typeof report) => {
        r.checkedAt += 60_000
      },
      (r: typeof report) => {
        r.items.pop()
      },
      (r: typeof report) => {
        r.items[1] = r.items[0]
      },
      (r: typeof report) => {
        r.items[0].findings.pop()
      },
      (r: typeof report) => {
        r.items[0].findings[0].key = 'other'
      },
      (r: typeof report) => {
        r.items[0].findings[0].value = 'unknown'
      },
      (r: typeof report) => {
        r.items[0].findings[0].managed = true
      },
      (r: typeof report) => {
        r.ruleSource = 'unknown'
      },
      (r: typeof report) => {
        r.items[0].paths = []
      },
      (r: typeof report) => {
        r.items[0].paths[0].access = 'restricted'
      },
      (r: typeof report) => {
        r.items[0].paths[0].clientProtection = 'matched'
      },
      (r: typeof report) => {
        r.items[0].paths[0].location = '/other.plist'
      },
      (r: typeof report) => {
        r.items[0].paths[0].state = 'missing'
      },
    ]) {
      const changed = structuredClone(report)
      mutate(changed)
      expect(() => readMaintenanceReport(changed, report.checkedAt, report.checkedAt)).toThrow()
    }
  })
  it('never calls observed values executable and retains positive evidence among unknowns', () => {
    const report = maintenanceReport()
    expect(maintenanceConclusion(report.items[0])).toBe('两项设置已开启，无须重复设置')
    expect(maintenanceConclusion(report.items[1])).toBe('已受保护，保持现状')
    const partial = structuredClone(report.items[1])
    partial.code = 'unknown'
    partial.taskProtection = 'notMatched'
    partial.findings[0].value = 'enabled'
    expect(maintenanceConclusion(partial)).toBe('存在启用的覆盖设置，其余状态未确认')
  })
  it('keeps path guards and access restrictions ahead of otherwise observed values', () => {
    const report = maintenanceReport()
    const first = report.items[0]
    first.paths[0].clientProtection = 'matched'
    first.code = 'protected'
    expect(readMaintenanceReport(report, report.checkedAt, report.checkedAt)).toEqual(report)
    expect(maintenanceConclusion(first)).toBe('已受保护，保持现状')
    first.paths[0].clientProtection = 'notMatched'
    first.paths[0].access = 'restricted'
    first.code = 'restricted'
    expect(readMaintenanceReport(report, report.checkedAt, report.checkedAt)).toEqual(report)
    expect(maintenanceConclusion(first)).toBe('发现文件访问限制，保持现状')
    first.paths[0].state = 'missing'
    first.paths[0].access = 'notChecked'
    first.code = 'unknown'
    expect(readMaintenanceReport(report, report.checkedAt, report.checkedAt)).toEqual(report)
    expect(maintenanceConclusion(first)).toBe('部分状态未知，暂不作维护判断')
  })
})

describe('explicit-domain original evidence', () => {
  it('requires both fixed scopes, exact evidence states and type consistency without raw payloads', () => {
    const report = maintenanceReport()
    const evidence = report.items[0].findings[0].originalEvidence
    evidence[0] = { scope: 'anyHost', state: 'observed', kind: 'integer' }
    expect(readMaintenanceReport(report, report.checkedAt, report.checkedAt)).toEqual(report)
    const invalid = [
      [],
      [evidence[0]],
      [evidence[1], evidence[0]],
      [evidence[0], evidence[0]],
      [{ ...evidence[0], kind: null }, evidence[1]],
      [{ ...evidence[0], state: 'missing' }, evidence[1]],
      [{ ...evidence[0], kind: 'float' }, evidence[1]],
      [{ ...evidence[0], rawValue: 'private original' }, evidence[1]],
      [{ ...evidence[0], state: 'unconfirmed' }, evidence[1]],
    ]
    for (const originalEvidence of invalid) {
      const changed = structuredClone(report)
      Object.assign(changed.items[0].findings[0], { originalEvidence })
      expect(() => readMaintenanceReport(changed, report.checkedAt, report.checkedAt)).toThrow()
    }
  })
  it('does not let effective values hide observed changes in an explicit scope', () => {
    const report = maintenanceReport()
    report.items[0].findings[0].originalEvidence[1].state = 'changed'
    expect(() => readMaintenanceReport(report, report.checkedAt, report.checkedAt)).toThrow()
    report.items[0].code = 'unknown'
    expect(readMaintenanceReport(report, report.checkedAt, report.checkedAt)).toEqual(report)
  })
})
