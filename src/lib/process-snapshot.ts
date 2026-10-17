import type { ProcessSample } from '../types'

export type ProcessSort = 'cpu' | 'memory' | 'name' | 'pid'

export function processMetric(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : null
}

export function processSampleTime(value: unknown): string | null {
  if (typeof value !== 'string' || !value.trim()) return null
  const time = Date.parse(value)
  return Number.isFinite(time) ? new Date(time).toISOString() : null
}

export function matchingProcesses(
  samples: ProcessSample[],
  query: string,
  sort: ProcessSort,
): ProcessSample[] {
  const search = query.trim().toLocaleLowerCase('zh-CN')
  return samples
    .filter((sample) => `${sample.name}\n${sample.pid}`.toLocaleLowerCase('zh-CN').includes(search))
    .sort((a, b) => {
      if (sort === 'cpu' || sort === 'memory') {
        const field = sort === 'cpu' ? 'cpu' : 'memory_bytes'
        const x = processMetric(a[field])
        const y = processMetric(b[field])
        if (x == null && y != null) return 1
        if (x != null && y == null) return -1
        if (x != null && y != null && x !== y) return y - x
      }
      if (sort === 'name') {
        const name = a.name.localeCompare(b.name, 'zh-CN')
        if (name) return name
      }
      return a.pid - b.pid || a.name.localeCompare(b.name, 'zh-CN')
    })
}
