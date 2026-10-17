import { describe, expect, it } from 'vitest'
import { AnalysisContexts, type AnalysisContext } from './analysis-context'

const context = (): AnalysisContext => ({
  view: 'directory',
  directory: {
    query: '中文 $(literal)',
    type: 'file',
    sort: 'name',
    page: 2,
    selectedPath: '/a/file',
    scrollTop: 123,
  },
  windowScroll: 500,
})

describe('session analysis contexts', () => {
  it('keeps isolated copies of UI state and shares the latest context for repeated paths', () => {
    const cache = new AnalysisContexts()
    const value = context()
    cache.remember('/a', value)
    value.directory.query = 'changed outside'
    const restored = cache.get('/a')!
    expect(restored.directory.query).toBe('中文 $(literal)')
    restored.directory.selectedPath = '/a/changed'
    expect(cache.get('/a')?.directory.selectedPath).toBe('/a/file')
    cache.remember('/a', { ...context(), view: 'large' })
    expect(cache.get('/a')?.view).toBe('large')
    expect(Object.keys(cache.get('/a')!).sort()).toEqual(['directory', 'view', 'windowScroll'])
  })
  it('bounds retained directories and drops contexts evicted or removed by a new history branch', () => {
    const cache = new AnalysisContexts()
    for (let i = 0; i < 80; i++) cache.remember(`/visit-${i}`, context())
    expect(cache.get('/visit-47')).toBeUndefined()
    expect(cache.get('/visit-48')).toBeDefined()
    cache.retain(['/visit-79', '/visit-79', '/new'])
    expect(cache.get('/visit-48')).toBeUndefined()
    expect(cache.get('/visit-79')).toBeDefined()
    expect(cache.get('/new')).toBeUndefined()
    expect(new AnalysisContexts().get('/visit-79')).toBeUndefined()
  })
})
