// Return only to a live, visible target in the currently interactive scope.
// Missing/disabled triggers must not pull focus out of another modal dialog.
export function focusReturnTarget(
  ...targets: (HTMLElement | null | undefined)[]
): HTMLElement | null {
  const dialogs = Array.from(document.querySelectorAll<HTMLDialogElement>('dialog:modal'))
  return (
    targets.find(
      (target): target is HTMLElement =>
        target != null &&
        target.ownerDocument === document &&
        target !== document.body &&
        target !== document.documentElement &&
        target.isConnected &&
        !target.matches(':disabled') &&
        !target.closest('[hidden], [inert]') &&
        target.getClientRects().length > 0 &&
        getComputedStyle(target).visibility === 'visible' &&
        dialogs.every((dialog) => dialog.contains(target)),
    ) ?? null
  )
}

export function restoreFocus(...targets: (HTMLElement | null | undefined)[]) {
  focusReturnTarget(...targets)?.focus({ preventScroll: true })
}
