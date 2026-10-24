import { moleUninstallRevision } from './application-inspection'
export const refreshSteps = ['thumbnailCache', 'generators'] as const
export type RefreshStep = (typeof refreshSteps)[number]
export const refreshSessionKey = 'moomole-quicklook-request-v1'
export type RefreshDraft = {
  id: string
  planId: string
  selected: RefreshStep[]
  createdAt: number
  expiresAt: number
  snapshotSha256: string
}
export type RefreshSummary = {
  id: string
  planId: string
  method: 'quickLookRefresh'
  state: string
  createdAt: number
  finishedAt: number | null
  selectedCount: number
  succeededCount: number
  uncertainCount: number
  notStartedCount: number
  retainedForReview: boolean
  reviewedAt: number | null
}
type Result = {
  state: string
  exitCode: number | null
  stop: 'cancelled' | 'timedOut' | null
  reason: string
}
export type RefreshItem = Result & {
  position: number
  step: RefreshStep
  finishedAt: number | null
}
export type RefreshEvent = Result & { position: number; createdAt: number }
export type RefreshDetail = RefreshSummary & {
  revision: string
  snapshotSha256: string
  items: RefreshItem[]
  events: RefreshEvent[]
}
export type RefreshSnapshot = {
  version: 1
  requestId: string
  active: boolean
  record: RefreshDetail | null
}
export type RefreshTransport = {
  prepare: (selected: RefreshStep[]) => Promise<unknown>
  discard: (id: string) => Promise<unknown>
  execute: (draft: RefreshDraft, requestId: string) => Promise<unknown>
  read: (id: string) => Promise<unknown>
  list: () => Promise<unknown>
  cancel: (id: string) => Promise<unknown>
  acknowledge: (record: RefreshDetail) => Promise<unknown>
}
const fail = (): never => {
  throw new Error('维护数据无法核对；保留已知记录，不会执行或自动重试。')
}
const object = (v: unknown): Record<string, unknown> =>
  v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : fail()
const text = (v: unknown, max = 1024): string =>
  typeof v === 'string' &&
  v.trim().length > 0 &&
  !v.includes('\0') &&
  new TextEncoder().encode(v).length <= max
    ? v
    : fail()
const integer = (v: unknown): number =>
  typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? v : fail()
const nullable = (v: unknown): number | null => (v === null ? null : integer(v))
export const refreshId = (v: unknown): string =>
  typeof v === 'string' && /^[\da-f]{8}(-[\da-f]{4}){3}-[\da-f]{12}$/i.test(v) ? v : fail()
const hash = (v: unknown, n = 64): string =>
  typeof v === 'string' && new RegExp(`^[\\da-f]{${n}}$`).test(v) ? v : fail()
