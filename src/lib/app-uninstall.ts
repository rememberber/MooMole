// Structured native boundaries. Parsing never grants permission or starts work.
import { moleUninstallRevision } from './application-inspection'

export type UninstallTarget = {
  taskId: string
  index: number
  name: string
  path: string
  bundleId: string
}
export type UninstallPlan = {
  id: string
  name: string
  path: string
  bundleId: string
  createdAt: number
  expiresAt: number
}
export type UninstallConfirmation = UninstallPlan & { planId: string; planSnapshotSha256: string }
export type UninstallReview = { confirmation: UninstallConfirmation | null; message: string }
export type UninstallSummary = {
  id: string
  planId: string
  method: 'appBundleTrash'
  state: string
  itemState: string
  name: string
  path: string
  bundleId: string
  createdAt: number
  finishedAt: number | null
  retainedForReview: boolean
}
export type UninstallReceipt = {
  version: 1
  actual: { destination: string; device: string; inode: string }
  observedInventory: { version: 1; digest: string; entries: number } | null
}
export type UninstallDetail = UninstallSummary & {
  revision: string
  snapshotSha256: string
  reason: string
  receipt: UninstallReceipt | null
  events: { state: string; createdAt: number; reason: string; receipt: UninstallReceipt | null }[]
}
export type UninstallSnapshot = {
  version: 1
  requestId: string
  active: boolean
  record: UninstallDetail | null
}
export type UninstallTransport = {
  prepare: (taskId: string, index: number) => Promise<unknown>
  review: (planId: string) => Promise<unknown>
  discardPlan: (planId: string) => Promise<unknown>
  discardConfirmation: (id: string) => Promise<unknown>
  execute: (planId: string, confirmationId: string, requestId: string) => Promise<unknown>
  read: (requestId: string) => Promise<unknown>
  list: () => Promise<unknown>
  cancel: (requestId: string) => Promise<unknown>
}
function fail(): never {
  throw new Error('卸载数据无法核对，请重新读取；不会执行或自动重试。')
}
const object = (v: unknown): Record<string, unknown> =>
  v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : fail()
const text = (v: unknown, max = 4096): string =>
  typeof v === 'string' &&
  v.length > 0 &&
  new TextEncoder().encode(v).length <= max &&
  !v.includes('\0')
    ? v
    : fail()
const integer = (v: unknown): number =>
  typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? v : fail()
const bool = (v: unknown): boolean => (typeof v === 'boolean' ? v : fail())
const uuidPattern = /^[\da-f]{8}-[\da-f]{4}-[\da-f]{4}-[\da-f]{4}-[\da-f]{12}$/i
export const uninstallId = (v: unknown): string =>
  typeof v === 'string' && uuidPattern.test(v) ? v : fail()
