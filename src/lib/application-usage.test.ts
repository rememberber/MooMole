import { expect, it } from 'vitest'
import { matchesUsage, readAppUsage, type AppUsageReport } from './application-usage'
import type { AppEntry } from '../types'
const taskId = '11000000-0000-4000-8000-000000000001',
  requestId = '11000000-0000-4000-8000-000000000002',
  now = 1790000000000
const apps: AppEntry[] = [0, 1, 2].map((i) => ({
  name: 'App',
  path: `/Applications/Copy${i}.app`,
  bundle_id: 'org.example.same',
  source: 'App',
  uninstall_name: 'unused',
  size: '1GB',
}))
function report(): AppUsageReport {
  return {
    version: 1,
    taskId,
    requestId,
    checkedAt: now,
    expiresAt: now + 300000,
    requestedIndices: [0, 1, 2],
    complete: true,
    cancelled: false,
    rows: apps.map((a, index) => ({
      index,
      path: a.path,
      bundleId: a.bundle_id,
      state: index === 2 ? 'unknown' : index === 1 ? 'running' : 'observed',
      lastOpenedAt: index === 2 ? null : now - 200 * 86400000,
      issue: index === 2 ? 'notReported' : null,
    })),
  }
}
it('old records are hints for one exact copy; running and unknown apps are excluded', () => {
  const r = readAppUsage(report(), taskId, requestId, [0, 1, 2], apps)
  expect(matchesUsage(r.rows[0], '90', now)).toBe(true)
  expect(matchesUsage(r.rows[0], '365', now)).toBe(false)
  expect(matchesUsage(r.rows[1], '90', now)).toBe(false)
  expect(matchesUsage(r.rows[2], '90', now)).toBe(false)
  expect(matchesUsage(undefined, '90', now)).toBe(false)
  expect(matchesUsage(r.rows[2], 'unknown', now)).toBe(true)
})
it('rejects mismatched tasks, index order, dates and non-terminal cancellation claims', () => {
  for (const mutate of [
    (r: AppUsageReport) => {
      r.taskId = requestId
    },
    (r: AppUsageReport) => {
      r.rows[0].index = 1
    },
    (r: AppUsageReport) => {
      r.rows[0].path = apps[1].path
    },
    (r: AppUsageReport) => {
      r.rows[0].lastOpenedAt = now + 1
    },
    (r: AppUsageReport) => {
      r.rows[2].state = 'observed'
    },
    (r: AppUsageReport) => {
      r.cancelled = true
    },
    (r: AppUsageReport) => {
      r.expiresAt++
    },
    (r: AppUsageReport) => {
      r.requestedIndices.reverse()
    },
  ]) {
    const r = report()
    mutate(r)
    expect(() => readAppUsage(r, taskId, requestId, [0, 1, 2], apps)).toThrow()
  }
  const partial = report()
  partial.rows = partial.rows.slice(0, 1)
  partial.complete = false
  partial.cancelled = true
  expect(readAppUsage(partial, taskId, requestId, [0, 1, 2], apps).rows).toHaveLength(1)
})
