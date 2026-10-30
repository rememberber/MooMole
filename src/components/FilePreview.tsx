import { useEffect, useRef, useState } from 'react'
import { Modal } from './Modal'
import {
  previewFile,
  readFilePreview,
  releaseMediaPreview,
  type PreviewContent,
  type PreviewTarget,
  type PreviewTransport,
  type FilePreviewReport,
} from '../lib/file-preview'
import { useNow } from '../lib/use-now'
function MediaPreview({
  content: c,
  onExpired,
}: {
  content: Extract<PreviewContent, { kind: 'media' }>
  onExpired: () => void
}) {
  const media = useRef<HTMLMediaElement | null>(null)
  const [playing, setPlaying] = useState(false),
    [ready, setReady] = useState(false),
    [position, setPosition] = useState(0),
    [duration, setDuration] = useState(0),
    [error, setError] = useState('')
  const now = useNow(),
    expired = now >= c.expiresAt
  const time = (n: number) => `${Math.floor(n / 60)}:${String(Math.floor(n % 60)).padStart(2, '0')}`
  useEffect(() => {
    const player = media.current
    return () => {
      if (player) {
        player.pause()
        player.removeAttribute('src')
        player.load()
      }
    }
  }, [])
  useEffect(() => {
    if (expired) {
      media.current?.pause()
      media.current?.removeAttribute('src')
      media.current?.load()
      onExpired()
    }
  }, [expired, onExpired])
  const props = {
    src: c.url,
    preload: 'metadata',
    onLoadedMetadata: () => {
      const n = media.current?.duration ?? 0
      setDuration(Number.isFinite(n) && n > 0 ? n : 0)
      setReady(true)
    },
    onTimeUpdate: () => setPosition(media.current?.currentTime ?? 0),
    onPlay: () => setPlaying(true),
    onPause: () => setPlaying(false),
    onEnded: () => setPlaying(false),
    onError: () =>
      setError(
        '影音无法继续播放，可能是编码未支持、预览失效或文件已变化。请关闭预览后重新扫描查看。',
      ),
  }
  return (
    <section className="file-preview-media" aria-label="影音播放预览">
      {c.mediaKind === 'video' ? (
        <video
          {...props}
          ref={(node) => {
            media.current = node
          }}
          playsInline
          aria-label="视频内容预览"
        />
      ) : (
        <audio
          {...props}
          ref={(node) => {
            media.current = node
          }}
          aria-label="音频内容预览"
        />
      )}
      {!ready && !error && !expired && <p role="status">正在读取影音信息…</p>}
      <div className="file-preview-pages">
        <button
          className="button secondary compact"
          disabled={!ready || !!error || expired}
          onClick={() => {
            const p = media.current
            if (!p) return
            if (p.paused)
              void p.play().catch(() => setError('无法开始播放，请关闭预览后重新查看。'))
            else p.pause()
          }}
        >
          {playing ? '暂停' : '播放'}
        </button>
        <input
          type="range"
          aria-label="播放位置"
          min="0"
          max={duration || 1}
          step="0.1"
          value={Math.min(position, duration || 1)}
          disabled={!ready || !duration || !!error || expired}
          onChange={(e) => {
            const p = media.current
            if (p) {
              p.currentTime = Number(e.target.value)
              setPosition(p.currentTime)
            }
          }}
        />
        <span data-i18n="off">
          {time(position)} / {duration ? time(duration) : '—'}
        </span>
      </div>
      {expired ? (
        <p className="notice warning" role="alert">
          影音预览已过期，请关闭后重新扫描查看。
        </p>
      ) : (
        error && (
          <p className="notice warning" role="alert">
            {error}
          </p>
        )
      )}
      <p className="muted">
        默认不自动播放。按需读取本地文件片段，关闭预览后停止；实际可播放格式取决于系统编码支持。
      </p>
    </section>
  )
}
export function FilePreview({
  target,
  onClose,
  transport = previewFile,
  release = releaseMediaPreview,
}: {
  target: PreviewTarget | null
  onClose: () => void
  transport?: PreviewTransport
  release?: (leaseId: string) => Promise<void>
}) {
  return (
    target && (
      <Preview
        key={`${target.scanId}:${target.itemId}`}
        target={target}
        onClose={onClose}
        transport={transport}
        release={release}
      />
    )
  )
}
function Preview({
  target,
  onClose,
  transport,
  release,
}: {
  target: PreviewTarget
  onClose: () => void
  transport: PreviewTransport
  release: (leaseId: string) => Promise<void>
}) {
  const [page, setPage] = useState(1),
    [report, setReport] = useState<FilePreviewReport | null>(null)
  const [pending, setPending] = useState(true),
    [error, setError] = useState('')
  useEffect(() => {
    let live = true
    let lease: string | null = null
    setPending(true)
    setError('')
    setReport(null)
    void transport(target, page)
      .then((raw) => {
        const r = readFilePreview(raw, target, page)
        if (r.content.kind === 'media') lease = r.content.leaseId
        if (live) setReport(r)
        else if (lease) void release(lease).catch(() => {})
      })
      .catch((e) => {
        if (live) setError(String(e).replace(/^Error: /, ''))
      })
      .finally(() => {
        if (live) setPending(false)
      })
    return () => {
      live = false
      if (lease) void release(lease).catch(() => {})
    }
  }, [target, page, transport, release])
  const c = report?.content
  return (
    <Modal title="文件内容预览" onClose={onClose} className="file-preview-modal">
      <h2>文件内容预览</h2>
      <p className="mono full-path" data-i18n="off">
        {target.path}
      </p>
      {pending && <p role="status">正在读取预览…</p>}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {c?.kind === 'media' ? (
        <MediaPreview content={c} onExpired={() => void release(c.leaseId).catch(() => {})} />
      ) : c?.kind === 'text' ? (
        <>
          <pre className="file-preview-text" data-i18n="off">
            {c.text}
          </pre>
          {c.truncated && <p className="notice">仅显示前 64 KiB 文本，后续内容尚未显示。</p>}
        </>
      ) : (
        c && (
          <figure className="file-preview-image">
            <img src={c.dataUrl} alt={target.path.split('/').at(-1) ?? ''} data-i18n="off" />
            {c.kind === 'image' && (
              <figcaption>
                {c.width} × {c.height} 像素{c.frames > 1 ? ' · 多帧图片，仅显示第一帧' : ''}
              </figcaption>
            )}
          </figure>
        )
      )}
      {c?.kind === 'pdf' && (
        <div className="file-preview-pages">
          <button
            className="button secondary compact"
            disabled={pending || page <= 1}
            onClick={() => setPage((p) => p - 1)}
          >
            上一页
          </button>
          <span>
            第 {c.page} / {c.pages} 页
          </span>
          <button
            className="button secondary compact"
            disabled={pending || page >= c.pages}
            onClick={() => setPage((p) => p + 1)}
          >
            下一页
          </button>
        </div>
      )}
      <p className="muted">
        预览只读取当前扫描中的文件，不改变选择或授权处理。图片和 PDF
        显示缩略内容，影音按需读取本地片段；实际文件请按需在对应应用中检查。
      </p>
      <div className="modal-actions">
        <button className="button secondary" onClick={onClose}>
          返回文件列表
        </button>
      </div>
    </Modal>
  )
}
