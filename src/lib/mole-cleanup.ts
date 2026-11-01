import { invoke } from '@tauri-apps/api/core'
export const moleCleanScopes = [
  ['system', '系统缓存与过期日志'],
  ['userEssentials', '用户缓存、日志与废纸篓'],
  ['appCaches', '应用与沙盒缓存'],
  ['browsers', '浏览器缓存'],
  ['cloudOffice', '云盘与办公软件缓存'],
  ['developerTools', '开发工具与包管理器缓存'],
  ['appsUtilities', '常用应用与实用工具'],
  ['virtualization', '虚拟化工具缓存'],
  ['applicationSupport', '应用支持目录中的日志'],
  ['appLeftovers', '应用残留与空容器'],
  ['appleSilicon', 'Apple Silicon 缓存'],
  ['firmware', '设备固件下载缓存'],
  ['timeMachine', '失败的 Time Machine 备份'],
  ['largeFiles', '大文件与设备备份提示'],
  ['projectArtifacts', '工程产物提示'],
] as const
export interface MoleCleanPlan {
  version: 1
  id: string
  revision: string
  engineVersion: string
  home: string
  system: boolean
  method: 'molePermanent'
  protectedPaths: string[]
  createdAt: number
  expiresAt: number
  scope: string[]
}
export type MoleCleanState =
  'running' | 'completed' | 'cancelled' | 'failed' | 'uncertain' | 'interrupted'
export interface MoleCleanSummary {
  id: string
  state: MoleCleanState
  createdAt: number
  finishedAt: number | null
  eventCount: number
  reviewedAt: number | null
}
export interface MoleCleanEvent {
  seq: number
  kind:
    | 'intent'
    | 'removed'
    | 'retained'
    | 'uncertain'
    | 'commandCompleted'
    | 'section'
    | 'upstream'
    | 'authorization'
    | 'complete'
  path: string
  detail: string
  token: string
}
export interface MoleCleanRecord {
  id: string
  plan: MoleCleanPlan
  state: MoleCleanState
  createdAt: number
  finishedAt: number | null
  beforeBytes: number | null
  afterBytes: number | null
  error: string | null
  diagnostic: string
  reviewedAt: number | null
  events: MoleCleanEvent[]
  eventsTruncated: boolean
}
export interface MoleCleanSnapshot {
  version: 1
  activeRequest: string | null
  authorizationRequired: boolean
  records: MoleCleanSummary[]
  current: MoleCleanRecord | null
}
const uuid = (v: unknown): v is string =>
  typeof v === 'string' && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(v)
const n = (v: unknown): v is number => Number.isSafeInteger(v) && (v as number) >= 0
const path = (v: unknown): v is string =>
  typeof v === 'string' &&
  v.startsWith('/') &&
  v.length <= 4096 &&
  !v.includes('\0') &&
  !v.split('/').some((part) => part === '.' || part === '..')
