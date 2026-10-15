import { afterEach, describe, expect, it, vi } from 'vitest'
import { observeTask, type TaskTransport } from './task-watcher'
import type { TaskEvent } from '../types'

const event = (state = 'succeeded'): TaskEvent => ({
  id: 'id',
  kind: 'status',
  state,
  result: { cpu: { usage: 12 } },
  error: null,
})

function fixture() {
  let receive!: (event: TaskEvent) => void
  const dispose = vi.fn()
  const transport: TaskTransport = {
    listen: vi.fn(async (handler) => {
      receive = handler
      return dispose
    }),
    snapshot: vi.fn(async () => event('running')),
  }
  return { transport, dispose, receive: (value: TaskEvent) => receive(value) }
}

afterEach(() => {
  vi.useRealTimers()
})

describe('native task observation', () => {
  it('recovers a lost completion event through snapshots when event registration fails', async () => {
    vi.useFakeTimers()
    const f = fixture()
    vi.mocked(f.transport.listen).mockRejectedValue(new Error('no event channel'))
    vi.mocked(f.transport.snapshot)
      .mockResolvedValueOnce(event('running'))
      .mockResolvedValueOnce(event())
    const result = observeTask(f.transport, { id: 'id', kind: 'status', pollMs: 10 })
    await vi.advanceTimersByTimeAsync(10)
    expect(await result).toEqual(event())
    expect(vi.getTimerCount()).toBe(0)
  })

  it('buffers very fast completion until native startup is accepted and ignores other tasks', async () => {
    const f = fixture()
    let accept!: () => void
    const start = new Promise<void>((resolve) => {
      accept = resolve
    })
    const onStart = vi.fn()
    const result = observeTask(f.transport, {
      id: 'id',
      kind: 'status',
      start: () => start,
      onStart,
    })
    await Promise.resolve()
    f.receive({ ...event(), id: 'other' })
    f.receive({ ...event(), kind: 'clean' })
    f.receive(event())
    expect(onStart).not.toHaveBeenCalled()
    accept()
    expect(await result).toEqual(event())
    expect(onStart).toHaveBeenCalledOnce()
    await vi.waitFor(() => expect(f.dispose).toHaveBeenCalledOnce())
  })

  it('cleans up a late subscription on abort without cancelling or starting native work', async () => {
    const f = fixture()
    let attach!: (dispose: () => void) => void
    vi.mocked(f.transport.listen).mockReturnValue(
      new Promise((resolve) => {
        attach = resolve
      }),
    )
    const controller = new AbortController()
    const result = observeTask(f.transport, { id: 'id', kind: 'status', signal: controller.signal })
    const rejected = expect(result).rejects.toThrow('停止跟踪')
    controller.abort()
    attach(f.dispose)
    await rejected
    await Promise.resolve()
    expect(f.dispose).toHaveBeenCalledOnce()
    const start = vi.fn()
    await expect(
      observeTask(f.transport, { id: 'id', kind: 'status', start, signal: controller.signal }),
    ).rejects.toThrow('停止跟踪')
    expect(start).not.toHaveBeenCalled()
  })

  it('stops after repeated status failures and can subsequently attach to the same native task', async () => {
    vi.useFakeTimers()
    const f = fixture()
    vi.mocked(f.transport.snapshot).mockRejectedValue(new Error('IPC unavailable'))
    const failed = expect(
      observeTask(f.transport, { id: 'id', kind: 'status', pollMs: 10 }),
    ).rejects.toThrow('继续跟踪')
    await vi.advanceTimersByTimeAsync(30)
    await failed
    expect(f.dispose).toHaveBeenCalledOnce()
    vi.mocked(f.transport.snapshot).mockResolvedValue(event())
    expect(await observeTask(f.transport, { id: 'id', kind: 'status' })).toEqual(event())
    expect(vi.getTimerCount()).toBe(0)
  })

  it('preserves terminal cancellation and bounds an unresponsive status request', async () => {
    vi.useFakeTimers()
    const f = fixture()
    vi.mocked(f.transport.snapshot).mockResolvedValueOnce({
      ...event('cancelled'),
      result: null,
      error: '已取消',
    })
    expect((await observeTask(f.transport, { id: 'id', kind: 'status' })).state).toBe('cancelled')
    vi.mocked(f.transport.snapshot).mockReturnValue(new Promise(() => {}))
    const expired = expect(
      observeTask(f.transport, { id: 'id', kind: 'status', timeoutMs: 20 }),
    ).rejects.toThrow('跟踪已超时')
    await vi.advanceTimersByTimeAsync(20)
    await expired
    expect(vi.getTimerCount()).toBe(0)
  })
})
