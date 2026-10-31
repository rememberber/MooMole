import { useEffect, useRef, useState } from 'react'
import { Modal } from './Modal'
import { bytes } from '../lib/format'
import { useNow } from '../lib/use-now'
import {
  videoComparison,
  readVideoComparison,
  type VideoTarget,
  type VideoReport,
  type VideoComparisonTransport,
} from '../lib/video-comparison'

export function VideoComparison({
  targets,
  onClose,
  onChoose,
  transport = videoComparison,
}: {
  targets: VideoTarget[] | null
  onClose: () => void
  onChoose: (ids: string[]) => string | void
  transport?: VideoComparisonTransport
}) {
  return (
    targets && (
      <Comparison
        key={targets.map((t) => `${t.scanId}:${t.itemId}`).join(':')}
        targets={targets}
        onClose={onClose}
        onChoose={onChoose}
        transport={transport}
      />
    )
  )
}
function Comparison({
  targets,
  onClose,
  onChoose,
  transport,
}: {
  targets: VideoTarget[]
  onClose: () => void
  onChoose: (ids: string[]) => string | void
  transport: VideoComparisonTransport
}) {
  const [id] = useState(() => crypto.randomUUID()),
    [reports, setReports] = useState<VideoReport[] | null>(null),
    [error, setError] = useState(''),
    [ready, setReady] = useState<
      Record<string, { duration: number; width: number; height: number }>
    >({}),
    [chosen, setChosen] = useState<string[]>([]),
    [audio, setAudio] = useState<string | null>(null),
    [position, setPosition] = useState(0),
    [starting, setStarting] = useState(false),
    [selectionError, setSelectionError] = useState('')
  const players = useRef<HTMLVideoElement[]>([]),
    invalid = useRef(false),
    live = useRef(true),
    session = useRef<string | null>(null),
    deadline = useRef(Infinity)
  const now = useNow(),
    expired =
      !!reports &&
      (reports.some((r) => now >= r.content.expiresAt) || performance.now() >= deadline.current)
  const loaded =
      !!reports &&
      reports.length === targets.length &&
      targets.every((t) => ready[t.itemId]?.duration > 0),
    usable = loaded && !error && !expired
  const maximum = loaded ? Math.min(...targets.map((t) => ready[t.itemId].duration)) : 1
  const time = (n: number) => `${Math.floor(n / 60)}:${String(Math.floor(n % 60)).padStart(2, '0')}`
  function pause() {
    for (const p of players.current) p?.pause()
  }
  function detach() {
    for (const p of players.current) {
      if (p) {
        p.pause()
        p.removeAttribute('src')
        p.load()
      }
    }
  }
  function invalidate(reason: string) {
    if (invalid.current) return
    invalid.current = true
    setError(reason)
    detach()
    if (session.current) void transport.release(session.current).catch(() => {})
  }
  useEffect(() => {
    live.current = true
    invalid.current = false
    let active = true
    const request = crypto.randomUUID()
    session.current = request
    void transport
      .prepare(request, targets)
      .then((raw) => {
        if (!active) {
          void transport.release(request).catch(() => {})
          return
        }
        const next = readVideoComparison(raw, request, targets)
        deadline.current =
          performance.now() +
          Math.max(0, Math.min(...next.map((r) => r.content.expiresAt - Date.now())))
        setReports(next)
      })
      .catch((e) => {
        if (active) invalidate(String(e).replace(/^Error: /, ''))
      })
    return () => {
      active = false
      live.current = false
      invalid.current = true
      detach()
      players.current = []
      void transport.release(request).catch(() => {})
    }
  }, [id, targets, transport])
  useEffect(() => {
    if (expired) invalidate('视频对比已过期，请关闭后重新扫描。')
  }, [expired])
  async function playTogether() {
    if (!usable || starting) return
    setStarting(true)
    const pending = players.current.map((p) => {
      p.currentTime = Math.min(position, p.duration)
      return p.play()
    })
    const results = await Promise.allSettled(pending)
    if (!live.current || invalid.current) {
      pause()
      return
    }
    setStarting(false)
    if (results.some((r) => r.status === 'rejected'))
      invalidate('部分视频无法播放，已停止全部对比；请核对系统编码支持或重新扫描。')
  }
  return (
    <Modal title="视频并排对比" className="video-comparison-modal" onClose={onClose}>
      <h2>视频并排对比</h2>
      <p>
        逐段核对画面、声音和路径，再选择需要处理的文件。默认不选、不自动播放，也不认定这些视频相同或相似。
      </p>
      {!reports && !error && <p role="status">正在准备所选视频…</p>}
      {reports && (
        <>
          <div className="video-comparison-controls">
            <button
              className="button secondary compact"
              disabled={!usable || starting}
              onClick={() => void playTogether()}
            >
              一起播放
            </button>
            <button
              className="button secondary compact"
              disabled={!loaded || !!error || expired}
              onClick={pause}
            >
              全部暂停
            </button>
            <label>
              共同定位
              <input
                type="range"
                aria-label="视频共同定位"
                min={0}
                max={maximum}
                step={0.05}
                value={Math.min(position, maximum)}
                disabled={!usable}
                onChange={(e) => {
                  pause()
                  const value = Number(e.target.value)
                  for (const p of players.current) p.currentTime = value
                  setPosition(value)
                }}
              />
            </label>
            <span data-i18n="off">
              {time(position)} / {loaded ? time(maximum) : '—'}
            </span>
            <button className="button text compact" onClick={() => setAudio(null)}>
              全部静音
            </button>
          </div>
          <p className="muted">
            共同定位以最短视频的时长为限；各段也可单独播放和定位。一起播放不保证逐帧同步，声音默认静音，每次只听一段。
          </p>
          <div className="video-comparison-grid">
            {reports.map((r, index) => {
              const target = targets[index],
                info = ready[r.itemId]
              return (
                <section className="video-comparison-card" key={r.itemId} aria-label={target.name}>
                  <video
                    src={r.content.url}
                    ref={(node) => {
                      if (node) players.current[index] = node
                    }}
                    controls={!error && !expired}
                    muted={audio !== r.itemId}
                    preload="metadata"
                    playsInline
                    aria-label={`对比视频 ${target.name}`}
                    onLoadedMetadata={() => {
                      const p = players.current[index]
                      if (
                        !p ||
                        !Number.isFinite(p.duration) ||
                        p.duration <= 0 ||
                        p.duration > Number.MAX_SAFE_INTEGER ||
                        p.videoWidth <= 0 ||
                        p.videoHeight <= 0
                      ) {
                        invalidate('视频画面或时长无法核对，已停止对比。')
                        return
                      }
                      // Request a first frame without autoplay, so comparison
                      // starts with actual pictures rather than blank players.
                      p.currentTime = Math.min(0.001, p.duration / 2)
                      setReady((old) => ({
                        ...old,
                        [r.itemId]: {
                          duration: p.duration,
                          width: p.videoWidth,
                          height: p.videoHeight,
                        },
                      }))
                    }}
                    onError={() =>
                      invalidate('部分视频无法播放，已停止全部对比；请核对系统编码支持或重新扫描。')
                    }
                  />
                  <strong data-i18n="off">{target.name}</strong>
                  <p className="mono full-path">{target.path}</p>
                  <p data-i18n="off">
                    {bytes(target.size)}
                    {info ? ` · ${info.width} × ${info.height} · ${time(info.duration)}` : ''}
                  </p>
                  <div className="video-comparison-options">
                    <label>
                      <input
                        type="checkbox"
                        aria-label={`待处理 ${target.path}`}
                        checked={chosen.includes(r.itemId)}
                        disabled={!usable}
                        onChange={(e) => {
                          setSelectionError('')
                          setChosen((old) =>
                            e.target.checked
                              ? [...old, r.itemId]
                              : old.filter((id) => id !== r.itemId),
                          )
                        }}
                      />
                      带回列表后处理
                    </label>
                    <label>
                      <input
                        type="radio"
                        name={`audio-${id}`}
                        aria-label={`听取 ${target.name}`}
                        disabled={!usable}
                        checked={audio === r.itemId}
                        onChange={() => setAudio(r.itemId)}
                      />
                      听此段声音
                    </label>
                  </div>
                </section>
              )
            })}
          </div>
        </>
      )}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {selectionError && (
        <p className="notice warning" role="alert">
          {selectionError}
        </p>
      )}
      <p className="muted">
        仅将本次对比的选择带回文件列表，其他文件的勾选保持。移动、归档或废纸篓处理仍需另行确认；对比不是内容相同的证明，也不是处理许可。
      </p>
      <div className="modal-actions">
        <button className="button secondary" onClick={onClose}>
          取消对比
        </button>
        <button
          className="button primary"
          disabled={!usable}
          onClick={() => {
            pause()
            const reason = onChoose(chosen)
            if (reason) {
              setSelectionError(reason)
              return
            }
            onClose()
          }}
        >
          将选择带回文件列表
        </button>
      </div>
    </Modal>
  )
}
