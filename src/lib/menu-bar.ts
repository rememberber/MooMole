import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
export interface MenuBarView {
  config: { enabled: boolean }
  visible: boolean
  error: string | null
}
export function readMenuBar(raw: unknown): MenuBarView {
  const v = raw as MenuBarView
  if (
    !v ||
    typeof v.config?.enabled !== 'boolean' ||
    typeof v.visible !== 'boolean' ||
    (v.error !== null && typeof v.error !== 'string')
  )
    throw Error('菜单栏状态无法核对，请刷新。')
  return v
}
export const menuBarTransport = {
  read: () => invoke('get_menu_bar'),
  configure: (enabled: boolean) => invoke('configure_menu_bar', { config: { enabled } }),
}
export type MenuBarTransport = typeof menuBarTransport
export const listenMenuBar = (open: () => void) =>
  listen<string>('moomole-menu-bar', ({ payload }) => {
    if (payload === 'storageMonitor') open()
  })
