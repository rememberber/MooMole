import { invoke } from '@tauri-apps/api/core'
export type SpaceItem = {
  version: 1
  path: string
  checkedAt: number
  isDirectory: boolean
  logicalBytes: number | null
  allocatedBytes: number | null
  dataless: boolean
  location:
    'icloudLocation' | 'providerLocation' | 'accountLibrary' | 'systemLocation' | 'unclassified'
}
export async function inspectSpaceItem(path: string): Promise<SpaceItem> {
  const r = await invoke<SpaceItem>('inspect_space_item', { path })
  const size = (n: unknown) => n === null || (Number.isSafeInteger(n) && (n as number) >= 0)
  if (
    !r ||
    r.version !== 1 ||
    r.path !== path ||
    !Number.isSafeInteger(r.checkedAt) ||
    r.checkedAt <= 0 ||
    typeof r.isDirectory !== 'boolean' ||
    typeof r.dataless !== 'boolean' ||
    !size(r.logicalBytes) ||
    !size(r.allocatedBytes) ||
    (r.isDirectory && (r.logicalBytes !== null || r.allocatedBytes !== null)) ||
    ![
      'icloudLocation',
      'providerLocation',
      'accountLibrary',
      'systemLocation',
      'unclassified',
    ].includes(r.location)
  ) {
    throw new Error('空间属性无法核对，请重新读取。')
  }
  return r
}
export const openStorageSettings = () => invoke<void>('open_storage_settings')
