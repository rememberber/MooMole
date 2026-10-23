import { expect, it } from 'vitest'
import cases from '../../tests/fixtures/analysis-protocol.json'
import { decodeAnalysis, analysisLimits } from './analysis-protocol'
import { entryBytes } from './analysis'
import { taskResult, TaskResultError } from './task-result'
import { failedAnalysisTask } from './analysis-task'

it('uses the same accepted and rejected field corpus as the native gate', () => {
  for (const [kind, samples] of Object.entries({ accept: cases.accept, reject: cases.reject }))
    for (const sample of samples) {
      const value = structuredClone(cases.base)
      const keys = sample.pointer.slice(1).split('/')
      let parent: any = value
      for (const key of keys.slice(0, -1)) parent = parent[key]
      parent[keys.at(-1)!] = sample.value
      if (kind === 'accept') expect(decodeAnalysis(value)).toBe(value)
      else expect(() => decodeAnalysis(value), sample.pointer).toThrow('未更新')
    }
})

it('rejects missing required fields and duplicate paths without keeping a partial new list', () => {
  for (const key of ['path', 'scan_status', 'total_size', 'entries']) {
    const value: Record<string, unknown> = structuredClone(cases.base)
    delete value[key]
    expect(() => decodeAnalysis(value)).toThrow()
  }
  for (const key of ['name', 'path', 'scan_status', 'size', 'is_dir']) {
    const value = structuredClone(cases.base)
    delete (value.entries[0] as Record<string, unknown>)[key]
    expect(() => decodeAnalysis(value)).toThrow()
  }
  const value = structuredClone(cases.base)
  value.entries.push({ ...value.entries[0] })
  expect(() => decodeAnalysis(value)).toThrow()
})

it('bounds array and UTF-8 field sizes while preserving zero and unknown sizes and literal names', () => {
  const value = structuredClone(cases.base)
  value.entries = Array.from({ length: analysisLimits.entries }, (_, i) => ({
    ...value.entries[0],
    name: `item${i}`,
    path: `/fixture/item${i}`,
  }))
  expect(decodeAnalysis(value).entries).toHaveLength(20_000)
  value.entries.push({ ...value.entries[0] })
  expect(() => decodeAnalysis(value)).toThrow('更小的目录')
  for (const name of ['中文\n[*] $(literal)', 'x'.repeat(1024)]) {
    const entry = { ...cases.base.entries[0], name, path: `/fixture/${name}`, size: -1 }
    expect(entryBytes(decodeAnalysis({ ...cases.base, entries: [entry] }).entries[0])).toBeNull()
    entry.size = 0
    expect(entryBytes(decodeAnalysis({ ...cases.base, entries: [entry] }).entries[0])).toBe(0)
  }
  for (const name of ['x'.repeat(1025), '中'.repeat(342)])
    expect(() =>
      decodeAnalysis({
        ...cases.base,
        entries: [{ ...cases.base.entries[0], name, path: `/fixture/${name}` }],
      }),
    ).toThrow()
  expect(
    decodeAnalysis({ ...cases.base, large_files: Array(1000).fill(null) }).large_files,
  ).toHaveLength(1000)
  expect(() => decodeAnalysis({ ...cases.base, large_files: Array(1001).fill(null) })).toThrow()
  const root = '/' + 'x'.repeat(4095)
  expect(decodeAnalysis({ ...cases.base, path: root, entries: [] }).path).toBe(root)
  expect(() => decodeAnalysis({ ...cases.base, path: root + 'x', entries: [] })).toThrow()
  expect(
    decodeAnalysis({
      ...cases.base,
      path: '/',
      entries: [{ ...cases.base.entries[0], path: '/child' }],
    }).path,
  ).toBe('/')
})

it('classifies malformed successful results as unavailable with the same task identity', () => {
  for (const result of [
    [],
    {},
    { ...cases.base, entries: [null] },
    { ...cases.base, total_size: NaN },
  ]) {
    try {
      taskResult({ id: 'analysis', kind: 'analyze', state: 'succeeded', error: null, result })
      throw new Error('expected rejection')
    } catch (error) {
      expect(error).toBeInstanceOf(TaskResultError)
      expect(
        failedAnalysisTask({ id: 'analysis', path: '/fixture', state: 'running' }, error),
      ).toMatchObject({
        state: 'unavailable',
        path: '/fixture',
        detail: expect.stringContaining('未更新'),
      })
      const failed = failedAnalysisTask(
        { id: 'analysis', path: '/fixture', state: 'running' },
        error,
      )
      expect(failed.detail).not.toContain('诊断：')
      expect(failed.diagnostic).toMatch(/^不匹配字段：/)
    }
  }
})
