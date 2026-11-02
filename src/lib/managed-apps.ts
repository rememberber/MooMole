import { invoke } from '@tauri-apps/api/core'
import type { UninstallTarget } from './app-uninstall'
export interface ManagedUpgrade {
  version: string
  url: string
  sha256: string
  publisher: string
}
export interface ManagedDraft {
  upgrade?: ManagedUpgrade
  id: string
  taskId: string
  index: number
  path: string
  bundleId: string
  token: string
  name: string
  installedVersion: string
  managerPath: string
  metadataSha256: string
  createdAt: number
  expiresAt: number
}
export interface ManagedRecord {
  id: string
  requestId: string
  draft: ManagedDraft
  state: 'running' | 'upgraded' | 'removed' | 'retained' | 'cancelled' | 'unknown' | 'interrupted'
  phase: 'checking' | 'intent' | 'finished'
  createdAt: number
  finishedAt: number | null
  reason: string
  reviewedAt: number | null
  upgradeResult?: {
    version: string
    publisher: string
    bundleSha256: string
    receiptSha256: string
  }
}
const fail = (): never => {
  throw Error('Homebrew 返回结果无法核对，请查看实际包状态；不会自动重试。')
}
const uuid = (v: unknown): v is string =>
  typeof v === 'string' && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(v)
const path = (v: unknown): v is string =>
  typeof v === 'string' &&
  v.startsWith('/') &&
  v.length <= 4096 &&
  !/[\u0000-\u001f\u007f]/.test(v) &&
  !v.split('/').some((s) => s === '.' || s === '..')
const token = (v: unknown): v is string =>
  typeof v === 'string' &&
  v.length <= 256 &&
  [1, 3].includes(v.split('/').length) &&
  v.split('/').every((s) => /^[a-z0-9][a-z0-9._+@-]{0,95}$/.test(s))
