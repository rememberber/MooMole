import { describe, expect, it } from 'vitest'
import type { CleanCandidate } from '../types'
import { pathOverlaps, previewTotal, protectionFor } from './clean-preview'

const candidate = (path: string, size: number | null): CleanCandidate => ({
  id: path,
  path,
  size,
  upstreamIdentity: path,
  section: 'Apps',
  itemCount: 1,
  isDirectory: true,
  coveredBy: null,
  resolvedPath: null,
})

describe('structured preview review', () => {
  it('counts selected descendants only when no measured selected ancestor covers them', () => {
    const parent = candidate('/a', 100)
    const child = candidate('/a/b', 40)
    const sibling = candidate('/ab', 20)
    expect(previewTotal([parent, child, sibling])).toEqual({ size: 120, unknown: 0 })
    expect(previewTotal([child, sibling])).toEqual({ size: 60, unknown: 0 })
    expect(previewTotal([candidate('/a', null), child])).toEqual({ size: 40, unknown: 1 })
    expect(previewTotal([parent, candidate('/a/unknown', null)])).toEqual({ size: 100, unknown: 0 })
  })
  it('protects literal parent/child paths and resolved aliases without sibling or glob matches', () => {
    expect(pathOverlaps('/a/b/', '/a')).toBe(true)
    expect(pathOverlaps('/a', '/ab')).toBe(false)
    expect(pathOverlaps('/a/*', '/a/b')).toBe(false)
    expect(protectionFor(candidate('/a', 1), ['/a/keep', '/ab'])).toEqual(['/a/keep'])
    expect(
      protectionFor({ ...candidate('/link/cache', 1), resolvedPath: '/real/cache' }, ['/real']),
    ).toEqual(['/real'])
    expect(protectionFor(candidate('/any', 1), ['/'])).toEqual(['/'])
  })
})
