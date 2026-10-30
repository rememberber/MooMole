import { invoke } from '@tauri-apps/api/core'
export interface MountedVolume {
  id: string
  name: string
  path: string
  kind: 'account' | 'startup' | 'mounted'
  fileSystem: string | null
  readOnly: boolean | null
  totalBytes: number | null
  availableBytes: number | null
  canAnalyze: boolean
}
export interface MountedVolumesReport {
  version: 1
  createdAt: number
  expiresAt: number
  complete: boolean
  volumes: MountedVolume[]
}
const uuid = (s: unknown) =>
  typeof s === 'string' && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(s)
const path = (s: unknown) =>
  typeof s === 'string' &&
  s.startsWith('/') &&
  s.length <= 4096 &&
  !/[\x00-\x1f\x7f]/.test(s) &&
  !s.split('/').some((p) => p === '.' || p === '..')
export const mountedVolumesTransport = {
  async read(): Promise<MountedVolumesReport> {
    const r = await invoke<MountedVolumesReport>('get_mounted_volumes')
    if (
      !r ||
      r.version !== 1 ||
      !Number.isSafeInteger(r.createdAt) ||
      r.createdAt < 0 ||
      r.createdAt > Date.now() ||
      r.expiresAt !== r.createdAt + 120_000 ||
      r.expiresAt <= Date.now() ||
      typeof r.complete !== 'boolean' ||
      !Array.isArray(r.volumes) ||
      r.volumes.length > 32 ||
      new Set(r.volumes.map((v) => v.id)).size !== r.volumes.length ||
      new Set(r.volumes.map((v) => v.path)).size !== r.volumes.length ||
      r.volumes.some(
        (v) =>
          !uuid(v.id) ||
          !path(v.path) ||
          typeof v.name !== 'string' ||
          !['account', 'startup', 'mounted'].includes(v.kind) ||
          (v.kind === 'startup' && v.path !== '/') ||
          (v.kind === 'mounted' &&
            (!/^\/Volumes\/[^/]+$/.test(v.path) || v.path.split('/')[2].startsWith('.'))) ||
          (v.fileSystem !== null &&
            (typeof v.fileSystem !== 'string' || !/^[a-zA-Z0-9_.-]{1,16}$/.test(v.fileSystem))) ||
          ![null, true, false].includes(v.readOnly) ||
          typeof v.canAnalyze !== 'boolean' ||
          (v.canAnalyze && (v.fileSystem === null || v.readOnly === null)) ||
          (v.totalBytes === null
            ? v.availableBytes !== null
            : !Number.isSafeInteger(v.totalBytes) ||
              v.totalBytes <= 0 ||
              !Number.isSafeInteger(v.availableBytes) ||
              v.availableBytes === null ||
              v.availableBytes < 0 ||
              v.availableBytes > v.totalBytes),
      )
    )
      throw Error('磁盘列表无法核对，请刷新后重新选择。')
    return r
  },
  async resolve(volume: MountedVolume): Promise<string> {
    const p = await invoke<string>('resolve_mounted_volume', { id: volume.id })
    if (p !== volume.path) throw Error('磁盘位置与所选记录不一致；未自动开始分析，请刷新列表。')
    return p
  },
}
