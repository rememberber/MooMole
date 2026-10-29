import { invoke } from '@tauri-apps/api/core'
import type { PersonalFile } from './personal-files'
export interface TransferTarget {
  scanId: string
  root: string
  items: PersonalFile[]
  destination: string
  conflict: 'skip' | 'keepBoth'
}
export interface TransferDraft {
  id: string
  scanId: string
  expiresAt: number
  directory: string
  conflict: 'skip' | 'keepBoth'
  items: PersonalFile[]
  targets: { id: string; destination: string | null }[]
}
interface Receipt {
  destination: string
  device: string
  inode: string
}
export interface TransferEntry {
  id: string
  path: string
  target: string | null
  temporaryPath: string | null
  state: string
  reason: string
  copyReceipt: Receipt | null
  sourceTrashReceipt: Receipt | null
  sha256: string | null
  metadataSha256: string | null
}
export interface TransferRecord {
  protocol: 1
  revision: string
  id: string
  requestId: string
  createdAt: number
  finishedAt: number | null
  state: string
  items: TransferEntry[]
  reviewedAt: number | null
  reason: string
}
const revision = 'MooMole.cross-volume-move.v1'
const id = (v: unknown): v is string =>
  typeof v === 'string' && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(v)
const path = (v: unknown): v is string =>
  typeof v === 'string' &&
  v.startsWith('/') &&
  v.length <= 4096 &&
  !/[\x00-\x1f\x7f]/.test(v) &&
  !v.split('/').some((p) => p === '.' || p === '..')
const number = (v: unknown) => Number.isSafeInteger(v) && (v as number) >= 0
const receipt = (r: Receipt | null) =>
  r === null ||
  (r &&
    path(r.destination) &&
    typeof r.device === 'string' &&
    /^\d+$/.test(r.device) &&
    typeof r.inode === 'string' &&
    /^[1-9]\d*$/.test(r.inode))
