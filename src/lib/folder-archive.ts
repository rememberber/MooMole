import { invoke } from '@tauri-apps/api/core'
export interface FolderArchiveDraft {
  id: string
  root: string
  outputPath: string
  files: number
  directories: number
  hidden: number
  bytes: number
  metadataBytes: number
  sourceSha256: string
  samples: { name: string; directory: boolean }[]
  createdAt: number
  expiresAt: number
}
export interface FolderArchiveRecord {
  id: string
  requestId: string
  draft: FolderArchiveDraft
  temporaryPath: string
  createdAt: number
  finishedAt: number | null
  state: 'running' | 'completed' | 'retained' | 'cancelled' | 'unknown' | 'interrupted'
  phase: 'building' | 'publishing' | 'finished'
  artifact: { device: string; inode: string; bytes: number; sha256: string } | null
  reason: string
  reviewedAt: number | null
}
export interface FolderArchiveView {
  records: FolderArchiveRecord[]
  activeRequests: string[]
}
function fail(): never {
  throw Error('文件夹归档数据无法核对，请检查归档记录与实际位置；不会自动重试。')
}
function object(v: unknown): Record<string, unknown> {
  return v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : fail()
}
function text(v: unknown): string {
  return typeof v === 'string' &&
    v.length > 0 &&
    v.length <= 4096 &&
    !/[\u0000-\u001f\u007f]/.test(v)
    ? v
    : fail()
}
function number(v: unknown): number {
  return typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? v : fail()
}
function id(v: unknown): string {
  const s = text(v)
  return /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(s) ? s : fail()
}
function hash(v: unknown): string {
  const s = text(v)
  return /^[0-9a-f]{64}$/.test(s) ? s : fail()
}
function path(v: unknown): string {
  const s = text(v)
  return s.startsWith('/') && s !== '/' && !s.split('/').some((p) => p === '.' || p === '..')
    ? s
    : fail()
}
export function readFolderArchiveDraft(
  v: unknown,
  requested?: { root: string; directory: string; filename: string },
): FolderArchiveDraft {
  const a = object(v),
    d = {
      id: id(a.id),
      root: path(a.root),
      outputPath: path(a.outputPath),
      files: number(a.files),
      directories: number(a.directories),
      hidden: number(a.hidden),
      bytes: number(a.bytes),
      metadataBytes: number(a.metadataBytes),
      sourceSha256: hash(a.sourceSha256),
      samples: [] as FolderArchiveDraft['samples'],
      createdAt: number(a.createdAt),
      expiresAt: number(a.expiresAt),
    }
  const rootName = d.root.slice(d.root.lastIndexOf('/') + 1),
    parent = d.outputPath.slice(0, d.outputPath.lastIndexOf('/'))
  if (
    !d.outputPath.endsWith('.zip') ||
    parent === d.root ||
    parent.startsWith(d.root + '/') ||
    d.files + d.directories > 10000 ||
    d.directories === 0 ||
    d.hidden > d.files + d.directories ||
    d.bytes + d.metadataBytes > 8 * 1024 ** 3 ||
    d.metadataBytes > 256 * 1024 ** 2 ||
    d.expiresAt - d.createdAt !== 120000 ||
    !Array.isArray(a.samples) ||
    a.samples.length > 50 ||
    a.samples.length > d.files + d.directories ||
    (requested &&
      (d.root !== requested.root ||
        d.outputPath !== `${requested.directory}/${requested.filename}`))
  )
    fail()
  d.samples = (a.samples as unknown[]).map((v) => {
    const s = object(v),
      name = text(s.name)
    if (
      typeof s.directory !== 'boolean' ||
      name.split('/')[0] !== rootName ||
      name.split('/').some((p) => !p || p === '.' || p === '..') ||
      /[\\:]/.test(name)
    )
      fail()
    return { name, directory: s.directory as boolean }
  })
  if (new Set(d.samples.map((s) => s.name)).size !== d.samples.length) fail()
  return d
}
export function readFolderArchiveRecord(v: unknown): FolderArchiveRecord {
  const a = object(v),
    draft = readFolderArchiveDraft(a.draft)
  const r = {
    id: id(a.id),
    requestId: id(a.requestId),
    draft,
    temporaryPath: path(a.temporaryPath),
    createdAt: number(a.createdAt),
    finishedAt: a.finishedAt === null ? null : number(a.finishedAt),
    state: text(a.state),
    phase: text(a.phase),
    artifact: a.artifact === null ? null : object(a.artifact),
    reason: text(a.reason),
    reviewedAt: a.reviewedAt === null ? null : number(a.reviewedAt),
  }
  const parent = draft.outputPath.slice(0, draft.outputPath.lastIndexOf('/'))
  if (
    r.id !== draft.id ||
    r.temporaryPath !== `${parent}/.MooMole-folder-archive-${r.id}.partial` ||
    r.createdAt < draft.createdAt ||
    (r.finishedAt !== null && r.finishedAt < r.createdAt) ||
    (r.reviewedAt !== null && r.reviewedAt < r.createdAt) ||
    !['running', 'completed', 'retained', 'cancelled', 'unknown', 'interrupted'].includes(
      r.state,
    ) ||
    !['building', 'publishing', 'finished'].includes(r.phase) ||
    r.reason.length > 4000 ||
    (r.state === 'completed' && (!r.artifact || r.phase !== 'finished' || r.finishedAt === null))
  )
    fail()
  if (r.artifact) {
    for (const key of ['device', 'inode']) {
      const s = text(r.artifact[key])
      if (!/^[1-9]\d*$/.test(s) || BigInt(s) > 2n ** 64n - 1n) fail()
    }
    const size = number(r.artifact.bytes)
    if (!size || size > 10 * 1024 ** 3) fail()
    hash(r.artifact.sha256)
  }
  return r as FolderArchiveRecord
}
export function readFolderArchiveView(v: unknown): FolderArchiveView {
  const a = object(v)
  if (
    !Array.isArray(a.records) ||
    a.records.length > 64 ||
    !Array.isArray(a.activeRequests) ||
    a.activeRequests.length > 3
  )
    fail()
  const records = (a.records as unknown[]).map(readFolderArchiveRecord),
    activeRequests = (a.activeRequests as unknown[]).map(id)
  if (
    new Set(records.map((r) => r.id)).size !== records.length ||
    new Set(records.map((r) => r.requestId)).size !== records.length ||
    new Set(activeRequests).size !== activeRequests.length ||
    activeRequests.some((id) => !records.some((r) => r.requestId === id && r.state === 'running'))
  )
    fail()
  return { records, activeRequests }
}
export const folderArchive = {
  prepare: async (root: string, directory: string, filename: string, requestId: string) =>
    readFolderArchiveDraft(
      await invoke('prepare_folder_archive', { root, directory, filename, requestId }),
      { root, directory, filename },
    ),
  discard: (id: string) => invoke('discard_folder_archive', { id }),
  execute: async (draft: FolderArchiveDraft, requestId: string) => {
    const r = readFolderArchiveRecord(
      await invoke('execute_folder_archive', { id: draft.id, requestId, confirmed: true }),
    )
    if (
      r.id !== draft.id ||
      r.requestId !== requestId ||
      JSON.stringify(r.draft) !== JSON.stringify(draft)
    )
      fail()
    return r
  },
  read: async () => readFolderArchiveView(await invoke('get_folder_archives')),
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
  acknowledge: (id: string) => invoke('acknowledge_folder_archive', { id, confirmed: true }),
  reveal: (path: string) => invoke('reveal_path', { path }),
}
export const folderArchiveState: Record<FolderArchiveRecord['state'], string> = {
  running: '正在创建文件夹归档',
  completed: '整份文件夹已归档，原件保留',
  retained: '未发布整份归档，原件保留',
  cancelled: '文件夹归档已取消',
  unknown: '文件夹归档结果待核实',
  interrupted: '文件夹归档已中断',
}
