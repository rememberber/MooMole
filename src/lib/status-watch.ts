import type { Snapshot } from '../types'
export const statusIntervals = [1, 2, 5] as const
export type StatusInterval = (typeof statusIntervals)[number]

export interface StatusWatchSnapshot {
  version: 2
  id: string
  state: 'starting' | 'running' | 'stopped' | 'failed'
  intervalSeconds: StatusInterval
  sequence: number
  receivedAt: number | null
  data: Snapshot | null
  error: string | null
}
export function statusWatchSnapshot(
  value: StatusWatchSnapshot,
  id: string,
  interval: StatusInterval = 2,
): StatusWatchSnapshot {
  if (
    value.version !== 2 ||
    value.id !== id ||
    value.intervalSeconds !== interval ||
    !['starting', 'running', 'stopped', 'failed'].includes(value.state) ||
    !Number.isSafeInteger(value.sequence) ||
    value.sequence < 0 ||
    (value.data != null &&
      (!Number.isFinite(Date.parse(value.data.collected_at)) ||
        !Number.isFinite(value.receivedAt))) ||
    (value.sequence > 0 && value.data == null)
  )
    throw new Error('状态采集响应无法识别，请重新开始。')
  return value
}

export interface StatusTransport {
  start: (id: string, interval: StatusInterval) => Promise<void>
  read: (id: string) => Promise<StatusWatchSnapshot>
  stop: (id: string) => Promise<void>
}
function delay(signal: AbortSignal) {
  return new Promise<void>((resolve) => {
    const done = () => {
      clearTimeout(timer)
      signal.removeEventListener('abort', done)
      resolve()
    }
    const timer = setTimeout(done, 1000)
    signal.addEventListener('abort', done, { once: true })
    if (signal.aborted) done()
  })
}
function bounded<T>(request: Promise<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error('采集响应超时，已请求停止；请稍后重试。')),
      5000,
    )
    request.then(resolve, reject).finally(() => clearTimeout(timer))
  })
}
export async function followStatus(
  transport: StatusTransport,
  id: string,
  signal: AbortSignal,
  receive: (value: StatusWatchSnapshot) => void,
  interval: StatusInterval = 2,
): Promise<void> {
  let started = false
  let finished = false
  let sequence = 0
  let lastRead = Date.now()
  let cancelRead!: () => void
  const aborted = new Promise<undefined>((resolve) => {
    cancelRead = () => resolve(undefined)
  })
  signal.addEventListener('abort', cancelRead, { once: true })
  try {
    if (signal.aborted) return
    if (!statusIntervals.includes(interval)) throw new Error('采样周期无法识别，请重新选择。')
    started = true
    const admission = transport.start(id, interval)
    // A late admission response after cancellation/timeout still stops its ID.
    void admission.then(
      () => {
        if (signal.aborted || finished) void transport.stop(id).catch(() => {})
      },
      () => {},
    )
    await bounded(admission)
    while (!signal.aborted) {
      // Timers can be frozen while hidden/asleep. Resume requires a new collector.
      if (Date.now() - lastRead >= 12_000 || Date.now() < lastRead) {
        throw new Error('页面离开或休眠后已暂停，请重新开始采集。')
      }
      const response = await Promise.race([bounded(transport.read(id)), aborted])
      if (signal.aborted) return
      if (!response) throw new Error('状态采集响应缺失，请重新开始。')
      const value = statusWatchSnapshot(response, id, interval)
      lastRead = Date.now()
      if (value.sequence < sequence) throw new Error('状态采集响应次序异常，请重新开始。')
      sequence = value.sequence
      receive(value)
      if (value.state === 'failed') throw new Error(value.error ?? '状态采集已中断，请重新开始。')
      if (value.state === 'stopped') return
      await delay(signal)
    }
  } finally {
    finished = true
    signal.removeEventListener('abort', cancelRead)
    // Even aborting a pending start must stop that exact session after admission.
    if (started) await bounded(transport.stop(id))
  }
}