function steps(v: unknown): RefreshStep[] {
  if (
    !Array.isArray(v) ||
    ![1, 2].includes(v.length) ||
    v.some((x) => !refreshSteps.includes(x)) ||
    (v.length === 2 && (v[0] !== refreshSteps[0] || v[1] !== refreshSteps[1]))
  )
    return fail()
  return v as RefreshStep[]
}
export function readRefreshDraft(v: unknown, selected: RefreshStep[], began: number): RefreshDraft {
  const r = object(v)
  const chosen = steps(r.selected)
  const createdAt = integer(r.createdAt)
  const expiresAt = integer(r.expiresAt)
  if (
    r.version !== 1 ||
    r.action !== 'cache_refresh' ||
    r.method !== 'quickLookRefresh' ||
    r.scope !== 'quickLookServiceOnly' ||
    r.revision !== moleUninstallRevision ||
    r.canSubmit !== true ||
    r.preparationReadOnly !== true ||
    r.requiresRebuildAcknowledgement !== true ||
    JSON.stringify(chosen) !== JSON.stringify(selected) ||
    createdAt < began - 1000 ||
    createdAt > Date.now() + 1000 ||
    expiresAt <= createdAt ||
    expiresAt > createdAt + 30_000
  )
    return fail()
  return {
    id: refreshId(r.id),
    planId: refreshId(r.planId),
    selected: chosen,
    createdAt,
    expiresAt,
    snapshotSha256: hash(r.snapshotSha256),
  }
}
export function readRefreshSummary(v: unknown): RefreshSummary {
  const r = object(v)
  const state = text(r.state)
  const finishedAt = nullable(r.finishedAt)
  const reviewedAt = nullable(r.reviewedAt)
  const selectedCount = integer(r.selectedCount),
    succeededCount = integer(r.succeededCount),
    uncertainCount = integer(r.uncertainCount),
    notStartedCount = integer(r.notStartedCount)
  if (
    r.method !== 'quickLookRefresh' ||
    ![
      'running',
      'completed',
      'partial',
      'noChanges',
      'cancelled',
      'needsReview',
      'interrupted',
    ].includes(state) ||
    ![1, 2].includes(selectedCount) ||
    succeededCount + uncertainCount + notStartedCount > selectedCount ||
    (state === 'running') !== (finishedAt === null) ||
    (state !== 'running' && succeededCount + uncertainCount + notStartedCount !== selectedCount) ||
    (reviewedAt !== null && !['interrupted', 'needsReview'].includes(state))
  )
    return fail()
  if (
    (state === 'completed' && succeededCount !== selectedCount) ||
    (state === 'needsReview' && uncertainCount === 0) ||
    (state === 'partial' &&
      (succeededCount === 0 || succeededCount === selectedCount || uncertainCount !== 0)) ||
    (state === 'cancelled' && (notStartedCount === 0 || uncertainCount !== 0)) ||
    (state === 'noChanges' && notStartedCount !== selectedCount)
  )
    return fail()
  const retained =
    reviewedAt === null &&
    (['running', 'interrupted', 'needsReview'].includes(state) || uncertainCount > 0)
  if (r.retainedForReview !== retained) return fail()
  return {
    id: refreshId(r.id),
    planId: refreshId(r.planId),
    method: 'quickLookRefresh',
    state,
    createdAt: integer(r.createdAt),
    finishedAt,
    selectedCount,
    succeededCount,
    uncertainCount,
    notStartedCount,
    retainedForReview: retained,
    reviewedAt,
  }
}
function result(v: unknown): Result {
  const r = object(v)
  const state = text(r.state)
  const code = nullable(r.exitCode)
  const stop = r.stop
  if (
    !['pending', 'intent', 'commandSucceeded', 'notStarted', 'cancelled', 'uncertain'].includes(
      state,
    ) ||
    (stop !== null && stop !== 'cancelled' && stop !== 'timedOut') ||
    (code !== null && code > 255)
  )
    return fail()
  if (
    (state === 'commandSucceeded' && (code !== 0 || stop !== null)) ||
    (['pending', 'intent', 'notStarted'].includes(state) && (code !== null || stop !== null)) ||
    (state === 'cancelled' && (code !== null || stop !== 'cancelled')) ||
    (state === 'uncertain' && code === 0 && stop === null)
  )
    return fail()
  return { state, exitCode: code, stop, reason: text(r.reason) }
}
export function readRefreshDetail(v: unknown): RefreshDetail {
  const r = object(v)
  const summary = readRefreshSummary(r)
  if (
    !Array.isArray(r.items) ||
    r.items.length !== summary.selectedCount ||
    !Array.isArray(r.events) ||
    r.events.length < summary.selectedCount ||
    r.events.length > 6
  )
    return fail()
  const items: RefreshItem[] = r.items.map((raw, position) => {
    const x = object(raw)
    const item = {
      ...result(x),
      position: integer(x.position),
      step: steps([x.step])[0],
      finishedAt: nullable(x.finishedAt),
    }
    if (
      item.position !== position ||
      ['pending', 'intent'].includes(item.state) !== (item.finishedAt === null)
    )
      return fail()
    return item
  })
  steps(items.map((i) => i.step))
  const events: RefreshEvent[] = r.events.map((raw) => {
    const x = object(raw)
    const e = { ...result(x), position: integer(x.position), createdAt: integer(x.createdAt) }
    if (e.position >= items.length) return fail()
    return e
  })
  const previous = items.map(() => 'pending')
  events.forEach((e, index) => {
    if (index < items.length && (e.position !== index || e.state !== 'pending')) fail()
    if (e.state === 'intent' && previous.slice(0, e.position).some((s) => s !== 'commandSucceeded'))
      fail()
    previous[e.position] = e.state
  })
  for (const item of items) {
    const chain = events.filter((e) => e.position === item.position),
      last = chain.at(-1)
    if (
      !last ||
      chain.length > 3 ||
      chain[0].state !== 'pending' ||
      chain[0].createdAt !== summary.createdAt ||
      chain.some(
        (e, p) =>
          p > 0 &&
          !(
            chain[p - 1].state === 'pending'
              ? ['intent', 'notStarted', 'cancelled']
              : chain[p - 1].state === 'intent'
                ? ['commandSucceeded', 'uncertain', 'notStarted', 'cancelled']
                : []
          ).includes(e.state),
      ) ||
      last.state !== item.state ||
      last.reason !== item.reason ||
      last.exitCode !== item.exitCode ||
      last.stop !== item.stop ||
      (item.finishedAt !== null && item.finishedAt !== last.createdAt)
    )
      fail()
  }
  if (
    summary.succeededCount !== items.filter((i) => i.state === 'commandSucceeded').length ||
    summary.uncertainCount !== items.filter((i) => i.state === 'uncertain').length ||
    summary.notStartedCount !==
      items.filter((i) => ['notStarted', 'cancelled'].includes(i.state)).length ||
    (summary.state !== 'running' && items.some((i) => ['pending', 'intent'].includes(i.state)))
  )
    fail()
  return {
    ...summary,
    revision: hash(r.revision, 40),
    snapshotSha256: hash(r.snapshotSha256),
    items,
    events,
  }
}
export function readRefreshSnapshot(v: unknown, id: string): RefreshSnapshot {
  const r = object(v)
  if (r.version !== 1 || r.requestId !== id || typeof r.active !== 'boolean') return fail()
  const record = r.record === null ? null : readRefreshDetail(r.record)
  if (record && record.id !== id) return fail()
  return { version: 1, requestId: id, active: r.active, record }
}
export function readRefreshList(v: unknown): RefreshSummary[] {
  if (!Array.isArray(v) || v.length > 64) return fail()
  const rows = v.map(readRefreshSummary)
  if (new Set(rows.map((r) => r.id)).size !== rows.length) return fail()
  return rows
}
export function preserveRefresh(
  previous: RefreshSnapshot | null,
  next: RefreshSnapshot,
): RefreshSnapshot {
  const p = previous?.record,
    n = next.record
  if (!p || previous?.requestId !== next.requestId) return next
  if (
    n &&
    (p.planId !== n.planId ||
      p.snapshotSha256 !== n.snapshotSha256 ||
      p.revision !== n.revision ||
      p.createdAt !== n.createdAt ||
      JSON.stringify(p.items.map((i) => i.step)) !== JSON.stringify(n.items.map((i) => i.step)))
  )
    fail()
  const rank = (s: string) => (s === 'pending' ? 0 : s === 'intent' ? 1 : 2)
  if (
    !n ||
    (p.state !== 'running' && n.state === 'running') ||
    (p.reviewedAt !== null && n.reviewedAt === null) ||
    p.items.some(
      (i, index) =>
        !n.items[index] ||
        rank(i.state) > rank(n.items[index].state) ||
        (rank(i.state) === 2 && JSON.stringify(i) !== JSON.stringify(n.items[index])),
    )
  )
    return { ...next, record: p, active: p.state !== 'running' ? previous.active : next.active }
  return next
}
export const refreshStepName = (step: RefreshStep) =>
  step === 'thumbnailCache' ? '缩略图缓存' : '生成器列表'
export const refreshStateName = (state: string) =>
  ({
    running: '正在刷新',
    completed: '所选命令已完成',
    partial: '部分完成',
    noChanges: '未启动刷新',
    cancelled: '已取消后续步骤',
    needsReview: '结果待核实',
    interrupted: '维护已中断',
    pending: '等待开始',
    intent: '处理中，结果未确认',
    commandSucceeded: '系统命令已完成',
    notStarted: '未开始',
    uncertain: '结果待核实',
  })[state] ?? state
export const refreshReason = (reason: string) =>
  ({
    commandSucceeded: '命令正常返回，后续预览由系统处理。',
    gateRefused: '执行前条件未通过，此步骤未启动。',
    identityChanged: '系统工具或账户发生变化，此步骤未启动。',
    spawnFailed: '系统工具未能启动。',
    cancelledBeforeStart: '开始前已取消。',
    stoppedAfterStart: '命令启动后停止，实际刷新结果待核实。',
    commandFailedAfterStart: '命令启动后未正常完成，实际刷新结果待核实。',
    childOwnershipUnknown: '系统命令状态无法确认，请核查结果。',
    collectionFailed: '命令结果未能完整收集，请核查。',
  })[reason] ?? reason
