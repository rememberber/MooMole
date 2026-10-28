import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { windowDragFallback } from './window-drag'

beforeEach(() => vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] }))
afterEach(() => vi.useRealTimers())

const transport = () => ({
  position: vi.fn(async () => ({ x: -200, y: 400 })),
  scale: vi.fn(async () => 2),
  move: vi.fn(async (_point: { x: number; y: number }) => {}),
})

test('a click or tiny movement never repositions the window', async () => {
  const window = transport()
  const drag = windowDragFallback(window, { x: 100, y: 200 })
  drag.move({ x: 102, y: 201 })
  await vi.advanceTimersByTimeAsync(1000)
  expect(window.move).not.toHaveBeenCalled()
  drag.stop()
})

test('native handoff gets a grace period; a stalled drag follows physical displacement at Retina scale', async () => {
  const window = transport()
  const drag = windowDragFallback(window, { x: 100, y: 200 })
  drag.move({ x: 130, y: 220 })
  await vi.advanceTimersByTimeAsync(31)
  expect(window.move).not.toHaveBeenCalled()
  await vi.advanceTimersByTimeAsync(1)
  expect(window.move).toHaveBeenLastCalledWith({ x: -140, y: 440 })
  drag.stop()
  drag.move({ x: 400, y: 400 })
  await vi.advanceTimersByTimeAsync(500)
  expect(window.move).toHaveBeenCalledTimes(1)
})

test('a successful native move retires the fallback without overwriting native geometry', async () => {
  const window = transport()
  window.position.mockResolvedValueOnce({ x: -200, y: 400 }).mockResolvedValue({ x: -180, y: 440 })
  const drag = windowDragFallback(window, { x: 100, y: 200 })
  drag.move({ x: 130, y: 220 })
  await vi.advanceTimersByTimeAsync(32)
  drag.move({ x: 160, y: 250 })
  await vi.advanceTimersByTimeAsync(500)
  expect(window.move).not.toHaveBeenCalled()
})

test('position updates are serialized and coalesce to the latest held pointer', async () => {
  const window = transport()
  let finish!: () => void
  window.move.mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        finish = resolve
      }),
  )
  const drag = windowDragFallback(window, { x: 100, y: 200 })
  drag.move({ x: 110, y: 210 })
  await vi.advanceTimersByTimeAsync(32)
  drag.move({ x: 120, y: 220 })
  drag.move({ x: 130, y: 230 })
  await vi.advanceTimersByTimeAsync(100)
  expect(window.move).toHaveBeenCalledTimes(1)
  finish()
  await vi.advanceTimersByTimeAsync(16)
  expect(window.move).toHaveBeenCalledTimes(2)
  expect(window.move).toHaveBeenLastCalledWith({ x: -140, y: 460 })
  drag.stop()
})

test('release during a late initial read cannot move a window afterwards', async () => {
  const window = transport()
  let finish!: (point: { x: number; y: number }) => void
  window.position.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve
      }),
  )
  const drag = windowDragFallback(window, { x: 100, y: 200 })
  drag.move({ x: 130, y: 220 })
  await vi.advanceTimersByTimeAsync(32)
  drag.stop()
  finish({ x: -200, y: 400 })
  await vi.advanceTimersByTimeAsync(500)
  expect(window.move).not.toHaveBeenCalled()
})

test('a swallowed native move stream commits the observed release once, but never overwrites a native move', async () => {
  const window = transport()
  const drag = windowDragFallback(window, { x: 100, y: 200 })
  await vi.advanceTimersByTimeAsync(0)
  expect(await drag.finish({ x: 150, y: 230 })).toBe(true)
  expect(window.move).toHaveBeenCalledExactlyOnceWith({ x: -100, y: 460 })
  expect(await drag.finish({ x: 160, y: 240 })).toBe(false)
  const native = transport()
  native.position.mockResolvedValueOnce({ x: -200, y: 400 }).mockResolvedValue({ x: 0, y: 450 })
  const completed = windowDragFallback(native, { x: 100, y: 200 })
  await vi.advanceTimersByTimeAsync(0)
  expect(await completed.finish({ x: 150, y: 230 })).toBe(false)
  expect(native.move).not.toHaveBeenCalled()
})
