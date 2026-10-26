// Observation only: never starts, cancels or replays an execution. Callers own
// the observation lifetime; failed reads leave the previous evidence intact.
export function watchExecution<T>({
  load,
  onValue,
  onError,
  signal,
}: {
  load: () => Promise<T>
  onValue: (value: T) => void
  onError: (message: string) => void
  signal: AbortSignal
}) {
  let timer: ReturnType<typeof setTimeout> | undefined
  let failures = 0
  let stopped = false
  const inactive = () => stopped || signal.aborted
  const stop = () => {
    stopped = true
    clearTimeout(timer)
  }
  signal.addEventListener('abort', stop, { once: true })
  async function read() {
    if (inactive()) return
    try {
      const value = await load()
      if (inactive()) return
      failures = 0
      onValue(value)
      onError('')
    } catch (error) {
      if (inactive()) return
      failures++
      onError(
        `执行状态更新失败，保留上次记录；不会重新执行。${String(error).replace(/^Error: /, '')}`,
      )
    }
    if (!inactive()) timer = setTimeout(() => void read(), Math.min(1500 * 2 ** failures, 15000))
  }
  void read()
  return () => {
    stop()
    signal.removeEventListener('abort', stop)
  }
}

// A late list response must not turn a committed terminal result back into a
// running batch. Within a running batch, committed move counts never decrease.
export function preserveExecutionProgress<
  T extends { id: string; state: string; movedCount: number; reviewedAt?: number | null },
>(next: T[], previous: T[]): T[] {
  const known = new Map(previous.map((item) => [item.id, item]))
  return next.map((item) => {
    const prior = known.get(item.id)
    return prior &&
      ((prior.state !== 'running' && item.state === 'running') ||
        prior.movedCount > item.movedCount ||
        (prior.reviewedAt != null && item.reviewedAt == null))
      ? prior
      : item
  })
}
