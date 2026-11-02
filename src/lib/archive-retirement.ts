import { invoke } from '@tauri-apps/api/core'
import { archiveHeader, type ArchiveHeader } from './execution-archive'
export type RetirementDraft = {
  id: string
  header: ArchiveHeader
  path: string
  exported: string
  bytes: number
  sha256: string
  createdAt: number
  expiresAt: number
}
export type RetirementRecord = {
  id: string
  requestId: string
  draft: RetirementDraft
  state: 'running' | 'moved' | 'retained' | 'cancelled' | 'unknown'
  phase: 'checking' | 'intent' | 'finished'
  reason: string
  createdAt: number
  finishedAt: number | null
  receipt: { destination: string; device: string; inode: string } | null
  reviewedAt: number | null
}
const fail = (): never => {
  throw Error('归档副本整理数据无法核对，请保留证据并刷新；不会自动重试。')
}
const obj = (v: unknown) =>
  v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : fail()
const num = (v: unknown) =>
  typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 && v <= 8.64e15 ? v : fail()
const id = (v: unknown) =>
  typeof v === 'string' && /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(v) ? v : fail()
const text = (v: unknown) =>
  typeof v === 'string' &&
  v.length > 0 &&
  new TextEncoder().encode(v).length <= 4096 &&
  !/[\u0000-\u001f\u007f-\u009f]/u.test(v)
    ? v
    : fail()
const path = (v: unknown) => {
  const s = text(v)
  return s.startsWith('/') && !s.split('/').some((p) => p === '.' || p === '..') ? s : fail()
}
const sameHeader = (a: ArchiveHeader, b: ArchiveHeader) =>
  a.id === b.id &&
  a.version === b.version &&
  a.createdAt === b.createdAt &&
  a.payloadSha256 === b.payloadSha256 &&
  a.payloadBytes === b.payloadBytes &&
  a.counts.cleanup === b.counts.cleanup &&
  a.counts.uninstall === b.counts.uninstall &&
  a.counts.maintenance === b.counts.maintenance &&
  (['moleClean', 'extraction', 'managedApp'] as const).every(
    (k) => (a.counts[k] ?? 0) === (b.counts[k] ?? 0),
  )
const sameDraft = (a: RetirementDraft, b: RetirementDraft) =>
  sameHeader(a.header, b.header) &&
  (['id', 'path', 'exported', 'bytes', 'sha256', 'createdAt', 'expiresAt'] as const).every(
    (k) => a[k] === b[k],
  )
export function retirementDraft(raw: unknown, expected?: ArchiveHeader): RetirementDraft {
  const v = obj(raw),
    r = {
      id: id(v.id),
      header: archiveHeader(v.header),
      path: path(v.path),
      exported: path(v.exported),
      bytes: num(v.bytes),
      sha256: text(v.sha256),
      createdAt: num(v.createdAt),
      expiresAt: num(v.expiresAt),
    }
  if (
    r.path === r.exported ||
    !r.path.endsWith('/' + r.header.id + '.moomole-history') ||
    !r.exported.endsWith('.moomole-history') ||
    r.bytes <= r.header.payloadBytes ||
    r.bytes - r.header.payloadBytes > 2048 ||
    r.bytes > 128 * 1024 ** 2 + 2048 ||
    !/^[a-f0-9]{64}$/.test(r.sha256) ||
    r.expiresAt - r.createdAt !== 120000 ||
    (expected && !sameHeader(r.header, expected))
  )
    fail()
  return r
}
export function retirementRecord(
  raw: unknown,
  draft?: RetirementDraft,
  request?: string,
): RetirementRecord {
  const v = obj(raw),
    d = retirementDraft(v.draft),
    r = {
      id: id(v.id),
      requestId: id(v.requestId),
      draft: d,
      state: v.state,
      phase: v.phase,
      reason: text(v.reason),
      createdAt: num(v.createdAt),
      finishedAt: v.finishedAt === null ? null : num(v.finishedAt),
      receipt: null as RetirementRecord['receipt'],
      reviewedAt: v.reviewedAt === null ? null : num(v.reviewedAt),
    }
  if (v.receipt !== null) {
    const p = obj(v.receipt),
      decimal = (n: unknown) =>
        typeof n === 'string' && /^[1-9][0-9]{0,19}$/.test(n) && BigInt(n) <= 2n ** 64n - 1n
          ? n
          : fail()
    r.receipt = {
      destination: path(p.destination),
      device: decimal(p.device),
      inode: decimal(p.inode),
    }
  }
  if (
    r.id !== d.id ||
    !['running', 'moved', 'retained', 'cancelled', 'unknown'].includes(String(r.state)) ||
    !['checking', 'intent', 'finished'].includes(String(r.phase)) ||
    r.createdAt < d.createdAt ||
    (r.finishedAt !== null && r.finishedAt < r.createdAt) ||
    (r.reviewedAt !== null && r.reviewedAt < r.createdAt) ||
    (r.state === 'running') !== (r.finishedAt === null) ||
    (r.phase === 'finished') !== (r.state !== 'running') ||
    (r.state === 'moved' && !r.receipt) ||
    (request && r.requestId !== request) ||
    (draft && !sameDraft(d, draft))
  )
    fail()
  return r as RetirementRecord
}
export function retirementSnapshot(raw: unknown) {
  const v = obj(raw)
  if (!Array.isArray(v.records) || v.records.length > 64 || !Array.isArray(v.activeRequests)) fail()
  const records = (v.records as unknown[]).map((r) => retirementRecord(r)),
    activeRequests = (v.activeRequests as unknown[]).map(id)
  if (
    new Set(records.map((r) => r.id)).size !== records.length ||
    new Set(activeRequests).size !== activeRequests.length ||
    activeRequests.some(
      (id) => !records.some((r) => r.requestId === id && r.state === 'running'),
    ) ||
    records.some((r) => r.state === 'running' && !activeRequests.includes(r.requestId))
  )
    fail()
  return { records, activeRequests }
}
export const archiveRetirement = {
  prepare: async (header: ArchiveHeader, requestId: string) => {
    const raw = await invoke('prepare_archive_retirement', { archiveId: header.id, requestId })
    return raw === null ? null : retirementDraft(raw, header)
  },
  execute: async (draft: RetirementDraft, requestId: string) =>
    retirementRecord(
      await invoke('execute_archive_retirement', { id: draft.id, requestId, confirmed: true }),
      draft,
      requestId,
    ),
  discard: (id: string) => invoke('discard_archive_retirement', { id }),
  get: async () => retirementSnapshot(await invoke('get_archive_retirements')),
  acknowledge: (id: string) => invoke('acknowledge_archive_retirement', { id }),
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
}
