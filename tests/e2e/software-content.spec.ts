import { expect, test } from '@playwright/test'
import { installSoftwareContentMock } from './fixtures/software-content-native'
async function open(page: import('@playwright/test').Page) {
  await installSoftwareContentMock(page)
  await page.goto('/')
  await page.getByRole('navigation').getByRole('button', { name: '软件清理', exact: true }).click()
  await page.getByRole('tab', { name: '附件与下载', exact: true }).click()
  return page.getByRole('main', { name: '软件清理内容' })
}
test('protected modern Mail content is previewable and cannot be selected or submitted', async ({
  page,
}) => {
  const main = await open(page)
  await page.getByRole('button', { name: '扫描附件副本', exact: true }).click()
  await expect(main).toContainText('受到 Mole 核心规则保护，仅供预览')
  const list = main.getByRole('region', { name: '个人文件列表' })
  await expect(list.getByRole('checkbox').first()).toBeDisabled()
  await expect(main.getByRole('button', { name: '移入废纸篓…' })).toBeDisabled()
  await list
    .getByRole('article')
    .filter({ hasText: 'notes.txt' })
    .getByRole('button', { name: '预览内容' })
    .click()
  await expect(page.getByRole('dialog', { name: '文件内容预览' }).locator('pre')).toHaveText(
    'owned attachment preview',
  )
  await page.getByRole('button', { name: '返回文件列表' }).click()
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('software-content-calls')!),
  )
  expect(
    calls.some(
      (c: any) => c.command === 'prepare_software_trash' || c.command === 'execute_software_trash',
    ),
  ).toBe(false)
})
test('supported attachment copies use catalog sources, filters and one explicit confirmed request', async ({
  page,
}) => {
  const main = await open(page)
  await main.getByRole('combobox', { name: '软件附件来源' }).selectOption('mailDownloads')
  await page.getByRole('button', { name: '扫描附件副本', exact: true }).click()
  await expect(main.getByRole('textbox', { name: '软件附件目录' })).toHaveAttribute('readonly', '')
  const list = main.getByRole('region', { name: '个人文件列表' })
  await expect(list.getByRole('article')).toHaveCount(2)
  await main.getByRole('searchbox', { name: '搜索个人文件' }).fill('notes')
  await list.getByRole('checkbox').check()
  await main.getByRole('button', { name: '移入废纸篓…' }).click()
  const dialog = page.getByRole('dialog', { name: '确认处理软件附件副本' })
  await expect(dialog).toContainText('/Users/demo/Library/Mail Downloads/notes.txt')
  await expect(dialog.getByRole('button', { name: '确认移入废纸篓', exact: true })).toBeDisabled()
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认移入废纸篓', exact: true }).click()
  await expect(main.getByRole('region', { name: '个人文件处理结果' })).toBeVisible()
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('software-content-calls')!),
  )
  const start = calls.find((c: any) => c.command === 'start_software_content')
  expect(start.args.source).toBe('mailDownloads')
  expect(start.args).not.toHaveProperty('root')
  expect(
    calls.find((c: any) => c.command === 'prepare_software_trash').args.selectedIds,
  ).toHaveLength(1)
  expect(calls.filter((c: any) => c.command === 'execute_software_trash')).toHaveLength(1)
})

test('browser downloads keep their source explicit and process only separately confirmed local files', async ({
  page,
}) => {
  const main = await open(page)
  await main.getByRole('combobox', { name: '软件附件来源' }).selectOption('chromeDownloads')
  await page.getByRole('button', { name: '扫描下载记录', exact: true }).click()
  await expect(main.getByRole('textbox', { name: '下载文件范围' })).toHaveValue('/Users/demo')
  await expect(main).toContainText('不遍历整个账户目录')
  await expect(main).toContainText('浏览器历史和下载记录保持原样')
  const list = main.getByRole('region', { name: '个人文件列表' })
  await expect(list.getByRole('article')).toHaveCount(2)
  await list.getByRole('article').filter({ hasText: 'notes.txt' }).getByRole('checkbox').check()
  await main.getByRole('button', { name: '移入废纸篓…' }).click()
  const dialog = page.getByRole('dialog', { name: '确认处理浏览器下载' })
  await expect(dialog).toContainText('/Users/demo/Downloads/notes.txt')
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认移入废纸篓', exact: true }).click()
  await expect(main.getByRole('region', { name: '个人文件处理结果' })).toBeVisible()
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('software-content-calls')!),
  )
  expect(calls.find((c: any) => c.command === 'start_software_content').args.source).toBe(
    'chromeDownloads',
  )
  expect(calls.filter((c: any) => c.command === 'execute_software_trash')).toHaveLength(1)
})
