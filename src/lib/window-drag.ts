// Native dragging remains the first choice. Some macOS virtual-input sessions
// deliver a valid gesture but AppKit never moves the window. Only that case
// uses this window-local fallback; no global input hook or Accessibility API.
export interface DragPoint {
  x: number
  y: number
}
interface DragWindow {
  position: () => Promise<DragPoint>
  scale: () => Promise<number>
  move: (point: DragPoint) => Promise<void>
}

export function windowDragFallback(transport: DragWindow, start: DragPoint) {
  let stopped = false
  let cancelled = false
  let latest = start
  let pending = false
  let fallback = false
  let timer: ReturnType<typeof setTimeout> | undefined
  let origin: DragPoint | null = null
  let scale = 1
  let revision = 0
  const startedAt = performance.now()
  const stop = () => {
    stopped = true
    cancelled = true
    clearTimeout(timer)
  }
  const ready = Promise.all([transport.position(), transport.scale()])
    .then(([position, factor]) => {
      if (
        !Number.isFinite(factor) ||
        factor <= 0 ||
        !Number.isFinite(position.x) ||
        !Number.isFinite(position.y)
      ) {
        stop()
        return
      }
      origin = position
      scale = factor
    })
    .catch(stop)
  async function follow() {
    if (stopped || pending) return
    pending = true
    const observedRevision = revision
    try {
      await ready
      if (stopped || !origin) return
      if (!fallback) {
        const current = await transport.position()
        if (stopped) return
        // The native window manager took over, including snapping/tiling.
        if (Math.abs(current.x - origin.x) > 1 || Math.abs(current.y - origin.y) > 1) {
          stop()
          return
        }
        fallback = true
      }
      if (!stopped)
        await transport.move({
          x: Math.round(origin.x + (latest.x - start.x) * scale),
          y: Math.round(origin.y + (latest.y - start.y) * scale),
        })
    } catch {
      stop()
    } finally {
      pending = false
      if (!stopped && revision !== observedRevision) schedule()
    }
  }
  function schedule() {
    if (stopped || pending || timer != null) return
    timer = setTimeout(
      () => {
        timer = undefined
        void follow()
      },
      fallback ? 16 : Math.max(0, 32 - (performance.now() - startedAt)),
    )
  }
  return {
    move(point: DragPoint) {
      if (stopped || !Number.isFinite(point.x) || !Number.isFinite(point.y)) return
      latest = point
      revision++
      if (Math.hypot(point.x - start.x, point.y - start.y) >= 4) schedule()
    },
    async finish(point: DragPoint) {
      if (stopped) return false
      stopped = true
      clearTimeout(timer)
      // AppKit can swallow the move stream on a failed handoff. A genuine
      // release still arrives: commit its final position once, only if the
      // initial read completed and native dragging did not move this window.
      if (
        !origin ||
        !Number.isFinite(point.x) ||
        !Number.isFinite(point.y) ||
        Math.hypot(point.x - start.x, point.y - start.y) < 4
      )
        return false
      if (fallback) return true
      try {
        const current = await transport.position()
        if (cancelled || Math.abs(current.x - origin.x) > 1 || Math.abs(current.y - origin.y) > 1)
          return false
        await transport.move({
          x: Math.round(origin.x + (point.x - start.x) * scale),
          y: Math.round(origin.y + (point.y - start.y) * scale),
        })
        return true
      } catch {
        return false
      }
    },
    stop,
  }
}
