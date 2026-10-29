import { useEffect, useRef, useState } from 'react'
import { bytes, dateTime } from '../lib/format'
import { useNow } from '../lib/use-now'
import {
  storageMonitorDefaults,
  readStorageMonitor,
  type StorageMonitorView,
  type StorageMonitorConfig,
  type StorageMonitorTransport,
} from '../lib/storage-monitor'

export function StorageMonitor({
  enabled,
  transport,
  onAnalyze,
}: {
  enabled: boolean
  transport: StorageMonitorTransport
  onAnalyze: () => void
}) {
  const [view, setView] = useState<StorageMonitorView | null>(null),
    [config, setConfig] = useState<StorageMonitorConfig>(storageMonitorDefaults)
  const [pending, setPending] = useState(false),
    [error, setError] = useState('')
  const busy = useRef(false),
    ticket = useRef(0),
    dirty = useRef(false)
  const now = useNow()
  useEffect(() => {
    if (!enabled) return
    let live = true
    async function refresh() {
      if (busy.current) return
      const at = ++ticket.current
      try {
        const next = readStorageMonitor(await transport.read())
        if (live && ticket.current === at) {
          setView(next)
          if (!dirty.current) setConfig(next.config)
          setError('')
        }
      } catch (e) {
        if (live && ticket.current === at) setError(String(e).replace(/^Error: /, ''))
      }
    }
    void refresh()
    const timer = window.setInterval(() => void refresh(), 30_000)
    return () => {
      live = false
      clearInterval(timer)
      ticket.current++
    }
  }, [enabled, transport])
  function edit(change: Partial<StorageMonitorConfig>) {
    dirty.current = true
    setConfig((v) => ({ ...v, ...change }))
  }
  async function run(save: boolean) {
    if (!enabled || busy.current) return
    busy.current = true
    setPending(true)
    setError('')
    const at = ++ticket.current
    try {
      const next = readStorageMonitor(
        await (save ? transport.configure(config) : transport.sample()),
      )
      if (ticket.current === at) {
        setView(next)
        if (save || !dirty.current) {
          setConfig(next.config)
          dirty.current = false
        }
      }
    } catch (e) {
      if (ticket.current === at) setError(String(e).replace(/^Error: /, ''))
    } finally {
      busy.current = false
      setPending(false)
    }
  }
  const latest = view?.samples.at(-1)
  const sameVolume = latest
    ? view!.samples
        .filter((s) => s.volumeId === latest.volumeId && s.totalBytes === latest.totalBytes)
        .slice(-60)
    : []
  const fresh =
    latest &&
    now >= latest.measuredAt &&
    now - latest.measuredAt <= (view?.config.intervalMinutes ?? 60) * 120_000
  const low = latest && view && latest.availableBytes < view.config.warnBelowBytes
  const ordered = sameVolume.every((s, i) => !i || s.measuredAt > sameVolume[i - 1].measuredAt)
  const start = sameVolume[0]?.measuredAt ?? 0,
    end = latest?.measuredAt ?? 0
  const points =
    ordered && end > start
      ? sameVolume
          .map(
            (s) =>
              `${8 + ((s.measuredAt - start) / (end - start)) * 584},${112 - (s.availableBytes / s.totalBytes) * 104}`,
          )
          .join(' ')
      : ''
  return (
    <section className="storage-monitor" aria-label="持续空间管理">
      <div className="storage-monitor-settings">
        <label>
          <input
            type="checkbox"
            checked={config.enabled}
            disabled={!enabled || pending || !view}
            onChange={(e) => edit({ enabled: e.target.checked })}
          />
          定期记录空间
        </label>
        <label>
          记录间隔
          <select
            aria-label="空间记录间隔"
            value={config.intervalMinutes}
            disabled={!enabled || pending || !view}
            onChange={(e) => edit({ intervalMinutes: Number(e.target.value) })}
          >
            <option value={15}>每 15 分钟</option>
            <option value={60}>每小时</option>
            <option value={360}>每 6 小时</option>
          </select>
        </label>
        <label>
          低空间提示
          <select
            aria-label="低空间阈值"
            value={config.warnBelowBytes}
            disabled={!enabled || pending || !view}
            onChange={(e) => edit({ warnBelowBytes: Number(e.target.value) })}
          >
            {[5, 10, 20].map((n) => (
              <option key={n} value={n * 1024 ** 3}>
                {bytes(n * 1024 ** 3)}
              </option>
            ))}
          </select>
        </label>
        <button
          className="button secondary compact"
          disabled={!enabled || pending || !view}
          onClick={() => void run(true)}
        >
          保存观察设置
        </button>
        <button
          className="button primary compact"
          disabled={!enabled || pending || view?.collecting}
          onClick={() => void run(false)}
        >
          {pending || view?.collecting ? '正在记录…' : '记录当前空间'}
        </button>
      </div>
      <p className="muted">
        仅在 MooMole 运行时按间隔记录本账户所在卷的容量。休眠后只补一次当前观察，退出 App
        后停止；设置会保留到下次打开。最多保留 720 条样本。
      </p>
      {view && (
        <p role="status">
          {view.config.enabled ? '定期空间观察已开启' : '定期空间观察已关闭'} ·{' '}
          {view.samples.length} 条真实记录
        </p>
      )}
      {!enabled && <p className="notice">请在桌面 App 中记录真实空间趋势。</p>}
      {(error || view?.error) && (
        <p className="notice warning" role="alert">
          {error || view?.error}
        </p>
      )}
      {latest ? (
        <>
          <div className="storage-monitor-reading">
            <div>
              <span>账户所在卷可用空间</span>
              <strong>{bytes(latest.availableBytes)}</strong>
              <small>
                {dateTime(latest.measuredAt)} · {fresh ? '最近观察' : '历史观察，当前状态未知'}
              </small>
            </div>
            <button className="button secondary compact" onClick={onAnalyze}>
              分析空间去向
            </button>
          </div>
          {low && (
            <p className="notice warning" role="status">
              {fresh
                ? '最近观察的可用空间低于提醒阈值，建议检查大文件与可整理内容。'
                : '上次观察的可用空间低于阈值，请先记录当前空间再判断。'}
            </p>
          )}
          {points ? (
            <figure className="storage-trend">
              <svg viewBox="0 0 600 128" role="img" aria-label="同一卷可用空间趋势">
                <line x1="8" y1="112" x2="592" y2="112" stroke="var(--border)" />
                <polyline points={points} fill="none" stroke="currentColor" strokeWidth="2" />
              </svg>
              <figcaption>
                {dateTime(start)} — {dateTime(end)} · 仅连接最近同一卷的真实观察点
              </figcaption>
            </figure>
          ) : (
            <p className="muted">
              至少两条时间递增的同卷记录才展示趋势。卷变化或时间回拨不合并推算。
            </p>
          )}
          <p className="muted">
            可用空间以系统当前向本账户报告的数值为准。APFS
            共享容量、缓存、快照与其他应用活动可能导致波动，变化不等于本次清理效果。低空间提示仅在此页面显示。
          </p>
        </>
      ) : (
        <p className="muted">尚无空间记录。可先记录一次，再开启定期观察。</p>
      )}
      <p className="muted">记录仅保存在本机。采集与低空间提示不会自动处理文件。</p>
    </section>
  )
}
