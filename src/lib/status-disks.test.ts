import { expect, it } from 'vitest'
import { diskBytes, diskMount, diskReport, systemDisk } from './status-disks'
it('computes remaining capacity and proportion from consistent precise bytes without using reported percentages', () => {
  expect(
    diskReport({ mount: '/', total: 4096, used: 1024, used_percent: 99, external: false }),
  ).toEqual({
    mount: '/',
    total: 4096,
    used: 1024,
    remaining: 3072,
    percent: 25,
    external: false,
    inconsistent: false,
  })
  expect(diskReport({ total: 4096, used: 0 })).toMatchObject({
    used: 0,
    remaining: 4096,
    percent: 0,
  })
  expect(diskReport({ total: 4096, used: 4096 })).toMatchObject({ remaining: 0, percent: 100 })
  expect(diskBytes(0)).toBe('0 B')
  expect(diskBytes(null)).toBe('未知')
  expect(diskBytes(undefined)).toBe('未知')
})
it('keeps inconsistent, missing, zero total and imprecise counters unknown instead of clamping to an empty or full disk', () => {
  expect(diskReport({ total: 1024, used: 2048 })).toMatchObject({
    total: 1024,
    used: null,
    remaining: null,
    percent: null,
    inconsistent: true,
  })
  expect(diskReport({ total: 0, used: 0 })).toMatchObject({
    total: null,
    remaining: null,
    percent: null,
  })
  expect(diskReport({ used_percent: 50 })).toMatchObject({
    total: null,
    used: null,
    remaining: null,
    percent: null,
  })
  for (const invalid of [-1, 0.5, Infinity, NaN, Number.MAX_SAFE_INTEGER + 1]) {
    expect(diskReport({ total: invalid, used: 1 }).remaining).toBeNull()
    expect(diskReport({ total: 4096, used: invalid }).percent).toBeNull()
  }
  expect(diskReport({ total: Number.MAX_SAFE_INTEGER, used: 1 }).remaining).toBe(
    Number.MAX_SAFE_INTEGER - 1,
  )
})
it('selects only a unique literal system root, preserving other and duplicate records without guessing physical identity', () => {
  const external = {
    mount: '/Volumes/Internal-looking',
    total: 100,
    used: 50,
    used_percent: 50,
    external: false,
  }
  const root = { ...external, mount: '/' }
  expect(systemDisk([external])).toBeNull()
  expect(systemDisk(undefined)).toBeNull()
  expect(systemDisk([root, { ...root }])).toBeNull()
  expect(systemDisk([external, root])?.mount).toBe('/')
  for (const invalid of [
    'relative',
    '/a/../b',
    '/a//b',
    '/root/',
    '/bad\nname',
    '/' + 'a'.repeat(4096),
  ])
    expect(diskMount(invalid)).toBeNull()
  expect(diskMount('/Volumes/含 空格 [literal]')).toBe('/Volumes/含 空格 [literal]')
  expect(diskReport({ mount: '/', external: true }).external).toBe(true)
  expect(diskReport({}).external).toBeNull()
})
