import { describe, expect, it } from 'vitest'
import { cleanupVisibilityText, readCleanupVisibility } from './cleanup-visibility'

const idle = {
  version: 1,
  readOnly: true,
  requestId: null,
  state: 'idle',
  startedAt: null,
  finishedAt: null,
  observation: null,
}
const running = {
  ...idle,
  state: 'running',
  requestId: '8a2d1589-dda4-4fb5-b22f-150724c26fbb',
  startedAt: 1000,
}
const finished = {
  ...running,
  state: 'finished',
  finishedAt: 2000,
  observation: 'completeViewObserved',
}

describe('cleanup visibility diagnostic', () => {
  it('accepts bound lifecycle snapshots and never describes the proof as cleanup permission', () => {
    for (const value of [idle, running, { ...running, state: 'cancelling' }, finished])
      expect(readCleanupVisibility(value)).toEqual(value)
    expect(cleanupVisibilityText(readCleanupVisibility(finished))).toContain(
      '仍须对所选对象逐项复核',
    )
    for (const observation of ['incompleteOrFailed', 'cancelled', 'expired', 'unsupported']) {
      const snapshot = readCleanupVisibility({ ...finished, observation })
      expect(cleanupVisibilityText(snapshot)).not.toContain('已读到')
    }
  })
  it('rejects mismatched phases, missing provenance, unsafe timestamps and unknown results', () => {
    for (const value of [
      null,
      [],
      {},
      { ...idle, version: 2 },
      { ...idle, readOnly: false },
      { ...idle, readOnly: undefined },
      { ...idle, state: 'ready' },
      { ...idle, observation: 'completeViewObserved' },
      { ...running, requestId: null },
      { ...running, requestId: '../path' },
      { ...running, startedAt: NaN },
      { ...running, startedAt: 1.5 },
      { ...running, startedAt: 0 },
      { ...running, startedAt: Number.MAX_SAFE_INTEGER },
      { ...running, finishedAt: 2000 },
      { ...running, observation: 'completeViewObserved' },
      { ...finished, finishedAt: null },
      { ...finished, observation: null },
      { ...finished, observation: 'readyToClean' },
    ])
      expect(() => readCleanupVisibility(value)).toThrow('无法确认')
  })
})
