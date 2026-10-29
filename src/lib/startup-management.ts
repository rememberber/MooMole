import { invoke } from '@tauri-apps/api/core'
export interface StartupRow {
  id: string
  name: string
  path: string
  label: string
  program: string
  arguments: string[]
  paused: boolean
  eligible: boolean
  reason: string
}
export interface StartupReport {
  id: string
  createdAt: number
  expiresAt: number
  root: string
  complete: boolean
  cancelled: boolean
  skipped: number
  items: StartupRow[]
}
export interface StartupRecord {
  id: string
  requestId: string
  createdAt: number
  finishedAt: number | null
  action: 'pause' | 'resume'
  path: string
  target: string
  label: string
  program: string
  state: 'running' | 'completed' | 'partial' | 'failed' | 'cancelled' | 'uncertain' | 'interrupted'
  phase: string
  configurationChanged: boolean
  reason: string
  reviewedAt: number | null
  receipt: { destination: string; device: string; inode: string } | null
}
export interface StartupView {
  report: StartupReport | null
  records: StartupRecord[]
  activeRequest: string | null
}
export interface StartupDraft {
  id: string
  scanId: string
  itemId: string
  expiresAt: number
  action: 'pause' | 'resume'
  path: string
  target: string
  label: string
  program: string
  arguments: string[]
}
export const startupSuffix = '.moomole-disabled'
const id = (s: unknown): s is string =>
  typeof s === 'string' && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(s)
const path = (s: unknown): s is string =>
  typeof s === 'string' &&
  s.startsWith('/') &&
  s.length <= 4096 &&
  !/[\x00-\x1f\x7f]/.test(s) &&
  !s.split('/').some((p) => p === '.' || p === '..')
const num = (v: unknown) => Number.isSafeInteger(v) && (v as number) >= 0
const fail = (): never => {
  throw Error('启动管理数据无法核对，请刷新并查看历史；不会重复操作。')
}
export function readStartupRecord(raw: unknown): StartupRecord {
  const r = raw as StartupRecord
  if (
    !r ||
    !id(r.id) ||
    !id(r.requestId) ||
    !path(r.path) ||
    !path(r.target) ||
    !path(r.program) ||
    !num(r.createdAt) ||
    (r.finishedAt !== null && !num(r.finishedAt)) ||
    (r.reviewedAt !== null && !num(r.reviewedAt)) ||
    !['pause', 'resume'].includes(r.action) ||
    ![
      'running',
      'completed',
      'partial',
      'failed',
      'cancelled',
      'uncertain',
      'interrupted',
    ].includes(r.state) ||
    typeof r.label !== 'string' ||
    typeof r.phase !== 'string' ||
    typeof r.reason !== 'string' ||
    typeof r.configurationChanged !== 'boolean' ||
    (r.state === 'completed' &&
      (!r.configurationChanged || !r.receipt || r.receipt.destination !== r.target))
  )
    fail()
  return r
}
export function readStartupView(raw: unknown): StartupView {
  const v = raw as StartupView
  if (
    !v ||
    !Array.isArray(v.records) ||
    v.records.length > 64 ||
    (v.activeRequest !== null && !id(v.activeRequest))
  )
    fail()
  const r = v.report
  if (r !== null) {
    if (
      !r ||
      !id(r.id) ||
      !path(r.root) ||
      !num(r.createdAt) ||
      !num(r.expiresAt) ||
      r.expiresAt <= r.createdAt ||
      r.expiresAt - r.createdAt > 300000 ||
      typeof r.complete !== 'boolean' ||
      typeof r.cancelled !== 'boolean' ||
      !num(r.skipped) ||
      !Array.isArray(r.items) ||
      r.items.length > 200 ||
      new Set(r.items.map((i) => i.id)).size !== r.items.length
    )
      fail()
    for (const i of r.items)
      if (
        !id(i.id) ||
        !path(i.path) ||
        i.path.slice(0, i.path.lastIndexOf('/')) !== r.root ||
        !path(i.program) ||
        typeof i.name !== 'string' ||
        typeof i.label !== 'string' ||
        typeof i.paused !== 'boolean' ||
        typeof i.eligible !== 'boolean' ||
        typeof i.reason !== 'string' ||
        !Array.isArray(i.arguments) ||
        i.arguments.length > 128 ||
        i.arguments.some((a) => typeof a !== 'string' || a.length > 4096)
      )
        fail()
  }
  return { report: r, records: v.records.map(readStartupRecord), activeRequest: v.activeRequest }
}
export const startupTransport = {
  async read(): Promise<StartupView> {
    return readStartupView(await invoke('get_startup_management'))
  },
  scan: (requestId: string) => invoke('scan_startup_management', { requestId }),
  async prepare(report: StartupReport, row: StartupRow): Promise<StartupDraft> {
    const action = row.paused ? 'resume' : 'pause',
      d = await invoke<StartupDraft>('prepare_startup_management', {
        scanId: report.id,
        itemId: row.id,
        action,
      })
    const target = row.paused
      ? row.path.slice(0, -startupSuffix.length)
      : `${row.path}${startupSuffix}`
    if (
      !d ||
      !id(d.id) ||
      d.scanId !== report.id ||
      d.itemId !== row.id ||
      d.action !== action ||
      d.path !== row.path ||
      d.target !== target ||
      d.label !== row.label ||
      d.program !== row.program ||
      JSON.stringify(d.arguments) !== JSON.stringify(row.arguments) ||
      !num(d.expiresAt) ||
      d.expiresAt <= Date.now() ||
      d.expiresAt > Date.now() + 30000
    )
      fail()
    return d
  },
  discard: (id: string) => invoke('discard_startup_management', { id }),
  async execute(d: StartupDraft, requestId: string): Promise<StartupRecord> {
    const r = readStartupRecord(
      await invoke('execute_startup_management', { id: d.id, requestId, confirmed: true }),
    )
    if (
      r.id !== d.id ||
      r.requestId !== requestId ||
      r.action !== d.action ||
      r.path !== d.path ||
      r.target !== d.target ||
      r.label !== d.label ||
      r.program !== d.program
    )
      fail()
    return r
  },
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
  async acknowledge(id: string): Promise<StartupRecord> {
    const r = readStartupRecord(
      await invoke('acknowledge_startup_management', { id, confirmed: true }),
    )
    if (r.id !== id || !r.reviewedAt) fail()
    return r
  },
  settings: () => invoke('open_login_item_settings'),
}
export const startupRecordName = (r: StartupRecord) =>
  r.state === 'completed'
    ? r.action === 'pause'
      ? '已暂停此登录配置'
      : '已恢复此登录配置'
    : ({
        running: '正在处理启动配置',
        partial: '部分完成，请核对',
        failed: '启动操作未完成',
        cancelled: '启动操作已取消',
        uncertain: '启动结果待核实',
        interrupted: '启动操作曾中断',
      }[r.state] ?? '启动结果待核实')
