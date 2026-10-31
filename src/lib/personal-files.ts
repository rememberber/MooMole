import { invoke } from '@tauri-apps/api/core'
import type { ExecutionDetail } from '../types'
import { fileKind, type FileKind } from './large-files'
import { getLocale } from '../i18n/locale'

export interface PersonalFile {
  id: string
  path: string
  name: string
  size: number
  modifiedAt: number
}
export interface PersonalReport {
  id: string
  root: string
  createdAt: number
  expiresAt: number
  complete: boolean
  cancelled: boolean
  skipped: number
  visited: number
  items: PersonalFile[]
  aiFocus?: {
    snapshotId: string
    agentId: string
    analyzedAt: number
    items: {
      evidenceId: string
      itemId: string
      path: string
      recommendation: 'keep' | 'investigate'
      reason: string
      impact: string
    }[]
  }
  duplicates?: {
    groups: { id: string; size: number; itemIds: string[] }[]
    bytesRead: number
    checkedFiles: number
    extraBytes: number
  }
  similarPhotos?: {
    method: 'visionFeaturePrintV2'
    groups: { id: string; itemIds: string[]; captureSpanMs?: number }[]
    images: { id: string; width: number; height: number; thumbnail: string; capturedAt?: string }[]
    checkedFiles: number
    bytesRead: number
    sequence?: { missingMetadata: number }
  }
  softwareContent?: {
    source:
      | 'mailDownloads'
      | 'mailSandboxDownloads'
      | 'chromeDownloads'
      | 'vlcPlaylist'
      | 'telegramExport'
    minimumAgeDays: number
    minimumDirectoryBytes: number
    minimumSizeMet: boolean
    executionSupported: boolean
    playlistPath?: string
    exportPath?: string
  }
}
export interface PersonalSnapshot {
  phase: 'idle' | 'scanning' | 'ready' | 'processing' | 'completed' | 'failed'
  requestId: string | null
  runId: string | null
  report: PersonalReport | null
  error: string | null
}
export interface PersonalConfirmation {
  id: string
  root: string
  createdAt: number
  expiresAt: number
  items: PersonalFile[]
  movement?: PersonalMovement & { targets: { id: string; destination: string | null }[] }
  keptFiles?: PersonalFile[]
}
export interface PersonalMovement {
  directory: string
  conflict: 'skip' | 'keepBoth'
}
export interface PersonalTransport {
  origin?: 'personalFiles' | 'duplicateFiles' | 'similarPhotos' | 'softwareContent'
  read: () => Promise<unknown>
  start: (root: string, requestId: string, photoSequences?: boolean) => Promise<void>
  startFromAi?: (snapshotId: string, evidenceIds: string[], requestId: string) => Promise<string>
  prepare: (scanId: string, selectedIds: string[], movement?: PersonalMovement) => Promise<unknown>
  discard: (confirmationId: string) => Promise<void>
  execute: (confirmationId: string, requestId: string, moving?: boolean) => Promise<void>
  cancel: (requestId: string) => Promise<void>
  detail: (runId: string) => Promise<ExecutionDetail>
}
export const personalTransport: PersonalTransport = {
  read: () => invoke('get_personal_files'),
  start: (root, requestId) => invoke('start_personal_scan', { root, requestId }),
  startFromAi: (snapshotId, evidenceIds, requestId) =>
    invoke('start_personal_from_ai', { snapshotId, evidenceIds, requestId }),
  prepare: (scanId, selectedIds, movement) =>
    movement
      ? invoke('prepare_personal_move', {
          scanId,
          selectedIds,
          destination: movement.directory,
          conflict: movement.conflict,
        })
      : invoke('prepare_personal_trash', { scanId, selectedIds }),
  discard: (confirmationId) => invoke('discard_personal_trash', { confirmationId }),
  execute: (confirmationId, requestId, moving = false) =>
    invoke(moving ? 'execute_personal_move' : 'execute_personal_trash', {
      confirmationId,
      requestId,
      personalFilesConfirmed: true,
    }),
  cancel: (taskId) => invoke('cancel_task', { taskId }),
  detail: (runId) => invoke('get_execution', { runId }),
}
export const duplicateTransport: PersonalTransport = {
  ...personalTransport,
  origin: 'duplicateFiles',
  read: () => invoke('get_duplicate_files'),
  start: (root, requestId) => invoke('start_duplicate_scan', { root, requestId }),
  prepare: (scanId, selectedIds) => invoke('prepare_duplicate_trash', { scanId, selectedIds }),
  discard: (confirmationId) => invoke('discard_duplicate_trash', { confirmationId }),
  execute: (confirmationId, requestId) =>
    invoke('execute_duplicate_trash', { confirmationId, requestId, personalFilesConfirmed: true }),
}
export const similarTransport: PersonalTransport = {
  ...personalTransport,
  origin: 'similarPhotos',
  read: () => invoke('get_similar_photos'),
  start: (root, requestId, photoSequences = false) =>
    invoke('start_similar_scan', { root, requestId, photoSequences }),
  prepare: (scanId, selectedIds) => invoke('prepare_similar_trash', { scanId, selectedIds }),
  discard: (confirmationId) => invoke('discard_similar_trash', { confirmationId }),
  execute: (confirmationId, requestId) =>
    invoke('execute_similar_trash', { confirmationId, requestId, personalFilesConfirmed: true }),
}
const fail = (): never => {
  throw Error('个人文件数据无法核对，请重新读取；不会重复处理。')
}
const object = (v: unknown): Record<string, unknown> =>
  v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : fail()