const hash = (v: unknown): string => (typeof v === 'string' && /^[\da-f]{64}$/.test(v) ? v : fail())
const absolute = (v: unknown): string => {
  const s = text(v)
  return s.startsWith('/') && s !== '/' && !s.split('/').some((p) => p === '.' || p === '..')
    ? s
    : fail()
}
function scope(v: Record<string, unknown>) {
  if (v.scope !== 'appBundleOnly' || v.relatedData !== 'retainAll') fail()
}
function lifetime(v: Record<string, unknown>, limit: number) {
  const createdAt = integer(v.createdAt),
    expiresAt = integer(v.expiresAt)
  if (expiresAt <= createdAt || expiresAt - createdAt > limit) fail()
  return { createdAt, expiresAt }
}
export function parseUninstallPlan(raw: unknown, target: UninstallTarget): UninstallPlan {
  const v = object(raw)
  if (
    v.version !== 1 ||
    v.readOnly !== true ||
    v.canExecute !== false ||
    v.state !== 'requiresExecutionReview' ||
    v.revision !== moleUninstallRevision ||
    v.sourceTaskId !== target.taskId ||
    v.sourceIndex !== target.index ||
    v.path !== target.path ||
    v.bundleId !== target.bundleId ||
    v.name !== target.name
  )
    fail()
  scope(v)
  hash(v.snapshotSha256)
  const time = lifetime(v, 300000),
    checked = integer(v.checkedAt)
  if (checked < time.createdAt || checked >= time.expiresAt) fail()
  return {
    id: uninstallId(v.id),
    name: text(v.name, 1024),
    path: absolute(v.path),
    bundleId: text(v.bundleId, 255),
    ...time,
  }
}
function observation(v: Record<string, unknown>, plan: UninstallPlan) {
  if (v.version !== 1 || v.planId !== plan.id || v.canExecute !== false) fail()
  const checked = integer(v.checkedAt),
    expires = integer(v.expiresAt)
  if (
    checked < plan.createdAt ||
    expires <= checked ||
    expires - checked > 5000 ||
    expires > plan.expiresAt
  )
    fail()
  bool(v.canPrepareConfirmation)
  return hash(v.planSnapshotSha256)
}
export function parseUninstallReview(raw: unknown, plan: UninstallPlan): UninstallReview {
  const v = object(raw),
    runtime = object(v.runtime),
    support = object(v.support)
  const digest = observation(runtime, plan)
  if (
    observation(support, plan) !== digest ||
    runtime.scope !== 'bundleExecutablePaths' ||
    support.scope !== 'ordinaryAppDefaultLaunchAndCaskroom'
  )
    fail()
  if (
    !['notObserved', 'observed', 'unknown', 'unsupported'].includes(text(runtime.code)) ||
    !['completePass', 'incomplete'].includes(text(runtime.coverage)) ||
    !Array.isArray(runtime.pids) ||
    runtime.pids.length > 32 ||
    runtime.pids.some((p) => integer(p) === 0) ||
    new Set(runtime.pids).size !== runtime.pids.length
  )
    fail()
  if (runtime.code === 'observed' ? runtime.pids.length === 0 : runtime.pids.length !== 0) fail()
  const background = object(support.background),
    management = object(support.management)
  for (const item of [background, management]) {
    if (!['notDetected', 'detected', 'unknown'].includes(text(item.code))) fail()
    integer(item.checkedEntries)
  }
  if (v.confirmation == null)
    return {
      confirmation: null,
      message:
        runtime.code === 'observed'
          ? '发现此 App 的运行程序。请自行退出后重新检查。'
          : background.code === 'detected' || management.code === 'detected'
            ? '发现后台服务或安装管理线索，本次保留。请使用对应管理工具或专用卸载方式。'
            : runtime.code === 'unsupported'
              ? '此 App 的主程序格式暂不支持卸载，本次保留。'
              : '运行、后台或管理方式尚未确认，本次保留；请稍后重新检查。',
    }
  const c = object(v.confirmation)
  if (
    runtime.code !== 'notObserved' ||
    runtime.coverage !== 'completePass' ||
    runtime.canPrepareConfirmation !== true ||
    support.canPrepareConfirmation !== true ||
    background.code !== 'notDetected' ||
    management.code !== 'notDetected' ||
    c.version !== 2 ||
    c.canExecute !== true ||
    c.requiresOrdinaryAppConfirmation !== true ||
    c.supportScope !== 'ordinaryAppDefaultLaunchAndCaskroom' ||
    c.planId !== plan.id ||
    c.planSnapshotSha256 !== digest ||
    c.method !== 'appBundleTrash' ||
    c.name !== plan.name ||
    c.path !== plan.path ||
    c.bundleId !== plan.bundleId
  )
    fail()
  scope(c)
  const time = lifetime(c, 30000)
  if (
    time.createdAt < Math.max(integer(runtime.checkedAt), integer(support.checkedAt)) ||
    time.expiresAt > plan.expiresAt
  )
    fail()
  return {
    confirmation: {
      ...plan,
      ...time,
      id: uninstallId(c.id),
      planId: plan.id,
      planSnapshotSha256: digest,
    },
    message: '仅将此 App 包移入废纸篓；包外设置、缓存和个人数据全部保留。',
  }
}
const runState: Record<string, string> = {
  pending: 'running',
  intent: 'running',
  moved: 'completed',
  skipped: 'noChanges',
  failed: 'noChanges',
  cancelled: 'cancelled',
  notStarted: 'interrupted',
  uncertain: 'interrupted',
}
export const uninstallStateName = (state: string) =>
  ({
    pending: '尚未开始',
    intent: '等待实际结果',
    moved: '已移入废纸篓',
    skipped: '已保留',
    failed: '未作修改',
    cancelled: '已取消，未调用处理',
    notStarted: '中断后未开始',
    uncertain: '结果待核实',
  })[state] ?? '状态待核实'