const states = ['running', 'completed', 'cancelled', 'failed', 'uncertain', 'interrupted']
const fail = (): never => {
  throw Error('完整清理数据无法核对，未自动重试任何操作。')
}
export function readMoleCleanPlan(raw: unknown): MoleCleanPlan {
  const p = raw as MoleCleanPlan
  if (
    !p ||
    p.version !== 1 ||
    !uuid(p.id) ||
    p.revision !== '308bf9911179ddfa40e5df9fdc42095a18ba5cfc' ||
    !/^\d+\.\d+\.\d+$/.test(p.engineVersion) ||
    !path(p.home) ||
    typeof p.system !== 'boolean' ||
    p.method !== 'molePermanent' ||
    !n(p.createdAt) ||
    p.expiresAt !== p.createdAt + 120_000 ||
    !Array.isArray(p.protectedPaths) ||
    p.protectedPaths.length > 500 ||
    p.protectedPaths.some((s) => !path(s)) ||
    !Array.isArray(p.scope) ||
    JSON.stringify(p.scope) !== JSON.stringify(moleCleanScopes.map(([id]) => id))
  )
    fail()
  return p
}
export function readMoleCleanSnapshot(raw: unknown): MoleCleanSnapshot {
  const s = raw as MoleCleanSnapshot
  if (
    !s ||
    s.version !== 1 ||
    (s.activeRequest !== null && !uuid(s.activeRequest)) ||
    typeof s.authorizationRequired !== 'boolean' ||
    (s.authorizationRequired && !s.activeRequest) ||
    !Array.isArray(s.records) ||
    s.records.length > 64 ||
    new Set(s.records.map((r) => r.id)).size !== s.records.length
  )
    fail()
  for (const r of s.records) {
    if (
      !uuid(r.id) ||
      !states.includes(r.state) ||
      !n(r.createdAt) ||
      (r.finishedAt !== null && (!n(r.finishedAt) || r.finishedAt < r.createdAt)) ||
      !n(r.eventCount) ||
      r.eventCount > 20_000 ||
      (r.reviewedAt !== null && !n(r.reviewedAt))
    )
      fail()
  }
  if (s.activeRequest && (!s.current || s.current.id !== s.activeRequest)) fail()
  if (
    s.authorizationRequired &&
    (!s.current || !s.current.plan.system || s.current.state !== 'running')
  )
    fail()
  if (s.current) {
    const r = s.current
    readMoleCleanPlan(r.plan)
    if (
      !uuid(r.id) ||
      !states.includes(r.state) ||
      !n(r.createdAt) ||
      (r.finishedAt !== null && !n(r.finishedAt)) ||
      ![r.beforeBytes, r.afterBytes].every((v) => v === null || n(v)) ||
      (r.error !== null && typeof r.error !== 'string') ||
      typeof r.diagnostic !== 'string' ||
      r.diagnostic.length > 1024 * 1024 ||
      (r.reviewedAt !== null && !n(r.reviewedAt)) ||
      !Array.isArray(r.events) ||
      r.events.length > 2000 ||
      typeof r.eventsTruncated !== 'boolean'
    )
      fail()
    for (const [index, e] of r.events.entries()) {
      if (
        !n(e.seq) ||
        e.seq === 0 ||
        (index > 0 && e.seq <= r.events[index - 1].seq) ||
        ![
          'intent',
          'removed',
          'retained',
          'uncertain',
          'commandCompleted',
          'section',
          'upstream',
          'authorization',
          'complete',
        ].includes(e.kind) ||
        typeof e.path !== 'string' ||
        (e.path !== '' && !path(e.path)) ||
        typeof e.detail !== 'string' ||
        e.detail.length > 8192 ||
        (e.token !== '' && !uuid(e.token))
      )
        fail()
    }
  }
  return s
}
export const moleCleanTransport = {
  async prepare(system: boolean) {
    return readMoleCleanPlan(await invoke('prepare_mole_cleanup', { system }))
  },
  discard: (planId: string) => invoke<void>('discard_mole_cleanup', { planId }),
  execute: (planId: string, requestId: string) =>
    invoke<void>('execute_mole_cleanup', {
      planId,
      requestId,
      permanentConfirmed: true,
      trashConfirmed: true,
    }),
  async read(requestId: string | null = null) {
    return readMoleCleanSnapshot(await invoke('get_mole_cleanup', { requestId }))
  },
  cancel: (requestId: string) => invoke<void>('cancel_mole_cleanup', { requestId }),
  authorize: (requestId: string, password: string) =>
    invoke<void>('authorize_mole_cleanup', { requestId, password }),
  acknowledge: (requestId: string, lastSeq: number) =>
    invoke<void>('acknowledge_mole_cleanup', { requestId, lastSeq }),
  saveExport: (requestId: string) => invoke<boolean>('save_mole_cleanup_export', { requestId }),
  export: (requestId: string) => invoke<string>('export_mole_cleanup', { requestId }),
  clear: (requestId: string, lastSeq: number) =>
    invoke<void>('clear_mole_cleanup_record', { requestId, lastSeq, clearConfirmed: true }),
}
