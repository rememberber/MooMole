import { invoke } from '@tauri-apps/api/core'
export type StorageNotificationPermission = {
  authorization: 'notDetermined' | 'denied' | 'authorized' | 'provisional' | 'unknown'
  alerts: boolean
  center: boolean
}
export type StorageNotificationRecord = {
  id: string
  createdAt: number
  finishedAt: number | null
  kind: 'lowSpace' | 'test'
  state: 'submitting' | 'submitted' | 'blocked' | 'unknown'
  availableBytes: number | null
  thresholdBytes: number | null
  error: string | null
}
export type StorageNotificationsView = {
  enabled: boolean
  monitoringEnabled: boolean
  permission: StorageNotificationPermission | null
  error: string | null
  records: StorageNotificationRecord[]
}
const fail = (): never => {
  throw Error('系统提醒状态无法核对，请刷新；不会自动申请权限或重新发送。')
}
const obj = (v: unknown): Record<string, unknown> =>
  v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : fail()
const time = (v: unknown): v is number =>
  Number.isSafeInteger(v) && (v as number) >= 0 && (v as number) <= 8.64e15
const text = (v: unknown): v is string =>
  typeof v === 'string' && v.length <= 1024 && !v.includes('\0')
export function notificationAllowed(p: StorageNotificationPermission | null): boolean {
  return !!p && ['authorized', 'provisional'].includes(p.authorization) && (p.alerts || p.center)
}
export function readStorageNotifications(raw: unknown): StorageNotificationsView {
  const v = obj(raw)
  if (
    typeof v.enabled !== 'boolean' ||
    typeof v.monitoringEnabled !== 'boolean' ||
    (v.error !== null && !text(v.error)) ||
    !Array.isArray(v.records) ||
    v.records.length > 50
  )
    fail()
  if (v.permission !== null) {
    const p = obj(v.permission)
    if (
      !['notDetermined', 'denied', 'authorized', 'provisional', 'unknown'].includes(
        String(p.authorization),
      ) ||
      typeof p.alerts !== 'boolean' ||
      typeof p.center !== 'boolean'
    )
      fail()
  } else if (v.error === null) fail()
  const records = (v.records as unknown[]).map((raw) => {
    const r = obj(raw)
    if (
      typeof r.id !== 'string' ||
      !/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(r.id) ||
      !time(r.createdAt) ||
      (r.finishedAt !== null && (!time(r.finishedAt) || r.finishedAt < r.createdAt)) ||
      !['test', 'lowSpace'].includes(String(r.kind)) ||
      !['submitting', 'submitted', 'blocked', 'unknown'].includes(String(r.state)) ||
      (r.state === 'submitting') !== (r.finishedAt === null) ||
      (r.error !== null && !text(r.error)) ||
      (['submitting', 'submitted'].includes(String(r.state)) && r.error !== null) ||
      (['blocked', 'unknown'].includes(String(r.state)) && !r.error)
    )
      fail()
    if (r.kind === 'test') {
      if (r.availableBytes !== null || r.thresholdBytes !== null) fail()
    } else if (
      !time(r.availableBytes) ||
      ![5, 10, 20].map((n) => n * 1024 ** 3).includes(r.thresholdBytes as number) ||
      r.availableBytes >= (r.thresholdBytes as number)
    )
      fail()
    return r as unknown as StorageNotificationRecord
  })
  if (new Set(records.map((r) => r.id)).size !== records.length) fail()
  return { ...v, records } as StorageNotificationsView
}
export const storageNotificationsTransport = {
  read: () => invoke('get_storage_notifications'),
  request: () => invoke('request_storage_notification_permission', { confirmed: true }),
  configure: (enabled: boolean) => invoke('configure_storage_notifications', { enabled }),
  test: () => invoke('test_storage_notification', { confirmed: true }),
}
export type StorageNotificationsTransport = typeof storageNotificationsTransport
export const notificationState: Record<StorageNotificationRecord['state'], string> = {
  submitting: '正在提交，显示结果尚未知',
  submitted: '已提交给 macOS，不代表已显示',
  blocked: '本次未能提交',
  unknown: '提交结果未知，不会自动重试',
}
export function notificationReason(code: unknown): string {
  return (
    (
      {
        requiresPackagedApp: '请在打包后的 macOS App 中使用系统提醒。',
        permissionDenied: '系统尚未允许提醒，请检查 macOS 通知设置。',
        authorizationTimeout: '系统授权尚未取得结果；未开启提醒，请核对系统提示后刷新。',
        unsupportedSettings: '提醒设置来自其他版本，原设置保持，请使用相应版本打开。',
        notificationBusy: '另一项提醒操作仍在进行，请稍后刷新。',
        interrupted: '上次提交在应用退出前没有取得完整结果，不会重新发送。',
        submissionTimeout: '未及时取得系统提交结果；通知可能已经提交，不会自动重发。',
        firstUseRequired: '请先确认首次使用说明。',
        closing: '应用正在退出。',
      } as Record<string, string>
    )[String(code)] ?? '系统提醒未取得可靠结果，请刷新核对；不会自动申请权限或重新发送。'
  )
}
