import { statusPercent } from './status-metrics'
export const gpuPlaceholders = [
  'No GPU metrics available',
  'GPU read failed',
  'GPU info unavailable',
]
export function gpuName(value: unknown): string | null {
  if (typeof value !== 'string') return null
  const name = value.trim()
  return name.length > 0 &&
    name.length <= 4096 &&
    !/[\u0000-\u001f\u007f-\u009f]/.test(name) &&
    !gpuPlaceholders.some((p) => p.toLowerCase() === name.toLowerCase())
    ? name
    : null
}
export function positiveCount(value: unknown): number | null {
  return typeof value === 'number' && Number.isSafeInteger(value) && value > 0 ? value : null
}
export function batteryCapacity(value: unknown): number | null {
  const percent = statusPercent(value)
  return percent !== null && percent > 0 && Number.isInteger(percent) ? percent : null
}
export function batteryTemperature(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 && value < 1000
    ? value
    : null
}
export function batteryState(value: unknown): string {
  switch (typeof value === 'string' ? value.toLowerCase() : '') {
    case 'charging':
      return '正在充电'
    case 'discharging':
      return '使用电池'
    case 'charged':
      return '已充满'
    default:
      return '状态未知'
  }
}
