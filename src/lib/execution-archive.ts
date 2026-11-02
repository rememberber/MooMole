import { invoke } from '@tauri-apps/api/core'
import {
  isJournalKind,
  readJournalSummary,
  readJournalRecord,
  journalSummary,
  type JournalKind,
  type JournalSummary,
  type JournalRecord,
} from './archived-journals'
import type { ExecutionDetail, ExecutionSummary } from '../types'
import {
  parseUninstallDetail,
  parseUninstallSummary,
  type UninstallDetail,
  type UninstallSummary,
} from './app-uninstall'
import {
  readRefreshDetail,
  readRefreshSummary,
  type RefreshDetail,
  type RefreshSummary,
} from './quicklook-refresh'
export type ArchiveKind = 'cleanup' | 'uninstall' | 'maintenance' | JournalKind
export type ArchiveCounts = {
  cleanup: number
  uninstall: number
  maintenance: number
  moleClean?: number
  extraction?: number
  managedApp?: number
}
export type ArchiveHeader = {
  version: 1 | 2 | 3 | 4 | 5
  id: string
  createdAt: number
  counts: ArchiveCounts
  payloadSha256: string
  payloadBytes: number
}
export type ArchivePreview = { counts: ArchiveCounts; fingerprint: string; reservedBytes: number }
export type ArchiveEntry = {
  kind: ArchiveKind
  id: string
  summary: ExecutionSummary | UninstallSummary | RefreshSummary | JournalSummary
}
export type ArchiveView = { header: ArchiveHeader; entries: ArchiveEntry[] }
export type ArchiveDetail = ExecutionDetail | UninstallDetail | RefreshDetail | JournalRecord
export type ArchiveTransport = {
  prepare: () => Promise<unknown>
  archive: (fingerprint: string) => Promise<unknown>
  list: () => Promise<unknown>
  view: (id: string) => Promise<unknown>
  detail: (archiveId: string, kind: ArchiveKind, runId: string) => Promise<unknown>
  import: () => Promise<unknown>
  export: (id: string) => Promise<boolean>
  reveal: () => Promise<void>
}
export const archiveTransport: ArchiveTransport = {
  prepare: () => invoke('prepare_execution_archive'),
  archive: (fingerprint) => invoke('archive_execution_history', { fingerprint, confirmed: true }),
  list: () => invoke('list_execution_archives'),
  view: (archiveId) => invoke('read_execution_archive', { archiveId }),
  detail: (archiveId, kind, runId) => invoke('get_archived_execution', { archiveId, kind, runId }),
  import: () => invoke('import_execution_archive'),
  export: (archiveId) => invoke('export_execution_archive', { archiveId }),
  reveal: () => invoke('reveal_execution_archives'),
}
const fail = (): never => {
  throw Error('归档数据无法核对，请保留原文件并重试读取。')
}
const object = (v: unknown): Record<string, unknown> =>
  v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : fail()
const integer = (v: unknown): number =>
  typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? v : fail()
const id = (v: unknown): string =>
  typeof v === 'string' && /^[a-f\d]{8}(-[a-f\d]{4}){3}-[a-f\d]{12}$/.test(v) ? v : fail()
const digest = (v: unknown): string =>
  typeof v === 'string' && /^[a-f\d]{64}$/.test(v) ? v : fail()
