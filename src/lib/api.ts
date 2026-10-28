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
  UpdateSnapshot,
  ApplicationInspection,
} from '../types'
import { demoBootstrap } from './demo'
import { translate } from '../i18n/translate'
import { observeTask, type TaskTransport } from './task-watcher'
import { taskResult } from './task-result'
import type { StatusTransport } from './status-watch'
import type { UninstallTransport } from './app-uninstall'
import type { RefreshTransport } from './quicklook-refresh'

export const statusTransport: StatusTransport = {
  start: (sessionId, intervalSeconds) =>
    invoke('start_status_watch', { sessionId, intervalSeconds }),
  read: (sessionId) => invoke('get_status_watch', { sessionId }),
  stop: (sessionId) => invoke('stop_status_watch', { sessionId }),
}

export const desktop = isTauri()
export const refreshTransport: RefreshTransport = {
  prepare: (selected) => invoke('prepare_quicklook_refresh', { selected }),
  discard: (confirmationId) => invoke('discard_quicklook_refresh', { confirmationId }),
  execute: (draft, requestId) =>
    invoke('execute_quicklook_refresh', {
      planId: draft.planId,
      confirmationId: draft.id,
      requestId,
      rebuildConfirmed: true,
    }),
  read: (requestId) => invoke('get_quicklook_refresh', { requestId }),
  list: () => (desktop ? invoke('list_quicklook_refreshes') : Promise.resolve([])),
  cancel: (requestId) => invoke('cancel_quicklook_refresh', { requestId }),
  acknowledge: (record) =>
    invoke('acknowledge_quicklook_refresh', {
      requestId: record.id,
      snapshotSha256: record.snapshotSha256,
      reviewed: true,
    }),
}
export const uninstallTransport: UninstallTransport = {
  prepare: (taskId, index) => invoke('prepare_application_uninstall_plan', { taskId, index }),
  review: (planId) => invoke('prepare_application_uninstall_confirmation', { planId }),
  discardPlan: (planId) => invoke('discard_application_uninstall_plan', { planId }),
  discardConfirmation: (id) => invoke('discard_application_uninstall_confirmation', { id }),
  execute: (planId, confirmationId, requestId) =>
    invoke('execute_application_uninstall', {
      planId,
      confirmationId,
      requestId,
      ordinaryAppConfirmed: true,
    }),
  read: (requestId) => invoke('get_application_uninstall', { requestId }),
  list: () => (desktop ? invoke('list_application_uninstalls') : Promise.resolve([])),
  cancel: (requestId) => invoke('cancel_application_uninstall', { requestId }),
  acknowledge: async (record) => {
    await invoke('acknowledge_execution_review', {
      kind: 'uninstall',
      runId: record.id,
      snapshotSha256: record.snapshotSha256,
      reviewed: true,
    })
    const snapshot = await invoke<{ record: unknown }>('get_application_uninstall', {
      requestId: record.id,
    })
    return snapshot.record
  },
}
export const getCleanupVisibility = (): Promise<unknown> => invoke('get_cleanup_visibility')
export const startCleanupVisibility = (): Promise<unknown> => invoke('start_cleanup_visibility')
export const cancelCleanupVisibility = (requestId: string): Promise<unknown> =>
  invoke('cancel_cleanup_visibility', { requestId })
export const inspectInstallerImage = (root: string, path: string): Promise<unknown> =>
  desktop
    ? invoke('inspect_installer_image', { root, path })
    : Promise.reject(new Error('请在桌面 App 中校验 DMG。'))
export const inspectInstallerUsage = (root: string, path: string): Promise<unknown> =>
  desktop
    ? invoke('inspect_installer_usage', { root, path })
    : Promise.reject(new Error('请在桌面 App 中检查文件占用。'))
export const inspectInstallerConnection = (root: string, path: string): Promise<unknown> =>
  desktop
    ? invoke('inspect_installer_connection', { root, path })
    : Promise.reject(new Error('请在桌面 App 中检查映像连接。'))
export const inspectInstallerZipData = (
  root: string,
  path: string,
  expectedStamp: string,
): Promise<unknown> =>
  desktop
    ? invoke('inspect_installer_zip_data', { root, path, expectedStamp })
    : Promise.reject(new Error('请在桌面 App 中校验 ZIP 数据。'))
export const inspectInstallerZip = (root: string, path: string): Promise<unknown> =>
  desktop
    ? invoke('inspect_installer_zip', { root, path })
    : Promise.reject(new Error('请在桌面 App 中检查 ZIP 条目。'))
export const inspectInstallers = (root: string): Promise<unknown> =>
  desktop
    ? invoke('inspect_installers', { root })
    : Promise.reject(new Error('请在桌面 App 中扫描安装包。'))
export const readProjectRoots = (): Promise<unknown> =>
  desktop
    ? invoke('read_project_roots')
    : Promise.reject(new Error('请在桌面 App 中读取 Mole 目录。'))
export const inspectProjectArtifacts = (root: string): Promise<unknown> =>
  desktop
    ? invoke('inspect_project_artifacts', { root })
    : Promise.reject(new Error('请在桌面 App 中扫描工程目录。'))
export const inspectProjectGit = (root: string, path: string): Promise<unknown> =>
  desktop
    ? invoke('inspect_project_git', { root, path })
    : Promise.reject(new Error('请在桌面 App 中检查 Git 记录。'))
