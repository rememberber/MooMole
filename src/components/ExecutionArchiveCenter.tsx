import { useEffect, useRef, useState } from 'react'
import { Modal } from './Modal'
import { bytes, dateTime } from '../lib/format'
import { refreshReason } from '../lib/quicklook-refresh'
import {
  archivePreview,
  archiveHeader,
  archiveList,
  archiveView,
  archiveDetail,
  archiveKindName,
  archiveStateName,
  type ArchiveTransport,
  type ArchivePreview,
  type ArchiveHeader,
  type ArchiveView,
  type ArchiveEntry,
  type ArchiveDetail,
} from '../lib/execution-archive'
const message = (e: unknown) => String(e).replace(/^Error: /, '')
const scope = (v: ArchiveHeader['counts']) =>
  `清理 ${v.cleanup} 批 · 卸载 ${v.uninstall} 批 · 维护 ${v.maintenance} 批`
export function ExecutionArchiveCenter({
  openSignal,
  enabled,
  transport,
  onChanged,
}: {
  openSignal: number
  enabled: boolean
  transport: ArchiveTransport
  onChanged: () => Promise<void>
}) {
  const [open, setOpen] = useState(false),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(''),
    [notice, setNotice] = useState(''),
    [needsRefresh, setNeedsRefresh] = useState(false)
  const [list, setList] = useState<ArchiveHeader[]>([]),
    [complete, setComplete] = useState(true),
    [unreadable, setUnreadable] = useState(0),
    [preview, setPreview] = useState<ArchivePreview | null>(null),
    [ack, setAck] = useState(false)
  const [view, setView] = useState<ArchiveView | null>(null),
    [selected, setSelected] = useState<ArchiveEntry | null>(null),
    [detail, setDetail] = useState<ArchiveDetail | null>(null)
  const mounted = useRef(true),
    request = useRef(0),
    guard = useRef(false),
    blocking = useRef(false)
  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
      request.current++
    }
  }, [])
  async function refresh() {
    const v = archiveList(await transport.list())
    if (mounted.current) {
      setList(v.archives)
      setComplete(v.complete)
      setUnreadable(v.unreadable)
    }
  }
  async function run(action: () => Promise<void>, mutates = false) {
    if (guard.current) return
    guard.current = true
    blocking.current = mutates
    setBusy(true)
    setError('')
    try {
      await action()
    } catch (e) {
      if (mounted.current) setError(message(e))
    } finally {
      guard.current = false
      blocking.current = false
      if (mounted.current) setBusy(false)
    }
  }
  useEffect(() => {
    if (openSignal > 0 && enabled) {
      setOpen(true)
      setPreview(null)
      setView(null)
      setSelected(null)
      setDetail(null)
      setNotice('')
      void run(refresh)
    }
  }, [openSignal])
  function close() {
    if (blocking.current) return
    request.current++
    setOpen(false)
    setPreview(null)
    setAck(false)
    setView(null)
    setSelected(null)
    setDetail(null)
  }
  async function openArchive(id: string) {
    const g = ++request.current
    const result = archiveView(await transport.view(id), id)
    if (mounted.current && g === request.current) {
      setView(result)
      setSelected(null)
      setDetail(null)
      setPreview(null)
    }
  }
  async function read(entry: ArchiveEntry) {
    if (!view) return
    const g = ++request.current
    setSelected(entry)
    setDetail(null)
    const result = archiveDetail(
      await transport.detail(view.header.id, entry.kind, entry.id),
      entry,
    )
    if (mounted.current && g === request.current) setDetail(result)
  }
  async function save() {
    if (!preview || !ack) return
    const expected = preview
    setNeedsRefresh(true)
    setPreview(null)
    setAck(false)
    const result = archiveHeader(await transport.archive(expected.fingerprint))
    if (
      result.payloadSha256 !== expected.fingerprint ||
      scope(result.counts) !== scope(expected.counts)
    )
      throw Error('归档响应与确认范围不符，请刷新活动记录和归档核对。')
    setPreview(null)
    setAck(false)
    setNotice('归档已保存，活动历史容量已释放；原结果与恢复证据保留在归档中。')
    try {
      await onChanged()
      setNeedsRefresh(false)
    } catch (e) {
      setError(`归档已保存，但活动记录刷新失败：${message(e)}`)
    }
    await refresh()
    await openArchive(result.id)
  }
  const title = preview ? '确认归档' : selected ? '归档记录' : '执行归档'
  return open ? (
    <Modal
      title={title}
      onClose={busy && blocking.current ? undefined : close}
      className="review-plan-modal"
    >
      <div
        className="execution-archive-content"
        onClickCapture={(event) => {
          if (event.detail > 1) {
            event.preventDefault()
            event.stopPropagation()
          }
        }}
      >
        <h2>{title}</h2>
        <p className="muted">
          仅整理本机执行历史，不处理原文件或废纸篓，不恢复执行，也不会自动重试。
        </p>
        {error && (
          <p className="notice warning" role="alert">
            {error}
          </p>
        )}
        {needsRefresh && !busy && (
          <button
            className="button secondary"
            onClick={() =>
              void run(async () => {
                await onChanged()
                await refresh()
                setNeedsRefresh(false)
              })
            }
          >
            刷新活动记录和归档
          </button>
        )}
        {notice && (
          <p className="notice" role="status">
            {notice}
          </p>
        )}
        {busy && <p role="status">正在核对或保存归档…</p>}
        {preview ? (
          <>
            <strong>{scope(preview.counts)}</strong>
            <p>
              先保存并核对完整归档，再从活动历史移出。原计划、逐项结果、收据、事件及核查标记都会保留。
            </p>
            <p>
              本批预留容量 {bytes(preview.reservedBytes)}
              ，归档仍占磁盘空间。归档含个人路径和检查结果，不含原文件正文；导出后请妥善保管。
            </p>
            <p>未知结果保持未知，不代表操作成功或文件已恢复。</p>
            <label className="uninstall-ack">
              <input
                type="checkbox"
                checked={ack}
                disabled={busy}
                onChange={(e) => setAck(e.target.checked)}
              />
              我已核对归档范围，了解原证据会保留，活动历史将移入归档。
            </label>
            <div className="modal-actions">
              <button
                className="button secondary"
                disabled={busy}
                onClick={() => {
                  setPreview(null)
                  setAck(false)
                }}
              >
                返回
              </button>
              <button
                className="button primary"
                disabled={busy || !ack}
                onClick={() => void run(save, true)}
              >
                保存归档并释放容量
              </button>
            </div>
          </>
        ) : selected ? (
          <>
            <strong>
              {archiveKindName[selected.kind]} · {archiveStateName(selected.summary.state)}
            </strong>
            {detail && (
              <>
                {'path' in detail && <p className="mono full-path">{detail.path}</p>}
                {detail.reviewedAt !== null && (
                  <p className="notice">已人工核查；原结果保持，核查不证明操作成功或已恢复。</p>
                )}
                {'reason' in detail && <p>{detail.reason}</p>}
                {'items' in detail &&
                  detail.items.map((item, i) => (
                    <article className="saved-plan-row" key={i}>
                      <div>
                        <strong>
                          {'path' in item
                            ? item.path
                            : item.step === 'generators'
                              ? 'Quick Look 生成器列表'
                              : '缩略图缓存'}{' '}
                          · {archiveStateName(item.state)}
                        </strong>
                        <p>
                          {'step' in item ? refreshReason(item.reason, item.stop) : item.reason}
                        </p>
                        {'receipt' in item && item.receipt && (
                          <p className="mono full-path">当时移入位置：{item.receipt.destination}</p>
                        )}
                      </div>
                    </article>
                  ))}
                {'receipt' in detail && detail.receipt && (
                  <p className="mono full-path">
                    当时移入位置：{detail.receipt.actual.destination}
                  </p>
                )}
                <p className="muted">
                  收据记录的是当时位置，当前可能已变化。需要取回时请在 Finder
                  核对对象；原位置已有同名对象时不要覆盖。
                </p>
                <details>
                  <summary>原始执行记录</summary>
                  <pre
                    className="mono full-path"
                    style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}
                  >
                    {JSON.stringify(detail, null, 2)}
                  </pre>
                </details>
              </>
            )}
            <div className="modal-actions">
              <button
                className="button secondary"
                disabled={busy}
                onClick={() => {
                  request.current++
                  setSelected(null)
                  setDetail(null)
                }}
              >
                返回归档
              </button>
            </div>
          </>
        ) : view ? (
          <>
            <p>
              {dateTime(view.header.createdAt)} · {scope(view.header.counts)} · 只读
            </p>
            {view.entries.map((e) => (
              <button
                key={e.kind + e.id}
                className="saved-plan-row"
                disabled={busy}
                onClick={() => void run(() => read(e))}
              >
                <span>
                  <strong>
                    {archiveKindName[e.kind]} ·{' '}
                    {'name' in e.summary ? e.summary.name : dateTime(e.summary.createdAt)}
                  </strong>
                  <small>
                    {archiveStateName(e.summary.state)}
                    {e.summary.reviewedAt !== null ? ' · 已人工核查' : ''}
                  </small>
                </span>
                <span>查看记录</span>
              </button>
            ))}
            <div className="modal-actions">
              <button
                className="button secondary"
                disabled={busy}
                onClick={() => {
                  setView(null)
                  setNotice('')
                  void run(refresh)
                }}
              >
                返回列表
              </button>
              <button
                className="button secondary"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    if (await transport.export(view.header.id))
                      setNotice('归档已导出；本机归档继续保留。')
                  }, true)
                }
              >
                导出归档…
              </button>
            </div>
          </>
        ) : (
          <>
            <p>
              归档已结束且完成必要核查的执行记录，可释放活动历史的 64 批 / 64 MiB
              预留上限。运行中或未核查的未知结果须先处理。
            </p>
            <div className="modal-actions">
              <button
                className="button primary"
                disabled={busy || needsRefresh}
                onClick={() =>
                  void run(async () => {
                    const p = archivePreview(await transport.prepare())
                    setPreview(p)
                    setAck(false)
                  })
                }
              >
                检查当前执行记录
              </button>
              <button
                className="button secondary"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    const raw = await transport.import()
                    if (raw === null) return
                    const h = archiveHeader(raw)
                    await refresh()
                    await openArchive(h.id)
                  }, true)
                }
              >
                打开归档文件…
              </button>
              <button
                className="button text"
                disabled={busy}
                onClick={() => void run(() => transport.reveal())}
              >
                在 Finder 中显示
              </button>
            </div>
            {unreadable > 0 && (
              <p className="notice warning">
                有 {unreadable} 份归档摘要无法读取，原文件未处理；可在 Finder
                中核对。其他归档仍可打开。
              </p>
            )}
            {!complete && (
              <p className="notice warning">
                目录超过本次 1,024 项读取范围。可在 Finder 管理，或选择确切归档文件打开。
              </p>
            )}
            {list.length
              ? list.map((h) => (
                  <button
                    className="saved-plan-row"
                    disabled={busy}
                    key={h.id}
                    onClick={() => void run(() => openArchive(h.id))}
                  >
                    <span>
                      <strong>{dateTime(h.createdAt)}</strong>
                      <small>{scope(h.counts)}</small>
                    </span>
                    <span>只读查看</span>
                  </button>
                ))
              : !busy && <p className="muted">暂无本机执行归档。</p>}
          </>
        )}
      </div>
    </Modal>
  ) : null
}
