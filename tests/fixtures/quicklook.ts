import { moleUninstallRevision } from '../../src/lib/application-inspection'
import type { RefreshItem, RefreshStep } from '../../src/lib/quicklook-refresh'
export const refreshRequest = '9d54bcdd-a9a4-4444-a1ab-282a11e33641'
export function draftRaw(selected: RefreshStep[] = ['thumbnailCache', 'generators']) {
  const now = Date.now()
  return {
    version: 1,
    id: '833b487c-cc89-4444-b0b7-975b679ea6d7',
    planId: '7a09bdca-3c68-4444-a7d4-76c599fb29de',
    action: 'cache_refresh',
    method: 'quickLookRefresh',
    scope: 'quickLookServiceOnly',
    revision: moleUninstallRevision,
    selected,
    createdAt: now,
    expiresAt: now + 30_000,
    snapshotSha256: 'a'.repeat(64),
    canSubmit: true,
    preparationReadOnly: true,
    requiresRebuildAcknowledgement: true,
  }
}
export function refreshDetailRaw(id = refreshRequest, state = 'completed', at = Date.now()) {
  const pending = (position: number) => ({
    position,
    state: 'pending',
    createdAt: at,
    exitCode: null,
    stop: null,
    reason: '尚未开始',
  })
  const events: any[] = [pending(0), pending(1)]
  const terminal = (position: number, state: string, time: number) => ({
    position,
    state,
    createdAt: time,
    exitCode: state === 'commandSucceeded' ? 0 : null,
    stop: state === 'cancelled' ? 'cancelled' : null,
    reason:
      state === 'commandSucceeded'
        ? 'commandSucceeded'
        : state === 'intent'
          ? '已提交意图，尚无完成证据'
          : state === 'uncertain'
            ? '已记录意图但缺少结果，禁止自动重试'
            : state === 'cancelled'
              ? 'cancelledBeforeStart'
              : '后续步骤未启动',
  })
  events.push(terminal(0, 'intent', at + 1))
  if (state !== 'running') {
    events.push(terminal(0, state === 'interrupted' ? 'uncertain' : 'commandSucceeded', at + 2))
    if (state === 'completed') events.push(terminal(1, 'intent', at + 3))
    events.push(
      terminal(
        1,
        state === 'completed'
          ? 'commandSucceeded'
          : state === 'cancelled'
            ? 'cancelled'
            : 'notStarted',
        at + 4,
      ),
    )
  }
  const items = (['thumbnailCache', 'generators'] as const).map((step, position) => {
    const e = events.filter((e) => e.position === position).at(-1)
    return {
      position,
      step,
      state: e.state,
      exitCode: e.exitCode,
      stop: e.stop,
      reason: e.reason,
      finishedAt: ['pending', 'intent'].includes(e.state) ? null : e.createdAt,
    }
  })
  return {
    id,
    planId: draftRaw().planId,
    method: 'quickLookRefresh',
    state,
    createdAt: at,
    finishedAt: state === 'running' ? null : at + 5,
    selectedCount: 2,
    succeededCount: items.filter((i) => i.state === 'commandSucceeded').length,
    uncertainCount: items.filter((i) => i.state === 'uncertain').length,
    notStartedCount: items.filter((i) => ['notStarted', 'cancelled'].includes(i.state)).length,
    retainedForReview: ['running', 'interrupted', 'needsReview'].includes(state),
    reviewedAt: null as number | null,
    revision: moleUninstallRevision,
    snapshotSha256: 'a'.repeat(64),
    items,
    events,
  }
}

export function stoppedRefreshDetailRaw(stop: RefreshItem['stop']) {
  const record = refreshDetailRaw(refreshRequest, 'interrupted')
  const events = [record.events[0], record.events[2], record.events[3]].map((event) => ({
    ...event,
    position: 0,
    ...(event.state === 'uncertain' ? { stop, reason: 'stoppedAfterStart' } : {}),
  }))
  const last = events[2]
  return {
    ...record,
    state: 'needsReview',
    selectedCount: 1,
    succeededCount: 0,
    uncertainCount: 1,
    notStartedCount: 0,
    items: [{ ...last, step: 'generators' as const, finishedAt: last.createdAt }],
    events,
  }
}
