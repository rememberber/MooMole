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
  duplicates?: {
    groups: { id: string; size: number; itemIds: string[] }[]
    bytesRead: number
    checkedFiles: number
    extraBytes: number
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
  origin?: 'personalFiles' | 'duplicateFiles'
  read: () => Promise<unknown>
  start: (root: string, requestId: string) => Promise<void>
  prepare: (scanId: string, selectedIds: string[], movement?: PersonalMovement) => Promise<unknown>
  discard: (confirmationId: string) => Promise<void>
  execute: (confirmationId: string, requestId: string, moving?: boolean) => Promise<void>
  cancel: (requestId: string) => Promise<void>
  detail: (runId: string) => Promise<ExecutionDetail>
}
export const personalTransport: PersonalTransport = {
  read: () => invoke('get_personal_files'),
  start: (root, requestId) => invoke('start_personal_scan', { root, requestId }),
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
  if (report.duplicates) {
    keptFiles = files(v.keptFiles, root, 20)
    const expected = report.duplicates.groups.filter((g) =>
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
