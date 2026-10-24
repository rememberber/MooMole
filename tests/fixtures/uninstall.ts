// Browser/unit protocol fixtures only; no native commands or filesystem access.
import { moleUninstallRevision } from '../../src/lib/application-inspection'
export const taskId = '11000000-0000-4000-8000-000000000001'
export const planId = '11000000-0000-4000-8000-000000000002'
export const confirmationId = '11000000-0000-4000-8000-000000000003'
export const requestId = '11000000-0000-4000-8000-000000000004'
export const target = {
  taskId,
  index: 0,
  name: '示例中文 App',
  path: '/Applications/示例 中文 App.app',
  bundleId: 'org.example.uninstall.fixture',
}
export function planRaw() {
  const now = Date.now()
  return {
    version: 1,
    id: planId,
    sourceTaskId: taskId,
    sourceIndex: target.index,
    createdAt: now - 1000,
    checkedAt: now,
    expiresAt: now + 299000,
    revision: moleUninstallRevision,
    name: target.name,
    path: target.path,
    bundleId: target.bundleId,
    appVersion: '1.0',
    build: '1',
    snapshotSha256: 'a'.repeat(64),
    readOnly: true,
    canExecute: false,
    scope: 'appBundleOnly',
    relatedData: 'retainAll',
    state: 'requiresExecutionReview',
    next: '固定范围',
  }
}
export function reviewRaw(plan = planRaw()) {
  const now = Date.now()
  const observation = {
    version: 1,
    planId,
    planSnapshotSha256: 'b'.repeat(64),
    checkedAt: now,
    expiresAt: Math.min(plan.expiresAt, now + 5000),
    canPrepareConfirmation: true,
    canExecute: false,
  }
  return {
    runtime: {
      ...observation,
      scope: 'bundleExecutablePaths',
      code: 'notObserved',
      pids: [],
      coverage: 'completePass',
      issue: null,
      next: '未发现',
    },
    support: {
      ...observation,
      scope: 'ordinaryAppDefaultLaunchAndCaskroom',
      background: { code: 'notDetected', reason: null, checkedEntries: 15 },
      management: { code: 'notDetected', reason: null, checkedEntries: 2 },
      next: '未发现',
    },
    confirmation: {
      version: 2,
      id: confirmationId,
      planId,
      planSnapshotSha256: 'b'.repeat(64),
      createdAt: now,
      expiresAt: Math.min(plan.expiresAt, now + 30000),
      name: plan.name,
      path: plan.path,
      bundleId: plan.bundleId,
      method: 'appBundleTrash',
      scope: 'appBundleOnly',
      relatedData: 'retainAll',
      supportScope: 'ordinaryAppDefaultLaunchAndCaskroom',
      requiresOrdinaryAppConfirmation: true,
      canExecute: true,
      next: '提交后复查',
    },
  }
}
export function detailRaw(id = requestId, itemState = 'moved') {
  const now = Date.now(),
    createdAt = now - 50
  const states: Record<string, string> = {
    pending: 'running',
    intent: 'running',
    moved: 'completed',
    skipped: 'noChanges',
    failed: 'noChanges',
    cancelled: 'cancelled',
    notStarted: 'interrupted',
    uncertain: 'interrupted',
  }
  const receipt = ['moved', 'uncertain'].includes(itemState)
    ? {
        version: 1,
        actual: {
          destination: '/Users/demo/.Trash/示例 中文 App.app',
          device: '16777234',
          inode: '9007199254740993',
        },
        observedInventory: { version: 1, digest: 'c'.repeat(64), entries: 4 },
      }
    : null
  const reason =
    itemState === 'moved'
      ? 'App 包已移入废纸篓，包外数据保留。'
      : itemState === 'uncertain'
        ? '系统结果待核实，不要重试。'
        : '本次未确认处理。'
  const events: { state: string; createdAt: number; reason: string; receipt: typeof receipt }[] = [
    {
      state: 'pending',
      createdAt,
      reason: itemState === 'pending' ? reason : '尚未开始',
      receipt: null,
    },
  ]
  if (['intent', 'moved', 'failed', 'uncertain'].includes(itemState))
    events.push({
      state: 'intent',
      createdAt: createdAt + 1,
      reason: itemState === 'intent' ? reason : '已记录意图',
      receipt: null,
    })
  if (!['pending', 'intent'].includes(itemState))
    events.push({ state: itemState, createdAt: now, reason, receipt })
  return {
    id,
    planId,
    method: 'appBundleTrash',
    state: states[itemState],
    itemState,
    name: target.name,
    path: target.path,
    bundleId: target.bundleId,
    createdAt,
    finishedAt: states[itemState] === 'running' ? null : now,
    retainedForReview: ['pending', 'intent', 'moved', 'uncertain', 'notStarted'].includes(
      itemState,
    ),
    revision: moleUninstallRevision,
    snapshotSha256: 'd'.repeat(64),
    reason,
    receipt,
    events,
  }
}
