import { getLocale } from '../i18n/locale'
import type { Analysis, LargeFile } from '../types'

export const fileKinds = {
  all: '全部类型',
  video: '视频',
  audio: '音频',
  image: '图片',
  archive: '压缩包与镜像',
  document: '文档',
  other: '其他或无扩展名',
}
export type FileKind = keyof typeof fileKinds

const extensions: Record<Exclude<FileKind, 'all' | 'other'>, string[]> = {
  video: ['mp4', 'mov', 'mkv', 'avi', 'webm', 'm4v'],
  audio: ['mp3', 'm4a', 'wav', 'flac', 'aac', 'aiff', 'ogg'],
  image: ['jpg', 'jpeg', 'png', 'gif', 'heic', 'webp', 'tif', 'tiff', 'psd', 'raw'],
  archive: ['zip', 'gz', 'bz2', 'xz', '7z', 'rar', 'tar', 'dmg', 'iso'],
  document: ['pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'pages', 'numbers', 'key', 'txt'],
}

export function fileKind(file: LargeFile): Exclude<FileKind, 'all'> {
  const name = file.path.slice(file.path.lastIndexOf('/') + 1)
  const dot = name.lastIndexOf('.')
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : ''
  for (const [kind, values] of Object.entries(extensions))
    if (values.includes(ext)) return kind as Exclude<FileKind, 'all' | 'other'>
  return 'other'
}

export function largeFileBytes(file: LargeFile): number | null {
  return Number.isSafeInteger(file.size) && file.size >= 0 ? file.size : null
}

export function largeFileReport(analysis: Analysis) {
  if (!Array.isArray(analysis.large_files))
    return { reported: false, files: [] as LargeFile[], unreadable: 0 }
  const files = analysis.large_files.filter(
    (file) => file && typeof file.name === 'string' && typeof file.path === 'string',
  )
  return { reported: true, files, unreadable: analysis.large_files.length - files.length }
}

// A literal scope check for read-only actions. Native commands still validate
// the current path; this does not establish file identity or mutation rights.
export function largeFileInScope(path: string, root: string): boolean {
  if (!path.startsWith('/') || !root.startsWith('/') || path.includes('\0')) return false
  if ([...path.split('/'), ...root.split('/')].some((part) => part === '.' || part === '..'))
    return false
  const base = root.replace(/\/+$/, '')
  return path.startsWith(`${base}/`) && path !== `${base}/` && !path.endsWith('/')
}

export function filterLargeFiles(
  files: LargeFile[],
  query: string,
  kind: FileKind,
  minimum: number,
  sort: 'size' | 'name',
) {
  const search = query.trim().toLocaleLowerCase(getLocale())
  return files
    .filter(
      (file) =>
        `${file.name}\n${file.path}`.toLocaleLowerCase(getLocale()).includes(search) &&
        (kind === 'all' || fileKind(file) === kind) &&
        (minimum === 0 || (largeFileBytes(file) ?? -1) >= minimum),
    )
    .sort((a, b) => {
      if (sort === 'size') {
        const left = largeFileBytes(a),
          right = largeFileBytes(b)
        if (left == null && right != null) return 1
        if (right == null && left != null) return -1
        if (left != null && right != null && left !== right) return right - left
      }
      return a.name.localeCompare(b.name, getLocale()) || a.path.localeCompare(b.path, getLocale())
    })
}
