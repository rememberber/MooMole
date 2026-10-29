import { useEffect, useRef, useState } from 'react'
import { Modal } from './Modal'
import { useNow } from '../lib/use-now'
import type {
  SearchRepairDraft,
  SearchRepairReport,
  SearchRepairTransport,
} from '../lib/search-repair'

export function SearchRepairCenter({
  openSignal,
  transport,
  onActiveChange,
  onOutcome,
}: {
  openSignal: number
  transport: SearchRepairTransport
  onActiveChange: (v: boolean) => void
  onOutcome: () => void
}) {
  const [open, setOpen] = useState(false),
    [pending, setPending] = useState(false)
  const [draft, setDraft] = useState<SearchRepairDraft | null>(null)
  const [report, setReport] = useState<SearchRepairReport | null>(null),
    [error, setError] = useState('')
  const busy = useRef(false),
    request = useRef<string | null>(null)
  const now = useNow()
  useEffect(() => {
    if (openSignal && !busy.current) {
      setOpen(true)
      request.current = null
      setDraft(null)
      setReport(null)
      setError('')
    }
  }, [openSignal])
  const valid = draft && now >= draft.createdAt && now < draft.expiresAt
  function close() {
    if (busy.current) return
    if (draft) void transport.discard(draft.id).catch(() => {})
    setOpen(false)
  }
  async function choose() {
    if (busy.current) return
    busy.current = true
    setPending(true)
    onActiveChange(true)
    setError('')
    try {
      const path = await transport.choose()
      if (path) {
        if (draft) await transport.discard(draft.id)
        setDraft(null)
        setDraft(await transport.prepare(path))
      }
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      setPending(false)
      onActiveChange(false)
    }
  }
  async function run() {
    if (busy.current || !draft || !valid) return
    busy.current = true
    setPending(true)
    onActiveChange(true)
    setError('')
    request.current = crypto.randomUUID()
    try {
      setReport(await transport.execute(draft, request.current))
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      setPending(false)
      onActiveChange(false)
      onOutcome()
    }
  }
  if (!open) return null
  return (
    <Modal title="更新文件搜索索引" onClose={pending ? undefined : close}>
      <h2>更新文件搜索索引</h2>
      <p>文件存在但 Finder 或 Spotlight 搜索不到时，可选择所在文件夹重新提交索引。</p>
      <p className="notice">
        会读取所选文件夹及其子目录并更新 Spotlight 索引，可能占用 CPU
        和磁盘。不删除原文件，不清空整卷索引，也不更改隐私排除；隐私排除、权限与不支持的格式仍可能影响搜索。
      </p>
      {draft && (
        <p className="mono full-path" data-i18n="off">
          {draft.path}
        </p>
      )}
      {!report && !request.current && (
        <button className="button secondary" disabled={pending} onClick={() => void choose()}>
          选择文件夹…
        </button>
      )}
      {draft && !valid && !report && <p role="status">目录确认已过期，请重新选择后确认。</p>}
      {pending && (
        <p role="status">正在处理索引请求；已提交给 Spotlight 的后台工作可能在停止后继续。</p>
      )}
      {report && (
        <section aria-label="索引操作结果">
          <strong>
            {report.attempt.outcome === 'commandSucceeded'
              ? '索引命令已完成'
              : report.attempt.outcome === 'notStarted'
                ? '索引命令未启动'
                : '索引结果待核实'}
          </strong>
          <p>
            后台索引可能仍在更新。请在 Finder 或 Spotlight
            中重新搜索原来的文件名，确认能否找到；命令完成不代表问题已修复。
          </p>
          {report.attempt.stop && (
            <p className="notice">命令已停止或未确认正常结束，已提交的后台索引无法撤销。</p>
          )}
          <p className="muted">
            操作和所选目录已保存在操作历史中。原文件保留，此操作没有恢复原索引的功能。
          </p>
        </section>
      )}
      {error && (
        <p role="alert" className="notice warning">
          {error}
        </p>
      )}
      <div className="modal-actions">
        {pending ? (
          request.current && (
            <button
              className="button secondary"
              onClick={() =>
                void transport.cancel(request.current!).catch((e) => setError(String(e)))
              }
            >
              停止索引命令
            </button>
          )
        ) : (
          <button className="button secondary" onClick={close}>
            返回维护
          </button>
        )}
        {!report && !request.current && (
          <button
            className="button primary"
            disabled={pending || !valid}
            onClick={() => void run()}
          >
            确认更新索引
          </button>
        )}
      </div>
    </Modal>
  )
}
