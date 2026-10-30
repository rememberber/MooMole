import { invoke } from '@tauri-apps/api/core'
import type { ExecutionDetail } from '../types'
import type { RestorationTarget } from './file-restoration'
export interface RestorationBatchTarget {
  runId: string
  items: RestorationTarget[]
}
export interface RestorationBatchDraft {
  id: string
  sourceRunId: string
  createdAt: number
  expiresAt: number
  items: { itemId: string; sourceItemId: string; currentPath: string; originalPath: string }[]
}
const uuid = (s: unknown): s is string =>
  typeof s === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(s)
const identity = (s: unknown) =>
  typeof s === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(s) && BigInt(s) <= 18446744073709551615n
export const restorationBatchTransport = {
  async prepare(target: RestorationBatchTarget): Promise<RestorationBatchDraft> {
    const d = await invoke<RestorationBatchDraft>('prepare_file_restoration_batch', {
      runId: target.runId,
      itemIds: target.items.map((i) => i.itemId),
    })
    if (
      !d ||
      !uuid(d.id) ||
      d.sourceRunId !== target.runId ||
      !Number.isSafeInteger(d.createdAt) ||
      d.createdAt > Date.now() ||
      d.expiresAt !== d.createdAt + 30_000 ||
      d.expiresAt <= Date.now() ||
      !Array.isArray(d.items) ||
      d.items.length !== target.items.length ||
      !d.items.length ||
      d.items.length > 20 ||
      new Set(d.items.map((i) => i.itemId)).size !== d.items.length ||
      d.items.some(
        (i, n) =>
          !uuid(i.itemId) ||
          i.sourceItemId !== target.items[n].itemId ||
          i.currentPath !== target.items[n].currentPath ||
          i.originalPath !== target.items[n].originalPath,
      )
    )
      throw Error('批量恢复确认无法核对，请重新选择；不会操作文件。')
    return d
  },
  discard: (id: string) => invoke('discard_file_restoration', { id }),
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
  async execute(draft: RestorationBatchDraft, requestId: string): Promise<ExecutionDetail> {
    const r = await invoke<ExecutionDetail>('execute_file_restoration_batch', {
      id: draft.id,
      requestId,
      confirmed: true,
    })
    if (
      !r ||
      !uuid(r.id) ||
      r.origin !== 'fileRestoration' ||
      r.planId !== draft.id ||
      r.method !== 'move' ||
      !['completed', 'partial', 'noChanges', 'cancelled', 'interrupted'].includes(r.state) ||
      !Array.isArray(r.items) ||
      r.items.length !== draft.items.length ||
      r.itemCount !== draft.items.length ||
      r.movedCount !== r.items.filter((i) => i.state === 'moved').length ||
      r.uncertainCount !== r.items.filter((i) => i.state === 'uncertain').length ||
      new Set(r.items.map((i) => i.id)).size !== r.items.length ||
      r.items.some((i) => {
        const d = draft.items.find((d) => d.itemId === i.id)
        return (
          !d ||
          i.path !== d.currentPath ||
          i.targetPath !== d.originalPath ||
          !['moved', 'skipped', 'failed', 'cancelled', 'notStarted', 'uncertain'].includes(
            i.state,
          ) ||
          (i.state === 'moved' &&
            (i.receipt?.destination !== d.originalPath ||
              !identity(i.receipt.device) ||
              !identity(i.receipt.inode) ||
              i.receipt.inode === '0'))
        )
      })
    )
      throw Error('批量恢复结果无法核对，请在历史检查实际状态；不会重复恢复。')
    return r
  },
}
