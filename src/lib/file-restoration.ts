import { invoke } from '@tauri-apps/api/core'
import type { ExecutionDetail } from '../types'
export interface RestorationTarget {
  runId: string
  itemId: string
  currentPath: string
  originalPath: string
}
export interface RestorationDraft {
  id: string
  sourceRunId: string
  sourceItemId: string
  currentPath: string
  originalPath: string
  expiresAt: number
}
export const restorationTransport = {
  async prepare(target: RestorationTarget): Promise<RestorationDraft> {
    const d = await invoke<RestorationDraft>('prepare_file_restoration', {
      runId: target.runId,
      itemId: target.itemId,
    })
    if (
      !d ||
      !/^[0-9a-f-]{36}$/.test(d.id) ||
      d.sourceRunId !== target.runId ||
      d.sourceItemId !== target.itemId ||
      d.currentPath !== target.currentPath ||
      d.originalPath !== target.originalPath ||
      !Number.isSafeInteger(d.expiresAt) ||
      d.expiresAt <= Date.now() ||
      d.expiresAt > Date.now() + 30_000
    )
      throw Error('恢复确认与原记录不一致，请重新检查；不会操作文件。')
    return d
  },
  discard: (id: string) => invoke('discard_file_restoration', { id }),
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
  async execute(draft: RestorationDraft, requestId: string): Promise<ExecutionDetail> {
    const r = await invoke<ExecutionDetail>('execute_file_restoration', {
      id: draft.id,
      requestId,
      confirmed: true,
    })
    if (
      !r ||
      r.origin !== 'fileRestoration' ||
      r.planId !== draft.id ||
      r.method !== 'move' ||
      !['completed', 'partial', 'noChanges', 'cancelled', 'interrupted'].includes(r.state) ||
      !Array.isArray(r.items) ||
      r.items.length !== 1 ||
      r.items[0].path !== draft.currentPath ||
      r.items[0].targetPath !== draft.originalPath ||
      !['moved', 'skipped', 'failed', 'cancelled', 'notStarted', 'uncertain'].includes(
        r.items[0].state,
      ) ||
      (r.items[0].state === 'moved' && r.items[0].receipt?.destination !== draft.originalPath)
    )
      throw Error('恢复结果无法核对，请在操作历史检查实际状态；不会重复恢复。')
    return r
  },
}
export type RestorationTransport = typeof restorationTransport
