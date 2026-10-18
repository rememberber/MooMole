import { useEffect, useRef, useState } from 'react'
import { Box } from 'lucide-react'
import { loadApplicationIcon } from '../lib/application-icons'

export function ApplicationIcon({ taskId, index }: { taskId?: string | null; index: number }) {
  const element = useRef<HTMLSpanElement>(null)
  const [visible, setVisible] = useState(false)
  const [image, setImage] = useState<{ key: string; value: string | null } | null>(null)
  const key = `${taskId}:${index}`
  const source = visible && image?.key === key ? image.value : null
  useEffect(() => {
    if (!element.current) return
    const observer = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting), {
      rootMargin: '100px',
    })
    observer.observe(element.current)
    return () => observer.disconnect()
  }, [])
  useEffect(() => {
    setImage(null)
    if (!visible || !taskId) return
    const controller = new AbortController()
    void loadApplicationIcon(taskId, index, controller.signal).then((value) => {
      if (!controller.signal.aborted) setImage({ key: `${taskId}:${index}`, value })
    })
    return () => controller.abort()
  }, [visible, taskId, index])
  return (
    <span ref={element} className={`app-icon${source ? ' has-image' : ''}`} aria-hidden="true">
      {source ? (
        <img
          src={source}
          alt=""
          width={32}
          height={32}
          draggable={false}
          onError={() => setImage(null)}
        />
      ) : (
        <Box size={20} strokeWidth={1.4} />
      )}
    </span>
  )
}
