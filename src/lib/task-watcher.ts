import type { TaskEvent, TaskKind } from '../types'

export interface TaskTransport {
  listen: (receive: (event: TaskEvent) => void) => Promise<() => void>
  snapshot: (id: string) => Promise<TaskEvent>
}

// Events provide immediate feedback; snapshots recover missed/late events.
// Stopping observation never cancels the underlying native task.
export function observeTask(
  transport: TaskTransport,
  options: {
    id: string
    kind: TaskKind
    start?: () => Promise<void>
    onStart?: () => void
    signal?: AbortSignal
    pollMs?: number
    timeoutMs?: number
  },
): Promise<TaskEvent> {
  return new Promise((resolve, reject) => {
    let done = false
    let started = false
    let pending: TaskEvent | undefined
    let unlisten: (() => void) | undefined
    let poll: ReturnType<typeof setTimeout> | undefined
    let deadline: ReturnType<typeof setTimeout> | undefined
    let failures = 0
    const dispose = (unsubscribe: () => void) => {
      try {
        void Promise.resolve(unsubscribe()).catch(() => {})
      } catch {
        /* Best effort on WebView teardown. */
      }
    }
    const stop = (error?: Error, event?: TaskEvent) => {
      if (done) return
      done = true
      clearTimeout(poll)
      clearTimeout(deadline)
      options.signal?.removeEventListener('abort', abort)
      if (unlisten) dispose(unlisten)
      if (error) reject(error)
      else resolve(event!)
    }
    const abort = () => stop(new Error('已停止跟踪任务；本机任务不会因此自动取消。'))
    const receive = (event: TaskEvent) => {
      if (done || event.id !== options.id || event.kind !== options.kind) return
      if (event.state === 'running' || event.state === 'queued') return
      if (!['succeeded', 'failed', 'cancelled', 'interrupted'].includes(event.state)) {
        stop(new Error('任务状态无法识别，请从操作历史重新检查。'))
        return
      }
      if (!started) pending = event
      else stop(undefined, event)
    }
    const check = async () => {
      if (done) return
      try {
        const snapshot = await transport.snapshot(options.id)
        if (snapshot.id !== options.id || snapshot.kind !== options.kind)
          throw new Error('任务快照不匹配。')
        receive(snapshot)
        failures = 0
      } catch {
        if (++failures >= 3)
          stop(new Error('暂时无法获取任务状态；本机任务可能仍在运行，请从操作历史继续跟踪。'))
      }
      if (!done) poll = setTimeout(() => void check(), options.pollMs ?? 1500)
    }
    if (options.signal?.aborted) {
      abort()
      return
    }
    options.signal?.addEventListener('abort', abort, { once: true })
    deadline = setTimeout(
      () => stop(new Error('任务跟踪已超时；请从操作历史检查本机任务状态。')),
      options.timeoutMs ?? 10 * 60_000,
    )
    // A slow/failed event subscription does not prevent polling or task startup.
    void Promise.resolve()
      .then(() => transport.listen(receive))
      .then((unsubscribe) => {
        if (done) dispose(unsubscribe)
        else unlisten = unsubscribe
      })
      .catch(() => {})
    void (async () => {
      try {
        if (done) return
        await options.start?.()
        if (done) return
        started = true
        options.onStart?.()
        if (pending) receive(pending)
        if (!done) void check()
      } catch (error) {
        stop(error instanceof Error ? error : new Error(String(error)))
      }
    })()
  })
}
