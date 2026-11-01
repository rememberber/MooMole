import type { Page } from '@playwright/test'
import { installPersonalMock } from './personal-files-native'
export async function installOfficePreviewMock(page: Page, mode = 'normal') {
  await installPersonalMock(page)
  await page.addInitScript((mode) => {
    const native = (window as any).__TAURI_INTERNALS__,
      original = native.invoke
    let resolveLate: ((value: unknown) => void) | undefined
    const excerpts = (format: string, part: number) => ({
      kind: 'document',
      format,
      part,
      sections:
        format === 'docx'
          ? [{ title: '', hidden: false }]
          : format === 'xlsx'
            ? [
                { title: '清理', hidden: false },
                { title: 'Archive', hidden: true },
              ]
            : [
                { title: '', hidden: false },
                { title: '', hidden: false },
              ],
      text:
        mode === 'empty'
          ? ''
          : format === 'docx'
            ? '<script>window.officeInjected=true</script>\nWord 正文 literal'
            : format === 'xlsx'
              ? part === 1
                ? 'A1\t<script>literal</script>\nB1\t=SUM(A2:A3) → 4'
                : 'A1\thidden saved cell'
              : part === 1
                ? 'First slide 中文'
                : 'Second slide 中文',
      truncated: mode === 'truncated',
    })
    native.invoke = async (command: string, args: Record<string, unknown> = {}) => {
      if (command === 'fixture_resolve_office') {
        resolveLate?.(null)
        return null
      }
      if (command === 'preview_personal_file') {
        const calls = JSON.parse(sessionStorage.getItem('personal-calls') || '[]')
        sessionStorage.setItem('personal-calls', JSON.stringify([...calls, { command, args }]))
        const source = JSON.parse(sessionStorage.getItem('personal-report')!),
          item = source.items.find((i: any) => i.id === args.itemId)
        if (!item || args.scanId !== source.id || args.domain !== 'personalFiles')
          throw Error('wrong office source')
        if (mode === 'unsupported')
          throw Error('办公文件结构、编码或容量暂不支持，请在对应应用中查看；原文件保持不变。')
        const value = {
          scanId: source.id,
          itemId: item.id,
          path: item.path,
          content: excerpts(item.name.split('.').at(-1), Number(args.page)),
        }
        if (mode === 'mismatch') value.content.part = Number(args.page) + 1
        if (mode === 'late')
          return new Promise((resolve) => {
            resolveLate = () => resolve(value)
          })
        return value
      }
      const result = await original(command, args)
      if (command === 'start_personal_scan') {
        const report = JSON.parse(sessionStorage.getItem('personal-report')!)
        for (let i = 0; i < 3; i++) {
          report.items[i].name = `review.${['docx', 'xlsx', 'pptx'][i]}`
          report.items[i].path = `${report.root}/${report.items[i].name}`
          report.items[i].size = 12345
        }
        sessionStorage.setItem('personal-report', JSON.stringify(report))
        const state = JSON.parse(sessionStorage.getItem('personal-state')!)
        state.report = report
        sessionStorage.setItem('personal-state', JSON.stringify(state))
      }
      return result
    }
  }, mode)
}
