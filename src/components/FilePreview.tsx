import { useEffect, useState } from 'react'
import { Modal } from './Modal'
import {
  previewFile,
  readFilePreview,
  type PreviewTarget,
  type PreviewTransport,
  type FilePreviewReport,
} from '../lib/file-preview'
export function FilePreview({
  target,
  onClose,
  transport = previewFile,
}: {
  target: PreviewTarget | null
  onClose: () => void
  transport?: PreviewTransport
}) {
  return (
    target && (
      <Preview
        key={`${target.scanId}:${target.itemId}`}
        target={target}
        onClose={onClose}
        transport={transport}
      />
    )
  )
}
function Preview({
  target,
  onClose,
  transport,
}: {
  target: PreviewTarget
  onClose: () => void
  transport: PreviewTransport
}) {
  const [page, setPage] = useState(1),
    [report, setReport] = useState<FilePreviewReport | null>(null)
  const [pending, setPending] = useState(true),
    [error, setError] = useState('')
  useEffect(() => {
    let live = true
    setPending(true)
    setError('')
    setReport(null)
    void transport(target, page)
      .then((raw) => {
        const r = readFilePreview(raw, target, page)
        if (live) setReport(r)
      })
      .catch((e) => {
        if (live) setError(String(e).replace(/^Error: /, ''))
      })
      .finally(() => {
        if (live) setPending(false)
      })
    return () => {
      live = false
    }
  }, [target, page, transport])
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
      {c?.kind === 'text' ? (
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
        显示缩略内容，实际文件请按需在对应应用中检查。
      </p>
      <div className="modal-actions">
        <button className="button secondary" onClick={onClose}>
          返回文件列表
        </button>
      </div>
    </Modal>
  )
}
