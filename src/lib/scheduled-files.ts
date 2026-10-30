import { invoke } from '@tauri-apps/api/core'
import type { PersonalFile } from './personal-files'
export interface ScheduledFileRule {
  root: string
  action: 'move' | 'trash'
  destination: string | null
  extensions: string[]
  minimumAgeDays: number
  maximumFiles: number
  maximumBytes: number
  intervalMinutes: number
}
export interface ScheduledFilePolicy {
  id: string
  rule: ScheduledFileRule
  enabled: boolean
  updatedAt: number
  lastAttemptAt: number | null
}
export interface ScheduledFileRecord {
  id: string
  policyId: string
  rule: ScheduledFileRule
  trigger: 'manual' | 'scheduled'
  startedAt: number
  finishedAt: number | null
  state: 'running' | 'completed' | 'paused' | 'interrupted'
  matchedFiles: number
  selectedFiles: number
  heldByLimits: number
  executionId: string | null
  reason: string
}
export interface ScheduledFileDraft {
  id: string
  policyId: string | null
  rule: ScheduledFileRule
  createdAt: number
  expiresAt: number
  matchedFiles: number
  selected: PersonalFile[]
  heldByLimits: number
}
export interface ScheduledFilesView {
  policies: ScheduledFilePolicy[]
  records: ScheduledFileRecord[]
  activeRequests: string[]
  error: string | null
}
const fail = (): never => {
  throw Error('定期处理数据无法核对，未自动处理或重试。')
}
const uuid = (s: unknown) =>
  typeof s === 'string' && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(s)
const number = (n: unknown) => typeof n === 'number' && Number.isSafeInteger(n) && n >= 0
const path = (s: unknown) =>
  typeof s === 'string' &&
  s.startsWith('/') &&
  s.length <= 4096 &&
  !/[\x00-\x1f\x7f]/.test(s) &&
  !s.split('/').some((p) => p === '.' || p === '..')
const signature = (r: ScheduledFileRule) =>
  JSON.stringify([
    r.root,
    r.action,
    r.destination,
    r.extensions,
    r.minimumAgeDays,
    r.maximumFiles,
    r.maximumBytes,
    r.intervalMinutes,
  ])
const rule = (r: ScheduledFileRule) =>
  r &&
  path(r.root) &&
  ['move', 'trash'].includes(r.action) &&
  (r.action === 'move' ? path(r.destination) : r.destination === null) &&
  Array.isArray(r.extensions) &&
  r.extensions.length > 0 &&
  r.extensions.length <= 8 &&
  r.extensions.every(
    (s, i) =>
      typeof s === 'string' && /^[a-z0-9]{1,12}$/.test(s) && (i === 0 || r.extensions[i - 1] < s),
  ) &&
  number(r.minimumAgeDays) &&
  r.minimumAgeDays >= 30 &&
  r.minimumAgeDays <= 3650 &&
  number(r.maximumFiles) &&
  r.maximumFiles > 0 &&
  r.maximumFiles <= 20 &&
  number(r.maximumBytes) &&
  r.maximumBytes > 0 &&
  r.maximumBytes <= 1_073_741_824 &&
  [60, 360, 1440].includes(r.intervalMinutes)
export function readScheduledFiles(raw: unknown): ScheduledFilesView {
  const v = raw as ScheduledFilesView
  if (
    !v ||
    !Array.isArray(v.policies) ||
    v.policies.length > 8 ||
    !Array.isArray(v.records) ||
    v.records.length > 64 ||
    !Array.isArray(v.activeRequests) ||
    v.activeRequests.length > 1 ||
    !v.activeRequests.every(uuid) ||
    (v.error !== null && typeof v.error !== 'string') ||
    new Set(v.policies.map((p) => p.id)).size !== v.policies.length ||
    new Set(v.records.map((r) => r.id)).size !== v.records.length ||
    v.policies.some(
      (p) =>
        !uuid(p.id) ||
        !rule(p.rule) ||
        typeof p.enabled !== 'boolean' ||
        !number(p.updatedAt) ||
        (p.lastAttemptAt !== null && !number(p.lastAttemptAt)),
    ) ||
    v.records.some(
      (r) =>
        !uuid(r.id) ||
        !uuid(r.policyId) ||
        !rule(r.rule) ||
        !['manual', 'scheduled'].includes(r.trigger) ||
        !['running', 'completed', 'paused', 'interrupted'].includes(r.state) ||
        !number(r.startedAt) ||
        (r.finishedAt !== null && (!number(r.finishedAt) || r.finishedAt < r.startedAt)) ||
        !number(r.matchedFiles) ||
        r.matchedFiles > 2000 ||
        !number(r.selectedFiles) ||
        r.selectedFiles > 20 ||
        !number(r.heldByLimits) ||
        r.heldByLimits > 2000 ||
        (r.executionId !== null && !uuid(r.executionId)) ||
        typeof r.reason !== 'string',
    )
  )
    return fail()
  return v
}
export const scheduledFilesTransport = {
  read: () => invoke('get_scheduled_files'),
  async preview(policyId: string | null, r: ScheduledFileRule): Promise<ScheduledFileDraft> {
    const d = await invoke<ScheduledFileDraft>('prepare_scheduled_files', {
      edit: { id: policyId, rule: r },
    })
    if (
      !d ||
      !uuid(d.id) ||
      d.policyId !== policyId ||
      !rule(d.rule) ||
      signature(d.rule) !== signature(r) ||
      !number(d.createdAt) ||
      d.createdAt > Date.now() ||
      d.expiresAt !== d.createdAt + 120_000 ||
      d.expiresAt <= Date.now() ||
      !number(d.matchedFiles) ||
      d.matchedFiles > 2000 ||
      !number(d.heldByLimits) ||
      !Array.isArray(d.selected) ||
      d.selected.length > r.maximumFiles ||
      d.matchedFiles !== d.selected.length + d.heldByLimits ||
      d.selected.some(
        (i) =>
          !uuid(i.id) ||
          !path(i.path) ||
          !i.path.startsWith(`${r.root}/`) ||
          typeof i.name !== 'string' ||
          !number(i.size) ||
          !Number.isSafeInteger(i.modifiedAt),
      )
    )
      return fail()
    return d
  },
  discard: (id: string) => invoke('discard_scheduled_files', { id }),
  async enable(d: ScheduledFileDraft): Promise<ScheduledFilePolicy> {
    const p = await invoke<ScheduledFilePolicy>('enable_scheduled_files', {
      id: d.id,
      confirmed: true,
    })
    readScheduledFiles({ policies: [p], records: [], activeRequests: [], error: null })
    if (
      !p.enabled ||
      (d.policyId !== null && d.policyId !== p.id) ||
      signature(p.rule) !== signature(d.rule)
    )
      throw Error('规则保存结果无法核对，可能已保存；请刷新并暂停核查，不会自动重试。')
    return p
  },
  pause: (policyId: string) => invoke('pause_scheduled_files', { policyId }),
  remove: (policyId: string) => invoke('remove_scheduled_files', { policyId }),
  run: (policyId: string, requestId: string) =>
    invoke('run_scheduled_files', { policyId, requestId }),
  cancel: (taskId: string) => invoke('cancel_task', { taskId }),
}
export type ScheduledFilesTransport = typeof scheduledFilesTransport