export function counts(raw: unknown): ArchiveCounts {
  const v = object(raw),
    result = {
      cleanup: integer(v.cleanup),
      uninstall: integer(v.uninstall),
      maintenance: integer(v.maintenance),
    }
  for (const key of ['moleClean', 'extraction', 'managedApp'] as const) {
    const n = v[key] === undefined ? 0 : integer(v[key])
    if (n) (result as ArchiveCounts)[key] = n
  }
  const total = Object.values(result).reduce((a, b) => a + b, 0)
  if (total < 1 || total > 64) fail()
  return result
}
export function archiveHeader(raw: unknown): ArchiveHeader {
  const v = object(raw)
  if (
    v.format !== 'MooMole execution evidence' ||
    ![1, 2, 3, 4, 5].includes(v.version as number) ||
    integer(v.payloadBytes) > 128 * 1024 * 1024 ||
    v.payloadBytes === 0
  )
    fail()
  const scope = counts(v.counts),
    extra = (scope.moleClean ?? 0) + (scope.extraction ?? 0) + (scope.managedApp ?? 0)
  if (
    ((v.version as number) >= 4 && extra === 0) ||
    ((v.version as number) < 4 && extra > 0) ||
    (v.version === 5 && !(scope.managedApp ?? 0))
  )
    fail()
  return {
    version: v.version as ArchiveHeader['version'],
    id: id(v.id),
    createdAt: integer(v.createdAt),
    counts: scope,
    payloadSha256: digest(v.payloadSha256),
    payloadBytes: integer(v.payloadBytes),
  }
}
export function archivePreview(raw: unknown): ArchivePreview {
  const v = object(raw),
    reservedBytes = integer(v.reservedBytes)
  if (reservedBytes < 1 || reservedBytes > 64 * 1024 * 1024) fail()
  return { counts: counts(v.counts), fingerprint: digest(v.fingerprint), reservedBytes }
}
export function archiveList(raw: unknown): {
  archives: ArchiveHeader[]
  complete: boolean
  unreadable: number
} {
  const v = object(raw)
  if (!Array.isArray(v.archives) || v.archives.length > 1024 || typeof v.complete !== 'boolean')
    return fail()
  const archives = v.archives.map(archiveHeader)
  if (new Set(archives.map((h) => h.id)).size !== archives.length) fail()
  return { archives, complete: v.complete, unreadable: integer(v.unreadable) }
}
function cleanupSummary(raw: unknown): ExecutionSummary {
  const v = object(raw)
  if (
    !['trash', 'move'].includes(String(v.method)) ||
    !['completed', 'partial', 'noChanges', 'cancelled', 'interrupted'].includes(String(v.state))
  )
    fail()
  id(v.id)
  id(v.planId)
  integer(v.createdAt)
  integer(v.finishedAt)
  const total = integer(v.itemCount),
    moved = integer(v.movedCount),
    unknown = integer(v.uncertainCount)
  if (!total || total > 2048 || moved + unknown > total || (unknown > 0 && v.reviewedAt === null))
    fail()
  if (v.reviewedAt !== null) integer(v.reviewedAt)
  return v as unknown as ExecutionSummary
}
export function archiveView(raw: unknown, expectedId: string): ArchiveView {
  const v = object(raw),
    header = archiveHeader(v.header)
  if (header.id !== expectedId || !Array.isArray(v.entries) || v.entries.length > 64) return fail()
  const found: ArchiveCounts = { cleanup: 0, uninstall: 0, maintenance: 0 }
  const entries: ArchiveEntry[] = v.entries.map((raw): ArchiveEntry => {
    const row = object(raw),
      kind = row.kind
    if (
      kind !== 'cleanup' &&
      kind !== 'uninstall' &&
      kind !== 'maintenance' &&
      !isJournalKind(kind)
    )
      return fail()
    if (isJournalKind(kind) && header.version < 4) fail()
    const summary = isJournalKind(kind)
      ? readJournalSummary(row.summary, kind)
      : kind === 'cleanup'
        ? cleanupSummary(row.summary)
        : kind === 'uninstall'
          ? parseUninstallSummary(row.summary)
          : readRefreshSummary(row.summary)
    if (
      kind === 'cleanup' &&
      'method' in summary &&
      summary.method === 'move' &&
      header.version < 3
    )
      fail()
    if ('operation' in summary && summary.operation === 'upgrade' && header.version < 5) fail()
    if (
      summary.id !== id(row.id) ||
      summary.state === 'running' ||
      summary.finishedAt === null ||
      (summary.reviewedAt === null &&
        (('uncertainCount' in summary && summary.uncertainCount > 0) ||
          ('itemState' in summary && summary.itemState === 'uncertain')))
    )
      fail()
    found[kind] = (found[kind] ?? 0) + 1
    return { kind, id: summary.id, summary }
  })
  if (
    new Set(entries.map((e) => e.kind + e.id)).size !== entries.length ||
    (
      ['cleanup', 'uninstall', 'maintenance', 'moleClean', 'extraction', 'managedApp'] as const
    ).some((k) => (found[k] ?? 0) !== (header.counts[k] ?? 0))
  )
    fail()
  if (
    (header.version === 5) !==
    entries.some((e) => 'operation' in e.summary && e.summary.operation === 'upgrade')
  )
    fail()
  return { header, entries }
}
export function archiveDetail(raw: unknown, entry: ArchiveEntry): ArchiveDetail {
  const v = object(raw)
  if (isJournalKind(entry.kind)) {
    const record = readJournalRecord(v, entry.kind),
      summary = journalSummary(entry.kind, record)
    if (
      record.id !== entry.id ||
      Object.entries(entry.summary).some(
        ([k, value]) =>
          JSON.stringify(summary[k as keyof JournalSummary]) !== JSON.stringify(value),
      )
    )
      fail()
    return record
  }
  const parsed =
    entry.kind === 'uninstall'
      ? parseUninstallDetail(v, entry.id)
      : entry.kind === 'maintenance'
        ? readRefreshDetail(v)
        : (cleanupSummary(v), v as unknown as ExecutionDetail)
  if (
    parsed.id !== entry.id ||
    parsed.state === 'running' ||
    parsed.finishedAt === null ||
    Object.entries(entry.summary).some(
      ([k, value]) => JSON.stringify(v[k]) !== JSON.stringify(value),
    )
  )
    fail()
  digest(v.snapshotSha256)
  return parsed
}
export const archiveKindName: Record<ArchiveKind, string> = {
  cleanup: '清理',
  uninstall: '卸载',
  maintenance: '维护',
  moleClean: '完整清理',
  extraction: '压缩包解压',
  managedApp: 'Homebrew 卸载',
}
export const archiveStateName = (state: string, method?: string) =>
  method === 'move' && state === 'moved'
    ? '已移动到目标目录'
    : ({
        completed: '已完成',
        partial: '部分完成',
        noChanges: '未作修改',
        cancelled: '已取消',
        interrupted: '已中断',
        needsReview: '结果待核实',
        moved: '已移入废纸篓',
        uncertain: '结果待核实',
        commandSucceeded: '命令完成',
        notStarted: '未开始',
        failed: '失败，未作修改',
        skipped: '已保留',
        pending: '未开始',
        intent: '等待完成证据',
      }[state] ?? '状态待核实')

export const archiveEntryStateName = (kind: ArchiveKind, state: string) =>
  kind === 'managedApp' && state === 'upgraded'
    ? '已核对新版包登记、App 版本与签名'
    : kind === 'moleClean' && state === 'failed'
      ? '执行失败，需核对已发生的结果'
      : kind === 'managedApp' && state === 'removed'
        ? '包与原 App 已核对不再存在'
        : ({ retained: '已保留', unknown: '结果待核实' }[state] ?? archiveStateName(state))
