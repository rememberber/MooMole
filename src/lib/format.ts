import type { ScanState } from '../types'

export function bytes(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value) || value < 0) return '—'
  if (value === 0) return '0 B'
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), 4)
  return `${(value / 1024 ** index).toLocaleString('zh-CN', { maximumFractionDigits: index ? 1 : 0 })} ${['B', 'KB', 'MB', 'GB', 'TB'][index]}`
}

export function measuredBytes(value: number | null | undefined, state: ScanState): string {
  if (state === 'unavailable') return '无法测量'
  return `${bytes(value)}${state === 'partial' ? '+' : ''}`
}

export function percent(value: number | undefined): string {
  return value == null || !Number.isFinite(value) ? '—' : `${value.toFixed(1)}%`
}
export function shortPath(path: string, home: string): string {
  return path === home ? '~' : path.startsWith(`${home}/`) ? `~${path.slice(home.length)}` : path
}
export function dateTime(value: number | string): string {
  return new Date(value).toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  })
}
export const kindNames = {
  analyze: '空间分析',
  status: '系统快照',
  apps: '应用列表',
  cleanup: '开发缓存检查',
  clean: '清理预览',
  history: 'Mole 历史读取',
}
export const stateNames: Record<string, string> = {
  running: '进行中',
  queued: '等待中',
  succeeded: '已完成',
  failed: '未完成',
  cancelled: '已取消',
  interrupted: '已中断',
}
