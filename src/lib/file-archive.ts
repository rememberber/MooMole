import { invoke } from '@tauri-apps/api/core'
import type { PersonalFile } from './personal-files'
export interface FileArchiveTarget {
  scanId: string
  root: string
  items: PersonalFile[]
}
export interface FileArchiveDraft {
  id: string
  scanId: string
  expiresAt: number
  outputPath: string
  items: PersonalFile[]
  names: string[]
}
export interface FileArchiveRecord {
  id: string
  requestId: string
  createdAt: number
  finishedAt: number | null
  state: 'running' | 'completed' | 'cancelled' | 'failed' | 'uncertain' | 'interrupted'
  phase: string
  outputPath: string
  temporaryPath: string
  reason: string
  reviewedAt: number | null
  entries: {
    id: string
    path: string
    name: string
    size: number
    state: string
    sha256: string | null
  }[]
  artifact: { device: number; inode: number; size: number; sha256: string } | null
}
const identifier = (s: unknown): s is string =>
  typeof s === 'string' && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(s)
const path = (s: unknown): s is string =>
  typeof s === 'string' &&
  s.startsWith('/') &&
  s.length <= 4096 &&
  !/[\x00-\x1f\x7f]/.test(s) &&
  !s.split('/').some((p) => p === '.' || p === '..')
const hash = (s: unknown): s is string => typeof s === 'string' && /^[0-9a-f]{64}$/.test(s)
const number = (n: unknown): n is number => Number.isSafeInteger(n) && (n as number) >= 0
const fail = (): never => {
  throw Error('文件归档数据无法核对，请在历史检查实际状态；不会重复归档。')
}
export function readFileArchive(raw: unknown): FileArchiveRecord {
  const r = raw as FileArchiveRecord
  if (
    !r ||
    !identifier(r.id) ||
    !identifier(r.requestId) ||
    !path(r.outputPath) ||
    !path(r.temporaryPath) ||
    !number(r.createdAt) ||
    (r.finishedAt !== null && !number(r.finishedAt)) ||
    (r.reviewedAt !== null && !number(r.reviewedAt)) ||
    !['running', 'completed', 'cancelled', 'failed', 'uncertain', 'interrupted'].includes(
      r.state,
    ) ||
    !['building', 'publishing', 'finished'].includes(r.phase) ||
    typeof r.reason !== 'string' ||
    !Array.isArray(r.entries) ||
    r.entries.length > 20 ||
    new Set(r.entries.map((e) => e.id)).size !== r.entries.length ||
    r.entries.some(
      (e) =>
        !identifier(e.id) ||
        !path(e.path) ||
        !number(e.size) ||
        typeof e.name !== 'string' ||
        e.name.startsWith('/') ||
        e.name.split('/').some((p) => !p || p === '.' || p === '..') ||
        !['pending', 'verified', 'archived', 'notArchived', 'uncertain'].includes(e.state) ||
        (e.sha256 !== null && !hash(e.sha256)),
    ) ||
    (r.artifact !== null &&
      (!number(r.artifact.device) ||
        !number(r.artifact.inode) ||
        !number(r.artifact.size) ||
        !hash(r.artifact.sha256))) ||
    (r.state === 'completed' &&
      (!r.artifact ||
        !r.entries.length ||
        r.entries.some((e) => e.state !== 'archived' || !e.sha256)))
  )
    fail()
  return r
}
export const fileArchiveTransport = {
  async prepare(
    target: FileArchiveTarget,
    directory: string,
    filename: string,
  ): Promise<FileArchiveDraft> {
    const d = await invoke<FileArchiveDraft>('prepare_personal_archive', {
      scanId: target.scanId,
      selectedIds: target.items.map((i) => i.id),
      directory,
      filename,
    })
    if (
      !d ||
      !identifier(d.id) ||
      d.scanId !== target.scanId ||
      d.outputPath !== `${directory}/${filename}` ||
      !number(d.expiresAt) ||
      d.expiresAt <= Date.now() ||
      d.expiresAt > Date.now() + 30_000 ||
      !Array.isArray(d.items) ||
      d.items.length !== target.items.length ||
      new Set(d.items.map((i) => i.id)).size !== d.items.length ||
      !Array.isArray(d.names) ||
      d.names.length !== d.items.length ||
      d.items.some(
        (i, index) =>
          !target.items.some(
            (s) =>
              s.id === i.id &&
              s.path === i.path &&
              s.size === i.size &&
              s.modifiedAt === i.modifiedAt,
          ) || d.names[index] !== i.path.slice(target.root.length + 1),
      )
    )
      fail()
    return d
  },
  discard: (id: string) => invoke('discard_personal_archive', { id }),
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
  async execute(d: FileArchiveDraft, requestId: string): Promise<FileArchiveRecord> {
    const r = readFileArchive(
      await invoke('execute_personal_archive', { id: d.id, requestId, confirmed: true }),
    )
    if (
      r.id !== d.id ||
      r.requestId !== requestId ||
      r.outputPath !== d.outputPath ||
      r.entries.length !== d.items.length ||
      r.entries.some(
        (e) => !d.items.some((i) => i.id === e.id && i.path === e.path && i.size === e.size),
      )
    )
      fail()
    return r
  },
  async list(): Promise<FileArchiveRecord[]> {
    const rows = await invoke<unknown[]>('get_personal_archives')
    if (!Array.isArray(rows) || rows.length > 64) fail()
    return rows.map(readFileArchive)
  },
  async acknowledge(id: string): Promise<FileArchiveRecord> {
    const r = readFileArchive(await invoke('acknowledge_personal_archive', { id, confirmed: true }))
    if (r.id !== id || !r.reviewedAt) fail()
    return r
  },
}
export const fileArchiveStates: Record<FileArchiveRecord['state'], string> = {
  running: '正在生成归档',
  completed: '归档已生成并校验',
  cancelled: '归档已取消',
  failed: '归档未发布',
  uncertain: '归档结果待核实',
  interrupted: '归档曾中断',
}