export function parseUninstallSummary(raw: unknown): UninstallSummary {
  const v = object(raw),
    itemState = text(v.itemState),
    state = text(v.state)
  if (
    runState[itemState] !== state ||
    v.method !== 'appBundleTrash' ||
    (state === 'running') !== (v.finishedAt === null)
  )
    fail()
  return {
    id: uninstallId(v.id),
    planId: uninstallId(v.planId),
    method: 'appBundleTrash',
    state,
    itemState,
    name: text(v.name, 1024),
    path: absolute(v.path),
    bundleId: text(v.bundleId, 255),
    createdAt: integer(v.createdAt),
    finishedAt: v.finishedAt === null ? null : integer(v.finishedAt),
    retainedForReview: bool(v.retainedForReview),
  }
}
function receipt(raw: unknown, state: string): UninstallReceipt | null {
  if (raw === null) {
    if (state === 'moved') fail()
    return null
  }
  if (!['moved', 'uncertain'].includes(state)) fail()
  const v = object(raw),
    actual = object(v.actual)
  if (v.version !== 1) fail()
  for (const value of [actual.device, actual.inode])
    if (
      typeof value !== 'string' ||
      !/^\d{1,20}$/.test(value) ||
      BigInt(value) > 18446744073709551615n
    )
      fail()
  if (BigInt(actual.inode as string) === 0n) fail()
  const inv = v.observedInventory === null ? null : object(v.observedInventory)
  if (inv && (inv.version !== 1 || integer(inv.entries) > 10000)) fail()
  if (state === 'moved' && !inv) fail()
  return {
    version: 1,
    actual: {
      destination: absolute(actual.destination),
      device: actual.device as string,
      inode: actual.inode as string,
    },
    observedInventory: inv
      ? { version: 1, digest: hash(inv.digest), entries: integer(inv.entries) }
      : null,
  }
}
export function parseUninstallDetail(raw: unknown, id: string): UninstallDetail {
  const summary = parseUninstallSummary(raw),
    v = object(raw)
  if (summary.id !== id || !Array.isArray(v.events) || v.events.length < 1 || v.events.length > 3)
    fail()
  const events = v.events.map((raw) => {
    const e = object(raw)
    return {
      state: text(e.state),
      createdAt: integer(e.createdAt),
      reason: text(e.reason, 1024),
      receipt: receipt(e.receipt, text(e.state)),
    }
  })
  const last = events.at(-1)!,
    reason = text(v.reason, 1024),
    value = receipt(v.receipt, summary.itemState)
  if (
    events[0].state !== 'pending' ||
    events[0].createdAt !== summary.createdAt ||
    last.state !== summary.itemState ||
    last.reason !== reason ||
    JSON.stringify(last.receipt) !== JSON.stringify(value) ||
    (summary.finishedAt !== null && last.createdAt !== summary.finishedAt)
  )
    fail()
  for (let i = 1; i < events.length; i++)
    if (
      !(
        events[i - 1].state === 'pending'
          ? ['intent', 'skipped', 'cancelled', 'notStarted']
          : events[i - 1].state === 'intent'
            ? ['moved', 'failed', 'uncertain', 'cancelled']
            : []
      ).includes(events[i].state)
    )
      fail()
  if (value?.actual.destination === summary.path) fail()
  return {
    ...summary,
    revision: text(v.revision, 128),
    snapshotSha256: hash(v.snapshotSha256),
    reason,
    receipt: value,
    events,
  }
}
export function parseUninstallSnapshot(raw: unknown, id: string): UninstallSnapshot {
  const v = object(raw)
  if (v.version !== 1 || v.requestId !== id) fail()
  return {
    version: 1,
    requestId: uninstallId(v.requestId),
    active: bool(v.active),
    record: v.record === null ? null : parseUninstallDetail(v.record, id),
  }
}
export function parseUninstallList(raw: unknown): UninstallSummary[] {
  if (!Array.isArray(raw) || raw.length > 64) fail()
  const items = raw.map(parseUninstallSummary)
  if (new Set(items.map((i) => i.id)).size !== items.length) fail()
  return items
}
export function preserveUninstall(
  next: UninstallSnapshot,
  previous: UninstallSnapshot | null,
): UninstallSnapshot {
  if (previous?.requestId !== next.requestId || !previous.record) return next
  if (
    !next.record ||
    (previous.record.state !== 'running' && next.record.state === 'running') ||
    previous.record.events.length > next.record.events.length
  )
    return { ...next, active: previous.active && next.active, record: previous.record }
  return next
}
export const uninstallSessionKey = 'moomole-uninstall-request-v1'
