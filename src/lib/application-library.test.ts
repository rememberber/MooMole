import { describe, expect, it } from 'vitest'
import type { AppEntry } from '../types'
import { appEstimateLabel, appSizeEstimate, compareApps } from './application-library'

describe('upstream application size estimates', () => {
  it('uses Mole decimal units so values across unit boundaries sort correctly', () => {
    expect(appSizeEstimate('1.00GB')).toBe(1_000_000_000)
    expect(appSizeEstimate('999.0MB')).toBe(999_000_000)
    expect(appSizeEstimate('10KB')).toBe(10_000)
    expect(appEstimateLabel(1_999_000_000)).toBe('2 GB')
  })
  it('keeps unknown, managed and unsupported values distinct from an explicit zero', () => {
    for (const value of [
      'N/A',
      'N/A (Steam-managed)',
      'Unknown',
      '',
      '-1MB',
      '1GiB',
      '1e9B',
      '1TB',
      '9'.repeat(400) + 'GB',
      null,
    ])
      expect(appSizeEstimate(value)).toBeNull()
    expect(appSizeEstimate('0B')).toBe(0)
  })
  it('puts unknown sizes after known zero and uses full paths to distinguish equal names', () => {
    const entry = (path: string, size: string): AppEntry => ({
      name: '同名应用',
      path,
      size,
      bundle_id: 'same.id',
      source: 'App',
      uninstall_name: 'unused',
    })
    const items = [
      entry('/z.app', 'N/A'),
      entry('/b.app', '0B'),
      entry('/a.app', '0B'),
      entry('/large.app', '1.00GB'),
    ]
    expect([...items].sort((a, b) => compareApps(a, b, 'size')).map((v) => v.path)).toEqual([
      '/large.app',
      '/a.app',
      '/b.app',
      '/z.app',
    ])
    expect(items[0].path).toBe('/z.app')
  })
})
