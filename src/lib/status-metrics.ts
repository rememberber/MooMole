import type { StatusWatchSnapshot } from './status-watch'

export interface StatusSample {
  sequence: number
  at: number
  cpu: number | null
  memory: number | null
  disk: { read: number | null; write: number | null }
  network: { name: string; rx: number | null; tx: number | null }[]
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
  if (statusRateValue(value) === null) return '未知'
  const rate = value as number
  if (rate === 0) return '0 MiB/s'
  if (rate < 0.01) return '< 0.01 MiB/s'
  return `${rate.toLocaleString('zh-CN', { maximumFractionDigits: 2 })} MiB/s`
}
export function statusRateValue(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : null
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
      disk: {
        read: statusRateValue(value.data.disk_io?.read_rate),
        write: statusRateValue(value.data.disk_io?.write_rate),
      },
      network: (value.data.network ?? []).slice(0, 3).flatMap((item, index, all) =>
        typeof item.name === 'string' &&
        item.name.length > 0 &&
        all.findIndex((other) => other.name === item.name) === index
          ? [
              {
                name: item.name,
                rx: statusRateValue(item.rx_rate_mbs),
                tx: statusRateValue(item.tx_rate_mbs),
              },
            ]
          : [],
      ),
      breakBefore:
        !previous ||
        value.sequence !== previous.sequence + 1 ||
        at <= previous.at ||
        at - previous.at > 10_000,
    },
  ].slice(-MAX_STATUS_SAMPLES)
}

export function statusTrend(samples: StatusSample[], metric: 'cpu' | 'memory') {
  return trendSegments(samples, (sample) => sample[metric], 100)
}
function trendSegments(
  samples: StatusSample[],
  read: (sample: StatusSample) => number | null,
  upper: number,
) {
  const first = samples[0]?.at ?? 0
  const last = samples.at(-1)?.at ?? first
  const segments: { x: number; y: number }[][] = []
  let current: { x: number; y: number }[] = []
  for (const sample of samples) {
    const value = read(sample)
    if (value == null || sample.breakBefore) {
      if (current.length) segments.push(current)
      current = []
    }
    if (value != null)
      current.push({
        x: last === first ? 160 : ((sample.at - first) / (last - first)) * 320,
        y: 62 - (value / upper) * 60,
      })
  }
  if (current.length) segments.push(current)
  return segments
}

export function statusRateTrend(samples: StatusSample[], metric: 'disk' | 'network', name = '') {
  const reads: { label: string; read: (sample: StatusSample) => number | null }[] =
    metric === 'disk'
      ? [
          { label: '读取', read: (s) => s.disk.read },
          { label: '写入', read: (s) => s.disk.write },
        ]
      : [
          { label: '接收', read: (s) => s.network.find((n) => n.name === name)?.rx ?? null },
          { label: '发送', read: (s) => s.network.find((n) => n.name === name)?.tx ?? null },
        ]
  const peak = Math.max(0, ...reads.flatMap(({ read }) => samples.flatMap((s) => read(s) ?? [])))
  // A shared zero-based axis for both directions; never a percentage or sum.
  const magnitude = 10 ** Math.floor(Math.log10(Math.max(0.01, peak)))
  const nice = [1, 2, 5, 10].map((n) => n * magnitude).find((n) => Number.isFinite(n) && n >= peak)
  const upper = peak === 0 ? 1 : Math.max(0.01, nice ?? peak)
  return {
    upper,
    series: reads.map(({ label, read }) => ({
      label,
      count: samples.filter((s) => read(s) !== null).length,
      segments: trendSegments(samples, read, upper),
    })),
  }
}
