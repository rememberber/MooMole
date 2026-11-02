import { invoke } from '@tauri-apps/api/core'
import type { AppUpdateRecord } from './application-update'
export interface RetainedAppDraft {
  id: string
  sourceId: string
  name: string
  installed: string
  retained: string
  stage: string
  installedVersion: string
  retainedVersion: string
  afterRestore: boolean
  publisher: string
  bytes: number
  createdAt: number
  expiresAt: number
}
export interface RetainedAppRecord {
  id: string
  requestId: string
  draft: RetainedAppDraft
  state: 'running' | 'moved' | 'retained' | 'cancelled' | 'unknown'
  phase: 'checking' | 'intent' | 'finished'
  reason: string
  createdAt: number
  finishedAt: number | null
  receipt: { destination: string; device: string; inode: string } | null
  stageRemoved: boolean
  reviewedAt: number | null
}
export interface RetainedAppView {
  records: RetainedAppRecord[]
  activeRequests: string[]
}
function fail(): never {
  throw Error('保留版本整理数据无法核对，请检查处理记录和实际位置；不会自动重试。')
}
function obj(v: unknown): Record<string, unknown> {
  return v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : fail()
}
function text(v: unknown): string {
  return typeof v === 'string' &&
    v.length > 0 &&
    v.length <= 4096 &&
    !/[\u0000-\u001f\u007f]/.test(v)
    ? v
    : fail()
}
function id(v: unknown): string {
  const s = text(v)
  return /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(s) ? s : fail()
}
function integer(v: unknown): number {
  return typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? v : fail()
}
function bool(v: unknown): boolean {
  return typeof v === 'boolean' ? v : fail()
}
function path(v: unknown): string {
  const s = text(v)
  return s.startsWith('/') && s !== '/' && !s.split('/').some((p) => p === '.' || p === '..')
    ? s
    : fail()
}
function decimal(v: unknown, nonzero = false): string {
  const s = text(v)
  return /^(0|[1-9]\d*)$/.test(s) && BigInt(s) <= 2n ** 64n - 1n && (!nonzero || s !== '0')
    ? s
    : fail()
}
export function retainedAppDraft(v: unknown, source?: AppUpdateRecord): RetainedAppDraft {
  const a = obj(v),
    d = {
      id: id(a.id),
      sourceId: id(a.sourceId),
      name: text(a.name),
      installed: path(a.installed),
      retained: path(a.retained),
      stage: path(a.stage),
      installedVersion: text(a.installedVersion),
      retainedVersion: text(a.retainedVersion),
      afterRestore: bool(a.afterRestore),
      publisher: text(a.publisher),
      bytes: integer(a.bytes),
      createdAt: integer(a.createdAt),
      expiresAt: integer(a.expiresAt),
    }
  const parent = d.installed.slice(0, d.installed.lastIndexOf('/')),
    name = d.installed.slice(d.installed.lastIndexOf('/') + 1)
  if (
    d.id === d.sourceId ||
    !d.installed.endsWith('.app') ||
    d.stage !== `${parent}/.moomole-app-update-${d.sourceId}` ||
    d.retained !== `${d.stage}/${name}` ||
    d.bytes > 8 * 1024 ** 3 ||
    d.expiresAt - d.createdAt !== 120000 ||
    (source &&
      (source.state !== 'updated' ||
        source.retainedCopyAvailable !== true ||
        d.sourceId !== source.id ||
        d.installed !== source.draft.path ||
        d.retained !== source.backup ||
        d.name !== source.draft.name ||
        d.publisher !== source.draft.publisher ||
        d.installedVersion !==
          (d.afterRestore ? source.draft.oldVersion : source.draft.newVersion) ||
        d.retainedVersion !== (d.afterRestore ? source.draft.newVersion : source.draft.oldVersion)))
  )
    fail()
  return d
}
export function retainedAppRecord(v: unknown): RetainedAppRecord {
  const a = obj(v),
    draft = retainedAppDraft(a.draft),
    receipt = a.receipt === null ? null : obj(a.receipt)
  const r = {
    id: id(a.id),
    requestId: id(a.requestId),
    draft,
    state: text(a.state),
    phase: text(a.phase),
    reason: text(a.reason),
    createdAt: integer(a.createdAt),
    finishedAt: a.finishedAt === null ? null : integer(a.finishedAt),
    receipt: receipt
      ? {
          destination: path(receipt.destination),
          device: decimal(receipt.device),
          inode: decimal(receipt.inode, true),
        }
      : null,
    stageRemoved: bool(a.stageRemoved),
    reviewedAt: a.reviewedAt === null ? null : integer(a.reviewedAt),
  }
  if (
    r.id !== draft.id ||
    r.createdAt < draft.createdAt ||
    (r.finishedAt !== null && r.finishedAt < r.createdAt) ||
    (r.reviewedAt !== null && r.reviewedAt < r.createdAt) ||
    !['running', 'moved', 'retained', 'cancelled', 'unknown'].includes(r.state) ||
    !['checking', 'intent', 'finished'].includes(r.phase) ||
    (r.state === 'running') !== (r.finishedAt === null) ||
    (r.phase === 'finished') !== (r.state !== 'running') ||
    (r.state === 'moved' && !r.receipt) ||
    (r.stageRemoved && r.state !== 'moved') ||
    r.reason.length > 4000
  )
    fail()
  return r as RetainedAppRecord
}
export function retainedAppView(v: unknown): RetainedAppView {
  const a = obj(v)
  if (
    !Array.isArray(a.records) ||
    a.records.length > 64 ||
    !Array.isArray(a.activeRequests) ||
    a.activeRequests.length > 3
  )
    fail()
  const records = (a.records as unknown[]).map(retainedAppRecord),
    activeRequests = (a.activeRequests as unknown[]).map(id)
  if (
    new Set(records.map((r) => r.id)).size !== records.length ||
    new Set(records.map((r) => r.requestId)).size !== records.length ||
    new Set(activeRequests).size !== activeRequests.length ||
    activeRequests.some((id) => !records.some((r) => r.requestId === id && r.state === 'running'))
  )
    fail()
  return { records, activeRequests }
}
export const retainedApplication = {
  prepare: async (source: AppUpdateRecord, requestId: string) =>
    retainedAppDraft(
      await invoke('prepare_retained_application', { sourceId: source.id, requestId }),
      source,
    ),
  discard: (id: string) => invoke('discard_retained_application', { id }),
  execute: async (draft: RetainedAppDraft, requestId: string) => {
    const r = retainedAppRecord(
      await invoke('execute_retained_application', { id: draft.id, requestId, confirmed: true }),
    )
    if (
      r.id !== draft.id ||
      r.requestId !== requestId ||
      JSON.stringify(r.draft) !== JSON.stringify(draft)
    )
      fail()
    return r
  },
  read: async () => retainedAppView(await invoke('get_retained_application_history')),
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
  acknowledge: (id: string) => invoke('acknowledge_retained_application', { id }),
  reveal: (path: string) => invoke('reveal_path', { path }),
}
export const retainedAppState: Record<RetainedAppRecord['state'], string> = {
  running: '保留版本整理进行中',
  moved: '保留版本已移入废纸篓',
  retained: '保留版本未处理',
  cancelled: '保留版本整理已取消',
  unknown: '保留版本整理结果待核实',
}
