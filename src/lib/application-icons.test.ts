import { describe, expect, it, vi } from 'vitest'
import { createIconLoader } from './application-icons'

const png = 'data:image/png;base64,aGVsbG8='
const tick = async () => {
  for (let i = 0; i < 8; i++) await Promise.resolve()
}

describe('optional application artwork', () => {
  it('bounds concurrency, abandons queued rows and keeps in-flight slots until completion', async () => {
    const calls: number[] = []
    const release: ((value: string | null) => void)[] = []
    const load = createIconLoader((_task, index) => {
      calls.push(index)
      return new Promise((resolve) => release.push(resolve))
    })
    const controllers = Array.from({ length: 4 }, () => new AbortController())
    const reads = controllers.map((c, index) => load('task', index, c.signal))
    await tick()
    expect(calls).toEqual([0, 1])
    controllers[0].abort()
    controllers[2].abort()
    expect(await reads[2]).toBeNull()
    await tick()
    expect(calls).toEqual([0, 1])
    release[0](png)
    expect(await reads[0]).toBeNull()
    await tick()
    expect(calls).toEqual([0, 1, 3])
    release[1](png)
    release[2](null)
    expect(await reads[1]).toBe(png)
    expect(await reads[3]).toBeNull()
  })
  it('binds cached artwork to task and row and refuses foreign URLs, SVG and oversized payloads', async () => {
    const calls: string[] = []
    const answers = [
      png,
      'https://untrusted/icon.png',
      'data:image/svg+xml;base64,abc',
      `data:image/png;base64,${'a'.repeat(32769)}`,
    ]
    const load = createIconLoader(async (id, index) => {
      calls.push(`${id}:${index}`)
      return answers[index] ?? null
    })
    const signal = new AbortController().signal
    expect(await load('first', 0, signal)).toBe(png)
    expect(await load('first', 0, signal)).toBe(png)
    expect(calls).toHaveLength(1)
    for (const index of [1, 2, 3]) expect(await load('first', index, signal)).toBeNull()
    expect(await load('second', 0, signal)).toBe(png)
    expect(calls.at(-1)).toBe('second:0')
    const aborted = new AbortController()
    aborted.abort()
    expect(await load('second', 1, aborted.signal)).toBeNull()
    expect(calls).toHaveLength(5)
  })
  it('evicts old entries and bounds waiting requests without rejecting optional decoration', async () => {
    let count = 0
    const load = createIconLoader(async () => {
      count += 1
      return png
    })
    const signal = new AbortController().signal
    for (let i = 0; i < 129; i++) await load('task', i, signal)
    expect(await load('task', 0, signal)).toBe(png)
    expect(count).toBe(130)
    const pending = createIconLoader(() => new Promise(() => {}))
    const controllers = Array.from({ length: 66 }, () => new AbortController())
    const reads = controllers.map((controller, index) => pending('task', index, controller.signal))
    expect(await pending('task', 67, signal)).toBeNull()
    for (let i = 2; i < 66; i++) controllers[i].abort()
    expect(await Promise.all(reads.slice(2))).toEqual(Array(64).fill(null))
  })
  it('expires failed observations and bounds cache bytes separately from entry count', async () => {
    const clock = vi.spyOn(performance, 'now').mockReturnValue(0)
    try {
      let calls = 0
      const load = createIconLoader(async () => {
        calls += 1
        return null
      })
      const signal = new AbortController().signal
      await load('task', 0, signal)
      clock.mockReturnValue(599999)
      await load('task', 0, signal)
      expect(calls).toBe(1)
      clock.mockReturnValue(600000)
      await load('task', 0, signal)
      expect(calls).toBe(2)
      let largeCalls = 0
      const large = createIconLoader(async () => {
        largeCalls += 1
        return `data:image/png;base64,${'a'.repeat(32768)}`
      })
      for (let index = 0; index < 65; index++) await large('task', index, signal)
      await large('task', 0, signal)
      expect(largeCalls).toBe(66)
    } finally {
      clock.mockRestore()
    }
  })
})
