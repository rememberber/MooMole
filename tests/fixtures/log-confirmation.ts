import type { ReviewPlan } from '../../src/types'
export function confirmationPlan(): ReviewPlan {
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
  const plan: ReviewPlan = {
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
      version: 1,
      planId: 'plan',
      scope: 'userLogFiles',
      method: 'trash',
      readOnly: true,
      checkedAt: 1000,
      expiresAt: 31_000,
      observedCount: 1,
      items: [
        { id: 'log', code: 'observed', reason: '本轮日志准备已核对。' },
        { id: 'cache', code: 'retained', reason: '缓存不在单文件日志范围，保留。' },
      ],
    },
  }
  plan.cleanupConfirmation = {
    version: 1,
    id: '12345678-1234-4123-8123-123456789abc',
    planId: plan.id,
    snapshotSha256: 'a'.repeat(64),
    method: 'trash',
    createdAt: 1000,
    expiresAt: 31000,
    retainedCount: 1,
    items: [{ id: 'log', path: '/fixture/log', sizeBytes: 10 }],
  }
  return plan
}
