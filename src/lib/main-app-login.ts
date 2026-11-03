import { invoke } from '@tauri-apps/api/core'
export type LoginStatus = 'notRegistered' | 'enabled' | 'requiresApproval' | 'notFound'
export interface LoginSystem {
  bundleId: string
  bundlePath: string
  status: LoginStatus
}
export interface LoginRecord {
  version: 1
  id: string
  enabled: boolean
  before: LoginSystem
  after: LoginSystem | null
  createdAt: number
  finishedAt: number | null
  state: 'running' | 'accepted' | 'noChange' | 'notStarted' | 'unknown'
  error: string | null
  nativeCode: number | null
  reviewedAt: number | null
}
export interface LoginView {
  system: LoginSystem | null
  records: LoginRecord[]
  activeRequests: string[]
  error: string | null
}
const product = 'io.github.rememberber.moomole'
const fail = (): never => {
  throw Error('登录启动状态无法核对，请刷新记录；不会自动重新提交设置。')
}
const obj = (v: unknown) =>
  v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : fail()
const text = (v: unknown, max = 4096) =>
  typeof v === 'string' &&
  v.length > 0 &&
  new TextEncoder().encode(v).length <= max &&
  !/[\u0000-\u001f\u007f]/u.test(v)
    ? v
    : fail()
const integer = (v: unknown): v is number =>
  Number.isSafeInteger(v) && (v as number) >= 0 && (v as number) <= 8.64e15
const id = (v: unknown) =>
  typeof v === 'string' && /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(v) ? v : fail()
const error = (v: unknown) =>
  v === null ? null : /^login[A-Za-z]{1,100}$/.test(text(v, 128)) ? (v as string) : fail()
export function readLoginSystem(raw: unknown, expectedId = product): LoginSystem {
  const v = obj(raw),
    p = text(v.bundlePath)
  if (
    v.bundleId !== expectedId ||
    !p.startsWith('/') ||
    !p.endsWith('.app') ||
    p.split('/').some((p) => p === '.' || p === '..') ||
    !['notRegistered', 'enabled', 'requiresApproval', 'notFound'].includes(v.status as string)
  )
    fail()
  return { bundleId: expectedId, bundlePath: p, status: v.status as LoginStatus }
}
const matches = (s: LoginStatus, enabled: boolean) =>
  enabled ? ['enabled', 'requiresApproval'].includes(s) : s === 'notRegistered'
