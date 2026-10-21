import { describe, expect, it } from 'vitest'
import {
  appendStatusSample,
  statusPercent,
  statusRate,
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
})
