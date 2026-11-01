import { useEffect, useRef, useState } from 'react'
import { LoaderCircle, ShieldCheck, Square } from 'lucide-react'
import { Modal } from './Modal'
import { bytes, dateTime } from '../lib/format'
import { useNow } from '../lib/use-now'
import {
  moleCleanScopes,
  moleCleanTransport,
  type MoleCleanPlan,
  type MoleCleanSnapshot,
  type MoleCleanState,
} from '../lib/mole-cleanup'
const stateNames: Record<MoleCleanState, string> = {
  running: '清理进行中',
  completed: 'Mole 本轮执行结束',
  cancelled: '清理已停止',
  failed: '清理未完成',
  uncertain: '清理结果待核查',
  interrupted: '清理曾中断',
}
const eventNames: Record<string, string> = {
  intent: '已保存执行意图',
  removed: '已确认删除',
  retained: '已保留',
  uncertain: '结果待核查',
  commandCompleted: '所有者命令已返回',
  section: '正在处理',
  upstream: 'Mole 逐项记录',
  authorization: '管理员授权',
  complete: '本轮结束',
}
export function MoleCleanupCenter({
  desktop,
  enabled,
  openSignal,
  historySignal,
  onActiveChange,
}: {
  desktop: boolean
  enabled: boolean
  openSignal: number
  historySignal: number
  onActiveChange: (active: boolean) => void
}) {
  useNow()
  const [open, setOpen] = useState(false),
    [view, setView] = useState<'prepare' | 'history'>('prepare'),
    [system, setSystem] = useState(false),
    [plan, setPlan] = useState<MoleCleanPlan | null>(null),
    [snapshot, setSnapshot] = useState<MoleCleanSnapshot | null>(null),
    [busy, setBusy] = useState(false),
    [agreed, setAgreed] = useState(false),
    [reviewed, setReviewed] = useState(false),
    [clearAgreed, setClearAgreed] = useState(false),
    [error, setError] = useState(''),
    [trackingError, setTrackingError] = useState(''),
    [password, setPassword] = useState(''),
    [submitting, setSubmitting] = useState(false),
    [selected, setSelected] = useState<string | null>(null),
    [limit, setLimit] = useState(100),
    [exported, setExported] = useState(false)
  const epoch = useRef(0),
    live = useRef(true),
    reading = useRef(false),
    pending = useRef(false),
    selection = useRef(selected),
    currentPlan = useRef(plan),
    callback = useRef(onActiveChange)
  callback.current = onActiveChange
  selection.current = selected
  currentPlan.current = plan
  async function read(id = selection.current, quiet = false) {
    if (!desktop || reading.current) return
    reading.current = true
    try {
      const next = await moleCleanTransport.read(id)
      if (!live.current) return
      setSnapshot(next)
      setTrackingError('')
      callback.current(!!next.activeRequest)
    } catch (e) {
      if (live.current && !quiet) setTrackingError(String(e).replace(/^Error: /, ''))
    } finally {
      reading.current = false
    }
  }
  useEffect(() => {
    live.current = true
    if (desktop) void read(null, true)
    return () => {
      live.current = false
      epoch.current++
      const p = currentPlan.current
      if (p) void moleCleanTransport.discard(p.id).catch(() => {})
    }
  }, [desktop])
  useEffect(() => {
    if (!desktop || (!open && !snapshot?.activeRequest) || trackingError) return
    const timer = setInterval(() => void read(), 1000)
    return () => clearInterval(timer)
  }, [desktop, open, snapshot?.activeRequest, trackingError])
  async function prepare(includeSystem: boolean) {
    if (pending.current || !desktop || !enabled || snapshot?.activeRequest) return
    pending.current = true
    const token = ++epoch.current
    setBusy(true)
    setPlan(null)
    setAgreed(false)
    setError('')
    setView('prepare')
    setSystem(includeSystem)
    try {
      const value = await moleCleanTransport.prepare(includeSystem)
      if (token !== epoch.current || !live.current) {
        void moleCleanTransport.discard(value.id).catch(() => {})
        return
      }
      setPlan(value)
    } catch (e) {
      if (token === epoch.current) setError(String(e).replace(/^Error: /, ''))
    } finally {
      pending.current = false
      if (token === epoch.current) setBusy(false)
    }
  }
  useEffect(() => {
    if (!openSignal) return
    setOpen(true)
    if (snapshot?.activeRequest) {
      setView('history')
      setSelected(snapshot.activeRequest)
      void read(snapshot.activeRequest)
    } else void prepare(false)
  }, [openSignal])
  useEffect(() => {
    if (!historySignal) return
    setOpen(true)
    setView('history')
    void read()
  }, [historySignal])
  useEffect(() => {
    setReviewed(false)
    setClearAgreed(false)
    setExported(false)
    setLimit(100)
  }, [snapshot?.current?.id])
  useEffect(() => {
    if (!snapshot?.authorizationRequired) setPassword('')
  }, [snapshot?.activeRequest, snapshot?.authorizationRequired])
  function close() {
    epoch.current++
    setOpen(false)
    setBusy(false)
    setPassword('')
    if (plan) void moleCleanTransport.discard(plan.id).catch(() => {})
    setPlan(null)
  }
  async function execute() {
    if (
      !plan ||
      !agreed ||
      !enabled ||
      pending.current ||
      Date.now() < plan.createdAt ||
      Date.now() >= plan.expiresAt
    )
      return
    const id = crypto.randomUUID()
    pending.current = true
    setBusy(true)
    setError('')
    setSelected(id)
    setView('history')
    callback.current(true)
    setSnapshot((previous) => ({
      version: 1,
      activeRequest: id,
      authorizationRequired: false,
      records: previous?.records ?? [],
      current: null,
    }))
    const idPlan = plan.id
    setPlan(null)
    try {
      await moleCleanTransport.execute(idPlan, id)
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    } finally {
      pending.current = false
      setBusy(false)
      await read(id)
    }
  }
  async function authorize() {
    const id = snapshot?.activeRequest
    if (!id || !snapshot.authorizationRequired || !password || submitting) return
    const value = password
    setPassword('')
    setSubmitting(true)
    setError('')
    try {
      await moleCleanTransport.authorize(id, value)
      await read(id)
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    } finally {
      setSubmitting(false)
    }
  }
  async function stop() {
    if (!snapshot?.activeRequest) return
    setError('')
    try {
      await moleCleanTransport.cancel(snapshot.activeRequest)
      await read(snapshot.activeRequest)
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    }
  }
  const record = snapshot?.current,
    active = !!snapshot?.activeRequest,
    fresh = !!plan && Date.now() >= plan.createdAt && Date.now() < plan.expiresAt
  const lastSeq = record?.events.at(-1)?.seq ?? 0
  const lastSection = record
    ? [...record.events].reverse().find((e) => e.kind === 'section')?.detail
    : undefined
  const currentSection = lastSection
    ? (
        {
          System: 'system',
          'User essentials': 'userEssentials',
          'App caches': 'appCaches',
          Browsers: 'browsers',
          'Cloud & Office': 'cloudOffice',
          'Developer tools': 'developerTools',
          'Apps & utilities': 'appsUtilities',
          Virtualization: 'virtualization',
          'Application Support': 'applicationSupport',
          'App leftovers': 'appLeftovers',
          'Device backups & firmware': 'firmware',
          'Time Machine': 'timeMachine',
          'Large files': 'largeFiles',
          'Project artifacts': 'projectArtifacts',
        } as Record<string, string>
      )[lastSection]
    : null
  const sectionLabel = moleCleanScopes.find(([id]) => id === currentSection)?.[1]
  async function acknowledge() {
    if (!record || !reviewed) return
    try {
      await moleCleanTransport.acknowledge(record.id, lastSeq)
      await read(record.id)
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    }
  }
  async function exportRecord() {
    if (!record) return
    try {
      const saved = await moleCleanTransport.saveExport(record.id)
      if (saved) setExported(true)
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    }
  }
  async function clearRecord() {
    if (!record || !clearAgreed || !exported) return
    try {
      await moleCleanTransport.clear(record.id, lastSeq)
      setSelected(null)
      setClearAgreed(false)
      setExported(false)
      await read(null)
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    }
  }
  return (
    <>
      {active && !open && (
        <section className="notice mole-clean-active" aria-label="完整清理状态">
          <LoaderCircle size={16} className="spin" />
          <span>完整 Mole 清理进行中</span>
          <button
            className="button secondary compact"
            onClick={() => {
              setView('history')
              setOpen(true)
              void read()
            }}
          >
            查看清理进度
          </button>
        </section>
      )}
      {open && (
        <Modal title="Mole 一键清理" className="mole-clean-modal" onClose={close}>
          <h2>Mole 一键清理</h2>
          <div className="mole-clean-tabs">
            <button
              className="button secondary compact"
              disabled={active || busy || !enabled}
              onClick={() => void prepare(system)}
            >
              完整清理
            </button>
            <button
              className="button secondary compact"
              onClick={() => {
                setView('history')
                void read()
              }}
            >
              清理记录
            </button>
          </div>
          {view === 'prepare' && (
            <>
              <p>使用与 mo clean 相同的完整清理规则，自动处理符合条件的项目。</p>
              <p className="notice warning">
                会永久删除 Mole
                判定可处理的缓存、过期日志等项目，并处理当前账户废纸篓。不会先移入废纸篓，MooMole
                无法撤销这些删除。请先备份重要数据。
              </p>
              <label className="mole-clean-choice">
                <input
                  type="checkbox"
                  checked={system}
                  disabled={busy || !enabled}
                  onChange={(e) => void prepare(e.target.checked)}
                />
                包括需要管理员权限的系统项目
              </label>
              {!system && (
                <p className="muted">本次只处理当前账户可执行的范围，跳过系统管理员项目。</p>
              )}
              {busy && (
                <p role="status">
                  <LoaderCircle size={15} className="spin" />
                  正在核对引擎、账户与保护规则…
                </p>
              )}
              {plan && (
                <>
                  <ul className="mole-clean-scopes">
                    {moleCleanScopes.map(([id, label]) => (
                      <li key={id}>
                        <ShieldCheck size={13} />
                        <span>{label}</span>
                        {id === 'system' && !plan.system && <small>本次跳过</small>}
                        {['largeFiles', 'projectArtifacts'].includes(id) && (
                          <small>仅提示审阅</small>
                        )}
                      </li>
                    ))}
                  </ul>
                  <p className="muted">
                    Mole 白名单和 MooMole
                    保护列表继续生效。正在使用、受保护、权限不足或无法确认的对象，按上游规则保留。Docker
                    数据、本地快照和个人大文件中的提示项目不会被当作自动删除项。
                  </p>
                  <details>
                    <summary>本次操作范围与版本</summary>
                    <p>内置 Mole {plan.engineVersion}</p>
                    <p className="mono full-path" data-i18n="off">
                      {plan.home}
                    </p>
                    <p>客户端保护 {plan.protectedPaths.length} 个位置；Mole 白名单另行生效。</p>
                    <p>
                      执行中可停止；已完成的删除不会撤销。包管理器命令可能已经处理部分内容，中断后不会自动重试。
                    </p>
                  </details>
                  {!fresh && (
                    <p className="notice warning" role="status">
                      完整清理确认已过期，请重新准备。
                    </p>
                  )}
                  <label className="mole-clean-choice">
                    <input
                      type="checkbox"
                      checked={agreed}
                      onChange={(e) => setAgreed(e.target.checked)}
                    />
                    我已了解永久删除和废纸篓处理不可撤销，确认执行上述完整范围。
                  </label>
                  <button
                    className="button primary"
                    disabled={!fresh || !agreed || busy || !enabled}
                    onClick={() => void execute()}
                  >
                    确认执行完整清理
                  </button>
                </>
              )}
            </>
          )}
          {view === 'history' && (
            <>
              {snapshot?.records.length ? (
                <label className="mole-clean-history-select">
                  清理批次
                  <select
                    disabled={active}
                    value={record?.id ?? ''}
                    onChange={(e) => {
                      setSelected(e.target.value)
                      setReviewed(false)
                      setClearAgreed(false)
                      setExported(false)
                      setLimit(100)
                      void read(e.target.value)
                    }}
                  >
                    {snapshot.records.map((r) => (
                      <option key={r.id} value={r.id}>
                        {dateTime(r.createdAt)} · {stateNames[r.state]}
                      </option>
                    ))}
                  </select>
                </label>
              ) : (
                !busy && <p>尚无完整 Mole 清理记录。</p>
              )}
              {record && (
                <>
                  <p className="mole-clean-run-state" role="status">
                    <strong>{stateNames[record.state]}</strong>
                    {sectionLabel && active && <span>{sectionLabel}</span>}
                  </p>
                  {snapshot?.authorizationRequired && (
                    <form
                      className="mole-clean-authorization card"
                      onSubmit={(e) => {
                        e.preventDefault()
                        void authorize()
                      }}
                    >
                      <h3>本机管理员授权</h3>
                      <p>
                        sudo 正在等待授权。密码仅发送给本次 sudo
                        授权进程，不写入日志、历史或设置，也不上传。取消会停止本轮清理。
                      </p>
                      <label>
                        当前账户密码
                        <input
                          type="password"
                          autoComplete="off"
                          value={password}
                          onChange={(e) => setPassword(e.target.value)}
                          disabled={submitting}
                          maxLength={512}
                        />
                      </label>
                      <button className="button primary compact" disabled={submitting || !password}>
                        提交本次授权
                      </button>
                    </form>
                  )}
                  {active && (
                    <button className="button secondary" onClick={() => void stop()}>
                      <Square size={12} />
                      停止本次清理
                    </button>
                  )}
                  <p className="muted">
                    已保存 {record.events.filter((e) => e.kind === 'removed').length}{' '}
                    条已确认删除记录，
                    {record.events.filter((e) => e.kind === 'commandCompleted').length}{' '}
                    条模块命令结果。第三方命令内部的文件明细可能不可得，不据命令返回值推算释放量。
                  </p>
                  {record.beforeBytes !== null && record.afterBytes !== null ? (
                    <p>
                      磁盘可用空间变化：{record.afterBytes >= record.beforeBytes ? '+' : '−'}
                      {bytes(Math.abs(record.afterBytes - record.beforeBytes))}
                      。这是前后观察值，也会受其他程序影响。
                    </p>
                  ) : (
                    <p className="muted">磁盘可用空间变化：尚无完整观察值，不推算释放量。</p>
                  )}
                  {record.error && (
                    <p className="notice warning" data-i18n="off">
                      {record.error}
                    </p>
                  )}
                  {['uncertain', 'interrupted'].includes(record.state) && (
                    <p className="notice warning">
                      有已保存意图或不完整结果，需要自行核查实际文件；不会自动重试，也不能把停止当作已经恢复。
                    </p>
                  )}
                  <details className="mole-clean-events">
                    <summary>逐项结果与处理过程</summary>
                    <ul>
                      {record.events.slice(-limit).map((e) => (
                        <li key={e.seq}>
                          <strong>{eventNames[e.kind]}</strong>
                          {e.path && (
                            <span className="mono full-path" data-i18n="off">
                              {e.path}
                            </span>
                          )}
                          <small data-i18n="off">{e.detail}</small>
                        </li>
                      ))}
                    </ul>
                    {record.events.length > limit && (
                      <button
                        className="button secondary compact"
                        onClick={() => setLimit((v) => v + 100)}
                      >
                        再显示 100 项
                      </button>
                    )}
                    {record.eventsTruncated && (
                      <p className="muted">页面显示最近的 2000 条记录；导出包含完整记录。</p>
                    )}
                  </details>
                  {record.diagnostic && (
                    <details>
                      <summary>Mole 原始输出与审阅提示</summary>
                      <pre data-i18n="off" className="mole-clean-output">
                        {record.diagnostic}
                      </pre>
                      <p className="muted">
                        原始输出仅供诊断，不用于决定清理对象；最多保留前 256 KB 文本。
                      </p>
                    </details>
                  )}
                  {!active && (
                    <details className="mole-clean-history-actions">
                      <summary>保存与核查记录</summary>
                      <p>导出包含本机文件路径，请妥善保管。</p>
                      <button
                        className="button secondary compact"
                        onClick={() => void exportRecord()}
                      >
                        导出完整清理记录
                      </button>
                      <label className="mole-clean-choice">
                        <input
                          type="checkbox"
                          checked={reviewed}
                          onChange={(e) => setReviewed(e.target.checked)}
                        />
                        我已核查本批次的实际结果，了解不会撤销或重新执行。
                      </label>
                      <button
                        className="button secondary compact"
                        disabled={!reviewed || record.reviewedAt !== null}
                        onClick={() => void acknowledge()}
                      >
                        标记为已核查
                      </button>
                      {record.reviewedAt !== null && (
                        <>
                          <label className="mole-clean-choice">
                            <input
                              type="checkbox"
                              checked={clearAgreed}
                              onChange={(e) => setClearAgreed(e.target.checked)}
                            />
                            我已保存导出的记录，确认永久清除本批次历史。
                          </label>
                          <button
                            className="button secondary compact"
                            disabled={!clearAgreed || !exported}
                            onClick={() => void clearRecord()}
                          >
                            清除已保存的历史记录
                          </button>
                        </>
                      )}
                    </details>
                  )}
                </>
              )}
            </>
          )}
          {error && (
            <p className="notice error" role="alert">
              {error}
            </p>
          )}
          {trackingError && (
            <p className="notice warning" role="alert">
              {trackingError}
            </p>
          )}
          <div className="modal-actions">
            <button className="button secondary" onClick={close}>
              {active ? '在后台继续' : '关闭'}
            </button>
            <button className="button secondary" onClick={() => void read()}>
              重新读取清理状态
            </button>
          </div>
        </Modal>
      )}
    </>
  )
}
