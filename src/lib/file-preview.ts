import { invoke } from '@tauri-apps/api/core'
export interface PreviewTarget {
  domain: 'personalFiles' | 'duplicateFiles' | 'similarPhotos' | 'softwareContent'
  scanId: string
  itemId: string
  path: string
}
export type PreviewContent =
  | { kind: 'image'; dataUrl: string; width: number; height: number; frames: number }
  | { kind: 'pdf'; dataUrl: string; page: number; pages: number }
  | { kind: 'text'; text: string; truncated: boolean }
  | {
      kind: 'media'
      url: string
      leaseId: string
      mime: 'audio/wav' | 'audio/mpeg' | 'audio/mp4' | 'video/mp4' | 'video/quicktime'
      mediaKind: 'audio' | 'video'
      expiresAt: number
    }
export interface FilePreviewReport {
  scanId: string
  itemId: string
  path: string
  content: PreviewContent
}
export type PreviewTransport = (target: PreviewTarget, page: number) => Promise<unknown>
export const previewFile: PreviewTransport = (target, page) =>
  invoke(
    /\.(wav|mp3|m4a|mp4|m4v|mov)$/i.test(target.path)
      ? 'prepare_media_preview'
      : 'preview_personal_file',
    {
      domain: target.domain,
      scanId: target.scanId,
      itemId: target.itemId,
      page,
    },
  )
export const releaseMediaPreview = (leaseId: string) =>
  invoke<void>('release_media_preview', { leaseId })
export function readFilePreview(
  raw: unknown,
  target: PreviewTarget,
  page: number,
): FilePreviewReport {
  const r = raw as FilePreviewReport,
    c = r?.content
  const positive = (n: number, max: number) => Number.isSafeInteger(n) && n > 0 && n <= max
  if (
    !r ||
    r.scanId !== target.scanId ||
    r.itemId !== target.itemId ||
    r.path !== target.path ||
    !c ||
    (c.kind === 'text'
      ? typeof c.text !== 'string' ||
        new TextEncoder().encode(c.text).length > 65536 ||
        typeof c.truncated !== 'boolean'
      : c.kind === 'image' || c.kind === 'pdf'
        ? typeof c.dataUrl !== 'string' ||
          !/^data:image\/png;base64,iVBOR[A-Za-z0-9+/=]+$/.test(c.dataUrl) ||
          c.dataUrl.length > 5_592_432 ||
          (c.kind === 'image'
            ? !positive(c.width, 60000) ||
              !positive(c.height, 60000) ||
              c.width * c.height > 80_000_000 ||
              !positive(c.frames, Number.MAX_SAFE_INTEGER)
            : c.page !== page || !positive(c.pages, 10000) || !positive(c.page, c.pages))
        : c.kind === 'media'
          ? !/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(c.leaseId) ||
            ![
              `moomole-media://localhost/${c.leaseId}/media`,
              `http://moomole-media.localhost/${c.leaseId}/media`,
            ].includes(c.url) ||
            !['audio', 'video'].includes(c.mediaKind) ||
            (c.mediaKind === 'audio'
              ? !['audio/wav', 'audio/mpeg', 'audio/mp4'].includes(c.mime)
              : !['video/mp4', 'video/quicktime'].includes(c.mime)) ||
            !Number.isSafeInteger(c.expiresAt) ||
            c.expiresAt <= Date.now() ||
            c.expiresAt > Date.now() + 900_000
          : true)
  )
    throw Error('预览结果与所选文件不一致，已停止显示，请重新扫描。')
  return r
}
