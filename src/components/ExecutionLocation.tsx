import { useRef, useState } from 'react'
import { FolderOpen } from 'lucide-react'

export type RevealExecutionItem = (runId: string, itemId: string) => Promise<void>

export function ExecutionLocation({
  runId,
  itemId,
  onReveal,
}: {
  runId: string
  itemId: string
  onReveal: RevealExecutionItem
}) {
  const [pending, setPending] = useState(false)
  const [message, setMessage] = useState('')
  const [error, setError] = useState('')
  const busy = useRef(false)
  async function reveal() {
    if (busy.current) return
    busy.current = true
    setPending(true)
    setMessage('')
    setError('')
    try {
      await onReveal(runId, itemId)
      setMessage('已请求 Finder 显示。请核对文件后使用系统提供的操作；记录不会自动标为已恢复。')
    } catch (error) {
      setError(String(error).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      setPending(false)
    }
  }
  return (
    <>
      <button className="button secondary compact" disabled={pending} onClick={() => void reveal()}>
        <FolderOpen size={14} />
        {pending ? '正在核对位置…' : '在 Finder 中查看'}
      </button>
      <p className="muted">
        需要取回时，可在 Finder
        核对“放回原处”是否可用；请勿覆盖原位置的已有文件。当前没有应用内恢复。
      </p>
      {message && <p role="status">{message}</p>}
      {error && (
        <p className="error-text" role="alert">
          {error}
        </p>
      )}
    </>
  )
}
