import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import type { Bootstrap, Settings, TaskEvent, TaskKind, TaskRecord, TaskResults } from '../types'
import { demoBootstrap } from './demo'

export const desktop = isTauri()
export const bootstrap = (): Promise<Bootstrap> =>
  desktop ? invoke('bootstrap') : Promise.resolve(structuredClone(demoBootstrap))
export const history = (): Promise<TaskRecord[]> =>
  desktop ? invoke('get_history') : Promise.resolve([])
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
export const cancelTask = (taskId: string): Promise<void> => invoke('cancel_task', { taskId })
export const chooseDirectory = async (): Promise<string | null> => {
  if (!desktop) return null
  const selected = await open({ directory: true, multiple: false, title: '选择要检查的文件夹' })
  return typeof selected === 'string' ? selected : null
}

export async function runTask<K extends TaskKind>(
  kind: K,
  path: string | undefined,
  onStart: (id: string) => void,
): Promise<TaskResults[K]> {
  if (!desktop) throw new Error('浏览器预览不能读取本机，请运行 npm run desktop。')
  const taskId = crypto.randomUUID()
  let resolveEvent!: (event: TaskEvent) => void
  const completed = new Promise<TaskEvent>((resolve) => {
    resolveEvent = resolve
  })
  const unlisten = await listen<TaskEvent>('task-event', ({ payload }) => {
    if (payload.id === taskId) resolveEvent(payload)
  })
  try {
    onStart(taskId)
    await invoke('start_task', { taskId, kind, path: path ?? null })
    const event = await completed
    if (event.error || event.state !== 'succeeded') throw new Error(event.error ?? '任务未完成。')
    return event.result as TaskResults[K]
  } finally {
    unlisten()
  }
}
