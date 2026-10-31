import { invoke } from '@tauri-apps/api/core'
import type { AppUpdateRecord } from './application-update'
export interface AppRestoreDraft {
  id: string
  sourceId: string
  name: string
  path: string
  retained: string
  fromVersion: string
  toVersion: string
  publisher: string
  bytes: number
  createdAt: number
  expiresAt: number
}
export interface AppRestoreRecord {
  id: string
  requestId: string
  draft: AppRestoreDraft
  createdAt: number
  finishedAt: number | null
  state: 'running' | 'restored' | 'retained' | 'cancelled' | 'unknown' | 'interrupted'
  phase: 'checking' | 'intent' | 'finished'
  reason: string
  receipt: {
    installedDevice: string
    installedInode: string
    retainedDevice: string
    retainedInode: string
    installedHash: string
    retainedHash: string
  } | null
  reviewedAt: number | null
}
export interface AppRestoreView {
  records: AppRestoreRecord[]
  activeRequests: string[]
}
function fail(): never {
  throw Error('旧版恢复数据无法核对，请检查恢复记录和两份应用；不会自动重试。')
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
function number(v: unknown): number {
  return typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? v : fail()
}
function path(v: unknown): string {
  const s = text(v)
  return s.startsWith('/') && s !== '/' && !s.split('/').some((p) => p === '.' || p === '..')
    ? s
    : fail()
}
export function appRestoreDraft(v: unknown, source?: AppUpdateRecord): AppRestoreDraft {
  const a = obj(v)
  const d = {
    id: id(a.id),
    sourceId: id(a.sourceId),
    name: text(a.name),
    path: path(a.path),
    retained: path(a.retained),
    fromVersion: text(a.fromVersion),
    toVersion: text(a.toVersion),
    publisher: text(a.publisher),
    bytes: number(a.bytes),
    createdAt: number(a.createdAt),
    expiresAt: number(a.expiresAt),
  }
  const parent = d.path.slice(0, d.path.lastIndexOf('/')),
    name = d.path.slice(d.path.lastIndexOf('/') + 1)
  if (
    d.id === d.sourceId ||
    !d.path.endsWith('.app') ||
    d.retained !== `${parent}/.moomole-app-update-${d.sourceId}/${name}` ||
    d.bytes > 8 * 1024 ** 3 ||
    d.expiresAt - d.createdAt !== 120000 ||
    (source &&
      (source.state !== 'updated' ||
        !source.recoveryAvailable ||
        d.sourceId !== source.id ||
        d.path !== source.draft.path ||
        d.retained !== source.backup ||
        d.name !== source.draft.name ||
        d.fromVersion !== source.draft.newVersion ||
        d.toVersion !== source.draft.oldVersion ||
        d.publisher !== source.draft.publisher))
  )
    fail()
  return d
}
export function appRestoreRecord(v: unknown): AppRestoreRecord {
  const a = obj(v),
    draft = appRestoreDraft(a.draft)
  const r = {
    id: id(a.id),
    requestId: id(a.requestId),
    draft,
    createdAt: number(a.createdAt),
    finishedAt: a.finishedAt === null ? null : number(a.finishedAt),
    state: text(a.state),
    phase: text(a.phase),
    reason: text(a.reason),
    receipt: a.receipt === null ? null : obj(a.receipt),
    reviewedAt: a.reviewedAt === null ? null : number(a.reviewedAt),
  }
  if (
    r.id !== draft.id ||
    r.createdAt < draft.createdAt ||
    (r.finishedAt !== null && r.finishedAt < r.createdAt) ||
    (r.reviewedAt !== null && r.reviewedAt < r.createdAt) ||
    !['running', 'restored', 'retained', 'cancelled', 'unknown', 'interrupted'].includes(r.state) ||
    !['checking', 'intent', 'finished'].includes(r.phase) ||
    r.reason.length > 4000 ||
    (r.state === 'restored' && (!r.receipt || r.phase !== 'finished' || r.finishedAt === null))
  )
    fail()
  if (r.receipt) {
    for (const key of ['installedDevice', 'installedInode', 'retainedDevice', 'retainedInode']) {
      const n = text(r.receipt[key])
      if (!/^[1-9]\d*$/.test(n) || BigInt(n) > 2n ** 64n - 1n) fail()
    }
    for (const key of ['installedHash', 'retainedHash'])
      if (!/^[0-9a-f]{64}$/.test(text(r.receipt[key]))) fail()
  }
  return r as AppRestoreRecord
}
export function appRestoreView(v: unknown): AppRestoreView {
  const a = obj(v)
  if (
    !Array.isArray(a.records) ||
    a.records.length > 64 ||
    !Array.isArray(a.activeRequests) ||
    a.activeRequests.length > 3
  )
    fail()
  const records = (a.records as unknown[]).map(appRestoreRecord),
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
export const applicationRestoration = {
  prepare: async (source: AppUpdateRecord, requestId: string) =>
    appRestoreDraft(
      await invoke('prepare_application_restoration', { sourceId: source.id, requestId }),
      source,
    ),
  discard: (id: string) => invoke('discard_application_restoration', { id }),
  execute: async (draft: AppRestoreDraft, requestId: string) => {
    const record = appRestoreRecord(
      await invoke('execute_application_restoration', { id: draft.id, requestId, confirmed: true }),
    )
    if (
      record.id !== draft.id ||
      record.requestId !== requestId ||
      JSON.stringify(record.draft) !== JSON.stringify(draft)
    )
      fail()
    return record
  },
  read: async () => appRestoreView(await invoke('get_application_restorations')),
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
  acknowledge: (id: string) =>
    invoke('acknowledge_application_restoration', { id, confirmed: true }),
  reveal: (path: string) => invoke('reveal_path', { path }),
}
export const appRestoreState: Record<AppRestoreRecord['state'], string> = {
  running: '旧版恢复进行中',
  restored: '旧版已恢复，新版保留',
  retained: '未恢复，两份应用保留',
  cancelled: '旧版恢复已取消',
  unknown: '旧版恢复结果待核实',
  interrupted: '旧版恢复已中断',
}
