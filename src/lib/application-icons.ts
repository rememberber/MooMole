import { applicationIcon, desktop } from './api'

const PREFIX = 'data:image/png;base64,'
const MAX_ICON = 32 * 1024 + PREFIX.length
const TTL = 10 * 60 * 1000

// Small, session-only LRU. Queued work can be abandoned; an in-flight native
// read keeps its slot until it actually returns. Nothing is persisted.
export function createIconLoader(load: (taskId: string, index: number) => Promise<string | null>) {
  const cache = new Map<string, { value: string | null; at: number }>()
  let bytes = 0
  let active = 0
  type Request = {
    key: string
    taskId: string
    index: number
    signal: AbortSignal
    started: boolean
    finish: (value: string | null) => void
  }
  const queue: Request[] = []
  function remember(key: string, value: string | null) {
    bytes -= cache.get(key)?.value?.length ?? 0
    cache.delete(key)
    cache.set(key, { value, at: performance.now() })
    bytes += value?.length ?? 0
    while (cache.size > 128 || bytes > 2 * 1024 * 1024) {
      const first = cache.keys().next().value!
      bytes -= cache.get(first)?.value?.length ?? 0
      cache.delete(first)
    }
  }
  function pump() {
    while (active < 2 && queue.length) {
      const request = queue.shift()!
      if (request.signal.aborted) {
        request.finish(null)
        continue
      }
      request.started = true
      active += 1
      void Promise.resolve()
        .then(() => load(request.taskId, request.index))
        .catch(() => null)
        .then((value) => {
          const valid =
            typeof value === 'string' &&
            value.length <= MAX_ICON &&
            /^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/.test(value)
              ? value
              : null
          if (!request.signal.aborted) remember(request.key, valid)
          request.finish(valid)
        })
        .finally(() => {
          active -= 1
          pump()
        })
    }
  }
  return (taskId: string, index: number, signal: AbortSignal): Promise<string | null> => {
    if (signal.aborted) return Promise.resolve(null)
    const key = `${taskId}:${index}`
    const cached = cache.get(key)
    if (cached && performance.now() - cached.at < TTL) {
      cache.delete(key)
      cache.set(key, cached)
      return Promise.resolve(cached.value)
    }
    if (cached) {
      bytes -= cached.value?.length ?? 0
      cache.delete(key)
    }
    if (queue.length >= 64) return Promise.resolve(null)
    return new Promise((resolve) => {
      const request: Request = {
        key,
        taskId,
        index,
        signal,
        started: false,
        finish(value) {
          signal.removeEventListener('abort', cancel)
          resolve(signal.aborted ? null : value)
        },
      }
      function cancel() {
        if (request.started) return
        const position = queue.indexOf(request)
        if (position !== -1) queue.splice(position, 1)
        request.finish(null)
      }
      signal.addEventListener('abort', cancel, { once: true })
      queue.push(request)
      pump()
    })
  }
}
export const loadApplicationIcon = createIconLoader((taskId, index) =>
  desktop ? applicationIcon(taskId, index) : Promise.resolve(null),
)
