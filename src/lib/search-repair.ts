import { invoke } from '@tauri-apps/api/core'
import { chooseDirectory } from './api'
export interface SearchRepairDraft {
  id: string
  path: string
  createdAt: number
  expiresAt: number
}
export interface SearchRepairReport {
  requestId: string
  path: string
  attempt: {
    step: 'importFolder'
    outcome: 'notStarted' | 'commandSucceeded' | 'uncertain'
    exitCode: number | null
    stop: 'cancelled' | 'timedOut' | 'supervisionFailed' | null
    reason: string
  }
}
export const searchRepairTransport = {
  choose: () => chooseDirectory('选择搜索不到文件的目录'),
  async prepare(path: string): Promise<SearchRepairDraft> {
    const d = await invoke<SearchRepairDraft>('prepare_search_repair', { path })
    if (
      !d ||
      !/^[0-9a-f-]{36}$/.test(d.id) ||
      d.path !== path ||
      !Number.isSafeInteger(d.createdAt) ||
      d.expiresAt !== d.createdAt + 120_000
    )
      throw Error('索引确认无法核对，请重新选择目录。')
    return d
  },
  discard: (id: string) => invoke('discard_search_repair', { id }),
  async execute(draft: SearchRepairDraft, requestId: string): Promise<SearchRepairReport> {
    const r = await invoke<SearchRepairReport>('execute_search_repair', {
      id: draft.id,
      requestId,
      confirmed: true,
    })
    if (
      !r ||
      r.requestId !== requestId ||
      r.path !== draft.path ||
      r.attempt?.step !== 'importFolder' ||
      !['notStarted', 'commandSucceeded', 'uncertain'].includes(r.attempt.outcome) ||
      ![null, 'cancelled', 'timedOut', 'supervisionFailed'].includes(r.attempt.stop) ||
      (r.attempt.exitCode !== null && !Number.isInteger(r.attempt.exitCode)) ||
      typeof r.attempt.reason !== 'string' ||
      (r.attempt.outcome === 'commandSucceeded' &&
        (r.attempt.exitCode !== 0 || r.attempt.stop !== null))
    )
      throw Error('索引结果无法核对，请检查实际搜索状态；不会自动重试。')
    return r
  },
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
}
export type SearchRepairTransport = typeof searchRepairTransport
