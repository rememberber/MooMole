import { invoke } from '@tauri-apps/api/core'
export interface FolderWatch {
  id: string
  root: string
  enabled: boolean
  intervalMinutes: number
  updatedAt: number
  lastAttemptAt: number | null
}
export interface FolderScheduleEdit {
  id: string | null
  root: string
  enabled: boolean
  intervalMinutes: number
}
export type FolderScanState =
  'running' | 'completed' | 'partial' | 'cancelled' | 'failed' | 'superseded' | 'interrupted'
export interface FolderScanSummary {
  complete: boolean
  visited: number
  skipped: number
  files: number
  logicalBytes: number
  comparison: 'first' | 'compared' | 'reset' | 'unavailable'
  added: number | null
  changed: number | null
  notSeen: number | null
  logicalDeltaBytes: number | null
  changes: { path: string; kind: 'added' | 'changed' | 'notSeen' }[]
  changesTruncated: boolean
}
export interface FolderScanRecord {
  version: 1
  id: string
  watchId: string
  generation: string
  root: string
  trigger: 'manual' | 'scheduled'
  startedAt: number
  finishedAt: number | null
  state: FolderScanState
  summary: FolderScanSummary | null
  error: string | null
}
export interface FolderSchedulesView {
  watches: FolderWatch[]
  records: FolderScanRecord[]
  activeRequests: string[]
  error: string | null
}
const uuid = (v: unknown): v is string =>
  typeof v === 'string' && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(v)
const path = (v: unknown): v is string =>
  typeof v === 'string' &&
  v.length <= 4096 &&
  v.startsWith('/') &&
  !/[\x00-\x1f\x7f]/.test(v) &&
  v
    .slice(1)
    .split('/')
    .every((p) => p && p !== '.' && p !== '..')
const n = (v: unknown): v is number => Number.isSafeInteger(v) && (v as number) >= 0
const fail = (): never => {
  throw Error('目录定期扫描数据无法核对，未发起新的扫描或文件操作。')
}
export function readFolderWatch(raw: unknown): FolderWatch {
  const w = raw as FolderWatch
  if (
    !w ||
    !uuid(w.id) ||
    !path(w.root) ||
    typeof w.enabled !== 'boolean' ||
    ![15, 60, 360, 1440].includes(w.intervalMinutes) ||
    !n(w.updatedAt) ||
    (w.lastAttemptAt !== null && !n(w.lastAttemptAt))
  )
    fail()
  return w
}
export function readFolderSchedules(raw: unknown): FolderSchedulesView {
  const v = raw as FolderSchedulesView
  if (
    !v ||
    !Array.isArray(v.watches) ||
    v.watches.length > 8 ||
    !Array.isArray(v.records) ||
    v.records.length > 64 ||
    !Array.isArray(v.activeRequests) ||
    v.activeRequests.length > 1 ||
    v.activeRequests.some((r) => !uuid(r)) ||
    (v.error !== null && typeof v.error !== 'string')
  )
    fail()
  v.watches.forEach(readFolderWatch)
  if (
    new Set(v.watches.map((w) => w.id)).size !== v.watches.length ||
    new Set(v.records.map((r) => r.id)).size !== v.records.length
  )
    fail()
  for (const r of v.records) {
    if (
      !r ||
      r.version !== 1 ||
      !uuid(r.id) ||
      !uuid(r.watchId) ||
      !uuid(r.generation) ||
      !path(r.root) ||
      !['manual', 'scheduled'].includes(r.trigger) ||
      !n(r.startedAt) ||
      (r.finishedAt !== null && !n(r.finishedAt)) ||
      (r.error !== null && typeof r.error !== 'string') ||
      ![
        'running',
        'completed',
        'partial',
        'cancelled',
        'failed',
        'superseded',
        'interrupted',
      ].includes(r.state)
    )
      fail()
    if ((r.state === 'running') !== (r.finishedAt === null)) fail()
    const s = r.summary
    if (s !== null) {
      if (
        !s ||
        typeof s.complete !== 'boolean' ||
        !n(s.visited) ||
        s.visited > 10000 ||
        !n(s.skipped) ||
        !n(s.files) ||
        s.files > 2000 ||
        !n(s.logicalBytes) ||
        !['first', 'compared', 'reset', 'unavailable'].includes(s.comparison) ||
        typeof s.changesTruncated !== 'boolean' ||
        !Array.isArray(s.changes) ||
        s.changes.length > 100 ||
        s.changes.some(
          (c) =>
            !c ||
            !path(c.path) ||
            !c.path.startsWith(r.root + '/') ||
            !['added', 'changed', 'notSeen'].includes(c.kind),
        )
      )
        fail()
      const compared = s.comparison === 'compared'
      if (compared) {
        if (
          !s.complete ||
          ![s.added, s.changed, s.notSeen].every((x) => n(x) && x <= 2000) ||
          !Number.isSafeInteger(s.logicalDeltaBytes)
        )
          fail()
        const total = s.added! + s.changed! + s.notSeen!
        if (s.changes.length !== Math.min(100, total) || s.changesTruncated !== total > 100) fail()
      } else if (
        s.added !== null ||
        s.changed !== null ||
        s.notSeen !== null ||
        s.logicalDeltaBytes !== null ||
        s.changes.length ||
        s.changesTruncated
      )
        fail()
      if (!s.complete && s.comparison !== 'unavailable') fail()
    }
    if (r.state === 'completed' && !s?.complete) fail()
    if (['running', 'failed', 'superseded', 'interrupted'].includes(r.state) && s !== null) {
      // Interrupted observations may retain only an already saved terminal
      // summary in a future protocol; v1 never stores one while running.
      fail()
    }
  }
  return v
}
export const folderScheduleTransport = {
  read: () => invoke('get_folder_schedules'),
  configure: async (edit: FolderScheduleEdit) => {
    const w = readFolderWatch(await invoke('configure_folder_schedule', { edit }))
    if (
      (edit.id !== null && w.id !== edit.id) ||
      w.root !== edit.root ||
      w.enabled !== edit.enabled ||
      w.intervalMinutes !== edit.intervalMinutes
    )
      fail()
    return w
  },
  remove: (id: string) => invoke<void>('remove_folder_schedule', { id }),
  scan: (watchId: string, requestId: string) =>
    invoke<void>('start_folder_scan', { watchId, requestId }),
  cancel: (taskId: string) => invoke<void>('cancel_task', { taskId }),
}
export type FolderScheduleTransport = typeof folderScheduleTransport
export const folderScanStates: Record<FolderScanState, string> = {
  running: '正在扫描目录',
  completed: '本次扫描已完成',
  partial: '仅扫描了部分内容',
  cancelled: '目录扫描已取消',
  failed: '目录扫描未完成',
  superseded: '设置变化，旧结果未保存',
  interrupted: '目录扫描曾中断',
}
