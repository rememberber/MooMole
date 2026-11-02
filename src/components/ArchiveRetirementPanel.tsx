import { useEffect, useRef, useState } from 'react'
import {
  archiveRetirement as api,
  type RetirementDraft,
  type RetirementRecord,
} from '../lib/archive-retirement'
import type { ArchiveHeader } from '../lib/execution-archive'
import { bytes, dateTime } from '../lib/format'
import { useNow } from '../lib/use-now'
export function ArchiveRetirementPanel({
  header,
  onBack,
  onActiveChange,
  onChanged,
}: {
  header: ArchiveHeader | null
  onBack: () => void
  onActiveChange: (active: boolean) => void
  onChanged: () => Promise<void>
}) {
  const [loaded, setLoaded] = useState(false)
  const [attempted, setAttempted] = useState(false)
  const [draft, setDraft] = useState<RetirementDraft | null>(null),
    [records, setRecords] = useState<RetirementRecord[]>([]),
    [activeRequests, setActiveRequests] = useState<string[]>([]),
    [pending, setPending] = useState(false),
    [ack, setAck] = useState(false),
    [review, setReview] = useState(''),
    [error, setError] = useState(''),
    [stopping, setStopping] = useState(false)
  const live = useRef(true),
    busy = useRef(false),
    request = useRef<string | null>(null),
    owned = useRef<string | null>(null)
  useNow()
  const expired = !!draft && (Date.now() < draft.createdAt || Date.now() >= draft.expiresAt),
    blocked =
      !loaded ||
      records.some((r) => r.state === 'unknown' && r.reviewedAt === null) ||
      activeRequests.length > 0
  useEffect(() => {
    live.current = true
    void refresh()
    return () => {
      live.current = false
      if (request.current) void api.cancel(request.current).catch(() => {})
      if (owned.current) void api.discard(owned.current).catch(() => {})
      onActiveChange(false)
    }
  }, [])
  useEffect(() => {
    if (!activeRequests.length) return
    const timer = window.setTimeout(() => void refresh(), 1000)
    return () => clearTimeout(timer)
  }, [activeRequests])
  async function refresh() {
    try {
      const snapshot = await api.get()
      if (live.current) {
        setLoaded(true)
        setRecords(snapshot.records)
        setActiveRequests(snapshot.activeRequests)
      }
    } catch (e) {
      if (live.current) setError(String(e).replace(/^Error: /, ''))
    }
  }
  function discard() {
    if (owned.current) void api.discard(owned.current).catch(() => {})
    owned.current = null
    setDraft(null)
    setAck(false)
  }
  async function run(action: (id: string) => Promise<void>) {
    if (busy.current) return
    busy.current = true
    setPending(true)
    setStopping(false)
    setError('')
    onActiveChange(true)
    const id = crypto.randomUUID()
    request.current = id
    try {
      await action(id)
    } catch (e) {
      if (live.current) setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      request.current = null
      if (live.current) {
        setPending(false)
        onActiveChange(false)
        void refresh()
      }
    }
  }
  async function prepare(id: string) {
    if (!header || blocked) return
    discard()
    const result = await api.prepare(header, id)
    if (!result) return
    if (!live.current) {
      void api.discard(result.id).catch(() => {})
      return
    }
    owned.current = result.id
    setDraft(result)
  }
  async function execute(id: string) {
    if (!draft || !ack || expired || blocked) return
    setAttempted(true)
    const selected = draft
    owned.current = null
    setDraft(null)
    setAck(false)
    const result = await api.execute(selected, id)
    if (live.current) {
      setRecords((old) => [result, ...old.filter((r) => r.id !== result.id)])
      try {
        await onChanged()
      } catch {
        setError('归档副本结果已保存，但列表刷新失败，请返回后重新读取。')
      }
    }
  }
  return (
    <section className="archive-retirement-panel" aria-label="归档副本整理">
      <h2>整理本机归档副本</h2>
      <p>
        先保存并核对完整导出副本，再单独确认将所选本机副本移入废纸篓。活动历史、原文件和导出副本保留，不会清空废纸篓或立即释放磁盘空间。
      </p>
      <p className="muted">
        同盘导出会暂时增加占用；需要迁出本机时，请自行选择合适的外接位置。关闭或取消后，已明确保存的导出文件仍保留。
      </p>
      {header && !draft && !attempted && (
        <>
          <p>
            所选归档：{dateTime(header.createdAt)} · 正文 {bytes(header.payloadBytes)}
          </p>
          <button
            className="button secondary"
            disabled={pending || blocked}
            onClick={() => void run(prepare)}
          >
            保存完整副本并检查…
          </button>
        </>
      )}
      {draft && (
        <>
          <dl className="application-update-facts">
            <dt>本机副本</dt>
            <dd className="mono full-path" data-i18n="off">
              {draft.path}
            </dd>
            <dt>完整导出副本</dt>
            <dd className="mono full-path" data-i18n="off">
              {draft.exported}
            </dd>
            <dt>文件大小</dt>
            <dd>{bytes(draft.bytes)}</dd>
          </dl>
          <p>已核对整份字节和归档内容。处理前会再次检查两份文件；两分钟有效，过期后需重新检查。</p>
          <label className="uninstall-ack">
            <input
              type="checkbox"
              checked={ack}
              disabled={pending || expired || blocked}
              onChange={(e) => setAck(e.target.checked)}
            />
            我已核对完整导出位置，确认仅将这份本机归档副本移入废纸篓。
          </label>
          <button
            className="button primary"
            disabled={pending || !ack || expired || blocked}
            onClick={() => void run(execute)}
          >
            确认整理本机副本
          </button>
          {expired && <p className="notice warning">归档副本确认已过期，已保存的导出文件保留。</p>}
        </>
      )}
      {pending && <p role="status">正在保存、核对或整理归档副本…</p>}
      {error && (
        <p role="alert" className="notice warning">
          {error}
        </p>
      )}
      {blocked && (
        <p className="notice warning">
          请先等待正在进行的副本整理，或核查下方未知结果；不会自动重复执行。
        </p>
      )}
      <div className="modal-actions">
        <button
          className="button secondary"
          disabled={pending}
          onClick={() => {
            discard()
            onBack()
          }}
        >
          返回归档列表
        </button>
        {draft && !pending && (
          <button className="button secondary" onClick={discard}>
            取消本次整理
          </button>
        )}
        {pending && (
          <button
            className="button secondary"
            disabled={stopping}
            onClick={() => {
              if (request.current) {
                setStopping(true)
                void api.cancel(request.current).catch((e) => {
                  if (live.current) {
                    setError(String(e))
                    setStopping(false)
                  }
                })
              }
            }}
          >
            {stopping ? '已请求停止' : '停止副本整理'}
          </button>
        )}
        <button className="button secondary" disabled={pending} onClick={() => void refresh()}>
          刷新副本整理记录
        </button>
      </div>
      <h3>副本整理记录</h3>
      {!records.length && !pending && <p className="muted">暂无归档副本整理记录。</p>}
      {records.map((r) => (
        <article className="card" key={r.id}>
          <strong>
            {r.state === 'moved'
              ? '本机副本已移入废纸篓'
              : r.state === 'unknown'
                ? '副本整理结果待核实'
                : r.state === 'running'
                  ? '副本整理进行中'
                  : r.state === 'cancelled'
                    ? '副本整理已停止'
                    : '本机副本保留'}
          </strong>
          <p>{r.reason}</p>
          <p className="mono full-path" data-i18n="off">
            {r.draft.path}
          </p>
          <p>完整导出位置</p>
          <p className="mono full-path" data-i18n="off">
            {r.draft.exported}
          </p>
          {r.receipt && (
            <>
              <p>系统返回位置</p>
              <p className="mono full-path" data-i18n="off">
                {r.receipt.destination}
              </p>
            </>
          )}
          {activeRequests.includes(r.requestId) && (
            <button
              className="button secondary"
              onClick={() => void api.cancel(r.requestId).catch((e) => setError(String(e)))}
            >
              停止此副本整理
            </button>
          )}
          {r.state === 'unknown' && r.reviewedAt === null && (
            <>
              <label className="uninstall-ack">
                <input
                  type="checkbox"
                  checked={review === r.id}
                  onChange={(e) => setReview(e.target.checked ? r.id : '')}
                />
                我已核对本机、导出和废纸篓的实际文件，了解未知结果不会改为成功。
              </label>
              <button
                className="button secondary"
                disabled={pending || review !== r.id}
                onClick={() =>
                  void run(async () => {
                    await api.acknowledge(r.id)
                    setReview('')
                  })
                }
              >
                记录副本核查
              </button>
            </>
          )}
          {r.reviewedAt !== null && <p>已人工核查；原结果保留。</p>}
        </article>
      ))}
    </section>
  )
}
