import { invoke } from '@tauri-apps/api/core'
export interface ZipExtractionDraft {
  format?: 'zip' | 'tar' | 'tarGzip'
  macMetadata?: {
    items: number
    attributes: number
    executable: number
    finderHidden: number
    skippedQuarantine: number
  }
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
export interface ZipExtractionRecord {
  id: string
  requestId: string
  draft: ZipExtractionDraft
  temporaryPath: string
  createdAt: number
  finishedAt: number | null
  state: 'running' | 'completed' | 'retained' | 'cancelled' | 'unknown' | 'interrupted'
  phase: 'building' | 'publishing' | 'finished'
  artifact: { device: string; inode: string; bytes: number; sha256: string } | null
  reason: string
  reviewedAt: number | null
}
export interface ZipExtractionView {
  records: ZipExtractionRecord[]
  activeRequests: string[]
}
export function extractionFormat(path: string): 'zip' | 'tar' | 'tarGzip' | null {
  const name = path.toLowerCase()
  return name.endsWith('.tar.gz') || name.endsWith('.tgz')
    ? 'tarGzip'
    : name.endsWith('.tar')
      ? 'tar'
      : name.endsWith('.zip')
        ? 'zip'
        : null
}
function fail(): never {
  throw Error('压缩包解压数据无法核对，请检查解压记录与实际位置；不会自动重试。')
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
function readMacMetadata(v: unknown): NonNullable<ZipExtractionDraft['macMetadata']> {
  const a = object(v)
  if (
    Object.keys(a).some(
      (k) =>
        !['items', 'attributes', 'executable', 'finderHidden', 'skippedQuarantine'].includes(k),
    )
  )
    fail()
  const result = {
    items: number(a.items),
    attributes: number(a.attributes),
    executable: number(a.executable),
    finderHidden: number(a.finderHidden),
    skippedQuarantine: number(a.skippedQuarantine),
  }
  if (result.items > 9999 || result.attributes > result.items * 1026) fail()
  return result
}
export function readZipExtractionDraft(
  v: unknown,
  requested?: { root: string; directory: string; filename: string; restoreMetadata?: boolean },
): ZipExtractionDraft {
  const a = object(v),
    d = {
      ...(a.format === undefined ? {} : { format: a.format as 'zip' | 'tar' | 'tarGzip' }),
      ...(a.macMetadata === undefined ? {} : { macMetadata: readMacMetadata(a.macMetadata) }),
      id: id(a.id),
      root: path(a.root),
      outputPath: path(a.outputPath),
      files: number(a.files),
      directories: number(a.directories),
      hidden: number(a.hidden),
      bytes: number(a.bytes),
      metadataBytes: number(a.metadataBytes),
      sourceSha256: hash(a.sourceSha256),
      samples: [] as ZipExtractionDraft['samples'],
      createdAt: number(a.createdAt),
      expiresAt: number(a.expiresAt),
    }
  const parent = d.outputPath.slice(0, d.outputPath.lastIndexOf('/')),
    format = a.format === undefined ? 'zip' : a.format
  if (
    !['zip', 'tar', 'tarGzip'].some((known) => known === format) ||
    extractionFormat(d.root) !== format ||
    (d.macMetadata !== undefined &&
      (format !== 'zip' ||
        d.macMetadata.items !== d.files + d.directories - 1 ||
        d.macMetadata.executable > d.files ||
        d.macMetadata.finderHidden > d.macMetadata.items ||
        d.macMetadata.skippedQuarantine > d.macMetadata.items ||
        d.metadataBytes > 256 * 1024 ** 2)) ||
    parent === d.root ||
    parent.startsWith(d.root + '/') ||
    d.files + d.directories > 10000 ||
    d.directories === 0 ||
    d.hidden > d.files + d.directories ||
    d.bytes > 8 * 1024 ** 3 ||
    d.metadataBytes > d.bytes ||
    d.expiresAt - d.createdAt !== 120000 ||
    !Array.isArray(a.samples) ||
    a.samples.length > 50 ||
    a.samples.length > d.files + d.directories ||
    (requested &&
      (d.root !== requested.root ||
        d.outputPath !== `${requested.directory}/${requested.filename}` ||
        !!d.macMetadata !== !!requested.restoreMetadata))
  )
    fail()
  d.samples = (a.samples as unknown[]).map((v) => {
    const s = object(v),
      name = text(s.name)
    if (
      typeof s.directory !== 'boolean' ||
      name.split('/').some((p) => !p || p === '.' || p === '..') ||
      /[\\:]/.test(name)
    )
      fail()
    return { name, directory: s.directory as boolean }
  })
  if (new Set(d.samples.map((s) => s.name)).size !== d.samples.length) fail()
  return d
}
export function readZipExtractionRecord(v: unknown): ZipExtractionRecord {
  const a = object(v),
    draft = readZipExtractionDraft(a.draft)
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
    r.temporaryPath !== `${parent}/.MooMole-zip-extract-${r.id}.partial` ||
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
    if (size !== draft.bytes) fail()
    hash(r.artifact.sha256)
  }
  return r as ZipExtractionRecord
}
export function readZipExtractionView(v: unknown): ZipExtractionView {
  const a = object(v)
  if (
    !Array.isArray(a.records) ||
    a.records.length > 64 ||
    !Array.isArray(a.activeRequests) ||
    a.activeRequests.length > 3
  )
    fail()
  const records = (a.records as unknown[]).map(readZipExtractionRecord),
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
export const zipExtraction = {
  prepare: async (
    root: string,
    directory: string,
    filename: string,
    requestId: string,
    restoreMetadata = false,
  ) =>
    readZipExtractionDraft(
      await invoke('prepare_zip_extraction', {
        root,
        directory,
        filename,
        requestId,
        ...(restoreMetadata ? { restoreMetadata: true } : {}),
      }),
      { root, directory, filename, restoreMetadata },
    ),
  discard: (id: string) => invoke('discard_zip_extraction', { id }),
  execute: async (draft: ZipExtractionDraft, requestId: string) => {
    const r = readZipExtractionRecord(
      await invoke('execute_zip_extraction', { id: draft.id, requestId, confirmed: true }),
    )
    if (
      r.id !== draft.id ||
      r.requestId !== requestId ||
      JSON.stringify(r.draft) !== JSON.stringify(draft)
    )
      fail()
    return r
  },
  read: async () => readZipExtractionView(await invoke('get_zip_extractions')),
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
  acknowledge: (id: string) => invoke('acknowledge_zip_extraction', { id, confirmed: true }),
  reveal: (path: string) => invoke('reveal_path', { path }),
}
export const zipExtractionState: Record<ZipExtractionRecord['state'], string> = {
  running: '正在解压压缩包',
  completed: '压缩包已解压，原压缩包保留',
  retained: '未发布解压结果，原压缩包保留',
  cancelled: '压缩包解压已取消',
  unknown: '压缩包解压结果待核实',
  interrupted: '压缩包解压已中断',
}
