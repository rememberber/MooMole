import { useEffect, useRef, useState } from 'react'
import type { UninstallTarget } from '../lib/app-uninstall'
import {
  appUpdateDiscovery as api,
  type AppUpdateProbe,
  type AppUpdateDiscovery,
} from '../lib/app-update-discovery'
import { bytes } from '../lib/format'
import { useNow } from '../lib/use-now'
import { Modal } from './Modal'
export function AppUpdateDiscoveryCenter({
  target,
  onClose,
  onActiveChange,
  onDownloaded,
}: {
  target: UninstallTarget | null
  onClose: () => void
  onActiveChange: (active: boolean) => void
  onDownloaded: (target: UninstallTarget) => void
}) {
  return target ? (
    <Discovery
      key={`${target.taskId}:${target.index}`}
      target={target}
      onClose={onClose}
      onActiveChange={onActiveChange}
      onDownloaded={onDownloaded}
    />
  ) : null
}
function Discovery({
  target,
  onClose,
  onActiveChange,
  onDownloaded,
}: {
  target: UninstallTarget
  onClose: () => void
  onActiveChange: (active: boolean) => void
  onDownloaded: (target: UninstallTarget) => void
}) {
  const [probe, setProbe] = useState<AppUpdateProbe | null>(null),
    [report, setReport] = useState<AppUpdateDiscovery | null>(null),
    [pending, setPending] = useState<'reading' | 'checking' | 'opening' | null>(null),
    [error, setError] = useState(''),
    [stopping, setStopping] = useState(false),
    [opened, setOpened] = useState(false)
  const active = useRef<string | null>(null),
    owned = useRef<string | null>(null),
    alive = useRef(true),
    busy = useRef(false)
  useNow()
  const now = Date.now(),
    expired = !!probe && (now < probe.createdAt || now >= probe.expiresAt)
  useEffect(() => {
    alive.current = true
    void prepare()
    return () => {
      alive.current = false
      if (active.current) void api.cancel(active.current).catch(() => {})
      if (owned.current) void api.discard(owned.current).catch(() => {})
      onActiveChange(false)
    }
  }, [])
  function withdraw() {
    if (owned.current) void api.discard(owned.current).catch(() => {})
    owned.current = null
    setProbe(null)
    setReport(null)
    setOpened(false)
  }
  async function prepare() {
    if (busy.current) return
    busy.current = true
    withdraw()
    setPending('reading')
    setError('')
    setStopping(false)
    onActiveChange(true)
    const id = crypto.randomUUID()
    active.current = id
    try {
      const p = await api.prepare(target, id)
      if (alive.current) {
        owned.current = p.id
        setProbe(p)
      } else void api.discard(p.id).catch(() => {})
    } catch (e) {
      if (alive.current) setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      active.current = null
      if (alive.current) {
        setPending(null)
        onActiveChange(false)
      }
    }
  }
  async function check() {
    if (!probe || expired || probe.state !== 'ready' || busy.current) return
    busy.current = true
    setPending('checking')
    setError('')
    setReport(null)
    setOpened(false)
    setStopping(false)
    onActiveChange(true)
    const id = crypto.randomUUID()
    active.current = id
    try {
      const r = await api.check(probe, id)
      if (alive.current) setReport(r)
    } catch (e) {
      if (alive.current) setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      active.current = null
      if (alive.current) {
        setPending(null)
        onActiveChange(false)
      }
    }
  }
  async function open(candidateId: string) {
    if (!probe || expired || busy.current) return
    busy.current = true
    setPending('opening')
    setError('')
    setStopping(false)
    onActiveChange(true)
    const id = crypto.randomUUID()
    active.current = id
    try {
      await api.open(probe.id, candidateId, id)
      if (alive.current) setOpened(true)
    } catch (e) {
      if (alive.current) setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      active.current = null
      if (alive.current) {
        setPending(null)
        onActiveChange(false)
      }
    }
  }
  return (
    <Modal
      title="检查此应用更新"
      className="application-update-modal application-discovery-modal"
      onClose={pending ? undefined : onClose}
    >
      <h2>检查此应用更新</h2>
      <strong data-i18n="off">{target.name}</strong>
      <p className="mono full-path" data-i18n="off">
        {target.path}
      </p>
      {probe && (
        <section aria-label="应用声明的更新来源">
          <dl className="application-update-facts">
            <dt>本机版本 / 构建</dt>
            <dd data-i18n="off">
              {probe.currentVersion ?? '—'} / {probe.currentBuild ?? '—'}
            </dd>
            <dt>来源状态</dt>
            <dd>{probe.state === 'ready' ? '此地址由应用声明，尚未验证发布者。' : probe.reason}</dd>
          </dl>
          {probe.feedUrl && (
            <>
              <p className="mono full-path" data-i18n="off">
                {probe.feedUrl}
              </p>
              <p>
                只请求以上地址，不附加本机清单或本地文件内容。版本和下载地址来自该来源，尚未验证安装包或发布者。
              </p>
            </>
          )}
        </section>
      )}
      {pending && (
        <p role="status">
          {pending === 'reading'
            ? '正在读取此副本的更新来源，尚未联网。'
            : pending === 'checking'
              ? '正在向显示的地址检查更新…'
              : '正在请求浏览器打开所选地址…'}
        </p>
      )}
      {report && (
        <section aria-label="应用更新发现结果">
          <h3>
            {report.state === 'newerReported'
              ? '此来源报告了较新版本'
              : report.state === 'noNewerInFeed'
                ? '此来源未报告本机适用的较新稳定版本'
                : '来源信息不足，版本状态未知'}
          </h3>
          <p>检查范围仅为此应用声明的地址，其他渠道请在应用自身确认；不会自动安装。</p>
          {!report.complete && (
            <p className="notice warning">部分更新条目无法核对，当前结果未完整覆盖。</p>
          )}
          {report.releases.map((r) => (
            <article key={r.id} className="card">
              <strong data-i18n="off">
                {r.version} · {r.build}
              </strong>
              {r.bytes !== null && <p>{bytes(r.bytes)}</p>}
              {r.requiresReview && (
                <p className="notice warning">此版本需要另行核对授权或升级要求。</p>
              )}
              <p className="mono full-path" data-i18n="off">
                {r.url}
              </p>
              <button
                className="button secondary compact"
                disabled={!!pending || expired}
                onClick={() => void open(r.id)}
              >
                {r.kind === 'package' ? '在浏览器打开下载地址' : '在浏览器打开版本页面'}
              </button>
            </article>
          ))}
        </section>
      )}
      {opened && <p role="status">已请求浏览器打开所选地址；下载是否完成请在浏览器确认。</p>}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {expired && (
        <p className="notice warning">此更新来源已过期，请重新读取后再检查或打开地址。</p>
      )}
      <p>
        下载并解压可信的新版 .app 后，可继续独立核对版本、签名与替换范围。旧包保留，不自动退出应用。
      </p>
      <div className="modal-actions">
        {pending ? (
          <button
            className="button secondary"
            disabled={stopping || !active.current}
            onClick={() => {
              if (active.current) {
                setStopping(true)
                void api.cancel(active.current).catch((e) => {
                  if (alive.current) {
                    setStopping(false)
                    setError(String(e))
                  }
                })
              }
            }}
          >
            {stopping ? '已请求停止' : '停止更新来源检查'}
          </button>
        ) : (
          <button className="button secondary" onClick={onClose}>
            关闭更新检查
          </button>
        )}
        {!pending && (
          <button className="button secondary" onClick={() => void prepare()}>
            重新读取更新来源
          </button>
        )}
        {probe?.state === 'ready' && (
          <button
            className="button primary"
            disabled={!!pending || expired}
            onClick={() => void check()}
          >
            向此地址检查
          </button>
        )}
        <button
          className="button secondary"
          disabled={!!pending}
          onClick={() => onDownloaded(target)}
        >
          使用已下载应用包…
        </button>
      </div>
    </Modal>
  )
}
