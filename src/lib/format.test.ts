import { describe, expect, it } from 'vitest'
import { bytes, measuredBytes, percent, shortPath } from './format'

describe('filesystem measurements', () => {
  it('keeps unknown or inaccessible sizes distinct from zero', () => {
    expect(bytes(undefined)).toBe('—')
    expect(bytes(null)).toBe('—')
    expect(bytes(Number.NaN)).toBe('—')
    expect(bytes(-1)).toBe('—')
    expect(bytes(0)).toBe('0 B')
    expect(measuredBytes(0, 'unavailable')).toBe('无法测量')
    expect(measuredBytes(1024, 'partial')).toBe('1 KB+')
  })
  it('does not replace unavailable metrics with a zero percentage', () => {
    expect(percent(undefined)).toBe('—')
    expect(percent(Number.NaN)).toBe('—')
    expect(percent(0)).toBe('0.0%')
  })
  it('abbreviates only the actual home directory or its children', () => {
    expect(shortPath('/Users/bob', '/Users/bob')).toBe('~')
    expect(shortPath('/Users/bob/Projects', '/Users/bob')).toBe('~/Projects')
    expect(shortPath('/Users/bobby/Projects', '/Users/bob')).toBe('/Users/bobby/Projects')
  })
})
