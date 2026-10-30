import { useEffect, useRef, useState } from 'react'
import { Modal } from './Modal'
import {
  deviceBackupsTransport as transport,
  backupState,
  type BackupView,
  type DeviceBackup,
  type BackupDraft,
} from '../lib/device-backups'
import * as api from '../lib/api'
import { bytes, dateTime } from '../lib/format'
import { useNow } from '../lib/use-now'
export function DeviceBackups({
  openSignal,
  enabled,
  onActiveChange,
}: {
  openSignal: number
  enabled: boolean
  onActiveChange: (active: boolean) => void
}) {
  const [open, setOpen] = useState(false),
    [view, setView] = useState<BackupView | null>(null)
  const [pending, setPending] = useState(false),
    [error, setError] = useState(''),
    [readError, setReadError] = useState('')
  const [draft, setDraft] = useState<BackupDraft | null>(null),
    [ack, setAck] = useState(false)
  const [review, setReview] = useState<string | null>(null),
    [reviewAck, setReviewAck] = useState(false)
  const busy = useRef(false),
    mounted = useRef(true)
  useNow()
  const now = Date.now(),
    active = !!view?.activeRequests.length,
    working = pending || active
  async function read() {
    try {
      const v = await transport.read()
      if (mounted.current) {
        setView(v)
        setReadError('')
      }
    } catch (e) {
      if (mounted.current) setReadError(String(e).replace(/^Error: /, ''))
    }
  }
  useEffect(() => {
    mounted.current = true
    if (api.desktop) void read()
    return () => {
      mounted.current = false
    }
  }, [])
  useEffect(() => {
    if (openSignal) {
      setOpen(true)
      void read()
    }
  }, [openSignal])
  useEffect(() => {
    onActiveChange(working)
  }, [working, onActiveChange])
  useEffect(() => {
    if (!api.desktop || (!open && !active)) return
    const t = setInterval(() => void read(), 1000)
    return () => clearInterval(t)
  }, [open, active])
  useEffect(
    () => () => {
      if (draft) void transport.discard(draft.id).catch(() => {})
    },
    [draft?.id],
  )
  async function action(run: () => Promise<unknown>) {
    if (busy.current) return
    busy.current = true
    setPending(true)
    setError('')
    try {
      await run()
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    } finally {
      await read()
      busy.current = false
      setPending(false)
    }
  }
  function close() {
    if (working) return
    setDraft(null)
    setReview(null)
    setOpen(false)
  }
  async function scan(directory: string | null) {
    setDraft(null)
    setReview(null)
    await action(() => transport.scan(directory, crypto.randomUUID()))
  }
  async function prepare(item: DeviceBackup) {
    await action(async () => {
      if (!view?.report) return
      const d = await transport.prepare(view.report.id, item)
      setDraft(d)
      setAck(false)
    })
  }
  if (!open) return null
  const report = view?.report,
    fresh = report && now >= report.createdAt && now < report.expiresAt
  return (
    <Modal title="设备备份" className="device-backups-modal" onClose={working ? undefined : close}>
      <h2>设备备份</h2>
      <p className="notice">
        管理本机 iPhone、iPad
        的整份备份或已复制的备份目录。备份可能是唯一恢复来源，较旧不代表可以删除；不拆删内部文件，不访问
        iCloud 备份。
      </p>
      {!draft && !review && (
        <>
          <div className="button-row">
            <button
              className="button secondary"
              disabled={working || !enabled}
              onClick={() => void scan(null)}
            >
              扫描本机备份
            </button>
            <button
              className="button secondary"
              disabled={working || !enabled}
              onClick={() =>
                void action(async () => {
                  const directory = await api.chooseDirectory('选择包含设备备份副本的目录')
                  if (directory) await transport.scan(directory, crypto.randomUUID())
                })
              }
            >
              选择备份副本目录…
            </button>
          </div>
          <p className="muted">
            仅检查所选目录的直属备份。读取设备名称、状态清单与目录元数据，不解密或打开备份中的个人内容。大小是普通文件逻辑大小，不是可释放空间。
          </p>
          {working && (
            <p role="status">
              {view?.phase === 'processing'
                ? '正在复查或处理整份备份；取消只停止尚未发出的请求。'
                : '正在检查备份目录，可取消。'}
            </p>
          )}
          {report && (
            <>
              <p className="mono full-path" data-i18n="off">
                {report.root}
              </p>
              {!report.complete && (
                <p className="notice warning" role="status">
                  列表或部分项目未完整核对，未知项目保留；不代表没有其他备份。{report.reason}
                </p>
              )}
              {report.cancelled && <p role="status">扫描已取消，已取得的列表不代表完整。</p>}
              {!fresh && <p role="status">备份扫描已过期，请重新扫描后选择。</p>}
              {!report.items.length && (
                <p>本次未列出备份；缺失、权限受限和未知格式不表示备份不存在。</p>
              )}
              <div className="card personal-list" role="region" aria-label="设备备份列表">
                {report.items.map((item) => (
                  <article key={item.id}>
                    <div>
                      <strong data-i18n="off">{item.deviceName || item.name}</strong>
                      <p className="mono full-path" data-i18n="off">
                        {item.path}
                      </p>
                      <p>
                        {item.size === null ? '大小未完整核对' : bytes(item.size)} ·{' '}
                        {item.encrypted === null
                          ? '加密状态未知'
                          : item.encrypted
                            ? '清单标记已加密'
                            : '清单标记未加密'}
                      </p>
                      <p>
                        {item.markedFinished ? '状态文件标记完成' : '状态未确认完成'} ·{' '}
                        {item.modifiedAt ? dateTime(item.modifiedAt) : '修改时间未知'}
                      </p>
                      <p data-i18n="off">{item.reason}</p>
                    </div>
                    <button
                      className="button secondary compact"
                      disabled={working || !enabled || !fresh || !item.eligible}
                      onClick={() => void prepare(item)}
                    >
                      审阅整份处理…
                    </button>
                  </article>
                ))}
              </div>
            </>
          )}
          {!!view?.records.length && (
            <section aria-label="设备备份处理历史">
              <h3>处理历史</h3>
              {view.records.map((r) => (
                <details className="card" key={r.id}>
                  <summary>
                    {backupState[r.state]} · {dateTime(r.createdAt)}{' '}
                    <span data-i18n="off">
                      {r.plan.item.row.deviceName || r.plan.item.row.name}
                    </span>
                  </summary>
                  <p className="mono full-path" data-i18n="off">
                    {r.plan.item.row.path}
                  </p>
                  <p data-i18n="off">{r.reason}</p>
                  {r.receipt && (
                    <>
                      <p>实际收据位置</p>
                      <p className="mono full-path" data-i18n="off">
                        {r.receipt.destination}
                      </p>
                      <button
                        className="button secondary compact"
                        onClick={() => void action(() => api.reveal(r.receipt!.destination))}
                      >
                        在 Finder 中核对收据
                      </button>
                    </>
                  )}
                  <p className="muted">
                    当前没有备份的应用内恢复。请在 Finder
                    按实际收据取回整份目录，原位置有同名项目时不要覆盖；不得将备份内部文件单独拼回。
                  </p>
                  {['unknown', 'interrupted'].includes(r.state) &&
                    (r.reviewedAt ? (
                      <p>已人工核对，原未知结果保持；不会重试。</p>
                    ) : (
                      <button
                        className="button secondary"
                        disabled={working}
                        onClick={() => {
                          setReview(r.id)
                          setReviewAck(false)
                        }}
                      >
                        核对中断结果…
                      </button>
                    ))}
                </details>
              ))}
            </section>
          )}
        </>
      )}
      {draft && (
        <>
          <p className="notice warning">
            将整份备份移入废纸篓，设备恢复时暂时不可用。请确认已有所需独立副本，并先结束
            Finder、iTunes 或其他工具的备份、同步和恢复任务；本次不能完整确认所有程序占用。
          </p>
          <p className="mono full-path" data-i18n="off">
            {draft.item.path}
          </p>
          <p>{draft.item.size === null ? '大小未知' : bytes(draft.item.size)}</p>
          <p>
            不清空废纸篓，不立即释放空间。取消不能撤销已发出的系统请求；结果未知时请核对原位置和收据，不自动重试。
          </p>
          {now >= draft.expiresAt && <p role="status">备份确认已过期，请返回重新审阅。</p>}
          <label className="confirm-check">
            <input
              type="checkbox"
              checked={ack}
              disabled={working || now >= draft.expiresAt}
              onChange={(e) => setAck(e.target.checked)}
            />
            我已核对整份备份、所需副本及恢复边界，并确认没有备份或同步任务正在使用它。
          </label>
          <div className="button-row">
            <button className="button secondary" disabled={working} onClick={() => setDraft(null)}>
              返回备份列表
            </button>
            <button
              className="button primary"
              disabled={working || !ack || now >= draft.expiresAt}
              onClick={() =>
                void action(async () => {
                  await transport.execute(draft.id, crypto.randomUUID())
                  setDraft(null)
                })
              }
            >
              确认整份移入废纸篓
            </button>
          </div>
        </>
      )}
      {review && (
        <>
          <p>
            请在 Finder
            检查原位置、废纸篓和实际收据；此确认只记录已核查，不会补发操作或把未知改成成功。
          </p>
          <label className="confirm-check">
            <input
              type="checkbox"
              checked={reviewAck}
              onChange={(e) => setReviewAck(e.target.checked)}
            />
            我已核对这条记录的实际文件状态。
          </label>
          <div className="button-row">
            <button className="button secondary" disabled={working} onClick={() => setReview(null)}>
              返回备份列表
            </button>
            <button
              className="button secondary"
              disabled={working || !reviewAck}
              onClick={() =>
                void action(async () => {
                  await transport.acknowledge(review)
                  setReview(null)
                })
              }
            >
              记录已核对
            </button>
          </div>
        </>
      )}
      {(error || readError || view?.error) && (
        <p role="alert" className="notice warning" data-i18n="off">
          {error || readError || view?.error}
        </p>
      )}
      {active && view?.requestId && (
        <button
          className="button secondary"
          onClick={() => void action(() => transport.cancel(view.requestId!))}
        >
          停止当前备份任务
        </button>
      )}
    </Modal>
  )
}
