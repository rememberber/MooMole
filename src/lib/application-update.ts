import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { translate } from '../i18n/translate'
import type { UninstallTarget } from './app-uninstall'
export type UpdateDraft = {
  id: string
  taskId: string
  index: number
  name: string
  path: string
  replacement: string
  bundleId: string
  oldVersion: string
  newVersion: string
  publisher: string
  bytes: number
  createdAt: number
  expiresAt: number
}
export type AppUpdateRecord = {
  id: string
  requestId: string
  createdAt: number
  finishedAt: number | null
  draft: UpdateDraft
  state: 'running' | 'updated' | 'retained' | 'cancelled' | 'unknown' | 'interrupted'
  phase: 'checking' | 'copying' | 'intent' | 'finished'
  reason: string
  backup: string
  reviewedAt: number | null
  recoveryAvailable?: boolean
  retainedCopyAvailable?: boolean
}
function fail(): never {
  throw Error('应用更新数据无法核对，请查看更新记录与实际安装位置；不会自动重试。')
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
function integer(v: unknown): number {
  return typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? v : fail()
}
function id(v: unknown): string {
  const s = text(v)
  return /^[\da-f]{8}-[\da-f]{4}-[\da-f]{4}-[\da-f]{4}-[\da-f]{12}$/i.test(s) ? s : fail()
}
function path(v: unknown): string {
  const s = text(v)
  return s.startsWith('/') && s !== '/' && !s.split('/').some((p) => p === '.' || p === '..')
    ? s
    : fail()
}
export function parseUpdateDraft(
  v: unknown,
  target?: UninstallTarget,
  replacement?: string,
): UpdateDraft {
  const a = object(v)
  const draft: UpdateDraft = {
    id: id(a.id),
    taskId: id(a.taskId),
    index: integer(a.index),
    name: text(a.name),
    path: path(a.path),
    replacement: path(a.replacement),
    bundleId: text(a.bundleId),
    oldVersion: text(a.oldVersion),
    newVersion: text(a.newVersion),
    publisher: text(a.publisher),
    bytes: integer(a.bytes),
    createdAt: integer(a.createdAt),
    expiresAt: integer(a.expiresAt),
  }
  if (
    draft.bytes > 8 * 1024 ** 3 ||
    draft.expiresAt - draft.createdAt !== 120000 ||
    draft.path === draft.replacement ||
    !draft.path.endsWith('.app') ||
    !draft.replacement.endsWith('.app') ||
    (target &&
      (draft.taskId !== target.taskId ||
        draft.index !== target.index ||
        draft.name !== target.name ||
        draft.path !== target.path ||
        draft.bundleId !== target.bundleId)) ||
    (replacement && draft.replacement !== replacement)
  )
    fail()
  return draft
}
export function parseAppUpdateRecord(v: unknown): AppUpdateRecord {
  const a = object(v),
    draft = parseUpdateDraft(a.draft),
    state = text(a.state),
    phase = text(a.phase)
  const result = {
    id: id(a.id),
    requestId: id(a.requestId),
    createdAt: integer(a.createdAt),
    finishedAt: a.finishedAt === null ? null : integer(a.finishedAt),
    draft,
    state,
    phase,
    reason: text(a.reason),
    backup: path(a.backup),
    reviewedAt: a.reviewedAt === null ? null : integer(a.reviewedAt),
    ...(a.retainedCopyAvailable === undefined
      ? {}
      : { retainedCopyAvailable: a.retainedCopyAvailable }),
    ...(a.recoveryAvailable === undefined ? {} : { recoveryAvailable: a.recoveryAvailable }),
  }
  const parent = draft.path.slice(0, draft.path.lastIndexOf('/')),
    name = draft.path.slice(draft.path.lastIndexOf('/') + 1)
  if (
    result.id !== draft.id ||
    (a.recoveryAvailable !== undefined && typeof a.recoveryAvailable !== 'boolean') ||
    (a.recoveryAvailable === true && state !== 'updated') ||
    (a.retainedCopyAvailable !== undefined && typeof a.retainedCopyAvailable !== 'boolean') ||
    (a.retainedCopyAvailable === true && state !== 'updated') ||
    !['running', 'updated', 'retained', 'cancelled', 'unknown', 'interrupted'].includes(state) ||
    !['checking', 'copying', 'intent', 'finished'].includes(phase) ||
    result.backup !== `${parent}/.moomole-app-update-${draft.id}/${name}` ||
    (result.finishedAt !== null && result.finishedAt < result.createdAt) ||
    (result.reviewedAt !== null && result.reviewedAt < result.createdAt) ||
    (state === 'updated' && (phase !== 'finished' || result.finishedAt === null))
  )
    fail()
  return result as AppUpdateRecord
}
export const appUpdateState: Record<AppUpdateRecord['state'], string> = {
  running: '更新进行中',
  updated: '已替换，旧版已保留',
  retained: '原应用保留',
  cancelled: '已取消',
  unknown: '更新结果待核实',
  interrupted: '更新已中断',
}
export const applicationUpdate = {
  choose: () =>
    open({
      title: translate('选择已下载并解压的新版应用'),
      directory: false,
      multiple: false,
      filters: [{ name: 'macOS Application', extensions: ['app'] }],
    }),
  prepare: async (target: UninstallTarget, replacement: string, requestId: string) =>
    parseUpdateDraft(
      await invoke('prepare_application_update', {
        taskId: target.taskId,
        index: target.index,
        replacement,
        requestId,
      }),
      target,
      replacement,
    ),
  discard: (id: string) => invoke('discard_application_update', { id }),
  execute: async (draft: UpdateDraft, requestId: string) => {
    const record = parseAppUpdateRecord(
      await invoke('execute_application_update', { id: draft.id, requestId, confirmed: true }),
    )
    if (
      record.id !== draft.id ||
      record.requestId !== requestId ||
      JSON.stringify(record.draft) !== JSON.stringify(draft)
    )
      fail()
    return record
  },
  list: async () => {
    const value = await invoke<unknown>('get_application_updates')
    if (!Array.isArray(value) || value.length > 64) fail()
    const rows = value.map(parseAppUpdateRecord)
    if (
      new Set(rows.map((r) => r.id)).size !== rows.length ||
      new Set(rows.map((r) => r.requestId)).size !== rows.length
    )
      fail()
    return rows
  },
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
  acknowledge: (id: string) => invoke('acknowledge_application_update', { id, confirmed: true }),
  reveal: (path: string) => invoke('reveal_path', { path }),
}