function version(v: string): bigint[] {
  if (!/^\d+(?:\.\d+){0,5}$/.test(v) || v.length > 128) return fail()
  const a = v.split('.').map(BigInt)
  if (a.some((n) => n > 18446744073709551615n)) return fail()
  while (a.length < 6) a.push(0n)
  return a
}
function offer(v: ManagedUpgrade, installed: string) {
  if (
    !v ||
    typeof v.version !== 'string' ||
    typeof v.url !== 'string' ||
    v.url.length > 4096 ||
    !/^[0-9a-f]{64}$/.test(v.sha256) ||
    typeof v.publisher !== 'string' ||
    !v.publisher ||
    v.publisher.length > 128 ||
    /[\u0000-\u001f\u007f]/.test(v.publisher)
  )
    fail()
  const a = version(installed),
    b = version(v.version),
    first = a.findIndex((n, i) => n !== b[i])
  if (first < 0 || b[first] <= a[first]) fail()
  let url: URL
  try {
    url = new URL(v.url)
  } catch {
    return fail()
  }
  // URL is display evidence, never a frontend download/command capability.
  if (
    !['https:', 'file:'].includes(url.protocol) ||
    url.username ||
    url.password ||
    url.hash ||
    (url.protocol === 'file:' && url.hostname)
  )
    fail()
}
export function managedDraftEqual(a: ManagedDraft, b: ManagedDraft): boolean {
  return (
    (
      [
        'id',
        'taskId',
        'index',
        'path',
        'bundleId',
        'token',
        'name',
        'installedVersion',
        'managerPath',
        'metadataSha256',
        'createdAt',
        'expiresAt',
      ] as const
    ).every((k) => a[k] === b[k]) &&
    ((a.upgrade === undefined && b.upgrade === undefined) ||
      (!!a.upgrade &&
        !!b.upgrade &&
        (['version', 'url', 'sha256', 'publisher'] as const).every(
          (k) => a.upgrade![k] === b.upgrade![k],
        )))
  )
}
export function readManagedDraft(
  raw: unknown,
  target?: UninstallTarget,
  upgrading?: boolean,
): ManagedDraft {
  const d = raw as ManagedDraft
  if (
    !d ||
    !uuid(d.id) ||
    !uuid(d.taskId) ||
    !Number.isSafeInteger(d.index) ||
    d.index < 0 ||
    !path(d.path) ||
    !['/opt/homebrew/bin/brew', '/usr/local/bin/brew', '/usr/local/Homebrew/bin/brew'].includes(
      d.managerPath,
    ) ||
    !token(d.token) ||
    typeof d.bundleId !== 'string' ||
    !d.bundleId ||
    d.bundleId.length > 255 ||
    /[\u0000-\u001f\u007f]/.test(d.bundleId) ||
    typeof d.name !== 'string' ||
    !d.name ||
    d.name.length > 1024 ||
    /[\u0000-\u001f\u007f]/.test(d.name) ||
    typeof d.installedVersion !== 'string' ||
    !d.installedVersion ||
    d.installedVersion.length > 128 ||
    /[\u0000-\u001f\u007f]/.test(d.installedVersion) ||
    !/^[0-9a-f]{64}$/.test(d.metadataSha256) ||
    !Number.isSafeInteger(d.createdAt) ||
    d.createdAt < 0 ||
    !Number.isSafeInteger(d.expiresAt) ||
    d.expiresAt - d.createdAt !== 120000 ||
    (target &&
      [
        d.taskId !== target.taskId,
        d.index !== target.index,
        d.path !== target.path,
        d.name !== target.name,
        d.bundleId !== target.bundleId,
      ].some(Boolean))
  )
    fail()
  if (d.upgrade !== undefined) offer(d.upgrade, d.installedVersion)
  if (upgrading !== undefined && !!d.upgrade !== upgrading) fail()
  return d
}
export function readManagedRecord(raw: unknown): ManagedRecord {
  const r = raw as ManagedRecord
  if (
    !r ||
    !uuid(r.id) ||
    !uuid(r.requestId) ||
    readManagedDraft(r.draft).id !== r.id ||
    !['running', 'removed', 'upgraded', 'retained', 'cancelled', 'unknown', 'interrupted'].includes(
      r.state,
    ) ||
    !['checking', 'intent', 'finished'].includes(r.phase) ||
    !Number.isSafeInteger(r.createdAt) ||
    r.createdAt < 0 ||
    r.createdAt < r.draft.createdAt ||
    (r.state === 'running' &&
      (r.finishedAt !== null || r.reviewedAt !== null || r.phase === 'finished')) ||
    (r.state !== 'running' && r.finishedAt === null) ||
    (r.finishedAt !== null &&
      (!Number.isSafeInteger(r.finishedAt) || r.finishedAt < r.createdAt)) ||
    (r.reviewedAt !== null &&
      (!Number.isSafeInteger(r.reviewedAt) || r.reviewedAt < r.createdAt)) ||
    typeof r.reason !== 'string' ||
    r.reason.length > 4000
  )
    fail()
  if (
    (r.state === 'removed' && r.draft.upgrade) ||
    (r.state === 'upgraded' && (!r.draft.upgrade || !r.upgradeResult || r.phase !== 'finished')) ||
    (r.state !== 'upgraded' && r.upgradeResult)
  )
    fail()
  if (
    r.upgradeResult &&
    (r.upgradeResult.version !== r.draft.upgrade?.version ||
      r.upgradeResult.publisher !== r.draft.upgrade?.publisher ||
      !/^[0-9a-f]{64}$/.test(r.upgradeResult.bundleSha256) ||
      !/^[0-9a-f]{64}$/.test(r.upgradeResult.receiptSha256))
  )
    fail()
  return r
}
export const managedTransport = {
  prepare: (target: UninstallTarget, requestId: string, upgrading = false) =>
    invoke(upgrading ? 'prepare_managed_app_upgrade' : 'prepare_managed_app', {
      taskId: target.taskId,
      index: target.index,
      requestId,
    }),
  discard: (id: string) => invoke('discard_managed_app', { id }),
  execute: (id: string, requestId: string) =>
    invoke('execute_managed_app', { id, requestId, confirmed: true }),
  read: () => invoke<unknown[]>('get_managed_apps'),
  review: (id: string) => invoke('acknowledge_managed_app', { id, confirmed: true }),
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
}
export const managedState: Record<ManagedRecord['state'], string> = {
  running: 'Homebrew 正在处理',
  removed: '已核对包与原 App 不再存在',
  upgraded: '已核对新版包登记、App 版本与签名',
  retained: '本次保留',
  cancelled: '执行前已停止',
  unknown: '结果待核实',
  interrupted: '任务已中断',
}
