import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import type {
  Bootstrap,
  Settings,
  TaskEvent,
  TaskKind,
  TaskRecord,
  TaskResults,
  ReviewPlan,
  ReviewPlanSummary,
  ExecutionSummary,
  ExecutionDetail,
  UpdateCheckResult,
  UpdateSnapshot,
} from '../types'
import { demoBootstrap } from './demo'
import { observeTask, type TaskTransport } from './task-watcher'
import { taskResult } from './task-result'

export const desktop = isTauri()
export const bootstrap = (): Promise<Bootstrap> =>
  desktop ? invoke('bootstrap') : Promise.resolve(structuredClone(demoBootstrap))
export const history = (): Promise<TaskRecord[]> =>
  desktop ? invoke('get_history') : Promise.resolve([])
export const prepareTaskDiagnostic = (taskId: string): Promise<string> =>
  desktop
    ? invoke('prepare_task_diagnostic', { taskId })
    : Promise.reject(new Error('请在桌面 App 中生成诊断摘要。'))
export const saveTaskDiagnostic = (report: string): Promise<boolean> =>
  desktop
    ? invoke('save_task_diagnostic', { report })
    : Promise.reject(new Error('请在桌面 App 中保存诊断摘要。'))
export const executions = (): Promise<ExecutionSummary[]> =>
  desktop ? invoke('list_executions') : Promise.resolve([])
export const execution = (runId: string): Promise<ExecutionDetail> =>
  desktop
    ? invoke('get_execution', { runId })
    : Promise.reject(new Error('请在桌面 App 中查看执行记录。'))
export const saveSettings = (settings: Settings): Promise<Settings> =>
  desktop ? invoke('save_settings', { settings }) : Promise.resolve(settings)
export const reveal = (path: string): Promise<void> =>
  desktop
    ? invoke('reveal_path', { path })
    : Promise.reject(new Error('请在桌面 App 中打开 Finder。'))
export const official = (): Promise<void> =>
  desktop
    ? invoke('open_official')
    : Promise.resolve(void window.open('https://mole.fit/zh/', '_blank', 'noopener,noreferrer'))
export const checkUpdates = (): Promise<UpdateCheckResult> =>
  desktop ? invoke('check_updates') : Promise.reject(new Error('请在桌面 App 中检查更新。'))
export const updateSnapshot = (): Promise<UpdateSnapshot> => invoke('get_update_snapshot')
export const downloadUpdate = (): Promise<UpdateSnapshot> => invoke('download_update')
export const cancelUpdateDownload = (): Promise<void> => invoke('cancel_update_download')
export const revealUpdateDownload = (): Promise<void> => invoke('reveal_update_download')
export const installUpdate = (): Promise<void> => invoke('install_update')
export const openUpdateReleases = (): Promise<void> =>
  desktop
    ? invoke('open_update_releases')
    : Promise.resolve(
        void window.open(
          'https://github.com/rememberber/MooMole/releases',
          '_blank',
          'noopener,noreferrer',
        ),
      )
export const cancelTask = (taskId: string): Promise<void> => invoke('cancel_task', { taskId })
export const reviewPlans = (): Promise<ReviewPlanSummary[]> =>
  desktop ? invoke('list_review_plans') : Promise.resolve([])
export const createReviewPlan = (scanId: string, selectedIds: string[]): Promise<ReviewPlan> =>
  desktop
    ? invoke('create_review_plan', { scanId, selectedIds })
    : Promise.reject(new Error('请在桌面 App 中保存审阅计划。'))
export const reviewPlan = (planId: string): Promise<ReviewPlan> =>
  desktop
    ? invoke('get_review_plan', { planId })
    : Promise.reject(new Error('请在桌面 App 中复核计划。'))
export async function checkReviewRules(planId: string, signal: AbortSignal): Promise<ReviewPlan> {
  if (!desktop) throw new Error('请在桌面 App 中复查 Mole 规则。')
  if (signal.aborted) throw new Error('规则复查已取消。')
  const requestId = crypto.randomUUID()
  const cancel = () => {
    void cancelTask(requestId).catch(() => {})
  }
  signal.addEventListener('abort', cancel, { once: true })
  try {
    const result = await invoke<ReviewPlan>('check_review_rules', { planId, requestId })
    if (signal.aborted) throw new Error('规则复查已取消。')
    return result
  } finally {
    signal.removeEventListener('abort', cancel)
  }
}
export const chooseDirectory = async (): Promise<string | null> => {
  if (!desktop) return null
  const selected = await open({ directory: true, multiple: false, title: '选择要检查的文件夹' })
  return typeof selected === 'string' ? selected : null
}

export async function runTask<K extends TaskKind>(
  kind: K,
  path: string | undefined,
  onStart: (id: string) => void,
  signal?: AbortSignal,
): Promise<TaskResults[K]> {
  if (!desktop) throw new Error('浏览器预览不能读取本机，请运行 npm run desktop。')
  const taskId = crypto.randomUUID()
  const event = await observeTask(taskTransport, {
    id: taskId,
    kind,
    signal,
    start: () => invoke('start_task', { taskId, kind, path: path ?? null }),
    onStart: () => onStart(taskId),
  })
  return taskResult<K>(event)
}

const taskTransport: TaskTransport = {
  listen: (receive) => listen<TaskEvent>('task-event', ({ payload }) => receive(payload)),
  snapshot: (taskId) => invoke<TaskEvent>('get_task_snapshot', { taskId }),
}

export async function resumeTask<K extends TaskKind>(
  taskId: string,
  kind: K,
  signal?: AbortSignal,
): Promise<TaskResults[K]> {
  if (!desktop) throw new Error('浏览器预览不能跟踪本机任务。')
  return taskResult<K>(await observeTask(taskTransport, { id: taskId, kind, signal }))
}
