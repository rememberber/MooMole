import { invoke } from '@tauri-apps/api/core'
import type { AppEntry } from '../types'
export interface AppUsageRow {
  index: number
  path: string
  bundleId: string
  state: 'observed' | 'running' | 'unknown' | 'protected'
  lastOpenedAt: number | null
  issue: string | null
}
export interface AppUsageReport {
  version: 1
  taskId: string
  requestId: string
  checkedAt: number
  expiresAt: number
  requestedIndices: number[]
  rows: AppUsageRow[]
  complete: boolean
  cancelled: boolean
}
export type UsageFilter = 'all' | '90' | '180' | '365' | 'unknown'
export function readAppUsage(
  value: unknown,
  taskId: string,
  requestId: string,
  indices: number[],
  apps: AppEntry[],
): AppUsageReport {
  const fail = (): never => {
    throw Error('应用使用线索与当前清单不匹配，请重新检查。')
  }
  if (!value || typeof value !== 'object') return fail()
  const r = value as AppUsageReport
  const time = (v: unknown): v is number =>
    typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 && v <= 8.64e15
  if (
    r.version !== 1 ||
    r.taskId !== taskId ||
    r.requestId !== requestId ||
    !time(r.checkedAt) ||
    !time(r.expiresAt) ||
    r.expiresAt - r.checkedAt !== 300000 ||
    !Array.isArray(r.requestedIndices) ||
    !indices.length ||
    indices.length > 512 ||
    JSON.stringify(r.requestedIndices) !== JSON.stringify(indices) ||
    new Set(indices).size !== indices.length ||
    !Array.isArray(r.rows) ||
    r.rows.length > indices.length ||
    typeof r.complete !== 'boolean' ||
    typeof r.cancelled !== 'boolean' ||
    (r.complete && (r.cancelled || r.rows.length !== indices.length))
  )
    return fail()
  for (const [i, row] of r.rows.entries()) {
    const app = apps[row?.index]
    if (
      !row ||
      row.index !== indices[i] ||
      !app ||
      row.path !== app.path ||
      row.bundleId !== app.bundle_id ||
      !['observed', 'running', 'unknown', 'protected'].includes(row.state) ||
      (row.lastOpenedAt !== null && (!time(row.lastOpenedAt) || row.lastOpenedAt > r.checkedAt)) ||
      (row.state === 'observed'
        ? row.lastOpenedAt === null || row.issue !== null
        : row.state === 'running'
          ? row.issue !== null
          : row.lastOpenedAt !== null ||
            ![
              'protected',
              'identityUnavailable',
              'identityChanged',
              'runningUnknown',
              'notReported',
              'notIndexed',
              'systemUnavailable',
              'metadataPathUnknown',
              'invalidDate',
            ].includes(row.issue ?? '')) ||
      (row.state === 'protected' && row.issue !== 'protected') ||
      (row.state === 'unknown' && row.issue === 'protected')
    )
      return fail()
  }
  return r
}
export function matchesUsage(row: AppUsageRow | undefined, filter: UsageFilter, checkedAt: number) {
  if (filter === 'all') return true
  if (filter === 'unknown') return !row || row.state === 'unknown'
  return (
    row?.state === 'observed' &&
    row.lastOpenedAt !== null &&
    checkedAt - row.lastOpenedAt >= Number(filter) * 86400000
  )
}
export const applicationUsage = {
  read: async (taskId: string, requestId: string, indices: number[], apps: AppEntry[]) =>
    readAppUsage(
      await invoke('inspect_application_usage', { taskId, requestId, indices }),
      taskId,
      requestId,
      indices,
      apps,
    ),
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
}
