import { describe, expect, it } from 'vitest'
import { recordAnalysisVisit, type AnalysisNavigationHistory } from './analysis-navigation'

const empty = (): AnalysisNavigationHistory => ({ paths: [], index: -1 })

describe('successful analysis visits', () => {
  it('records the first observation and refreshes without creating a duplicate', () => {
    const first = recordAnalysisVisit(empty(), '/a')
    expect(first).toEqual({ paths: ['/a'], index: 0 })
    expect(recordAnalysisVisit(first, '/a')).toBe(first)
    expect(recordAnalysisVisit(first, '/a/child')).toEqual({ paths: ['/a', '/a/child'], index: 1 })
    expect(first.paths).toEqual(['/a'])
  })
  it('retains forward visits on back/refresh and discards them only on a new successful branch', () => {
    const history = { paths: ['/a', '/b', '/c'], index: 2 }
    const back = recordAnalysisVisit(history, '/b', { index: 1, path: '/b' })
    expect(back).toEqual({ paths: ['/a', '/b', '/c'], index: 1 })
    expect(recordAnalysisVisit(back, '/b')).toBe(back)
    expect(recordAnalysisVisit(back, '/new')).toEqual({ paths: ['/a', '/b', '/new'], index: 2 })
    expect(history).toEqual({ paths: ['/a', '/b', '/c'], index: 2 })
  })
  it('uses the returned canonical path and verifies the request target before moving the cursor', () => {
    const history = { paths: ['/old', '/current'], index: 1 }
    expect(recordAnalysisVisit(history, '/resolved', { index: 0, path: '/old' })).toEqual({
      paths: ['/resolved', '/current'],
      index: 0,
    })
    for (const index of [-1, 0.5, 99])
      expect(recordAnalysisVisit(history, '/new', { index, path: '/old' })).toEqual({
        paths: ['/old', '/current', '/new'],
        index: 2,
      })
    expect(recordAnalysisVisit(history, '/new', { index: 0, path: '/stale' }).index).toBe(2)
  })
  it('bounds path history and keeps the cursor correct after eviction and branching', () => {
    let history = empty()
    for (let i = 0; i < 80; i++) history = recordAnalysisVisit(history, `/visit-${i}`)
    expect(history.paths).toHaveLength(32)
    expect(history.paths[0]).toBe('/visit-48')
    expect(history.paths[31]).toBe('/visit-79')
    expect(history.index).toBe(31)
    const back = recordAnalysisVisit(history, '/visit-48', { index: 0, path: '/visit-48' })
    expect(recordAnalysisVisit(back, '/branch')).toEqual({
      paths: ['/visit-48', '/branch'],
      index: 1,
    })
  })
})
