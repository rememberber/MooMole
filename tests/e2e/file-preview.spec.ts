import { expect, test } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { installPersonalMock } from './fixtures/personal-files-native'
const png =
  'data:image/png;base64,' +
  readFileSync(new URL('../fixtures/preview.png', import.meta.url)).toString('base64')
async function setup(page: import('@playwright/test').Page, mismatch = false) {
  await installPersonalMock(page)
  await page.addInitScript(
    ({ png, mismatch }) => {
      const native = (window as any).__TAURI_INTERNALS__,
        original = native.invoke
      native.invoke = async (command: string, args: any) => {
        if (command !== 'preview_personal_file') return original(command, args)
        const calls = JSON.parse(sessionStorage.getItem('personal-calls') || '[]')
        sessionStorage.setItem('personal-calls', JSON.stringify([...calls, { command, args }]))
        const report = JSON.parse(sessionStorage.getItem('personal-report')!),
          item = report.items.find((i: any) => i.id === args.itemId)
        if (!item || report.id !== args.scanId || args.domain !== 'personalFiles')
          throw Error('wrong preview source')
        return {
          scanId: args.scanId,
          itemId: args.itemId,
          path: mismatch ? '/other.txt' : item.path,
          content: item.name.endsWith('.pdf')
            ? { kind: 'pdf', dataUrl: png, page: args.page, pages: 2 }
            : { kind: 'text', text: '<script>literal preview</script> 中文', truncated: false },
        }
      }
    },
    { png, mismatch },
  )
  await page.goto('/')
  await page.getByRole('navigation').getByRole('button', { name: '文件整理', exact: true }).click()
  await page.getByRole('button', { name: '扫描文件', exact: true }).click()
  return page.getByRole('region', { name: '个人文件列表' })
}
test('a file preview shows literal text without selecting or executing it', async ({ page }) => {
  const list = await setup(page)
  await list
    .getByRole('article')
    .filter({ hasText: 'notes.txt' })
    .getByRole('button', { name: '预览内容' })
    .click()
  const dialog = page.getByRole('dialog', { name: '文件内容预览' })
  await expect(dialog.locator('pre')).toHaveText('<script>literal preview</script> 中文')
  await dialog.getByRole('button', { name: '返回文件列表' }).click()
  await expect(list.getByRole('checkbox', { name: /选择.*notes/ })).not.toBeChecked()
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('personal-calls')!))
  expect(calls.some((c: any) => /^(execute|prepare)_personal/.test(c.command))).toBe(false)
})
test('PDF navigation reads only the same exact scan item and fits a compact window', async ({
  page,
}) => {
  const list = await setup(page)
  await list
    .getByRole('article')
    .filter({ hasText: 'recent.pdf' })
    .getByRole('button', { name: '预览内容' })
    .click()
  const dialog = page.getByRole('dialog', { name: '文件内容预览' })
  await expect(dialog.getByRole('img')).toBeVisible()
  await expect(dialog).toContainText('第 1 / 2 页')
  await dialog.getByRole('button', { name: '下一页' }).click()
  await expect(dialog).toContainText('第 2 / 2 页')
  await expect(dialog.getByRole('button', { name: '下一页' })).toBeDisabled()
  await page.setViewportSize({ width: 960, height: 680 })
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('personal-calls')!))
  const previews = calls.filter((c: any) => c.command === 'preview_personal_file')
  expect(previews.map((c: any) => c.args.page)).toEqual([1, 2])
  expect(previews[0].args.itemId).toBe(previews[1].args.itemId)
  expect(previews[0].args).not.toHaveProperty('path')
})
test('a preview for a different file is rejected and never displayed', async ({ page }) => {
  const list = await setup(page, true)
  await list
    .getByRole('article')
    .filter({ hasText: 'notes.txt' })
    .getByRole('button', { name: '预览内容' })
    .click()
  const dialog = page.getByRole('dialog', { name: '文件内容预览' })
  await expect(dialog.getByRole('alert')).toContainText('预览结果与所选文件不一致')
  await expect(dialog.locator('pre')).toHaveCount(0)
})
