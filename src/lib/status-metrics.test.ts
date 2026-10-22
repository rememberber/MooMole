import { describe, expect, it } from 'vitest'
import {
  appendStatusSample,
  statusPercent,
  statusRate,
  statusRateTrend,
  statusTrend,
  type StatusSample,
} from './status-metrics'
import type { StatusWatchSnapshot } from './status-watch'
const base = Date.parse('2026-10-21T00:00:00Z')
function sample(
  sequence: number,
  time = sequence * 2000,
  cpu: number | undefined = 20,
): StatusWatchSnapshot {
  return {
    version: 2,
    id: 'test',
    state: 'running',
    intervalSeconds: 2,
    sequence,
    receivedAt: base + time,
    data: {
      collected_at: new Date(base + time).toISOString(),
      cpu: { usage: cpu },
      memory: { used_percent: 0 },
    },
    error: null,
  }
}
describe('bounded status history and displayed measures', () => {
  it('keeps true zero, rejects unknown measures and uses the upstream binary rate unit', () => {
    expect(statusPercent(0)).toBe(0)
    expect(statusPercent(100)).toBe(100)
    for (const invalid of [null, undefined, -1, Infinity, NaN, 101, '2'])
      expect(statusPercent(invalid)).toBeNull()
    expect(statusRate(0)).toBe('0 MiB/s')
    expect(statusRate(1)).toBe('1 MiB/s')
    expect(statusRate(0.001)).toBe('< 0.01 MiB/s')
    for (const invalid of [null, undefined, -1, Infinity, NaN, '1'])
      expect(statusRate(invalid)).toBe('未知')
    const estimate = sample(1)
    estimate.data!.cpu!.per_core_estimated = true
    expect(appendStatusSample([], estimate)[0].cpu).toBeNull()
  })
  it('bounds count and real collection time without inventing missing samples', () => {
    let history: StatusSample[] = []
    for (let i = 1; i <= 1000; i++) history = appendStatusSample(history, sample(i))
    expect(history).toHaveLength(120)
    expect(history[0].sequence).toBe(881)
    const afterGap = appendStatusSample(history, sample(1001, 2_400_000))
    expect(afterGap).toHaveLength(1)
    expect(afterGap[0].breakBefore).toBe(true)
    expect(afterGap[0].memory).toBe(0)
  })
  it('refuses duplicate/old sequence, resets a rolled-back clock and breaks skipped or slow snapshots', () => {
    const first = appendStatusSample([], sample(1))
    expect(appendStatusSample(first, sample(1))).toBe(first)
    const skipped = appendStatusSample(first, sample(3))
    expect(skipped).toHaveLength(2)
    expect(skipped[1].breakBefore).toBe(true)
    const slow = appendStatusSample(skipped, sample(4, 19_000))
    expect(slow[2].breakBefore).toBe(true)
    const rollback = appendStatusSample(slow, sample(5, 1000))
    expect(rollback).toHaveLength(1)
    expect(rollback[0].at).toBe(base + 1000)
  })
  it('draws separate segments for missing values and time gaps, with actual time spacing and fixed percentage axis', () => {
    let history = appendStatusSample([], sample(1, 1000, 0))
    history = appendStatusSample(history, sample(2, 2000, 100))
    history = appendStatusSample(history, {
      ...sample(3, 5000),
      data: { collected_at: new Date(base + 5000).toISOString() },
    })
    history = appendStatusSample(history, sample(4, 6000, 50))
    expect(statusTrend(history, 'cpu')).toEqual([
      [
        { x: 0, y: 62 },
        { x: 64, y: 2 },
      ],
      [{ x: 320, y: 32 }],
    ])
    expect(statusTrend([], 'cpu')).toEqual([])
    expect(statusTrend(history.slice(0, 1), 'cpu')).toEqual([[{ x: 160, y: 62 }]])
  })
  it('copies bounded per-direction rates without summing interfaces or turning missing values into zero', () => {
    const value = sample(1)
    value.data!.disk_io = { read_rate: 0, write_rate: -1 }
    value.data!.network = [
      { name: 'en0', rx_rate_mbs: 2, tx_rate_mbs: 0.001 },
      { name: 'utun9', rx_rate_mbs: 3, tx_rate_mbs: 0 },
      { name: 'en0', rx_rate_mbs: 99, tx_rate_mbs: 99 },
      { name: 'extra', rx_rate_mbs: 999 },
    ]
    const history = appendStatusSample([], value)
    expect(history[0].disk).toEqual({ read: 0, write: null })
    expect(history[0].network).toEqual([
      { name: 'en0', rx: 2, tx: 0.001 },
      { name: 'utun9', rx: 3, tx: 0 },
    ])
    value.data!.network[0].rx_rate_mbs = 100
    expect(history[0].network[0].rx).toBe(2)
    expect(statusRateTrend(history, 'network', 'en0').upper).toBe(2)
    expect(statusRateTrend(history, 'network', 'utun9').upper).toBe(5)
    expect(statusRateTrend(history, 'network', 'absent').series.every((s) => s.count === 0)).toBe(
      true,
    )
  })
  it('uses a shared zero-based rate axis and breaks each direction independently at missing records', () => {
    let history: StatusSample[] = []
    for (const [sequence, time, read, write] of [
      [1, 1000, 0, 1],
      [2, 2000, 2, 1],
      [3, 5000, NaN, 4],
      [4, 6000, 1, NaN],
    ]) {
      const value = sample(sequence, time)
      value.data!.disk_io = { read_rate: read, write_rate: write }
      history = appendStatusSample(history, value)
    }
    const result = statusRateTrend(history, 'disk')
    expect(result.upper).toBe(5)
    expect(result.series[0].count).toBe(3)
    expect(result.series[0].segments).toEqual([
      [
        { x: 0, y: 62 },
        { x: 64, y: 38 },
      ],
      [{ x: 320, y: 50 }],
    ])
    expect(result.series[1].segments).toHaveLength(1)
    expect(result.series[1].count).toBe(3)
    const skipped = sample(6, 8000)
    skipped.data!.disk_io = { read_rate: 2, write_rate: 3 }
    history = appendStatusSample(history, skipped)
    expect(statusRateTrend(history, 'disk').series[0].segments).toHaveLength(3)
    const slow = sample(7, 20000)
    slow.data!.disk_io = { read_rate: 1 }
    history = appendStatusSample(history, slow)
    expect(statusRateTrend(history, 'disk').series[0].segments).toHaveLength(4)
  })
  it('keeps absent interfaces disconnected when they return and handles extreme valid rates with finite coordinates', () => {
    let history: StatusSample[] = []
    for (let sequence = 1; sequence <= 3; sequence++) {
      const value = sample(sequence)
      value.data!.network =
        sequence === 2 ? [] : [{ name: 'en0', rx_rate_mbs: sequence, tx_rate_mbs: 0 }]
      history = appendStatusSample(history, value)
    }
    expect(statusRateTrend(history, 'network', 'en0').series[0].segments).toHaveLength(2)
    for (const rate of [0, 1e-12, 123, Number.MAX_VALUE]) {
      const value = sample(1)
      value.data!.disk_io = { read_rate: rate, write_rate: rate }
      const trend = statusRateTrend(appendStatusSample([], value), 'disk')
      expect(Number.isFinite(trend.upper)).toBe(true)
      expect(trend.upper).toBeGreaterThanOrEqual(rate)
      expect(trend.upper).toBeGreaterThan(0)
      for (const point of trend.series.flatMap((s) => s.segments.flat())) {
        expect(Number.isFinite(point.y)).toBe(true)
        expect(point.y).toBeGreaterThanOrEqual(2)
        expect(point.y).toBeLessThanOrEqual(62)
      }
    }
    expect(statusRateTrend([], 'disk').series.every((s) => s.count === 0)).toBe(true)
  })
})
