// Read-only archive validation. These values never become live plans or requests.
import { readManagedRecord, type ManagedRecord } from './managed-apps'
import { readZipExtractionRecord, type ZipExtractionRecord } from './zip-extraction'
import { moleCleanScopes, type MoleCleanRecord } from './mole-cleanup'
export type JournalKind = 'moleClean' | 'extraction' | 'managedApp'
export type JournalRecord = MoleCleanRecord | ZipExtractionRecord | ManagedRecord
export interface JournalSummary {
  id: string
  state: string
  createdAt: number
  finishedAt: number
  reviewedAt: number | null
  name: string
  path: string
  reason: string
  resultUncertain: boolean
  operation?: 'upgrade'
  macMetadata?: true
}
const fail = (): never => {
  throw Error('归档扩展记录无法核对；不会恢复执行或重新调用命令。')
}
const obj = (v: unknown): Record<string, unknown> =>
  v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : fail()
const id = (v: unknown): v is string =>
  typeof v === 'string' && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(v)
const integer = (v: unknown): v is number => Number.isSafeInteger(v) && (v as number) >= 0
const text = (v: unknown, max: number): v is string =>
  typeof v === 'string' && v.length <= max && !v.includes('\0')
const path = (v: unknown): v is string =>
  text(v, 4096) && v.startsWith('/') && !v.split('/').some((s) => s === '.' || s === '..')
export const isJournalKind = (v: unknown): v is JournalKind =>
  ['moleClean', 'extraction', 'managedApp'].includes(v as string)
