import type { StatusWatchSnapshot } from './status-watch'

export interface StatusSample {
  sequence: number
  at: number
  cpu: number | null
  memory: number | null
  breakBefore: boolean
}
export const MAX_STATUS_SAMPLES = 120
const WINDOW = 5 * 60_000
export function statusPercent(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= 100
    ? value
    : null
}
export function statusRate(value: unknown): string {
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) return '未知'
  if (value === 0) return '0 MiB/s'
  if (value < 0.01) return '< 0.01 MiB/s'
  return `${value.toLocaleString('zh-CN', { maximumFractionDigits: 2 })} MiB/s`
}
export function appendStatusSample(
  samples: StatusSample[],
  value: StatusWatchSnapshot,
): StatusSample[] {
  if (!value.data || !Number.isSafeInteger(value.sequence) || value.sequence < 1) return samples
  const at = Date.parse(value.data.collected_at)
  if (!Number.isFinite(at)) return samples
  const previous = samples.at(-1)
  if (previous && value.sequence <= previous.sequence) return samples
  // Restart the time axis on clock rollback; never draw time moving backwards.
  const kept = previous && at <= previous.at ? [] : samples.filter((s) => at - s.at <= WINDOW)
  return [
    ...kept,
    {
      sequence: value.sequence,
      at,
      cpu:
        value.data.cpu?.per_core_estimated === true ? null : statusPercent(value.data.cpu?.usage),
      memory: statusPercent(value.data.memory?.used_percent),
      breakBefore:
        !previous ||
        value.sequence !== previous.sequence + 1 ||
        at <= previous.at ||
        at - previous.at > 10_000,
    },
  ].slice(-MAX_STATUS_SAMPLES)
}

export function statusTrend(samples: StatusSample[], metric: 'cpu' | 'memory') {
  const first = samples[0]?.at ?? 0
  const last = samples.at(-1)?.at ?? first
  const segments: { x: number; y: number }[][] = []
  let current: { x: number; y: number }[] = []
  for (const sample of samples) {
    const value = sample[metric]
    if (value == null || sample.breakBefore) {
      if (current.length) segments.push(current)
      current = []
    }
    if (value != null)
      current.push({
        x: last === first ? 160 : ((sample.at - first) / (last - first)) * 320,
        y: 62 - value * 0.6,
      })
  }
  if (current.length) segments.push(current)
  return segments
}
