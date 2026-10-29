import { invoke } from '@tauri-apps/api/core'
import type { PersonalTransport } from './personal-files'
export const softwareContentSources = [
  {
    id: 'mailSandboxDownloads',
    label: 'Mail 附件副本',
    relative: 'Library/Containers/com.apple.mail/Data/Library/Mail Downloads',
  },
  { id: 'mailDownloads', label: 'Mail 旧位置附件副本', relative: 'Library/Mail Downloads' },
] as const
export type SoftwareContentSource = (typeof softwareContentSources)[number]['id']
export function softwareContentRoot(home: string, source: SoftwareContentSource) {
  return home ? `${home}/${softwareContentSources.find((s) => s.id === source)!.relative}` : ''
}
export function softwareContentTransport(source: SoftwareContentSource): PersonalTransport {
  return {
    origin: 'softwareContent',
    read: () => invoke('get_software_content'),
    start: (_root, requestId) => invoke('start_software_content', { source, requestId }),
    prepare: (scanId, selectedIds) => invoke('prepare_software_trash', { scanId, selectedIds }),
    discard: (confirmationId) => invoke('discard_software_trash', { confirmationId }),
    execute: (confirmationId, requestId) =>
      invoke('execute_software_trash', { confirmationId, requestId, personalFilesConfirmed: true }),
    cancel: (taskId) => invoke('cancel_task', { taskId }),
    detail: (runId) => invoke('get_execution', { runId }),
  }
}
