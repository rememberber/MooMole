import { useEffect, useRef, type MouseEvent } from 'react'
import { isTauri } from '@tauri-apps/api/core'
import { PhysicalPosition } from '@tauri-apps/api/dpi'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { windowDragFallback } from './window-drag'

export function useWindowDrag() {
  const cleanup = useRef<(() => void) | null>(null)
  const compatibility = useRef(false)
  useEffect(() => () => cleanup.current?.(), [])
  return (event: MouseEvent<HTMLElement>) => {
    if (!isTauri() || event.button !== 0 || event.detail > 1) return
    const target = event.target instanceof Element ? event.target : null
    if (!target) return
    const control = target.closest(
      'button, input, select, textarea, a, label, summary, [contenteditable]:not([contenteditable="false"]), [tabindex]:not([tabindex="-1"]), [role="button"], [role="link"], [role="menuitem"], [role="tab"], [role="checkbox"], [role="radio"], [role="switch"], [role="option"], [data-tauri-drag-region="false"]',
    )
    if (control && event.currentTarget.contains(control)) return
    cleanup.current?.()
    if (compatibility.current) {
      // A previous real gesture proved native handoff broken in this window
      // session. Keep its following gestures out of AppKit's stalled loop.
      event.preventDefault()
      event.stopPropagation()
    }
    const win = getCurrentWindow()
    const drag = windowDragFallback(
      {
        position: () => win.outerPosition(),
        scale: () => win.scaleFactor(),
        move: (point) => win.setPosition(new PhysicalPosition(point.x, point.y)),
      },
      { x: event.screenX, y: event.screenY },
    )
    const move = (value: globalThis.MouseEvent) => {
      drag.move({ x: value.screenX, y: value.screenY })
    }
    const stop = () => {
      drag.stop()
      window.removeEventListener('mousemove', move)
      window.removeEventListener('mouseup', release)
      window.removeEventListener('blur', stop)
      if (cleanup.current === stop) cleanup.current = null
    }
    const release = async (value: globalThis.MouseEvent) => {
      window.removeEventListener('mousemove', move)
      window.removeEventListener('mouseup', release)
      const used = await drag.finish({ x: value.screenX, y: value.screenY })
      if (cleanup.current === stop) {
        if (used) compatibility.current = true
        stop()
      }
    }
    cleanup.current = stop
    window.addEventListener('mousemove', move)
    window.addEventListener('mouseup', release)
    window.addEventListener('blur', stop)
  }
}
