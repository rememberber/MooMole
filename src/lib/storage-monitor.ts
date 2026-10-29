import { invoke } from '@tauri-apps/api/core'
import type { CleanupSpaceSample } from '../types'
export interface StorageMonitorConfig {
  enabled: boolean
  intervalMinutes: number
  warnBelowBytes: number
}
export interface StorageMonitorView {
  config: StorageMonitorConfig
  samples: CleanupSpaceSample[]
  collecting: boolean
  error: string | null
}
export const storageMonitorDefaults: StorageMonitorConfig = {
  enabled: false,
  intervalMinutes: 60,
  warnBelowBytes: 10 * 1024 ** 3,
}
export function readStorageMonitor(raw: unknown): StorageMonitorView {
  const v = raw as StorageMonitorView
  if (
    !v ||
    typeof v.config?.enabled !== 'boolean' ||
    ![15, 60, 360].includes(v.config.intervalMinutes) ||
    ![5, 10, 20].map((n) => n * 1024 ** 3).includes(v.config.warnBelowBytes) ||
    typeof v.collecting !== 'boolean' ||
    (v.error !== null && typeof v.error !== 'string') ||
    !Array.isArray(v.samples) ||
    v.samples.length > 720 ||
    v.samples.some(
      (s) =>
        !s ||
        !Number.isSafeInteger(s.measuredAt) ||
        s.measuredAt < 0 ||
        !/^[a-f0-9:]{1,128}$/.test(s.volumeId) ||
        !Number.isSafeInteger(s.totalBytes) ||
        s.totalBytes <= 0 ||
        !Number.isSafeInteger(s.availableBytes) ||
        s.availableBytes < 0 ||
        s.availableBytes > s.totalBytes,
    )
  )
    throw Error('空间观察数据无法核对，请刷新；不会用缺失数据推断可用空间。')
  return v
}
export const storageMonitorTransport = {
  read: () => invoke('get_storage_monitor'),
  configure: (config: StorageMonitorConfig) => invoke('configure_storage_monitor', { config }),
  sample: () => invoke('sample_storage_monitor'),
}
export type StorageMonitorTransport = typeof storageMonitorTransport
