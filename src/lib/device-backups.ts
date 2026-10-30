import { invoke } from '@tauri-apps/api/core'
export interface DeviceBackup {
  id: string
  path: string
  name: string
  deviceName: string | null
  product: string | null
  encrypted: boolean | null
  markedFinished: boolean
  modifiedAt: number
  size: number | null
  entries: number | null
  eligible: boolean
  reason: string
}
export interface BackupDraft {
  id: string
  expiresAt: number
  item: DeviceBackup
}
export interface BackupRecord {
  id: string
  requestId: string
  revision: string
  state: 'running' | 'moved' | 'retained' | 'cancelled' | 'unknown' | 'interrupted'
  phase: 'checking' | 'intent' | 'finished'
  createdAt: number
  finishedAt: number | null
  reviewedAt: number | null
  reason: string
  plan: { id: string; root: string; item: { row: DeviceBackup } }
  receipt: { destination: string; device: string; inode: string } | null
}
export interface BackupView {
  phase: string
  requestId: string | null
  error: string | null
  activeRequests: string[]
  report: null | {
    id: string
    root: string
    createdAt: number
    expiresAt: number
    complete: boolean
    cancelled: boolean
    reason: string
    items: DeviceBackup[]
  }
  records: BackupRecord[]
}
const uuid = (s: unknown): s is string =>
  typeof s === 'string' && /^[\da-f]{8}(?:-[\da-f]{4}){3}-[\da-f]{12}$/.test(s)
const path = (s: unknown): s is string =>
  typeof s === 'string' &&
  s.startsWith('/') &&
  s.length <= 4096 &&
  !/[\x00-\x1f\x7f]/.test(s) &&
  !s.split('/').some((p) => p === '.' || p === '..')
const integer = (n: unknown): n is number => Number.isSafeInteger(n) && (n as number) >= 0
const text = (s: unknown): s is string => typeof s === 'string' && s.length <= 4000
const optionalText = (s: unknown) => s === null || text(s)
const fail = (): never => {
  throw Error('设备备份数据无法核对，请检查实际状态；不会自动重试处理。')
}
function row(r: DeviceBackup): boolean {
  return (
    !!r &&
    uuid(r.id) &&
    path(r.path) &&
    text(r.name) &&
    optionalText(r.deviceName) &&
    optionalText(r.product) &&
    (r.encrypted === null || typeof r.encrypted === 'boolean') &&
    typeof r.markedFinished === 'boolean' &&
    integer(r.modifiedAt) &&
    (r.size === null || integer(r.size)) &&
    (r.entries === null || (integer(r.entries) && r.entries <= 500000)) &&
    typeof r.eligible === 'boolean' &&
    text(r.reason) &&
    (!r.eligible || (r.markedFinished && r.size !== null && r.entries !== null))
  )
}
export function readDeviceBackups(raw: unknown): BackupView {
  const v = raw as BackupView
  if (
    !v ||
    !['', 'scanning', 'ready', 'processing', 'failed'].includes(v.phase) ||
    !optionalText(v.error) ||
    (v.requestId !== null && !uuid(v.requestId)) ||
    !Array.isArray(v.activeRequests) ||
    v.activeRequests.length > 1 ||
    v.activeRequests.some((id) => !uuid(id) || id !== v.requestId) ||
    !Array.isArray(v.records) ||
    v.records.length > 64
  )
    fail()
  const r = v.report
  if (r !== null && (!r || typeof r !== 'object')) fail()
  if (
    r &&
    (!uuid(r.id) ||
      !path(r.root) ||
      !integer(r.createdAt) ||
      !integer(r.expiresAt) ||
      r.expiresAt <= r.createdAt ||
      r.expiresAt - r.createdAt > 900000 ||
      typeof r.complete !== 'boolean' ||
      typeof r.cancelled !== 'boolean' ||
      !text(r.reason) ||
      !Array.isArray(r.items) ||
      r.items.length > 128 ||
      new Set(r.items.map((i) => i.id)).size !== r.items.length ||
      r.items.some((i) => !row(i) || i.path.slice(0, i.path.lastIndexOf('/')) !== r.root))
  )
    fail()
  if (
    new Set(v.records.map((r) => r.id)).size !== v.records.length ||
    v.records.some(
      (r) =>
        !r ||
        !uuid(r.id) ||
        !uuid(r.requestId) ||
        r.revision !== 'MooMole.device-backups.v1' ||
        !['running', 'moved', 'retained', 'cancelled', 'unknown', 'interrupted'].includes(
          r.state,
        ) ||
        !['checking', 'intent', 'finished'].includes(r.phase) ||
        !integer(r.createdAt) ||
        (r.finishedAt !== null && (!integer(r.finishedAt) || r.finishedAt < r.createdAt)) ||
        (r.reviewedAt !== null && (!integer(r.reviewedAt) || r.reviewedAt < r.createdAt)) ||
        !text(r.reason) ||
        !r.plan ||
        r.plan.id !== r.id ||
        !path(r.plan.root) ||
        !row(r.plan.item?.row) ||
        r.plan.item.row.path.slice(0, r.plan.item.row.path.lastIndexOf('/')) !== r.plan.root ||
        (r.state === 'moved' && !r.receipt) ||
        (r.receipt !== null &&
          (!path(r.receipt.destination) ||
            !/^\d+$/.test(r.receipt.device) ||
            !/^[1-9]\d*$/.test(r.receipt.inode))),
    )
  )
    fail()
  return v
}
export const deviceBackupsTransport = {
  read: async () => readDeviceBackups(await invoke('get_device_backups')),
  scan: (directory: string | null, requestId: string) =>
    invoke('scan_device_backups', { directory, requestId }),
  async prepare(scanId: string, item: DeviceBackup): Promise<BackupDraft> {
    const d = await invoke<BackupDraft>('prepare_device_backup', { scanId, itemId: item.id })
    if (
      !d ||
      !uuid(d.id) ||
      !integer(d.expiresAt) ||
      d.expiresAt <= Date.now() ||
      d.expiresAt > Date.now() + 30000 ||
      !row(d.item) ||
      d.item.id !== item.id ||
      d.item.path !== item.path ||
      d.item.size !== item.size ||
      d.item.deviceName !== item.deviceName ||
      !d.item.eligible
    )
      fail()
    return d
  },
  discard: (id: string) => invoke('discard_device_backup', { id }),
  execute: (id: string, requestId: string) =>
    invoke('execute_device_backup', { id, requestId, confirmed: true }),
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
  acknowledge: (id: string) => invoke('acknowledge_device_backup', { id, confirmed: true }),
}
export const backupState: Record<BackupRecord['state'], string> = {
  running: '正在检查或处理',
  moved: '整份备份已移入废纸篓',
  retained: '备份保留',
  cancelled: '处理已取消',
  unknown: '结果待核实',
  interrupted: '先前处理已中断',
}
