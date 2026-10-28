import { getLocale } from '../i18n/locale'
import { describe, expect, it } from 'vitest'
import { matchingProcesses, processMetric, processSampleTime } from './process-snapshot'
import type { ProcessSample } from '../types'

const samples: ProcessSample[] = [
  { pid: 11, name: 'Worker', cpu: 50 },
  { pid: 10, name: 'Worker', cpu: 120, memory_bytes: 1024 },
  { pid: 22, name: '中文 空格 [*] $(literal)', cpu: 0, memory_bytes: 0 },
  { pid: 20, name: 'zed', cpu: null, memory_bytes: 5120 },
  { pid: 2, name: 'Beta', cpu: -1, memory_bytes: Number.NaN },
]

describe('Mole process samples', () => {
  it('keeps zero and values above 100 while treating missing, negative and non-finite metrics as unknown', () => {
    expect(processMetric(0)).toBe(0)
    expect(processMetric(120)).toBe(120)
    for (const value of [undefined, null, -1, NaN, Infinity, '1', false])
      expect(processMetric(value)).toBeNull()
    expect(matchingProcesses(samples, '', 'cpu').map((p) => p.pid)).toEqual([10, 11, 22, 2, 20])
    expect(matchingProcesses(samples, '', 'memory').map((p) => p.pid)).toEqual([20, 10, 22, 2, 11])
  })
  it('filters names and PIDs literally and retains distinct same-name records without mutating input', () => {
    expect(matchingProcesses(samples, ' WORKER ', 'pid').map((p) => p.pid)).toEqual([10, 11])
    expect(matchingProcesses(samples, '11', 'name')).toEqual([samples[0]])
    expect(matchingProcesses(samples, '[*] $(literal)', 'cpu')).toEqual([samples[2]])
    expect(matchingProcesses(samples, 'missing', 'name')).toEqual([])
    const names = matchingProcesses(samples, '', 'name').map((p) => p.name)
    expect(names).toEqual([...names].sort((a, b) => a.localeCompare(b, getLocale())))
    expect(samples.map((p) => p.pid)).toEqual([11, 10, 22, 20, 2])
  })
  it('never substitutes snapshot time for an absent or invalid process sample time', () => {
    for (const value of [undefined, null, '', 'invalid', 123])
      expect(processSampleTime(value)).toBeNull()
    expect(processSampleTime('2026-10-17T01:00:00+08:00')).toBe('2026-10-16T17:00:00.000Z')
  })
})