function finished(v: Record<string, unknown>, unknown: boolean) {
  if (
    !id(v.id) ||
    !integer(v.createdAt) ||
    !integer(v.finishedAt) ||
    v.finishedAt < v.createdAt ||
    v.state === 'running' ||
    (v.reviewedAt !== null && (!integer(v.reviewedAt) || v.reviewedAt < v.createdAt)) ||
    (unknown && v.reviewedAt === null)
  )
    fail()
}
export function readJournalSummary(v: unknown, kind: JournalKind): JournalSummary {
  const s = obj(v),
    states =
      kind === 'moleClean'
        ? ['completed', 'cancelled', 'failed', 'uncertain', 'interrupted']
        : kind === 'extraction'
          ? ['completed', 'retained', 'cancelled', 'unknown', 'interrupted']
          : ['removed', 'upgraded', 'retained', 'cancelled', 'unknown', 'interrupted']
  if (
    !states.includes(String(s.state)) ||
    (s.macMetadata !== undefined && (kind !== 'extraction' || s.macMetadata !== true)) ||
    (s.operation !== undefined && (kind !== 'managedApp' || s.operation !== 'upgrade')) ||
    (s.state === 'upgraded' && s.operation !== 'upgrade') ||
    (s.state === 'removed' && s.operation !== undefined) ||
    !text(s.name, 256) ||
    !s.name ||
    !path(s.path) ||
    !text(s.reason, 16384) ||
    typeof s.resultUncertain !== 'boolean' ||
    (['unknown', 'interrupted', 'uncertain'].includes(String(s.state)) && !s.resultUncertain)
  )
    fail()
  finished(s, s.resultUncertain as boolean)
  return s as unknown as JournalSummary
}
function readMoleRecord(raw: unknown): MoleCleanRecord {
  const r = obj(raw),
    p = obj(r.plan)
  if (
    p.version !== 1 ||
    !id(p.id) ||
    !text(p.revision, 40) ||
    !/^[0-9a-f]{40}$/.test(p.revision) ||
    !text(p.engineVersion, 128) ||
    !/^\d+\.\d+\.\d+(?:[-+][\w.+-]+)?$/.test(p.engineVersion) ||
    !path(p.home) ||
    typeof p.system !== 'boolean' ||
    p.method !== 'molePermanent' ||
    !integer(p.createdAt) ||
    !integer(p.expiresAt) ||
    p.expiresAt - p.createdAt !== 120000 ||
    !Array.isArray(p.protectedPaths) ||
    p.protectedPaths.length > 2048 ||
    p.protectedPaths.some((s) => !path(s)) ||
    !Array.isArray(p.scope) ||
    p.scope.length !== moleCleanScopes.length ||
    new Set(p.scope).size !== moleCleanScopes.length ||
    p.scope.some((s) => !moleCleanScopes.some(([id]) => id === s)) ||
    !['completed', 'cancelled', 'failed', 'uncertain', 'interrupted'].includes(String(r.state)) ||
    ![r.beforeBytes, r.afterBytes].every((v) => v === null || integer(v)) ||
    (r.error !== null && !text(r.error, 16384)) ||
    !text(r.diagnostic, 256 * 1024) ||
    !Array.isArray(r.events) ||
    r.events.length > 20000 ||
    r.eventsTruncated !== false
  )
    fail()
  const pending = new Map<string, string>(),
    seen = new Set<string>()
  let unknown = ['uncertain', 'interrupted'].includes(String(r.state)),
    eventBytes = 0
  for (const [index, raw] of (r.events as unknown[]).entries()) {
    const e = obj(raw)
    if (
      e.seq !== index + 1 ||
      ![
        'intent',
        'removed',
        'retained',
        'uncertain',
        'commandCompleted',
        'section',
        'upstream',
        'authorization',
        'complete',
      ].includes(String(e.kind)) ||
      !text(e.path, 4096) ||
      (e.path !== '' && !path(e.path)) ||
      !text(e.detail, 8192) ||
      (e.token !== '' && !id(e.token))
    )
      fail()
    const token = e.token as string,
      source = e.path as string
    eventBytes += new TextEncoder().encode(source + e.detail + token).length + 128
    if (eventBytes > 8 * 1024 * 1024) fail()
    if (e.kind === 'intent') {
      if (!token || seen.has(token)) fail()
      seen.add(token)
      pending.set(token, source)
    } else if (
      ['removed', 'retained', 'uncertain', 'commandCompleted'].includes(String(e.kind)) &&
      token
    ) {
      if (pending.get(token) !== source) fail()
      pending.delete(token)
    } else if (['removed', 'commandCompleted', 'uncertain'].includes(String(e.kind))) fail()
    unknown ||= e.kind === 'uncertain'
  }
  unknown ||= pending.size > 0
  finished(r, unknown)
  return r as unknown as MoleCleanRecord
}
export function readJournalRecord(raw: unknown, kind: JournalKind): JournalRecord {
  const r =
    kind === 'moleClean'
      ? readMoleRecord(raw)
      : kind === 'extraction'
        ? readZipExtractionRecord(raw)
        : readManagedRecord(raw)
  const summary = journalSummary(kind, r)
  finished(r as unknown as Record<string, unknown>, summary.resultUncertain)
  return r
}
export function journalSummary(kind: JournalKind, record: JournalRecord): JournalSummary {
  const common = {
    id: record.id,
    state: record.state,
    createdAt: record.createdAt,
    finishedAt: record.finishedAt!,
    reviewedAt: record.reviewedAt,
  }
  let resultUncertain = ['unknown', 'interrupted', 'uncertain'].includes(record.state)
  if (kind === 'moleClean') {
    const r = record as MoleCleanRecord,
      pending = new Set<string>()
    for (const e of r.events) {
      if (e.kind === 'intent') pending.add(e.token)
      else if (['removed', 'retained', 'uncertain', 'commandCompleted'].includes(e.kind))
        pending.delete(e.token)
      resultUncertain ||= e.kind === 'uncertain'
    }
    resultUncertain ||= pending.size > 0
    return { ...common, name: 'Mole', path: r.plan.home, reason: r.error ?? '', resultUncertain }
  }
  if (kind === 'extraction') {
    const r = record as ZipExtractionRecord
    return {
      ...common,
      name: r.draft.format === 'tar' ? 'TAR' : r.draft.format === 'tarGzip' ? 'TAR.GZ' : 'ZIP',
      path: r.draft.outputPath,
      reason: r.reason,
      resultUncertain,
      ...(r.draft.macMetadata ? { macMetadata: true as const } : {}),
    }
  }
  const r = record as ManagedRecord
  return {
    ...common,
    name: r.draft.token,
    path: r.draft.path,
    reason: r.reason,
    resultUncertain,
    ...(r.draft.upgrade ? { operation: 'upgrade' as const } : {}),
  }
}
