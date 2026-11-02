import { useEffect, useRef, useState } from 'react'
import { Modal } from './Modal'
import {
  previewFile,
  rawPhoto,
  cancelRawPreview,
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
    let inFlight = false
    const requestId = rawPhoto(target.path) ? crypto.randomUUID() : undefined
    setPending(true)
    setError('')
    setReport(null)
    void Promise.resolve()
      .then(() => {
        if (!live) return undefined
        inFlight = true
        return transport(target, page, requestId)
      })
      .then((raw) => {
        if (!inFlight) return
        const r = readFilePreview(raw, target, page)
        if (r.content.kind === 'media') lease = r.content.leaseId
        if (live) setReport(r)
        else if (lease) void release(lease).catch(() => {})
      })
      .catch((e) => {
        if (live) setError(String(e).replace(/^Error: /, ''))
      })
      .finally(() => {
        inFlight = false
        if (live) setPending(false)
      })
    return () => {
      live = false
      if (inFlight && requestId) void cancelRawPreview(requestId).catch(() => {})
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
      {pending && (
        <p role="status">
          {rawPhoto(target.path)
            ? '正在读取 RAW 系统预览；关闭窗口会停止本次任务。'
            : '正在读取预览…'}
        </p>
      )}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {c?.kind === 'rawImage' && (
        <p className="notice warning">
          这是系统生成的 RAW 预览，不代表完整动态范围、全部像素细节或编辑后的效果。RAW 原片与 JPEG
          不等价；原片和 XMP 等附属文件不会因预览而改写。
        </p>
      )}
      {c?.kind === 'media' ? (
        <MediaPreview content={c} onExpired={() => void release(c.leaseId).catch(() => {})} />
      ) : c?.kind === 'document' ? (
        <section className="file-preview-office" aria-label="办公文件文本摘录">
          <h3>
            {c.format === 'docx'
              ? 'Word 正文摘录'
              : c.format === 'xlsx'
                ? 'Excel 工作表摘录'
                : 'PowerPoint 幻灯片摘录'}
          </h3>
          <p className="muted">
            仅展示保存的文字或单元格值，不还原版式、图片、图表和完整附加内容；部分隐藏内容可能出现。请在对应应用中核对后再决定处理。
          </p>
          {c.format === 'xlsx' && (
            <p className="notice">
              单元格显示原始值或保存的公式缓存，未应用数字和日期格式，也未重新计算公式。
            </p>
          )}
          {c.sections.length > 1 && (
            <div className="file-preview-pages">
              <button
                className="button secondary compact"
                disabled={pending || page <= 1}
                onClick={() => setPage((p) => p - 1)}
              >
                上一部分
              </button>
              <select
                aria-label="选择办公文件部分"
                value={c.part}
                disabled={pending}
                onChange={(e) => setPage(Number(e.target.value))}
              >
                {c.sections.map((section, index) => (
                  <option key={index} value={index + 1} data-i18n="off">
                    {c.format === 'xlsx' ? section.title : String(index + 1)}
                  </option>
                ))}
              </select>
              <span>
                第 {c.part} / {c.sections.length} 部分
              </span>
              <button
                className="button secondary compact"
                disabled={pending || page >= c.sections.length}
                onClick={() => setPage((p) => p + 1)}
              >
                下一部分
              </button>
            </div>
          )}
          {c.sections[c.part - 1].hidden && <span className="tag">隐藏部分</span>}
          {c.text.trim() ? (
            <pre className="file-preview-text" data-i18n="off">
              {c.text}
            </pre>
          ) : (
            <p className="notice">未提取到可显示的文字；这不表示文件或所选部分为空。</p>
          )}
          {c.truncated && (
            <p className="notice">
              摘录已截断：本部分最多显示 64 KiB 文字或 5,000 个单元格，其余内容尚未显示。
            </p>
          )}
        </section>
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
            {(c.kind === 'image' || c.kind === 'rawImage') && (
              <figcaption>
                {c.width} × {c.height} 像素
                {c.kind === 'rawImage'
                  ? ' · RAW 系统预览'
                  : c.frames > 1
                    ? ' · 多帧图片，仅显示第一帧'
                    : ''}
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
