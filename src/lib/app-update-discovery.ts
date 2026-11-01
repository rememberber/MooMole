import { invoke } from '@tauri-apps/api/core'
import type { UninstallTarget } from './app-uninstall'
export interface AppUpdateProbe {
  id: string
  taskId: string
  index: number
  path: string
  bundleId: string
  currentVersion: string | null
  currentBuild: string | null
  feedUrl: string | null
  state: 'ready' | 'unsupported'
  reason: string
  createdAt: number
  expiresAt: number
}
export interface AppUpdateRelease {
  id: string
  version: string
  build: string
  url: string
  kind: 'package' | 'page'
  bytes: number | null
  requiresReview: boolean
}
export interface AppUpdateDiscovery {
  probeId: string
  requestId: string
  checkedAt: number
  expiresAt: number
  state: 'newerReported' | 'noNewerInFeed' | 'unknown'
  complete: boolean
  inspected: number
  skipped: number
  releases: AppUpdateRelease[]
}
function fail(): never {
  throw Error('应用更新来源结果无法核对，请重新检查；不会打开地址或更新应用。')
}
const object = (v: unknown) =>
  v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : fail()
const number = (v: unknown) =>
  typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 && v <= 8.64e15 ? v : fail()
function text(v: unknown, max = 4096) {
  return typeof v === 'string' &&
    v.length > 0 &&
    new TextEncoder().encode(v).length <= max &&
    !/[\u0000-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/u.test(v)
    ? v
    : fail()
}
function id(v: unknown) {
  const s = text(v, 36)
  return /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(s) ? s : fail()
}
function url(v: unknown) {
  const raw = text(v),
    u = new URL(raw)
  if (
    u.protocol !== 'https:' ||
    u.username ||
    u.password ||
    u.hash ||
    (u.port && u.port !== '443') ||
    !u.hostname.includes('.') ||
    u.hostname.endsWith('.') ||
    u.hostname.includes(':') ||
    /^\d+(?:\.\d+){3}$/.test(u.hostname) ||
    [
      'localhost',
      'local',
      'localdomain',
      'internal',
      'home',
      'test',
      'invalid',
      'example',
      'onion',
    ].some((h) => u.hostname === h || u.hostname.endsWith('.' + h))
  )
    fail()
  return raw
}
function build(v: string) {
  if (!/^\d+(?:\.\d+){0,5}$/.test(v) || v.length > 128) fail()
  const parts = v.split('.').map(BigInt)
  if (parts.some((n) => n > 2n ** 64n - 1n)) fail()
  while (parts.length < 6) parts.push(0n)
  return parts
}
function newer(a: string, b: string) {
  const x = build(a),
    y = build(b)
  for (let i = 0; i < 6; i++) if (x[i] !== y[i]) return x[i] > y[i]
  return false
}
export function readAppUpdateProbe(value: unknown, target: UninstallTarget): AppUpdateProbe {
  const p = object(value),
    r = {
      id: id(p.id),
      taskId: id(p.taskId),
      index: number(p.index),
      path: text(p.path),
      bundleId: text(p.bundleId, 255),
      currentVersion: p.currentVersion === null ? null : text(p.currentVersion, 128),
      currentBuild: p.currentBuild === null ? null : text(p.currentBuild, 128),
      feedUrl: p.feedUrl === null ? null : url(p.feedUrl),
      state: p.state,
      reason: text(p.reason),
      createdAt: number(p.createdAt),
      expiresAt: number(p.expiresAt),
    }
  if (
    r.taskId !== target.taskId ||
    r.index !== target.index ||
    r.path !== target.path ||
    r.bundleId !== target.bundleId ||
    r.expiresAt - r.createdAt !== 300000 ||
    !['ready', 'unsupported'].includes(r.state as string)
  )
    fail()
  if (r.state === 'ready') {
    if (!r.currentBuild || !r.feedUrl) fail()
    build(r.currentBuild)
  }
  return r as AppUpdateProbe
}
export function readAppUpdateDiscovery(
  value: unknown,
  probe: AppUpdateProbe,
  requestId: string,
): AppUpdateDiscovery {
  const p = object(value),
    r = {
      probeId: id(p.probeId),
      requestId: id(p.requestId),
      checkedAt: number(p.checkedAt),
      expiresAt: number(p.expiresAt),
      state: p.state,
      complete: p.complete,
      inspected: number(p.inspected),
      skipped: number(p.skipped),
      releases: [] as AppUpdateRelease[],
    }
  if (
    probe.state !== 'ready' ||
    r.probeId !== probe.id ||
    r.requestId !== requestId ||
    r.checkedAt < probe.createdAt ||
    r.checkedAt >= r.expiresAt ||
    r.expiresAt !== probe.expiresAt ||
    typeof r.complete !== 'boolean' ||
    r.inspected > 128 ||
    r.skipped > r.inspected ||
    !Array.isArray(p.releases) ||
    p.releases.length > r.inspected ||
    p.releases.length > 128
  )
    fail()
  r.releases = (p.releases as unknown[]).map((v) => {
    const a = object(v),
      release = {
        id: id(a.id),
        version: text(a.version, 128),
        build: text(a.build, 128),
        url: url(a.url),
        kind: a.kind,
        bytes: a.bytes === null ? null : number(a.bytes),
        requiresReview: a.requiresReview,
      }
    if (
      !['package', 'page'].includes(release.kind as string) ||
      typeof release.requiresReview !== 'boolean' ||
      (release.bytes !== null && (!release.bytes || release.bytes > 16 * 1024 ** 3)) ||
      !probe.currentBuild ||
      !newer(release.build, probe.currentBuild)
    )
      fail()
    return release as AppUpdateRelease
  })
  if (
    new Set(r.releases.map((r) => r.id)).size !== r.releases.length ||
    r.releases.length + r.skipped > r.inspected ||
    (r.releases.length
      ? r.state !== 'newerReported'
      : r.complete
        ? r.state !== 'noNewerInFeed' || !r.inspected
        : r.state !== 'unknown')
  )
    fail()
  return r as AppUpdateDiscovery
}
export const appUpdateDiscovery = {
  prepare: async (target: UninstallTarget, requestId: string) =>
    readAppUpdateProbe(
      await invoke('prepare_app_update_discovery', {
        taskId: target.taskId,
        index: target.index,
        requestId,
      }),
      target,
    ),
  check: async (probe: AppUpdateProbe, requestId: string) =>
    readAppUpdateDiscovery(
      await invoke('check_app_update_discovery', { id: probe.id, requestId, confirmed: true }),
      probe,
      requestId,
    ),
  discard: (id: string) => invoke('discard_app_update_discovery', { id }),
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
  open: (probeId: string, candidateId: string, requestId: string) =>
    invoke('open_app_update_candidate', { probeId, candidateId, requestId }),
}
