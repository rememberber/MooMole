import { invoke } from '@tauri-apps/api/core'
export interface TransferRestoreTarget {
  runId: string
  itemId: string
  originalPath: string
  copyPath: string
}
export interface TransferRestoreDraft extends TransferRestoreTarget {
  id: string
  bytes: number
  createdAt: number
  expiresAt: number
}
export interface TransferRestoreRecord {
  id: string
  requestId: string
  draft: TransferRestoreDraft
  createdAt: number
  finishedAt: number | null
  state: 'running' | 'restored' | 'retained' | 'cancelled' | 'unknown' | 'interrupted'
  phase: 'checking' | 'intent' | 'finished'
  temporaryPath: string
  receipt: { destination: string; device: string; inode: string } | null
  reason: string
  reviewedAt: number | null
}
export interface TransferRestoreView {
  records: TransferRestoreRecord[]
  activeRequests: string[]
}
const uuid = (v: unknown): v is string =>
  typeof v === 'string' && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(v)
const number = (v: unknown): v is number =>
  typeof v === 'number' && Number.isSafeInteger(v) && v >= 0
const path = (v: unknown): v is string =>
  typeof v === 'string' &&
  v.length <= 4096 &&
  v.startsWith('/') &&
  v !== '/' &&
  !/[\u0000-\u001f\u007f]/.test(v) &&
  !v.split('/').some((p) => p === '.' || p === '..')
function fail(): never {
  throw Error('跨磁盘恢复数据无法核对，请检查恢复记录和两端位置；不会自动重试。')
}
export function readTransferRestoreDraft(
  raw: unknown,
  target?: TransferRestoreTarget,
): TransferRestoreDraft {
  const d = raw as TransferRestoreDraft
  if (
    !d ||
    !uuid(d.id) ||
    !uuid(d.runId) ||
    !uuid(d.itemId) ||
    !path(d.copyPath) ||
    !path(d.originalPath) ||
    d.copyPath === d.originalPath ||
    !number(d.bytes) ||
    d.bytes > 16 * 1024 ** 3 ||
    !number(d.createdAt) ||
    !number(d.expiresAt) ||
    d.expiresAt - d.createdAt !== 120000 ||
    (target &&
      (d.runId !== target.runId ||
        d.itemId !== target.itemId ||
        d.originalPath !== target.originalPath ||
        d.copyPath !== target.copyPath))
  )
    fail()
  return {
    id: d.id,
    runId: d.runId,
    itemId: d.itemId,
    copyPath: d.copyPath,
    originalPath: d.originalPath,
    bytes: d.bytes,
    createdAt: d.createdAt,
    expiresAt: d.expiresAt,
  }
}
export function readTransferRestoreRecord(raw: unknown): TransferRestoreRecord {
  const r = raw as TransferRestoreRecord
  if (!r) fail()
  const d = readTransferRestoreDraft(r.draft),
    parent = d.originalPath.slice(0, d.originalPath.lastIndexOf('/'))
  if (
    !uuid(r.id) ||
    r.id !== d.id ||
    !uuid(r.requestId) ||
    !number(r.createdAt) ||
    r.createdAt < d.createdAt ||
    (r.finishedAt !== null && (!number(r.finishedAt) || r.finishedAt < r.createdAt)) ||
    (r.reviewedAt !== null && (!number(r.reviewedAt) || r.reviewedAt < r.createdAt)) ||
    !['running', 'restored', 'retained', 'cancelled', 'unknown', 'interrupted'].includes(r.state) ||
    !['checking', 'intent', 'finished'].includes(r.phase) ||
    typeof r.reason !== 'string' ||
    r.reason.length > 4000 ||
    r.temporaryPath !== `${parent}/.moomole-restore-${r.id}.tmp` ||
    (r.receipt !== null &&
      (!r.receipt ||
        r.receipt.destination !== d.originalPath ||
        typeof r.receipt.device !== 'string' ||
        !/^\d+$/.test(r.receipt.device) ||
        typeof r.receipt.inode !== 'string' ||
        !/^\d*[1-9]\d*$/.test(r.receipt.inode))) ||
    (r.state === 'restored' && (!r.receipt || r.phase !== 'finished' || r.finishedAt === null))
  )
    fail()
  return { ...r, draft: d }
}
export function readTransferRestoreView(raw: unknown): TransferRestoreView {
  const v = raw as TransferRestoreView
  if (
    !v ||
    !Array.isArray(v.records) ||
    v.records.length > 64 ||
    !Array.isArray(v.activeRequests) ||
    v.activeRequests.length > 3 ||
    v.activeRequests.some((i) => !uuid(i))
  )
    fail()
  const records = v.records.map(readTransferRestoreRecord)
  if (
    new Set(records.map((r) => r.id)).size !== records.length ||
    new Set(records.map((r) => r.requestId)).size !== records.length ||
    new Set(v.activeRequests).size !== v.activeRequests.length ||
    v.activeRequests.some((id) => !records.some((r) => r.requestId === id && r.state === 'running'))
  )
    fail()
  return { records, activeRequests: v.activeRequests }
}
export const transferRestoration = {
  prepare: async (target: TransferRestoreTarget, requestId: string) =>
    readTransferRestoreDraft(
      await invoke('prepare_transfer_restoration', {
        runId: target.runId,
        itemId: target.itemId,
        requestId,
      }),
      target,
    ),
  discard: (id: string) => invoke('discard_transfer_restoration', { id }),
  execute: async (draft: TransferRestoreDraft, requestId: string) => {
    const r = readTransferRestoreRecord(
      await invoke('execute_transfer_restoration', { id: draft.id, requestId, confirmed: true }),
    )
    if (
      r.id !== draft.id ||
      r.requestId !== requestId ||
      JSON.stringify(r.draft) !== JSON.stringify(draft)
    )
      fail()
    return r
  },
  read: async () => readTransferRestoreView(await invoke('get_transfer_restorations')),
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
  acknowledge: (id: string) => invoke('acknowledge_transfer_restoration', { id, confirmed: true }),
  reveal: (path: string) => invoke('reveal_path', { path }),
}
export const transferRestoreState: Record<TransferRestoreRecord['state'], string> = {
  running: '正在从跨磁盘副本恢复',
  restored: '已恢复到原位置，副本保留',
  retained: '未恢复，原副本保留',
  cancelled: '恢复已取消',
  unknown: '跨磁盘恢复结果待核实',
  interrupted: '跨磁盘恢复已中断',
}