export function readLoginRecord(raw: unknown, expectedId = product): LoginRecord {
  const v = obj(raw),
    before = readLoginSystem(v.before, expectedId),
    after = v.after === null ? null : readLoginSystem(v.after, expectedId)
  if (
    v.version !== 1 ||
    typeof v.enabled !== 'boolean' ||
    !integer(v.createdAt) ||
    (v.finishedAt !== null && (!integer(v.finishedAt) || v.finishedAt < v.createdAt)) ||
    !['running', 'accepted', 'noChange', 'notStarted', 'unknown'].includes(v.state as string) ||
    (v.state === 'running') !== (v.finishedAt === null) ||
    (v.reviewedAt !== null &&
      (v.state !== 'unknown' ||
        !integer(v.reviewedAt) ||
        v.reviewedAt < (v.finishedAt as number))) ||
    (after && after.bundlePath !== before.bundlePath) ||
    (v.nativeCode !== null &&
      (!Number.isSafeInteger(v.nativeCode) || Math.abs(v.nativeCode as number) > 8.64e15)) ||
    (['accepted', 'noChange'].includes(v.state as string) &&
      (v.error !== null || !after || !matches(after.status, v.enabled))) ||
    (v.state === 'running' && (after !== null || v.error !== null || v.nativeCode !== null)) ||
    (v.state === 'noChange' && !matches(before.status, v.enabled)) ||
    (v.state === 'notStarted' &&
      (JSON.stringify(after) !== JSON.stringify(before) ||
        v.error !== 'loginCancelledBeforeRequest'))
  )
    fail()
  return {
    version: 1,
    id: id(v.id),
    enabled: v.enabled as boolean,
    before,
    after,
    createdAt: v.createdAt as number,
    finishedAt: v.finishedAt as number | null,
    state: v.state as LoginRecord['state'],
    error: error(v.error),
    nativeCode: v.nativeCode as number | null,
    reviewedAt: v.reviewedAt as number | null,
  }
}
export function readLoginView(raw: unknown, expectedId = product): LoginView {
  const v = obj(raw)
  if (
    !Array.isArray(v.records) ||
    v.records.length > 50 ||
    !Array.isArray(v.activeRequests) ||
    v.activeRequests.length > 1
  )
    fail()
  const records = (v.records as unknown[]).map((v) => readLoginRecord(v, expectedId)),
    activeRequests = (v.activeRequests as unknown[]).map(id)
  if (
    new Set(records.map((r) => r.id)).size !== records.length ||
    new Set(activeRequests).size !== activeRequests.length ||
    activeRequests.some((id) => !records.some((r) => r.id === id && r.state === 'running'))
  )
    fail()
  return {
    system: v.system === null ? null : readLoginSystem(v.system, expectedId),
    records,
    activeRequests,
    error: error(v.error),
  }
}
async function request(command: string, args?: Record<string, unknown>) {
  try {
    return await invoke(command, args)
  } catch (e) {
    const s = String(e).replace(/^Error:\s*/, '')
    throw Error(s.startsWith('login') ? loginError(s) : s)
  }
}
export const mainAppLoginTransport = {
  read: async () => readLoginView(await request('get_main_app_login')),
  change: async (enabled: boolean, requestId: string, expected: LoginSystem) => {
    const r = readLoginRecord(
      await request('set_main_app_login', { enabled, requestId, confirmed: true }),
    )
    if (
      r.id !== requestId ||
      r.enabled !== enabled ||
      r.before.bundleId !== expected.bundleId ||
      r.before.bundlePath !== expected.bundlePath
    )
      fail()
    return r
  },
  acknowledge: (id: string) => request('acknowledge_main_app_login', { id, confirmed: true }),
  settings: () => request('open_login_item_settings'),
}
export type MainAppLoginTransport = typeof mainAppLoginTransport
export const loginStatus: Record<LoginStatus, string> = {
  notRegistered: '登录时不自动打开',
  enabled: 'macOS 已允许登录启动',
  requiresApproval: '登录启动等待系统批准',
  notFound: '系统未找到可用的登录服务',
}
export function loginError(code: string): string {
  const messages: Record<string, string> = {
    loginBusy: '系统登录设置仍在处理中，请稍后刷新。',
    loginRequiresPackagedApp: '请从安装后的 MooMole 应用中设置登录启动。',
    loginNeedsReview: '上次登录设置结果待核对，请先刷新系统状态并核查记录。',
    loginRequestPending: '等待系统请求超时，请刷新记录；请求可能仍在进行，不会自动重试。',
    loginReadPending: '系统状态读取尚未结束，请稍后刷新。',
    loginRequestRejected: 'macOS 未接受登录启动请求，请在系统登录项中核对。',
    loginInterrupted: '先前登录设置未确认结束，重开只保留未知记录，不重新提交。',
    loginCancelledBeforeRequest: '已在系统请求前停止，未提交登录启动变更。',
    loginStorageUnavailable: '无法保存登录设置记录，请先核对系统状态；不会自动重试。',
    loginInvalidHistory: '登录设置历史格式无法核对，请保留记录并检查。',
    loginInvalidResponse: '系统返回的登录设置状态无法核对。',
    loginConfirmationRequired: '请明确确认当前应用的登录启动设置。',
    loginUnsupportedSystem: '此系统尚不支持本轮登录启动接口。',
    loginResultUnknown: '登录设置结果未知，请核对系统状态；不会自动重复提交。',
  }
  return messages[code] ?? '登录启动状态未知，请检查系统登录项和最近记录。'
}
