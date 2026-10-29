import { useEffect, useRef, useState, type CSSProperties } from 'react'

// Stable positions avoid a new cloud on every progress update. Motion stays in
// CSS; only committed results change the number of visible particles.
const particles = Array.from({ length: 32 }, (_, index) => {
  const angle = index * 2.399963
  const radius = Math.sqrt((index + 0.5) / 32)
  return {
    left: `${50 + Math.cos(angle) * radius * 37}%`,
    top: `${50 + Math.sin(angle) * radius * 32}%`,
    '--dust-size': `${1.8 + (index % 4) * 0.65}px`,
    '--dust-x': `${Math.sin(index * 1.7) * 7}px`,
    '--dust-y': `${Math.cos(index * 2.1) * 5}px`,
    '--dust-duration': `${5 + (index % 7) * 0.7}s`,
    '--dust-delay': `${-index * 0.63}s`,
    '--dust-alpha': 0.24 + (index % 5) * 0.09,
  } as CSSProperties
})

export function DustCloud({
  remaining = 1,
  phase = 'detected',
}: {
  remaining?: number
  phase?: 'detected' | 'working' | 'settled' | 'paused'
}) {
  const ref = useRef<HTMLSpanElement>(null)
  const [visible, setVisible] = useState(false)
  useEffect(() => {
    const observer = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting))
    if (ref.current) observer.observe(ref.current)
    return () => observer.disconnect()
  }, [])
  const fraction = Number.isFinite(remaining) ? Math.max(0, Math.min(1, remaining)) : 1
  const count = Math.ceil(fraction * particles.length)
  return (
    <span
      ref={ref}
      className="dust-cloud"
      data-phase={phase}
      data-visible={visible}
      data-empty={count === 0}
      aria-hidden="true"
    >
      {particles.map((style, index) => (
        <span key={index} className="dust-particle" data-remaining={index < count} style={style}>
          <i />
        </span>
      ))}
    </span>
  )
}

export function useMotionVisibility() {
  useEffect(() => {
    let focused = document.hasFocus()
    const update = () => {
      document.documentElement.dataset.motionPaused = String(document.hidden || !focused)
      document.documentElement.dataset.windowInactive = String(!focused)
    }
    const focus = () => {
      focused = true
      update()
    }
    const blur = () => {
      focused = false
      update()
    }
    update()
    document.addEventListener('visibilitychange', update)
    window.addEventListener('focus', focus)
    window.addEventListener('blur', blur)
    return () => {
      document.removeEventListener('visibilitychange', update)
      window.removeEventListener('focus', focus)
      window.removeEventListener('blur', blur)
      delete document.documentElement.dataset.motionPaused
      delete document.documentElement.dataset.windowInactive
    }
  }, [])
}

export function WorkspaceAtmosphere({ phase }: { phase: 'idle' | 'scanning' | 'working' }) {
  return (
    <div className="workspace-atmosphere" data-phase={phase} aria-hidden="true">
      <span className="workspace-aura" />
      <span className="workspace-sweep" />
      <span className="workspace-grain" />
    </div>
  )
}
