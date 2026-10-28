import { describe, expect, it } from 'vitest'
import type { ReviewPlan } from '../types'
import { cleanupPreparation } from './cleanup-preparation'

function fixture(): ReviewPlan {
  const items = ['log', 'cache'].map((id) => ({
    id,
    path: `/fixture/${id}`,
    section: 'fixture',
    size: 10,
    state: 'unchanged',
    reason: 'fixture',
    permissions: { version: 1, code: 'noRestrictionObserved', reason: 'fixture', findings: [] },
    moleRule: {
      code: 'passed',
      source: 'fixture',
      reason: 'fixture',
      module: {
        version: 2,
        binding: 'userLogs',
        scope: 'userLogs',
        code: 'checked',
        source: 'fixture',
        reason: 'fixture',
      },
      activity: { version: 1, code: 'noOpenHandles', source: 'fixture', reason: 'fixture' },
    },
  }))
  return {
    id: 'plan',
    scanId: 'scan',
    createdAt: 100,
    checkedAt: 1000,
    expiresAt: 900_100,
    itemCount: 2,
    readOnly: true,
    requiresMoleRecheck: true,
    ruleCheckedAt: 1000,
    items,
    cleanupPreparation: {
      version: 2,
      planId: 'plan',
      scope: 'userLogFiles',
      method: 'trash',
      readOnly: true,
      checkedAt: 1000,
      expiresAt: 31_000,
      observedCount: 1,
      authorizationCount: 0,
      items: [
        { id: 'log', code: 'observed', reason: '本轮日志准备已核对。' },
        { id: 'cache', code: 'retained', reason: '缓存不在单文件日志范围，保留。' },
      ],
    },
  }
}

describe('single-log preparation evidence', () => {
  it('does not count authorization-required logs as observed or retained', () => {
    const plan = fixture()
    plan.items[0].state = 'activityUnknown'
    plan.items[0].moleRule!.activity!.code = 'unknown'
    plan.cleanupPreparation!.items[0].code = 'authorizationRequired'
    plan.cleanupPreparation!.observedCount = 0
    plan.cleanupPreparation!.authorizationCount = 1
    const result = cleanupPreparation(plan, 1001)!
    expect(result.valid).toBe(true)
    expect(result.summary).toContain('1 项仍需授权检查 · 1 项保留')
    expect(result.summary).not.toContain('完成本轮准备检查')
    expect(result.next).toContain('本人进行当次系统授权')
    for (const count of [-1, 0, 2, NaN, 1.5]) {
      plan.cleanupPreparation!.authorizationCount = count
      expect(cleanupPreparation(plan, 1001)?.valid).toBe(false)
    }
  })
  it('joins only the current plan, separates retained scope, and grants no action', () => {
    const plan = fixture()
    const result = cleanupPreparation(plan, 1001)!
    expect(result.valid).toBe(true)
    expect(result.summary).toContain('1 项完成本轮准备检查 · 1 项保留')
    expect(result.items.get('cache')?.code).toBe('retained')
    expect(result.next).toContain('不代表清理授权')
    plan.cleanupPreparation = null
    expect(cleanupPreparation(plan, 1001)).toBeNull()
  })
  it('withdraws at expiry, on clock rollback, and after a newer rule review', () => {
    for (const now of [999, 31_000, 900_100]) {
      const result = cleanupPreparation(fixture(), now)!
      expect(result.valid).toBe(false)
      expect(result.items.size).toBe(0)
    }
    const plan = fixture()
    plan.ruleCheckedAt = 1001
    expect(cleanupPreparation(plan, 1002)?.valid).toBe(false)
  })
  it('rejects unknown protocols, missing or duplicate selection, forged counts and methods', () => {
    const changes: ((p: ReviewPlan) => void)[] = [
      (p) => {
        p.cleanupPreparation!.version = 9
      },
      (p) => {
        p.cleanupPreparation!.planId = 'other'
      },
      (p) => {
        p.cleanupPreparation!.readOnly = false
      },
      (p) => {
        p.cleanupPreparation!.method = 'delete'
      },
      (p) => {
        p.cleanupPreparation!.scope = 'allCaches'
      },
      (p) => {
        p.cleanupPreparation!.observedCount = 2
      },
      (p) => {
        p.cleanupPreparation!.items.reverse()
      },
      (p) => {
        p.cleanupPreparation!.items[1].id = 'log'
      },
      (p) => {
        p.cleanupPreparation!.items[0].code = 'readyToDelete'
      },
      (p) => {
        p.cleanupPreparation!.items.pop()
      },
      (p) => {
        p.cleanupPreparation!.expiresAt++
      },
      (p) => {
        p.cleanupPreparation!.items[0] = null as never
      },
      (p) => {
        p.cleanupPreparation!.items[0].reason = ''
      },
    ]
    for (const change of changes) {
      const plan = fixture()
      change(plan)
      const result = cleanupPreparation(plan, 1001)!
      expect(result.valid).toBe(false)
      expect(result.items.size).toBe(0)
    }
  })
  it('never accepts observed over changed, protected, unknown permission or incomplete Mole rules', () => {
    const changes: ((p: ReviewPlan) => void)[] = [
      (p) => {
        p.items[0].state = 'protected'
      },
      (p) => {
        p.items[0].permissions = null
      },
      (p) => {
        p.items[0].permissions!.code = 'unknown'
      },
      (p) => {
        p.items[0].moleRule = null
      },
      (p) => {
        p.items[0].moleRule!.module!.binding = 'userCache'
      },
      (p) => {
        p.items[0].moleRule!.module!.code = 'unsupported'
      },
      (p) => {
        p.items[0].moleRule!.activity!.code = 'busy'
      },
    ]
    for (const change of changes) {
      const plan = fixture()
      change(plan)
      expect(cleanupPreparation(plan, 1001)?.valid).toBe(false)
    }
  })
})
