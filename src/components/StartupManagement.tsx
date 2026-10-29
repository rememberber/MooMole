import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import * as api from '../lib/api'
import {
  startupTransport as transport,
  startupRecordName,
  type StartupView,
  type StartupRow,
  type StartupDraft,
  type StartupRecord,
} from '../lib/startup-management'
import { dateTime } from '../lib/format'
import { useNow } from '../lib/use-now'
import { Modal } from './Modal'

export function StartupManagement({
  visible,
  historyVisible,
  enabled,
  toolbar,
  onActiveChange,
}: {
  visible: boolean
  historyVisible: boolean
  enabled: boolean
  toolbar: HTMLElement | null
  onActiveChange: (active: boolean) => void
}) {
  const [awake, setAwake] = useState(false),
    [epoch, setEpoch] = useState(0),
    [view, setView] = useState<StartupView>({ report: null, records: [], activeRequest: null })
  const [pending, setPending] = useState(false),
    [checking, setChecking] = useState(''),
    [request, setRequest] = useState<string | null>(null),
    [error, setError] = useState('')
  const [draft, setDraft] = useState<StartupDraft | null>(null),
    [result, setResult] = useState<StartupRecord | null>(null),
    [ack, setAck] = useState(false),
    [selected, setSelected] = useState<StartupRecord | null>(null),
    [review, setReview] = useState(false)
  const busy = useRef(false),
    now = useNow(),
    active = pending || !!view.activeRequest
  useEffect(() => {
    if (visible || historyVisible) setAwake(true)
  }, [visible, historyVisible])
  useEffect(() => {
    onActiveChange(active)
  }, [active, onActiveChange])
  useEffect(
    () => () => {
      if (draft) void transport.discard(draft.id).catch(() => {})
    },
    [draft?.id],
  )
  useEffect(() => {
    if (!awake || !api.desktop) return
    let live = true,
      timer: ReturnType<typeof setTimeout> | undefined
    const read = async () => {
      try {
        const v = await transport.read()
        if (!live) return
        setView(v)
        if (v.activeRequest || pending) timer = setTimeout(() => void read(), 700)
      } catch (e) {
        if (live) {
          setError(String(e))
          timer = setTimeout(() => void read(), 2000)
        }
      }
    }
    void read()
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [awake, epoch, pending])
  const report = view.report,
    fresh = !!report && now >= report.createdAt && now < report.expiresAt
  async function scan() {
    if (busy.current || active || !enabled) return
    busy.current = true
    const id = crypto.randomUUID()
    setRequest(id)
    setPending(true)
    setError('')
    try {
      await transport.scan(id)
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      setRequest(null)
      setPending(false)
      setEpoch((v) => v + 1)
    }
  }
  async function prepare(row: StartupRow) {
    if (!report || busy.current || !fresh || !row.eligible || active || !enabled) return
    busy.current = true
    setChecking(row.id)
    setError('')
    setResult(null)
    try {
      setDraft(await transport.prepare(report, row))
      setAck(false)
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      setChecking('')
    }
  }
  async function execute() {
    if (!draft || !ack || busy.current || request || now >= draft.expiresAt) return
    busy.current = true
    const id = crypto.randomUUID()
    setRequest(id)
    setPending(true)
    setError('')
    try {
      setResult(await transport.execute(draft, id))
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
      setDraft(null)
    } finally {
      busy.current = false
      setRequest(null)
      setPending(false)
      setEpoch((v) => v + 1)
    }
  }
  const stop = () => {
    const id = request || view.activeRequest
    if (id) void transport.cancel(id).catch((e) => setError(String(e)))
  }
  const tools =
    visible &&
    toolbar &&
    createPortal(
      <>
        <button
          className="button secondary compact"
          disabled={!api.desktop}
          onClick={() => void transport.settings().catch((e) => setError(String(e)))}
        >
          系统登录项设置
        </button>
        {active ? (
          <button className="button secondary compact" onClick={stop}>
            停止启动任务
          </button>
        ) : (
          <button
            className="button primary compact"
            disabled={!enabled || !!checking}
            onClick={() => void scan()}
          >
            读取启动配置
          </button>
        )}
      </>,
      toolbar,
    )
  return (
    <>
      {tools}
      {visible && (
        <section aria-label="启动与后台管理">
          <p className="notice">
            管理当前用户 LaunchAgents
            目录中的登录配置。暂停会保留配置文件并改变名称，再请求移除相同程序的后台注册；恢复后程序可能立即运行。可能影响同步、更新或其他工作，请逐项确认。文件在启动目录不表示当前正在运行或系统允许启动。
          </p>
          <p className="muted">
            现代登录项、其他账户、系统服务与管理员安装的后台项目请在系统登录项设置中管理；本页不代表全部后台来源。
          </p>
          {report && !report.complete && (
            <p className="notice warning" role="status">
              本轮未完整读取，未检查的范围仍未知；请结合系统登录项设置核对。
            </p>
          )}
          {report && !fresh && (
            <p className="notice warning" role="status">
              启动配置清单已过期，请重新读取。
            </p>
          )}
          {!report && !active && (
            <p className="muted">读取用户登录配置，或打开系统登录项设置查看其他来源。</p>
          )}
          {active && <p role="status">正在检查或处理启动配置；停止不会撤销已完成的配置更改。</p>}
          {report && (
            <div className="card personal-list" role="region" aria-label="用户启动配置列表">
              {!report.items.length && <p>本轮目录未发现可识别配置，不代表没有登录项。</p>}
              {report.items.map((row) => (
                <article key={row.id}>
                  <div>
                    <strong data-i18n="off">
                      {row.program
                        .split('/')
                        .find((p) => p.endsWith('.app'))
                        ?.slice(0, -4) ||
                        row.program.split('/').pop() ||
                        row.name}
                    </strong>
                    <span> · {row.paused ? '已暂停的配置' : '启动目录中的配置'}</span>
                    <small className="mono full-path" data-i18n="off">
                      {row.path}
                    </small>
                    <details>
                      <summary>程序与参数</summary>
                      <p className="mono full-path" data-i18n="off">
                        {row.label}
                      </p>
                      <p className="mono full-path" data-i18n="off">
                        {row.program}
                      </p>
                      <pre data-i18n="off">{JSON.stringify(row.arguments, null, 2)}</pre>
                    </details>
                    {row.reason && <p className="muted">{row.reason}</p>}
                  </div>
                  <button
                    className="button secondary compact"
                    disabled={!enabled || active || !!checking || !fresh || !row.eligible}
                    onClick={() => void prepare(row)}
                  >
                    {checking === row.id ? '正在检查…' : row.paused ? '恢复此配置…' : '暂停此配置…'}
                  </button>
                </article>
              ))}
            </div>
          )}
        </section>
      )}
      {(historyVisible || visible) && view.records.length > 0 && (
        <section className="card" aria-label="启动管理记录">
          <div className="list-toolbar">
            <h2>启动管理记录</h2>
            <button className="button secondary compact" onClick={() => setEpoch((v) => v + 1)}>
              刷新启动记录
            </button>
          </div>
          {view.records.map((r) => (
            <button
              className="saved-plan-row"
              key={r.id}
              onClick={() => {
                setSelected(r)
                setReview(false)
              }}
            >
              <span>
                <strong>{startupRecordName(r)}</strong>
                <small>{dateTime(r.createdAt)}</small>
                <span className="mono full-path" data-i18n="off">
                  {r.path}
                </span>
              </span>
            </button>
          ))}
        </section>
      )}
      {(visible || historyVisible) && error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {draft && (
        <Modal
          title={result ? '启动操作结果' : '确认启动配置操作'}
          onClose={
            pending
              ? undefined
              : () => {
                  setDraft(null)
                  setResult(null)
                }
          }
        >
          <h2>
            {result
              ? startupRecordName(result)
              : draft.action === 'pause'
                ? '暂停此登录配置'
                : '恢复此登录配置'}
          </h2>
          <p className="notice">
            暂停可能中断后台同步或更新；恢复可能立即执行当前程序。停止只阻止后续步骤，已完成的更名或系统请求不会自动撤销。未知结果须在历史核对，不会自动重试。
          </p>
          <p className="mono full-path" data-i18n="off">
            {draft.path}
          </p>
          <p className="mono full-path" data-i18n="off">
            → {draft.target}
          </p>
          <details>
            <summary>程序与参数</summary>
            <p className="mono full-path" data-i18n="off">
              {draft.program}
            </p>
            <pre data-i18n="off">{JSON.stringify(draft.arguments, null, 2)}</pre>
          </details>
          {result ? (
            <div role="region" aria-label="启动操作结果">
              <p>{result.reason}</p>
              <p>
                {result.configurationChanged
                  ? '配置文件更名已有完成证据。'
                  : '配置文件更名尚无完成证据。'}
              </p>
            </div>
          ) : (
            <>
              {now >= draft.expiresAt && <p role="status">启动确认已过期，请返回后重新检查。</p>}
              <label className="confirm-check">
                <input
                  type="checkbox"
                  checked={ack}
                  disabled={pending}
                  onChange={(e) => setAck(e.target.checked)}
                />
                我已核对配置和程序，了解可能影响后台工作。
              </label>
            </>
          )}
          {error && (
            <p className="notice warning" role="alert">
              {error}
            </p>
          )}
          <div className="dialog-actions">
            {!result && (
              <button
                className="button primary"
                disabled={pending || !ack || now >= draft.expiresAt}
                onClick={() => void execute()}
              >
                确认更改此配置
              </button>
            )}
            {pending ? (
              <button className="button secondary" onClick={stop}>
                停止启动任务
              </button>
            ) : (
              <button
                className="button secondary"
                onClick={() => {
                  setDraft(null)
                  setResult(null)
                }}
              >
                返回启动列表
              </button>
            )}
          </div>
        </Modal>
      )}
      {selected && (
        <Modal title="启动管理记录" onClose={() => setSelected(null)}>
          <h2>{startupRecordName(selected)}</h2>
          <p>{selected.reason}</p>
          <p className="mono full-path" data-i18n="off">
            {selected.path}
          </p>
          <p className="mono full-path" data-i18n="off">
            → {selected.target}
          </p>
          {['partial', 'uncertain', 'interrupted'].includes(selected.state) &&
            !selected.reviewedAt && (
              <>
                <p>
                  请核对配置两个位置和系统后台状态。记录已检查不会修改文件、重放操作或把原结果改成成功。
                </p>
                <label className="confirm-check">
                  <input
                    type="checkbox"
                    checked={review}
                    onChange={(e) => setReview(e.target.checked)}
                  />
                  我已核对配置位置和后台状态，了解原结果仍待核实。
                </label>
                <button
                  className="button secondary"
                  disabled={!review}
                  onClick={async () => {
                    try {
                      setSelected(await transport.acknowledge(selected.id))
                      setEpoch((v) => v + 1)
                    } catch (e) {
                      setError(String(e))
                    }
                  }}
                >
                  记录已核对
                </button>
              </>
            )}
          <button
            className="button secondary"
            onClick={() =>
              void api
                .reveal(selected.target.slice(0, selected.target.lastIndexOf('/')))
                .catch((e) => setError(String(e)))
            }
          >
            在 Finder 中查看目标目录
          </button>
          <button className="button secondary" onClick={() => setSelected(null)}>
            关闭启动记录
          </button>
        </Modal>
      )}
    </>
  )
}
