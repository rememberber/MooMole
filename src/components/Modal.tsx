import { useLayoutEffect, useRef, type MouseEvent, type ReactNode } from 'react'
import { X } from 'lucide-react'
import { restoreFocus } from '../lib/focus-return'

function isBackdrop(event: MouseEvent<HTMLDialogElement>) {
  if (event.target !== event.currentTarget) return false
  const { left, right, top, bottom } = event.currentTarget.getBoundingClientRect()
  return (
    event.clientX < left || event.clientX >= right || event.clientY < top || event.clientY >= bottom
  )
}

function containWheel(event: WheelEvent) {
  if (event.ctrlKey) return
  // WebKit can chain wheel events through a non-scrolling backdrop or body.
  // Leave the default behavior to scrollable controls and to an inner modal.
  for (const node of event.composedPath()) {
    if (node === event.currentTarget) break
    if (node instanceof HTMLDialogElement) return
    if (!(node instanceof HTMLElement)) continue
    const scrollsVertically = event.deltaY !== 0 && node.scrollHeight > node.clientHeight
    const scrollsHorizontally = event.deltaX !== 0 && node.scrollWidth > node.clientWidth
    if (!scrollsVertically && !scrollsHorizontally) continue
    const { overflowX, overflowY } = getComputedStyle(node)
    if (
      (scrollsVertically && /^(auto|scroll)$/.test(overflowY)) ||
      (scrollsHorizontally && /^(auto|scroll)$/.test(overflowX))
    )
      return
  }
  event.preventDefault()
}

export function Modal({
  title,
  heading = title,
  children,
  onClose,
  returnFocus,
  dismissOnBackdrop = true,
  className = '',
}: {
  title: string
  heading?: ReactNode
  children: ReactNode
  onClose?: () => void
  returnFocus?: () => HTMLElement | null
  dismissOnBackdrop?: boolean
  className?: string
}) {
  const ref = useRef<HTMLDialogElement>(null)
  const pressedBackdrop = useRef(false)
  const closedBy = onClose ? (dismissOnBackdrop ? 'any' : 'closerequest') : 'none'
  const needsBackdropFallback = closedBy === 'any' && !('closedBy' in HTMLDialogElement.prototype)
  const returnTarget = useRef(returnFocus)
  returnTarget.current = returnFocus
  useLayoutEffect(() => {
    const dialog = ref.current!
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null
    dialog.showModal()
    dialog.addEventListener('wheel', containWheel, { passive: false })
    return () => {
      dialog.removeEventListener('wheel', containWheel)
      const ownedFocus =
        dialog.contains(document.activeElement) || document.activeElement === document.body
      // Close before React removes the native dialog. Passive cleanup runs too
      // late for reliable browser focus restoration on an unmounted dialog.
      dialog.close()
      if (ownedFocus) restoreFocus(returnTarget.current ? returnTarget.current() : previous)
    }
  }, [])
  return (
    <dialog
      ref={ref}
      className={`modal ${className}`}
      aria-label={title}
      closedby={closedBy}
      onCancel={(event) => {
        event.preventDefault()
        event.stopPropagation()
        onClose?.()
      }}
      onPointerDown={
        needsBackdropFallback
          ? (event) => {
              pressedBackdrop.current = event.button === 0 && isBackdrop(event)
            }
          : undefined
      }
      onPointerCancel={needsBackdropFallback ? () => (pressedBackdrop.current = false) : undefined}
      onClick={
        needsBackdropFallback
          ? (event) => {
              const dismiss = pressedBackdrop.current && isBackdrop(event)
              pressedBackdrop.current = false
              if (dismiss) onClose?.()
            }
          : undefined
      }
    >
      <div className="modal-header">
        <h2 className="modal-title">{heading}</h2>
        {onClose && (
          <button
            type="button"
            className="icon-button modal-close"
            onClick={onClose}
            aria-label="关闭弹窗"
          >
            <X size={18} />
          </button>
        )}
      </div>
      <div className="modal-body">{children}</div>
    </dialog>
  )
}
