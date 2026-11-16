import { useState, type KeyboardEvent } from 'react'

export function ModeTabs<Mode extends string>({
  id,
  label,
  options,
  value,
  disabled,
  onChange,
}: {
  id: string
  label: string
  options: readonly (readonly [Mode, string])[]
  value: Mode
  disabled: boolean
  onChange: (value: Mode) => void
}) {
  const [focused, setFocused] = useState<Mode | null>(null)

  function moveFocus(event: KeyboardEvent<HTMLDivElement>) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key) || disabled) return
    const tabs = Array.from(event.currentTarget.querySelectorAll('button'))
    const current = tabs.findIndex((tab) => tab === event.target)
    if (current === -1) return
    event.preventDefault()
    if (event.key === 'Home') tabs[0].focus()
    else if (event.key === 'End') tabs.at(-1)?.focus()
    else {
      const offset = event.key === 'ArrowRight' ? 1 : -1
      tabs[(current + offset + tabs.length) % tabs.length].focus()
    }
  }

  return (
    <div
      className="software-content-modes"
      role="tablist"
      aria-label={label}
      onKeyDown={moveFocus}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setFocused(null)
      }}
    >
      {options.map(([mode, text]) => (
        <button
          key={mode}
          id={`${id}-${mode}`}
          type="button"
          role="tab"
          aria-selected={mode === value}
          tabIndex={(focused ?? value) === mode ? 0 : -1}
          className={`button compact ${mode === value ? 'primary' : 'secondary'}`}
          disabled={disabled}
          onFocus={() => setFocused(mode)}
          onClick={(event) => {
            event.currentTarget.focus({ preventScroll: true })
            onChange(mode)
          }}
        >
          {text}
        </button>
      ))}
    </div>
  )
}
