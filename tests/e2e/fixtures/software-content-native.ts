import type { Page } from '@playwright/test'
import { installPersonalMock } from './personal-files-native'
export async function installSoftwareContentMock(page: Page) {
  await installPersonalMock(page)
  await page.addInitScript(() => {
    const native = (window as any).__TAURI_INTERNALS__,
      original = native.invoke
    const root = (source: string) =>
      source === 'mailDownloads'
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
          .map((i: any) => ({ ...i, path: `${current}/${i.name}` }))
        report.softwareContent = {
          source,
          minimumAgeDays: 31,
          minimumDirectoryBytes: 5242880,
          minimumSizeMet: true,
          executionSupported: source === 'mailDownloads',
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
