import { expect, it } from 'vitest'
import { memoryByteValue, memoryPressure, statusMemory } from './status-memory'
it('accepts only pinned pressure labels without deriving pressure from high or low occupancy', () => {
  expect(memoryPressure('normal')).toEqual({ level: 'normal', label: '正常' })
  expect(memoryPressure('warn')).toEqual({ level: 'warn', label: '偏高' })
  expect(memoryPressure('critical')).toEqual({ level: 'critical', label: '很高' })
  for (const value of [
    undefined,
    null,
    '',
    'warning',
    'Normal',
    ' normal ',
    'constructor',
    0,
    {},
    'normal\n',
  ])
    expect(memoryPressure(value).level).toBe('unknown')
  expect(statusMemory({ used_percent: 100 }).pressure.level).toBe('unknown')
  expect(statusMemory({ used_percent: 100, pressure: 'normal' }).pressure.level).toBe('normal')
  expect(statusMemory({ used_percent: 0, pressure: 'critical' }).pressure.level).toBe('critical')
})
it('preserves zero and missing counters independently and rejects precision loss or inconsistent fields', () => {
  for (const value of [0, 1024, Number.MAX_SAFE_INTEGER]) expect(memoryByteValue(value)).toBe(value)
  for (const value of [
    undefined,
    null,
    -1,
    NaN,
    Infinity,
    1.5,
    Number.MAX_SAFE_INTEGER + 1,
    '1024',
  ])
    expect(memoryByteValue(value)).toBeNull()
  expect(statusMemory(null)).toMatchObject({
    available: null,
    swapUsed: null,
    swapTotal: null,
    inconsistentSwap: false,
  })
  expect(statusMemory({ total: 4096, available: 1024, swap_used: 0 })).toMatchObject({
    available: 1024,
    swapUsed: 0,
    swapTotal: null,
  })
  expect(statusMemory({ available: 1024, swap_total: 4096 })).toMatchObject({
    available: 1024,
    swapUsed: null,
    swapTotal: 4096,
  })
  expect(statusMemory({ total: 4096, available: 8192 }).available).toBeNull()
  expect(statusMemory({ swap_used: 2048, swap_total: 1024 })).toMatchObject({
    swapUsed: null,
    swapTotal: null,
    inconsistentSwap: true,
  })
  expect(statusMemory({ swap_used: 0, swap_total: 0 })).toMatchObject({
    swapUsed: 0,
    swapTotal: 0,
    inconsistentSwap: false,
  })
})
