import { invoke } from '@tauri-apps/api/core'
import type { UninstallTarget } from './app-uninstall'
export interface AppControlTarget extends UninstallTarget {
  action: 'open' | 'quit'
}
export interface AppControlReport {
  requestId: string
  action: 'open' | 'quit'
  path: string
  bundleId: string
  outcome: 'opened' | 'alreadyRunning' | 'quit' | 'notRunning' | 'pending' | 'cancelled' | 'unknown'
  requested: boolean
  pids: number[]
  reason: string
}
export async function controlApp(
  target: AppControlTarget,
  requestId: string,
): Promise<AppControlReport> {
  const value = await invoke<AppControlReport>('control_application', {
    taskId: target.taskId,
    index: target.index,
    action: target.action,
    requestId,
    confirmed: true,
  })
  if (
    !value ||
    value.requestId !== requestId ||
    value.action !== target.action ||
    value.path !== target.path ||
    value.bundleId !== target.bundleId ||
    !['opened', 'alreadyRunning', 'quit', 'notRunning', 'pending', 'cancelled', 'unknown'].includes(
      value.outcome,
    ) ||
    typeof value.requested !== 'boolean' ||
    typeof value.reason !== 'string' ||
    !Array.isArray(value.pids) ||
    value.pids.some((pid) => !Number.isSafeInteger(pid) || pid <= 0)
  )
    throw Error('应用操作结果无法核对，请刷新列表后检查实际状态；不会重复请求。')
  return value
}
export const stopAppControl = (taskId: string) => invoke('cancel_task', { taskId })
export const appControlOutcome: Record<AppControlReport['outcome'], string> = {
  opened: '已观察到应用运行',
  alreadyRunning: '所选副本已在运行',
  quit: '已观察到应用退出',
  notRunning: '所选副本未运行',
  pending: '尚未确认完成',
  cancelled: '已停止等待',
  unknown: '结果待核实',
}
