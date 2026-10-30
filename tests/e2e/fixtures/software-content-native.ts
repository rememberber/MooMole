import type { Page } from '@playwright/test'
import { installPersonalMock } from './personal-files-native'
export async function installSoftwareContentMock(page: Page) {
  await installPersonalMock(page)
  await page.addInitScript(() => {
    const native = (window as any).__TAURI_INTERNALS__,
      original = native.invoke
    const root = (source: string) =>
      source === 'chromeDownloads' || source === 'vlcPlaylist' || source === 'telegramExport'
        ? '/Users/demo'
        : source === 'mailDownloads'
          ? '/Users/demo/Library/Mail Downloads'
          : '/Users/demo/Library/Containers/com.apple.mail/Data/Library/Mail Downloads'
    const aliases: Record<string, string> = {
      prepare_software_trash: 'prepare_personal_trash',
      discard_software_trash: 'discard_personal_trash',
      execute_software_trash: 'execute_personal_trash',
    }
    native.invoke = async (command: string, args: any = {}) => {
      const calls = JSON.parse(sessionStorage.getItem('software-content-calls') || '[]')
      sessionStorage.setItem(
        'software-content-calls',
        JSON.stringify([...calls, { command, args }]),
      )
      if (
        command === 'plugin:dialog|open' &&
        args.options?.filters?.[0]?.extensions?.includes('json')
      ) {
        return sessionStorage.getItem('export-picker-cancelled')
          ? null
          : sessionStorage.getItem('export-picker-path') ||
              '/Users/demo/Downloads/ChatExport/result.json'
      }
      if (command === 'plugin:dialog|open')
        return sessionStorage.getItem('playlist-picker-cancelled')
          ? null
          : sessionStorage.getItem('playlist-picker-path') || '/Users/demo/Movies/offline.m3u8'
      if (command === 'start_software_content') {
        await original('start_personal_scan', {
          root: '/Users/demo/Downloads',
          requestId: args.requestId,
        })
        const source = args.source,
          current = root(source),
          report = JSON.parse(sessionStorage.getItem('personal-report')!)
        report.root = current
        report.items = report.items
          .filter((i: any) => ['old.zip', 'notes.txt'].includes(i.name))
          .map((i: any) => ({
            ...i,
            name:
              source === 'vlcPlaylist'
                ? i.name === 'old.zip'
                  ? 'offline.mov'
                  : 'recording.wav'
                : i.name,
            path:
              source === 'vlcPlaylist'
                ? `/Users/demo/Movies/${i.name === 'old.zip' ? 'offline.mov' : 'recording.wav'}`
                : source === 'telegramExport'
                  ? `${args.exportPath.slice(0, args.exportPath.lastIndexOf('/'))}/files/${i.name}`
                  : `${source === 'chromeDownloads' ? '/Users/demo/Downloads' : current}/${i.name}`,
          }))
        report.softwareContent = {
          source,
          minimumAgeDays: ['chromeDownloads', 'vlcPlaylist', 'telegramExport'].includes(source)
            ? 0
            : 31,
          minimumDirectoryBytes: ['chromeDownloads', 'vlcPlaylist', 'telegramExport'].includes(
            source,
          )
            ? 0
            : 5242880,
          minimumSizeMet: true,
          executionSupported: source !== 'mailSandboxDownloads',
          ...(source === 'vlcPlaylist' ? { playlistPath: args.playlistPath } : {}),
          ...(source === 'telegramExport' ? { exportPath: args.exportPath } : {}),
        }
        sessionStorage.setItem('personal-report', JSON.stringify(report))
        const state = JSON.parse(sessionStorage.getItem('personal-state')!)
        state.report = report
        sessionStorage.setItem('personal-state', JSON.stringify(state))
        return
      }
      if (command === 'get_software_content') return original('get_personal_files', args)
      if (command === 'get_execution')
        return { ...(await original(command, args)), origin: 'softwareContent' }
      if (command === 'prepare_software_trash') {
        const report = JSON.parse(sessionStorage.getItem('personal-report')!)
        return { ...(await original(aliases[command], args)), root: report.root }
      }
      if (command === 'execute_software_trash') {
        await original(aliases[command], args)
        const state = JSON.parse(sessionStorage.getItem('personal-state')!)
        state.phase = 'completed'
        sessionStorage.setItem('personal-state', JSON.stringify(state))
        return
      }
      if (command === 'preview_personal_file') {
        const report = JSON.parse(sessionStorage.getItem('personal-report')!),
          item = report.items.find((i: any) => i.id === args.itemId)
        if (args.domain !== 'softwareContent' || args.scanId !== report.id || !item)
          throw Error('wrong software preview source')
        return {
          scanId: report.id,
          itemId: item.id,
          path: item.path,
          content: { kind: 'text', text: 'owned attachment preview', truncated: false },
        }
      }
      return original(aliases[command] ?? command, args)
    }
  })
}