const hash = (v: unknown) => typeof v === 'string' && /^[0-9a-f]{64}$/.test(v)
const fail = (): never => {
  throw Error('跨磁盘数据无法核对，请在历史检查两端实际状态；不会重复移动。')
}
export function readTransferRecord(raw: unknown): TransferRecord {
  const r = raw as TransferRecord
  if (
    !r ||
    r.protocol !== 1 ||
    r.revision !== revision ||
    !id(r.id) ||
    !id(r.requestId) ||
    !number(r.createdAt) ||
    (r.finishedAt !== null && !number(r.finishedAt)) ||
    (r.reviewedAt !== null && !number(r.reviewedAt)) ||
    typeof r.reason !== 'string' ||
    ![
      'running',
      'completed',
      'partial',
      'cancelled',
      'noChanges',
      'uncertain',
      'interrupted',
    ].includes(r.state) ||
    !Array.isArray(r.items) ||
    !r.items.length ||
    r.items.length > 20 ||
    new Set(r.items.map((i) => i.id)).size !== r.items.length
  )
    fail()
  for (const i of r.items)
    if (
      !id(i.id) ||
      !path(i.path) ||
      (i.target !== null && !path(i.target)) ||
      (i.temporaryPath !== null && !path(i.temporaryPath)) ||
      ![
        'pending',
        'copying',
        'copyVerified',
        'trashIntent',
        'moved',
        'copyOnly',
        'skipped',
        'cancelled',
        'uncertain',
        'notStarted',
      ].includes(i.state) ||
      typeof i.reason !== 'string' ||
      !receipt(i.copyReceipt) ||
      !receipt(i.sourceTrashReceipt) ||
      (i.sha256 !== null && !hash(i.sha256)) ||
      (i.metadataSha256 !== null && !hash(i.metadataSha256)) ||
      (i.copyReceipt && i.copyReceipt.destination !== i.target) ||
      (i.state === 'moved' &&
        (!i.copyReceipt || !i.sourceTrashReceipt || !i.sha256 || !i.metadataSha256))
    )
      fail()
  if (r.state === 'completed' && r.items.some((i) => i.state !== 'moved')) fail()
  return r
}
export const transferTransport = {
  async strategy(scanId: string, selectedIds: string[], destination: string) {
    const r = await invoke('personal_move_strategy', { scanId, selectedIds, destination })
    if (r !== 'sameVolume' && r !== 'crossVolume') fail()
    return r
  },
  async prepare(target: TransferTarget): Promise<TransferDraft> {
    const d = await invoke<TransferDraft>('prepare_personal_transfer', {
      scanId: target.scanId,
      selectedIds: target.items.map((i) => i.id),
      destination: target.destination,
      conflict: target.conflict,
    })
    if (
      !d ||
      !id(d.id) ||
      d.scanId !== target.scanId ||
      d.directory !== target.destination ||
      d.conflict !== target.conflict ||
      !number(d.expiresAt) ||
      d.expiresAt <= Date.now() ||
      d.expiresAt > Date.now() + 30000 ||
      !Array.isArray(d.items) ||
      d.items.length !== target.items.length ||
      new Set(d.items.map((i) => i.id)).size !== d.items.length ||
      d.items.some(
        (i) =>
          !target.items.some(
            (s) =>
              s.id === i.id &&
              s.path === i.path &&
              s.size === i.size &&
              s.modifiedAt === i.modifiedAt,
          ),
      ) ||
      !Array.isArray(d.targets) ||
      d.targets.length !== d.items.length ||
      d.targets.some(
        (t, index) =>
          t.id !== d.items[index].id ||
          (t.destination !== null &&
            (!path(t.destination) ||
              t.destination.slice(0, t.destination.lastIndexOf('/')) !== d.directory)),
      )
    )
      fail()
    return d
  },
  discard: (id: string) => invoke('discard_personal_transfer', { id }),
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
  async execute(draft: TransferDraft, requestId: string): Promise<TransferRecord> {
    const r = readTransferRecord(
      await invoke('execute_personal_transfer', { id: draft.id, requestId, confirmed: true }),
    )
    if (
      r.id !== draft.id ||
      r.requestId !== requestId ||
      r.items.length !== draft.items.length ||
      r.items.some(
        (i) =>
          !draft.items.some((s) => s.id === i.id && s.path === i.path) ||
          i.target !== draft.targets.find((t) => t.id === i.id)?.destination,
      )
    )
      fail()
    return r
  },
  async read(): Promise<{ records: TransferRecord[]; activeRequests: string[] }> {
    const v = await invoke<{ records: unknown[]; activeRequests: string[] }>(
      'get_personal_transfers',
    )
    if (
      !v ||
      !Array.isArray(v.records) ||
      v.records.length > 64 ||
      !Array.isArray(v.activeRequests) ||
      v.activeRequests.length > 3 ||
      v.activeRequests.some((s) => !id(s))
    )
      fail()
    return { records: v.records.map(readTransferRecord), activeRequests: v.activeRequests }
  },
  async acknowledge(id: string): Promise<TransferRecord> {
    const r = readTransferRecord(
      await invoke('acknowledge_personal_transfer', { id, confirmed: true }),
    )
    if (r.id !== id || !r.reviewedAt) fail()
    return r
  },
}
export const transferState: Record<string, string> = {
  running: '正在跨磁盘移动',
  completed: '所选文件已完成跨磁盘移动',
  partial: '部分完成，副本或原件需核对',
  cancelled: '跨磁盘移动已取消',
  noChanges: '未完成跨磁盘移动',
  uncertain: '跨磁盘结果待核实',
  interrupted: '跨磁盘移动曾中断',
  pending: '尚未开始',
  copying: '正在复制并校验',
  copyVerified: '目标副本已校验',
  trashIntent: '等待原件处理证据',
  moved: '目标副本与原件收据已保存',
  copyOnly: '副本已建立，原件未完成移动',
  skipped: '按计划保留',
  notStarted: '未继续处理',
}
