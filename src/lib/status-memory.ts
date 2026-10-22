import type { Snapshot } from '../types'
export function memoryPressure(value: unknown) {
  switch (value) {
    case 'normal':
      return { level: 'normal', label: '正常' } as const
    case 'warn':
      return { level: 'warn', label: '偏高' } as const
    case 'critical':
      return { level: 'critical', label: '很高' } as const
    default:
      return { level: 'unknown', label: '未知' } as const
  }
}
export function memoryByteValue(value: unknown): number | null {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? value : null
}
export function statusMemory(memory: Snapshot['memory']) {
  const total = memoryByteValue(memory?.total)
  const available = memoryByteValue(memory?.available)
  const used = memoryByteValue(memory?.swap_used)
  const swapTotal = memoryByteValue(memory?.swap_total)
  const inconsistentSwap = used !== null && swapTotal !== null && used > swapTotal
  return {
    pressure: memoryPressure(memory?.pressure),
    available: available !== null && total !== null && available > total ? null : available,
    swapUsed: inconsistentSwap ? null : used,
    swapTotal: inconsistentSwap ? null : swapTotal,
    inconsistentSwap,
  }
}