const number = (v: unknown): number =>
  typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? v : fail()
const id = (v: unknown): string =>
  typeof v === 'string' && /^[a-f\d]{8}(?:-[a-f\d]{4}){3}-[a-f\d]{12}$/.test(v) ? v : fail()
const path = (v: unknown): string =>
  typeof v === 'string' &&
  v.startsWith('/') &&
  v.length <= 4096 &&
  !/[\x00-\x1f\x7f]/.test(v) &&
  !v.split('/').some((part) => part === '.' || part === '..')
    ? v
    : fail()
// Validate raw labels without assuming a local zone. EXIF nanoseconds within
// one day fit safely in JavaScript integers.
function captureTime(raw: unknown) {
  if (typeof raw !== 'string') return fail()
  const m =
    /^(\d{4}):(\d{2}):(\d{2}) (\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,9}))?([+-]\d{2}:\d{2})?$/.exec(raw)
  if (!m) return fail()
  const [year, month, day, hour, minute, second] = m.slice(1, 7).map(Number)
  const date = new Date(Date.UTC(year, month - 1, day))
  if (
    year < 1970 ||
    date.getUTCFullYear() !== year ||
    date.getUTCMonth() !== month - 1 ||
    date.getUTCDate() !== day ||
    hour > 23 ||
    minute > 59 ||
    second > 59
  )
    fail()
  if (m[8]) {
    const h = Number(m[8].slice(1, 3)),
      min = Number(m[8].slice(4))
    if (h > 14 || min > 59 || (h === 14 && min !== 0)) fail()
  }
  return {
    label: raw,
    key: `${raw.slice(0, 10)}|${m[8] ?? ''}`,
    nanos: ((hour * 60 + minute) * 60 + second) * 1e9 + Number((m[7] ?? '').padEnd(9, '0')),
  }
}
function files(raw: unknown, root: string, max: number): PersonalFile[] {
  if (!Array.isArray(raw) || raw.length > max) return fail()
  const rows = raw.map((entry) => {
    const v = object(entry),
      value = path(v.path)
    if (
      !value.startsWith(`${root}/`) ||
      typeof v.name !== 'string' ||
      v.name !== value.split('/').at(-1) ||
      !Number.isSafeInteger(v.modifiedAt) ||
      Math.abs(v.modifiedAt as number) > 8.64e15
    )
      fail()
    return {
      id: id(v.id),
      path: value,
      name: v.name as string,
      size: number(v.size),
      modifiedAt: v.modifiedAt as number,
    }
  })
  if (
    new Set(rows.map((row) => row.id)).size !== rows.length ||
    new Set(rows.map((row) => row.path)).size !== rows.length
  )
    fail()
  return rows
}
export function readPersonalSnapshot(raw: unknown): PersonalSnapshot {
  const v = object(raw)
  if (
    !['idle', 'scanning', 'ready', 'processing', 'completed', 'failed'].includes(
      v.phase as string,
    ) ||
    !(v.error === null || typeof v.error === 'string')
  )
    fail()
  let report: PersonalReport | null = null
  if (v.report !== null) {
    const r = object(v.report),
      root = path(r.root),
      createdAt = number(r.createdAt),
      expiresAt = number(r.expiresAt)
    if (
      expiresAt <= createdAt ||
      expiresAt - createdAt > 900000 ||
      typeof r.complete !== 'boolean' ||
      typeof r.cancelled !== 'boolean'
    )
      fail()
    report = {
      id: id(r.id),
      root,
      createdAt,
      expiresAt,
      complete: r.complete as boolean,
      cancelled: r.cancelled as boolean,
      skipped: number(r.skipped),
      visited: number(r.visited),
      items: files(r.items, root, 2000),
    }
    if (r.aiFocus !== undefined) {
      const a = object(r.aiFocus)
      if (
        !['codex', 'claude'].includes(a.agentId as string) ||
        !Array.isArray(a.items) ||
        !a.items.length ||
        a.items.length > 20 ||
        r.duplicates !== undefined ||
        r.similarPhotos !== undefined ||
        r.softwareContent !== undefined
      )
        fail()
      const seen = new Set<string>()
      const items = (a.items as unknown[]).map((raw) => {
        const f = object(raw),
          item = report!.items.find((i) => i.id === f.itemId)
        if (
          !item ||
          f.path !== item.path ||
          typeof f.evidenceId !== 'string' ||
          !/^e\d+$/.test(f.evidenceId) ||
          seen.has(item.id) ||
          !['keep', 'investigate'].includes(f.recommendation as string) ||
          typeof f.reason !== 'string' ||
          f.reason.length > 2000 ||
          typeof f.impact !== 'string' ||
          f.impact.length > 2000
        )
          fail()
        seen.add(item!.id)
        return {
          evidenceId: f.evidenceId as string,
          itemId: item!.id,
          path: item!.path,
          recommendation: f.recommendation as 'keep' | 'investigate',
          reason: f.reason as string,
          impact: f.impact as string,
        }
      })
      report.aiFocus = {
        snapshotId: id(a.snapshotId),
        agentId: a.agentId as string,
        analyzedAt: number(a.analyzedAt),
        items,
      }
    }
    if (r.softwareContent !== undefined) {
      const s = object(r.softwareContent)
      if (
        r.duplicates !== undefined ||
        r.similarPhotos !== undefined ||
        ![
          'mailDownloads',
          'mailSandboxDownloads',
          'chromeDownloads',
          'vlcPlaylist',
          'telegramExport',
        ].includes(s.source as string) ||
        s.minimumAgeDays !==
          (['chromeDownloads', 'vlcPlaylist', 'telegramExport'].includes(s.source as string)
            ? 0
            : 31) ||
        s.minimumDirectoryBytes !==
          (['chromeDownloads', 'vlcPlaylist', 'telegramExport'].includes(s.source as string)
            ? 0
            : 5242880) ||
        typeof s.minimumSizeMet !== 'boolean' ||
        typeof s.executionSupported !== 'boolean' ||
        s.executionSupported !== (s.source !== 'mailSandboxDownloads') ||
        (!s.minimumSizeMet && report.items.length) ||
        (s.source === 'vlcPlaylist'
          ? !path(s.playlistPath).startsWith(`${root}/`) ||
            !/\.m3u8$/i.test(s.playlistPath as string)
          : s.playlistPath !== undefined) ||
        (s.source === 'telegramExport'
          ? !path(s.exportPath).startsWith(`${root}/`) ||
            !path(s.exportPath).endsWith('/result.json') ||
            report.items.some(
              (i) =>
                i.path === s.exportPath ||
                !i.path.startsWith(
                  path(s.exportPath).slice(0, path(s.exportPath).lastIndexOf('/')) + '/',
                ),
            )
          : s.exportPath !== undefined)
      )
        fail()
      report.softwareContent = {
        source: s.source as
          | 'mailDownloads'
          | 'mailSandboxDownloads'
          | 'chromeDownloads'
          | 'vlcPlaylist'
          | 'telegramExport',
        minimumAgeDays: s.minimumAgeDays as number,
        minimumDirectoryBytes: s.minimumDirectoryBytes as number,
        minimumSizeMet: s.minimumSizeMet as boolean,
        executionSupported: s.executionSupported as boolean,
        ...(s.source === 'vlcPlaylist' ? { playlistPath: path(s.playlistPath) } : {}),
        ...(s.source === 'telegramExport' ? { exportPath: path(s.exportPath) } : {}),
      }
    }
    if (r.similarPhotos !== undefined) {
      const s = object(r.similarPhotos)
      if (
        r.duplicates !== undefined ||
        s.method !== 'visionFeaturePrintV2' ||
        !Array.isArray(s.groups) ||
        s.groups.length > 100 ||
        !Array.isArray(s.images) ||
        s.images.length > 200
      )
        fail()
      const covered = new Set<string>(),
        groupIds = new Set<string>()
      const sequence =
        s.sequence === undefined
          ? undefined
          : { missingMetadata: number(object(s.sequence).missingMetadata) }
      if (sequence && sequence.missingMetadata > number(s.checkedFiles)) fail()
      const groups = (s.groups as unknown[]).map((raw) => {
        const g = object(raw),
          key = id(g.id)
        if (
          groupIds.has(key) ||
          !Array.isArray(g.itemIds) ||
          g.itemIds.length < 2 ||
          g.itemIds.length > 200
        )
          fail()
        groupIds.add(key)
        const itemIds = (g.itemIds as unknown[]).map((raw) => {
          const value = id(raw)
          if (covered.has(value) || !report!.items.some((i) => i.id === value)) fail()
          covered.add(value)
          return value
        })
        if (sequence ? number(g.captureSpanMs) > 2000 : g.captureSpanMs !== undefined) fail()
        return { id: key, itemIds, ...(sequence ? { captureSpanMs: number(g.captureSpanMs) } : {}) }
      })
      const imageIds = new Set<string>()
      const images = (s.images as unknown[]).map((raw) => {
        const p = object(raw),
          key = id(p.id),
          width = number(p.width),
          height = number(p.height)
        if (
          !covered.has(key) ||
          imageIds.has(key) ||
          !width ||
          !height ||
          width > 60000 ||
          height > 60000 ||
          width * height > 80_000_000 ||
          typeof p.thumbnail !== 'string' ||
          p.thumbnail.length > 150000 ||
          !/^data:image\/png;base64,iVBOR[A-Za-z0-9+/=]+$/.test(p.thumbnail)
        )
          fail()
        imageIds.add(key)
        if (!sequence && p.capturedAt !== undefined) fail()
        return {
          id: key,
          width,
          height,
          thumbnail: p.thumbnail as string,
          ...(sequence ? { capturedAt: captureTime(p.capturedAt).label } : {}),
        }
      })
      if (covered.size !== report.items.length || imageIds.size !== covered.size) fail()
      if (sequence) {
        if (
          number(s.checkedFiles) > 200 ||
          number(s.checkedFiles) < images.length + sequence.missingMetadata
        )
          fail()
        for (const g of groups) {
          const members = g.itemIds.map((id) => ({
            time: captureTime(images.find((p) => p.id === id)?.capturedAt),
            file: report!.items.find((i) => i.id === id)!,
          }))
          const first = members[0]
          if (
            members.some(
              (p, i) =>
                p.time.key !== first.time.key ||
                p.file.path.slice(0, p.file.path.lastIndexOf('/')) !==
                  first.file.path.slice(0, first.file.path.lastIndexOf('/')) ||
                (i > 0 && p.time.nanos < members[i - 1].time.nanos),
            ) ||
            members.at(-1)!.time.nanos - first.time.nanos > 2e9 ||
            Math.ceil((members.at(-1)!.time.nanos - first.time.nanos) / 1e6) !== g.captureSpanMs
          )
            fail()
        }
      }
      report.similarPhotos = {
        method: 'visionFeaturePrintV2',
        groups,
        images,
        checkedFiles: number(s.checkedFiles),
        bytesRead: number(s.bytesRead),
        ...(sequence ? { sequence } : {}),
      }
    }
    if (r.duplicates !== undefined) {
      const d = object(r.duplicates)
      if (!Array.isArray(d.groups) || d.groups.length > 1000) fail()
      const covered = new Set<string>(),
        hashes = new Set<string>()
      const groups = (d.groups as unknown[]).map((raw) => {
        const g = object(raw),
          size = number(g.size)
        if (
          typeof g.id !== 'string' ||
          !/^[a-f\d]{64}$/.test(g.id) ||
          hashes.has(g.id) ||
          !size ||
          !Array.isArray(g.itemIds) ||
          g.itemIds.length < 2 ||
          g.itemIds.length > 2000
        )
          fail()
        hashes.add(g.id as string)
        const itemIds = (g.itemIds as unknown[]).map((raw) => {
          const value = id(raw)
          if (covered.has(value) || !report!.items.some((i) => i.id === value && i.size === size))
            fail()
          covered.add(value)
          return value
        })
        return { id: g.id as string, size, itemIds }
      })
      if (covered.size !== report.items.length) fail()
      const extraBytes = number(d.extraBytes)
      if (extraBytes !== groups.reduce((sum, g) => sum + g.size * (g.itemIds.length - 1), 0)) fail()
      report.duplicates = {
        groups,
        extraBytes,
        bytesRead: number(d.bytesRead),
        checkedFiles: number(d.checkedFiles),
      }
    }
  }
  return {
    phase: v.phase as PersonalSnapshot['phase'],
    requestId: v.requestId === null ? null : id(v.requestId),
    runId: v.runId === null ? null : id(v.runId),
    report,
    error: v.error as string | null,
  }
}
export function readPersonalConfirmation(
  raw: unknown,
  report: PersonalReport,
  selected: string[],
  now: number,
  expectedMovement?: PersonalMovement,
): PersonalConfirmation {
  const v = object(raw),
    root = path(v.root),
    createdAt = number(v.createdAt),
    expiresAt = number(v.expiresAt)
  const rows = files(v.items, root, 20)
  if (
    root !== report.root ||
    createdAt < report.createdAt ||
    expiresAt > report.expiresAt ||
    expiresAt - createdAt > 30000 ||
    now < createdAt ||
    now >= expiresAt ||
    rows.length !== selected.length ||
    !rows.length ||
    rows.some(
      (row) =>
        !selected.includes(row.id) ||
        !report.items.some(
          (original) =>
            original.id === row.id &&
            original.path === row.path &&
            original.size === row.size &&
            original.modifiedAt === row.modifiedAt,
        ),
    )
  )
    fail()
  let movement: PersonalConfirmation['movement']
  if (expectedMovement) {
    const m = object(v.movement),
      directory = path(m.directory)
    if (
      directory !== expectedMovement.directory ||
      m.conflict !== expectedMovement.conflict ||
      !Array.isArray(m.targets) ||
      m.targets.length !== rows.length
    )
      fail()
    const targets = (m.targets as unknown[]).map((raw, index) => {
      const t = object(raw),
        destination = t.destination === null ? null : path(t.destination)
      if (
        id(t.id) !== rows[index].id ||
        (destination !== null &&
          (destination.slice(0, destination.lastIndexOf('/')) !== directory ||
            destination === rows[index].path))
      )
        fail()
      return { id: t.id as string, destination }
    })
    const paths = targets.flatMap((t) =>
      t.destination === null ? [] : [t.destination.toLowerCase()],
    )
    if (new Set(paths).size !== paths.length) fail()
    movement = { directory, conflict: m.conflict as PersonalMovement['conflict'], targets }
  } else if (v.movement !== undefined) fail()
  let keptFiles: PersonalFile[] | undefined
  if (report.duplicates || report.similarPhotos) {
    keptFiles = files(v.keptFiles, root, 20)
    const expected = (report.duplicates?.groups ?? report.similarPhotos!.groups).filter((g) =>
      g.itemIds.some((value) => selected.includes(value)),
    )
    if (
      movement ||
      keptFiles.length !== expected.length ||
      keptFiles.some(
        (k) =>
          selected.includes(k.id) ||
          !report.items.some(
            (i) =>
              i.id === k.id &&
              i.path === k.path &&
              i.size === k.size &&
              i.modifiedAt === k.modifiedAt,
          ),
      ) ||
      expected.some((g) => keptFiles!.filter((k) => g.itemIds.includes(k.id)).length !== 1)
    )
      fail()
  } else if (v.keptFiles !== undefined) fail()
  return {
    id: id(v.id),
    root,
    createdAt,
    expiresAt,
    items: rows,
    ...(movement ? { movement } : {}),
    ...(keptFiles ? { keptFiles } : {}),
  }
}
export function filterPersonalFiles(
  items: PersonalFile[],
  query: string,
  kind: FileKind,
  minimum: number,
  olderDays: number,
  now: number,
  sort: 'size' | 'name' | 'modified',
) {
  const needle = query.trim().toLocaleLowerCase(getLocale())
  return items
    .filter(
      (item) =>
        `${item.name}\n${item.path}`.toLocaleLowerCase(getLocale()).includes(needle) &&
        (kind === 'all' || fileKind(item) === kind) &&
        item.size >= minimum &&
        (!olderDays || item.modifiedAt <= now - olderDays * 86400000),
    )
    .sort((a, b) =>
      sort === 'name'
        ? a.name.localeCompare(b.name, getLocale())
        : sort === 'modified'
          ? a.modifiedAt - b.modifiedAt
          : b.size - a.size,
    )
}
