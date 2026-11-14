import { useLayoutEffect, useRef, type ReactNode } from 'react'
import { X } from 'lucide-react'
import { restoreFocus } from '../lib/focus-return'

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
  const returnTarget = useRef(returnFocus)
  returnTarget.current = returnFocus
  useLayoutEffect(() => {
    const dialog = ref.current!
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null
    dialog.showModal()
    return () => {
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
      onCancel={(event) => {
        event.preventDefault()
        event.stopPropagation()
        onClose?.()
      }}
      onClick={(event) => {
        if (dismissOnBackdrop && event.target === ref.current) {
          const rect = ref.current!.getBoundingClientRect()
          if (
            event.clientX < rect.left ||
            event.clientX > rect.right ||
            event.clientY < rect.top ||
            event.clientY > rect.bottom
          )
            onClose?.()
        }
      }}
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