export const measureProjectArtifact = (root: string, path: string): Promise<unknown> =>
  desktop
    ? invoke('measure_project_artifact', { root, path })
    : Promise.reject(new Error('请在桌面 App 中测量工程产物。'))
export const inspectMaintenance = (): Promise<unknown> =>
  desktop
    ? invoke('inspect_maintenance')
    : Promise.reject(new Error('请在桌面 App 中检查本机维护设置。'))
export const prepareMaintenanceReview = (
  observationId: string,
  selectedIds: string[],
): Promise<unknown> =>
  desktop
    ? invoke('prepare_maintenance_review', { observationId, selectedIds })
    : Promise.reject(new Error('请在桌面 App 中审阅本机维护条件。'))
export const bootstrap = (): Promise<Bootstrap> =>
  desktop
    ? invoke('bootstrap')
    : Promise.resolve({ ...structuredClone(demoBootstrap), settings: readPreviewSettings() })
export const inspectApplication = (taskId: string, index: number): Promise<ApplicationInspection> =>
  invoke('inspect_application', { taskId, index })
export const applicationIcon = (taskId: string, index: number): Promise<string | null> =>
  invoke('application_icon', { taskId, index })
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
export const acknowledgeExecution = async (record: ExecutionDetail): Promise<ExecutionDetail> => {
  await invoke('acknowledge_execution_review', {
    kind: 'cleanup',
    runId: record.id,
    snapshotSha256: record.snapshotSha256,
    reviewed: true,
  })
  return execution(record.id)
}
export const revealExecutionItem = (runId: string, itemId: string): Promise<void> =>
  desktop
    ? invoke('reveal_execution_item', { runId, itemId })
    : Promise.reject(new Error('请在桌面 App 中核对收据位置。'))
export const cancelLogCleanup = (runId: string): Promise<void> =>
  desktop
    ? invoke('cancel_log_cleanup', { runId })
    : Promise.reject(new Error('请在桌面 App 中停止清理。'))
export const saveSettings = (settings: Settings): Promise<Settings> =>
  desktop ? invoke('save_settings', { settings }) : savePreviewSettings(settings)
export const reveal = (path: string): Promise<void> =>
  desktop
    ? invoke('reveal_path', { path })
    : Promise.reject(new Error('请在桌面 App 中打开 Finder。'))
export const official = (): Promise<void> =>
  desktop
    ? invoke('open_official')
    : Promise.resolve(void window.open('https://mole.fit/zh/', '_blank', 'noopener,noreferrer'))
export const checkUpdates = (): Promise<UpdateSnapshot> =>
  desktop ? invoke('check_updates') : Promise.reject(new Error('请在桌面 App 中检查更新。'))
export const updateSnapshot = (): Promise<UpdateSnapshot> => invoke('get_update_snapshot')
export const setUpdateAutoDownload = (enabled: boolean): Promise<UpdateSnapshot> =>
  invoke('set_update_auto_download', { enabled })
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
export const discardCleanupConfirmation = (confirmationId: string): Promise<void> =>
  desktop ? invoke('discard_cleanup_confirmation', { confirmationId }) : Promise.resolve()

export async function executeLogCleanup(
  confirmationId: string,
  signal: AbortSignal,
): Promise<ExecutionDetail> {
  if (!desktop) throw new Error('请在桌面 App 中清理。')
  if (signal.aborted) throw new Error('开始前已取消，未请求处理。')
  const requestId = crypto.randomUUID()
  const cancel = () => {
    void cancelTask(requestId).catch(() => {})
  }
  signal.addEventListener('abort', cancel, { once: true })
  try {
    // Cancellation only stops later items. Always collect the actual response.
    return await invoke('execute_log_cleanup', { confirmationId, requestId })
  } finally {
    signal.removeEventListener('abort', cancel)
  }
}
export const chooseDirectory = async (title = '选择要检查的文件夹'): Promise<string | null> => {
  if (!desktop) return null
  const selected = await open({ directory: true, multiple: false, title: translate(title) })
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

export { archiveTransport } from './execution-archive'

export const aiTransport: import('./ai-scan').AiTransport = {
  detect: () => invoke('detect_ai_agents'),
  read: () => invoke('get_ai_scan'),
  start: (agentId, root, metadataConsent) =>
    invoke('start_ai_scan', { agentId, root, metadataConsent }),
  cancel: (requestId) => invoke('cancel_ai_scan', { requestId }),
}

// Browser previews retain settings across reloads, independently of native SQLite.
const previewSettingsKey = 'moomole.preview.settings'
function readPreviewSettings(): Settings {
  try {
    const saved = JSON.parse(localStorage.getItem(previewSettingsKey) ?? 'null')
    if (saved && ['system', 'zh', 'en', 'ja'].includes(saved.language))
      return { ...structuredClone(demoBootstrap.settings), ...saved }
  } catch {
    /* Storage can be unavailable in private/embedded browsers. */
  }
  return structuredClone(demoBootstrap.settings)
}
function savePreviewSettings(settings: Settings): Promise<Settings> {
  try {
    localStorage.setItem(previewSettingsKey, JSON.stringify(settings))
  } catch {
    return Promise.reject(new Error('Unable to save settings.'))
  }
  return Promise.resolve(settings)
}
