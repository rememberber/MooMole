import { useEffect, useRef, useState } from 'react'

export function ExecutionReview({
  reviewedAt,
  onAcknowledge,
}: {
  reviewedAt: number | null
  onAcknowledge: () => Promise<void>
}) {
  const [confirmed, setConfirmed] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  const live = useRef(true),
    busy = useRef(false)
  useEffect(() => {
    live.current = true
    return () => {
      live.current = false
    }
  }, [])
  async function save() {
    if (!confirmed || busy.current || reviewedAt !== null) return
    busy.current = true
    setSaving(true)
    setError('')
    try {
      await onAcknowledge()
    } catch (error) {
      if (live.current) setError(String(error).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      if (live.current) setSaving(false)
    }
  }
  if (reviewedAt !== null)
    return <p className="notice">已人工核查，原未知结果保持；新的操作仍需重新检查。</p>
  return (
    <div className="execution-review">
      <p className="notice warning">
        请先在 Finder
        核对原对象、废纸篓和逐项结果。不要按原路径重复执行或覆盖已有对象；核查只记录确认，不代表操作成功或已恢复。
      </p>
      <label className="uninstall-ack">
        <input
          type="checkbox"
          checked={confirmed}
          disabled={saving}
          onChange={(event) => setConfirmed(event.target.checked)}
        />
        我已核查原对象、废纸篓和逐项结果，了解未知结果保持且不会自动重试。
      </label>
      <button
        className="button secondary"
        disabled={!confirmed || saving}
        onClick={() => void save()}
      >
        {saving ? '正在保存核查…' : '记录已核查'}
      </button>
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
    </div>
  )
}
