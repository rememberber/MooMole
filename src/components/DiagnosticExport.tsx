import { useEffect, useRef, useState } from 'react'
import * as api from '../lib/api'

export function DiagnosticExport({ taskId }: { taskId: string }) {
  const [report, setReport] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState('')
  const [error, setError] = useState('')
  const request = useRef(0)
  const preview = useRef<HTMLPreElement>(null)
  const saveButton = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    if (report != null) preview.current?.focus()
  }, [report])
  useEffect(
    () => () => {
      ++request.current
    },
    [],
  )
  async function prepare() {
    const current = ++request.current
    setBusy(true)
    setReport(null)
    setMessage('')
    setError('')
    try {
      const next = await api.prepareTaskDiagnostic(taskId)
      if (request.current === current) setReport(next)
    } catch (error) {
      if (request.current === current) setError(String(error).replace(/^Error: /, ''))
    } finally {
      if (request.current === current) setBusy(false)
    }
  }
  async function save() {
    if (report == null) return
    const current = ++request.current
    setBusy(true)
    setError('')
    setMessage('正在选择位置或保存，请在系统窗口中完成操作。')
    try {
      const saved = await api.saveTaskDiagnostic(report)
      if (request.current === current)
        setMessage(saved ? '诊断摘要已保存到所选位置，未上传。' : '已取消保存，没有导出文件。')
    } catch (error) {
      if (request.current === current) {
        setError(String(error).replace(/^Error: /, ''))
        setMessage('')
      }
    } finally {
      if (request.current === current) {
        setBusy(false)
        requestAnimationFrame(() => {
          if (request.current === current) saveButton.current?.focus({ preventScroll: true })
        })
      }
    }
  }
  return (
    <section className="diagnostic-export" aria-label="诊断摘要导出">
      <button
        className="button secondary compact"
        disabled={!api.desktop || busy}
        onClick={() => void prepare()}
      >
        {busy && report == null ? '正在生成…' : report == null ? '生成诊断摘要' : '重新生成摘要'}
      </button>
      {report != null && (
        <>
          <p>
            请先检查以下完整内容，再选择位置保存。摘要固定为生成时的记录，不会随任务状态自动更新。
          </p>
          <p>
            包含类型、状态、记录用时及当前应用/引擎版本；不含路径、任务
            ID、精确时间和原始诊断。原始诊断可在上方详情查阅。
          </p>
          <pre ref={preview} tabIndex={0} aria-label="将保存的诊断摘要">
            {report}
          </pre>
          <p>只保存到你选择的位置，不自动上传。已有文件不会被覆盖，请使用新的 .json 文件名。</p>
          <button
            ref={saveButton}
            className="button secondary compact"
            disabled={busy}
            onClick={() => void save()}
          >
            选择位置并保存
          </button>
        </>
      )}
      {message && <p role="status">{message}</p>}
      {error && <p role="alert">{error}</p>}
    </section>
  )
}
