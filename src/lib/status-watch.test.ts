import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  followStatus,
  statusWatchSnapshot,
  type StatusTransport,
  type StatusWatchSnapshot,
  type StatusInterval,
} from './status-watch'

const snapshot = (sequence = 1): StatusWatchSnapshot => ({
  version: 2,
  id: 'session',
  state: 'running',
  intervalSeconds: 2,
  sequence,
  receivedAt: 1000,
  data: { collected_at: '2026-10-20T00:00:00Z' },
  error: null,
})
afterEach(() => vi.useRealTimers())
describe('status watch protocol and lifecycle', () => {
  it('rejects mismatches, unknown protocols, broken clocks and missing data', () => {
    expect(statusWatchSnapshot(snapshot(), 'session').sequence).toBe(1)
    for (const value of [
      { ...snapshot(), id: 'other' },
      { ...snapshot(), version: 3 },
      { ...snapshot(), version: 1 },
      { ...snapshot(), intervalSeconds: 5 },
      { ...snapshot(), intervalSeconds: undefined },
      { ...snapshot(), sequence: -1 },
      { ...snapshot(), data: null },
      { ...snapshot(), state: 'unexpected' },
      { ...snapshot(), receivedAt: null },
      { ...snapshot(), data: { collected_at: 'bad' } },
    ])
      expect(() => statusWatchSnapshot(value as StatusWatchSnapshot, 'session')).toThrow()
  })
  it('binds the chosen interval and refuses unsupported admission or mismatched responses', async () => {
    for (const interval of [1, 2, 5] as const) {
      const controller = new AbortController()
      const transport: StatusTransport = {
        start: vi.fn(async () => {}),
        read: vi.fn(async () => ({ ...snapshot(), intervalSeconds: interval })),
        stop: vi.fn(async () => {}),
      }
      await followStatus(
        transport,
        'session',
        controller.signal,
        () => controller.abort(),
        interval,
      )
      expect(transport.start).toHaveBeenCalledWith('session', interval)
      expect(transport.stop).toHaveBeenCalledWith('session')
    }
    const transport: StatusTransport = {
      start: vi.fn(async () => {}),
      read: vi.fn(async () => snapshot()),
      stop: vi.fn(async () => {}),
    }
    await expect(
      followStatus(
        transport,
        'session',
        new AbortController().signal,
        vi.fn(),
        7 as StatusInterval,
      ),
    ).rejects.toThrow('周期')
    expect(transport.start).not.toHaveBeenCalled()
    await expect(
      followStatus(transport, 'session', new AbortController().signal, vi.fn(), 5),
    ).rejects.toThrow('无法识别')
    expect(transport.stop).toHaveBeenCalledWith('session')
  })
  it('stops exact session even when cancelled while admission is pending, ignoring late data', async () => {
    let admit!: () => void
    const start = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          admit = resolve
        }),
    )
    const transport: StatusTransport = { start, read: vi.fn(), stop: vi.fn(async () => {}) }
    const controller = new AbortController()
    const receive = vi.fn()
    const result = followStatus(transport, 'session', controller.signal, receive)
    controller.abort()
    admit()
    await result
    expect(transport.stop).toHaveBeenCalledWith('session')
    expect(transport.read).not.toHaveBeenCalled()
    expect(receive).not.toHaveBeenCalled()
  })
  it('serializes reads, refuses reordered data and never restarts on failure', async () => {
    vi.useFakeTimers()
    let complete!: (value: StatusWatchSnapshot) => void
    const transport: StatusTransport = {
      start: vi.fn(async () => {}),
      read: vi.fn(
        () =>
          new Promise<StatusWatchSnapshot>((resolve) => {
            complete = resolve
          }),
      ),
      stop: vi.fn(async () => {}),
    }
    const received: number[] = []
    const result = followStatus(transport, 'session', new AbortController().signal, (s) =>
      received.push(s.sequence),
    )
    const error = expect(result).rejects.toThrow('次序异常')
    await vi.advanceTimersByTimeAsync(2000)
    expect(transport.read).toHaveBeenCalledTimes(1)
    complete(snapshot(2))
    await vi.advanceTimersByTimeAsync(1000)
    expect(transport.read).toHaveBeenCalledTimes(2)
    complete(snapshot(1))
    await error
    expect(received).toEqual([2])
    expect(transport.start).toHaveBeenCalledTimes(1)
    expect(transport.stop).toHaveBeenCalledTimes(1)
  })
  it('failed admission still attempts to stop only its own possible orphan', async () => {
    const transport: StatusTransport = {
      start: vi.fn(async () => {
        throw new Error('response lost')
      }),
      read: vi.fn(),
      stop: vi.fn(async () => {}),
    }
    await expect(
      followStatus(transport, 'session', new AbortController().signal, vi.fn()),
    ).rejects.toThrow('response lost')
    expect(transport.stop).toHaveBeenCalledWith('session')
    expect(transport.read).not.toHaveBeenCalled()
  })
  it('pausing a pending read stops promptly and rejects its late update', async () => {
    let complete!: (value: StatusWatchSnapshot) => void
    const controller = new AbortController()
    const transport: StatusTransport = {
      start: vi.fn(async () => {}),
      read: vi.fn(
        () =>
          new Promise<StatusWatchSnapshot>((resolve) => {
            complete = resolve
          }),
      ),
      stop: vi.fn(async () => {}),
    }
    const receive = vi.fn()
    const result = followStatus(transport, 'session', controller.signal, receive)
    await vi.waitFor(() => expect(transport.read).toHaveBeenCalledTimes(1))
    controller.abort()
    await result
    expect(transport.stop).toHaveBeenCalledWith('session')
    complete(snapshot())
    await Promise.resolve()
    expect(receive).not.toHaveBeenCalled()
  })
  it('read timeout stops the collector instead of keeping a stuck observer', async () => {
    vi.useFakeTimers()
    const transport: StatusTransport = {
      start: vi.fn(async () => {}),
      read: vi.fn(() => new Promise<StatusWatchSnapshot>(() => {})),
      stop: vi.fn(async () => {}),
    }
    const result = followStatus(transport, 'session', new AbortController().signal, vi.fn())
    const error = expect(result).rejects.toThrow('响应超时')
    await vi.advanceTimersByTimeAsync(5001)
    await error
    expect(transport.stop).toHaveBeenCalledTimes(1)
  })
})
