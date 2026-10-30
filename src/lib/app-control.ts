import { invoke } from '@tauri-apps/api/core'
import type { UninstallTarget } from './app-uninstall'
export interface AppControlTarget extends UninstallTarget {
  action: 'open' | 'quit' | 'register'
}
export interface AppControlReport {
  requestId: string
  action: 'open' | 'quit' | 'register'
  path: string
  bundleId: string
  outcome:
    | 'opened'
    | 'alreadyRunning'
    | 'quit'
    | 'notRunning'
    | 'pending'
    | 'cancelled'
    | 'unknown'
    | 'registered'
  requested: boolean
  pids: number[]
  reason: string
  registrationStatus?: number
  registeredPathObserved?: boolean
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
    !(
      target.action === 'register'
        ? ['registered', 'pending', 'cancelled', 'unknown']
        : ['opened', 'alreadyRunning', 'quit', 'notRunning', 'pending', 'cancelled', 'unknown']
    ).includes(value.outcome) ||
    typeof value.requested !== 'boolean' ||
    typeof value.reason !== 'string' ||
    !Array.isArray(value.pids) ||
    value.pids.some((pid) => !Number.isSafeInteger(pid) || pid <= 0) ||
    (target.action === 'register'
      ? value.pids.length > 0 ||
        (value.registrationStatus !== undefined &&
          (!Number.isSafeInteger(value.registrationStatus) ||
            value.registrationStatus < -2147483648 ||
            value.registrationStatus > 2147483647)) ||
        (value.registeredPathObserved !== undefined &&
          typeof value.registeredPathObserved !== 'boolean') ||
        (value.outcome === 'registered' &&
          (!value.requested ||
            value.registrationStatus !== 0 ||
            value.registeredPathObserved !== true)) ||
        (value.outcome === 'pending' &&
          (!value.requested ||
            value.registrationStatus !== 0 ||
            value.registeredPathObserved !== false))
      : value.registrationStatus !== undefined || value.registeredPathObserved !== undefined)
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
  registered: '已观察到所选副本注册',
}
