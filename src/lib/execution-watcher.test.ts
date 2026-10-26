import { afterEach, describe, expect, it, vi } from 'vitest'
import { preserveExecutionProgress, watchExecution } from './execution-watcher'

afterEach(() => vi.useRealTimers())
describe('execution observation never repeats mutations', () => {
  it('serializes reads, backs off failures, retains prior evidence and recovers', async () => {
    vi.useFakeTimers()
    const controller = new AbortController()
    const values: number[] = []
    const errors: string[] = []
    let resolve!: (n: number) => void
    const load = vi
      .fn<() => Promise<number>>()
      .mockReturnValueOnce(
        new Promise((done) => {
          resolve = done
        }),
      )
      .mockRejectedValueOnce(new Error('fixture read failure'))
      .mockResolvedValue(2)
    const stop = watchExecution({
      load,
      signal: controller.signal,
      onValue: (n) => values.push(n),
      onError: (s) => errors.push(s),
    })
    await vi.advanceTimersByTimeAsync(15000)
    expect(load).toHaveBeenCalledTimes(1)
    resolve(1)
    await vi.advanceTimersByTimeAsync(0)
    expect(values).toEqual([1])
    await vi.advanceTimersByTimeAsync(1500)
    expect(values).toEqual([1])
    expect(errors.at(-1)).toContain('保留上次记录')
    await vi.advanceTimersByTimeAsync(2999)
    expect(load).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(1)
    expect(values).toEqual([1, 2])
    expect(errors.at(-1)).toBe('')
    controller.abort()
    stop()
    await vi.advanceTimersByTimeAsync(30000)
    expect(load).toHaveBeenCalledTimes(3)
  })
  it('ignores late reads after abort or disposal and never starts an already aborted observation', async () => {
    for (const abort of [true, false]) {
      const controller = new AbortController()
      let resolve!: (n: number) => void
      const onValue = vi.fn(),
        onError = vi.fn()
      const stop = watchExecution({
        load: () =>
          new Promise((done) => {
            resolve = done
          }),
        signal: controller.signal,
        onValue,
        onError,
      })
      if (abort) controller.abort()
      else stop()
      resolve(1)
      await Promise.resolve()
      expect(onValue).not.toHaveBeenCalled()
      expect(onError).not.toHaveBeenCalled()
      stop()
    }
    const controller = new AbortController()
    controller.abort()
    const load = vi.fn()
    watchExecution({ load, signal: controller.signal, onValue: vi.fn(), onError: vi.fn() })()
    expect(load).not.toHaveBeenCalled()
  })
})

it('keeps committed progress when an older response arrives after completion', () => {
  const running = { id: 'first', state: 'running', movedCount: 1 }
  const terminal = { ...running, state: 'cancelled' }
  expect(preserveExecutionProgress([running], [terminal])).toEqual([terminal])
  expect(preserveExecutionProgress([{ ...running, movedCount: 0 }], [running])).toEqual([running])
  const next = [
    { ...terminal, movedCount: 2 },
    { id: 'second', state: 'running', movedCount: 0 },
  ]
  expect(preserveExecutionProgress(next, [running])).toEqual(next)
})

it('stale cleanup summaries keep a known review without changing the unknown count', () => {
  const reviewed = {
    id: 'first',
    state: 'interrupted',
    movedCount: 1,
    uncertainCount: 1,
    reviewedAt: 12,
  }
  const stale = { ...reviewed, reviewedAt: null as number | null }
  expect(preserveExecutionProgress([stale], [reviewed])).toEqual([reviewed])
})
