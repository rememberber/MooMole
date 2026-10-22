import { expect, it } from 'vitest'
import {
  batteryCapacity,
  batteryState,
  batteryTemperature,
  gpuName,
  gpuPlaceholders,
  positiveCount,
} from './status-hardware'
import { statusPercent } from './status-metrics'
it('distinguishes GPU identities from pinned fallback messages without inventing usable hardware', () => {
  expect(gpuName('Apple Fixture')).toBe('Apple Fixture')
  expect(gpuName('  GPU [literal]  ')).toBe('GPU [literal]')
  for (const name of [
    null,
    undefined,
    '',
    ' ',
    ...gpuPlaceholders,
    ' GPU INFO UNAVAILABLE ',
    'GPU\nextra',
    'a'.repeat(4097),
  ])
    expect(gpuName(name)).toBeNull()
  expect(statusPercent(0)).toBe(0)
  for (const value of [-1, null, undefined, Infinity, NaN, 101])
    expect(statusPercent(value)).toBeNull()
})
it('keeps zero charge distinct from unknown counters and marks only recognized battery states', () => {
  expect(statusPercent(0)).toBe(0)
  expect(batteryCapacity(85)).toBe(85)
  expect(batteryCapacity(100)).toBe(100)
  expect(positiveCount(42)).toBe(42)
  for (const value of [null, undefined, 0, -1, 0.5, Infinity, Number.MAX_SAFE_INTEGER + 1, '0']) {
    expect(positiveCount(value)).toBeNull()
    expect(batteryCapacity(value)).toBeNull()
  }
  expect(batteryCapacity(101)).toBeNull()
  expect(batteryState('charging')).toBe('正在充电')
  expect(batteryState('Discharging')).toBe('使用电池')
  expect(batteryState('charged')).toBe('已充满')
  for (const value of [null, undefined, '', 'not charging', 'AC', 'unknown', 'constructor'])
    expect(batteryState(value)).toBe('状态未知')
})
it('never turns absent or zero temperature into a measurement or derives CPU and GPU readings', () => {
  expect(batteryTemperature(35.5)).toBe(35.5)
  for (const value of [null, undefined, 0, -1, 1000, Infinity, NaN, '35'])
    expect(batteryTemperature(value)).toBeNull()
})
