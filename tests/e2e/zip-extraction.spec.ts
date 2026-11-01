import { test, expect, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { installPersonalMock } from './fixtures/personal-files-native'
test('file organizing exposes ZIP extraction without requiring a file scan', async ({ page }) => {
  await page.setViewportSize({ width: 960, height: 780 })
  await installPersonalMock(page)
  await page.goto('/')
  await page.getByRole('navigation').getByRole('button', { name: '文件整理', exact: true }).click()
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  await page
    .getByRole('banner', { name: '文件整理工具栏' })
    .getByRole('button', { name: '解压 ZIP…' })
    .click()
  const modal = page.getByRole('dialog', { name: '解压 ZIP' })
  await expect(modal.getByRole('textbox', { name: 'ZIP 来源', exact: true })).toHaveValue('')
  await expect(modal.getByRole('button', { name: '检查 ZIP 内容' })).toBeVisible()
  await expect(modal.getByRole('button', { name: '确认解压 ZIP' })).toHaveCount(0)
})
async function setup(page: Page, mode = 'normal') {
  await installTaskMock(page, 'history')
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/zip-extraction.tsx')).mount(mode),
    mode,
  )
  if (['resume', 'unknown'].includes(mode))
    return page.getByRole('dialog', { name: 'ZIP 解压记录' })
  await page
    .locator('#zip-extraction-fixture')
    .getByRole('button', { name: '解压 ZIP…', exact: true })
    .click()
  const modal = page.getByRole('dialog', { name: '解压 ZIP' })
  await modal
    .getByRole('textbox', { name: 'ZIP 来源', exact: true })
    .fill('/Users/demo/Archives/Trip.zip')
  await modal
    .getByRole('textbox', { name: '解压目标目录', exact: true })
    .fill('/Users/demo/Extracted')
  await modal.getByRole('textbox', { name: '新文件夹名称' }).fill('Trip')
  await modal.getByRole('button', { name: '检查 ZIP 内容', exact: true }).click()
  return modal
}
test('ZIP extraction reviews hidden scope and metadata limits before explicit execution and history', async ({
  page,
}, testInfo) => {
  const modal = await setup(page),
    confirm = modal.getByRole('button', { name: '确认解压 ZIP' })
  await expect(modal.getByRole('region', { name: 'ZIP 解压核对' })).toContainText('3 个文件')
  await expect(modal).toContainText('1 个隐藏项目')
  await expect(modal).toContainText('Mac 元数据文件保留为独立文件')
  await expect(confirm).toBeDisabled()
  await modal.getByText('查看前 6 项名称', { exact: true }).click()
  await expect(modal).toContainText('Trip/.notes')
  await page.setViewportSize({ width: 960, height: 780 })
  expect(await modal.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true)
  await page.screenshot({
    path: testInfo.outputPath('zip-extraction-review.png'),
    animations: 'disabled',
  })
  await modal.getByRole('checkbox').check()
  await confirm.click()
  await expect(modal.getByRole('region', { name: 'ZIP 解压结果' })).toContainText(
    'ZIP 已解压，原压缩包保留',
  )
  await expect(confirm).toHaveCount(0)
  const calls = JSON.parse(
      (await page.locator('#zip-extraction-fixture').getAttribute('data-calls'))!,
    ),
    execution = calls.filter((c: any) => c.command === 'execute_zip_extraction')
  expect(execution).toHaveLength(1)
  expect(execution[0].args).toMatchObject({ confirmed: true })
  expect(execution[0].args).not.toHaveProperty('root')
  await modal.getByRole('button', { name: '关闭解压窗口' }).click()
  const history = page
    .locator('#zip-extraction-fixture')
    .getByRole('region', { name: 'ZIP 解压记录' })
  await history.getByRole('button').filter({ hasText: 'ZIP 已解压' }).click()
  await expect(page.getByRole('dialog', { name: 'ZIP 解压记录' })).toContainText(
    '/Users/demo/Extracted/Trip',
  )
})
for (const mode of ['expired', 'mismatch', 'refused'])
  test(`${mode} ZIP cannot execute`, async ({ page }) => {
    const modal = await setup(page, mode)
    if (mode === 'expired')
      await expect(modal.getByRole('button', { name: '确认解压 ZIP' })).toBeDisabled()
    else
      await expect(modal.getByRole('alert')).toContainText(
        mode === 'refused' ? 'no partial archive' : 'ZIP 解压数据无法核对',
      )
    await expect(page.locator('#zip-extraction-fixture')).toHaveAttribute('data-executions', '0')
  })
for (const mode of ['checkingCancel', 'cancel'])
  test(`${mode} cancels only the owned extraction request`, async ({ page }) => {
    const modal = await setup(page, mode)
    if (mode === 'cancel') {
      await modal.getByRole('checkbox').check()
      await modal.getByRole('button', { name: '确认解压 ZIP' }).click()
    }
    await modal.getByRole('button', { name: '停止解压', exact: true }).click()
    if (mode === 'checkingCancel')
      await expect(modal.getByRole('alert')).toContainText('owned folder preparation cancelled')
    else
      await expect(modal.getByRole('region', { name: 'ZIP 解压结果' })).toContainText(
        'ZIP 解压已取消',
      )
    const calls = JSON.parse(
      (await page.locator('#zip-extraction-fixture').getAttribute('data-calls'))!,
    )
    expect(calls.find((c: any) => c.command === 'cancel_task').args.taskId).toBe(
      calls.find(
        (c: any) =>
          c.command === (mode === 'cancel' ? 'execute_zip_extraction' : 'prepare_zip_extraction'),
      ).args.requestId,
    )
  })
test('a changed source digest in the result never offers automatic retry', async ({ page }) => {
  const modal = await setup(page, 'badResult')
  await modal.getByRole('checkbox').check()
  await modal.getByRole('button', { name: '确认解压 ZIP' }).click()
  await expect(modal.getByRole('alert')).toContainText('ZIP 解压数据无法核对')
  await expect(modal.getByRole('button', { name: '确认解压 ZIP' })).toHaveCount(0)
})
test('file and directory pickers keep separate paths and revoke the previous confirmation', async ({
  page,
}) => {
  const modal = await setup(page)
  await modal.getByRole('button', { name: '更改解压范围…', exact: true }).click()
  await modal.getByRole('textbox', { name: 'ZIP 来源', exact: true }).fill('')
  await modal.getByRole('button', { name: '选择 ZIP 文件…', exact: true }).click()
  await expect(modal.getByRole('textbox', { name: 'ZIP 来源', exact: true })).toHaveValue(
    '/Users/demo/Archives/Trip.zip',
  )
  await modal.getByRole('textbox', { name: '解压目标目录', exact: true }).fill('')
  await modal.getByRole('button', { name: '选择目标目录…', exact: true }).click()
  await expect(modal.getByRole('textbox', { name: '解压目标目录', exact: true })).toHaveValue(
    '/Users/demo/Extracted',
  )
  await expect(modal.getByRole('button', { name: '确认解压 ZIP', exact: true })).toHaveCount(0)
  const calls = JSON.parse(
    (await page.locator('#zip-extraction-fixture').getAttribute('data-calls'))!,
  )
  expect(calls.filter((c: any) => c.command === 'discard_zip_extraction')).toHaveLength(1)
  await expect(page.locator('#zip-extraction-fixture')).toHaveAttribute('data-executions', '0')
})
test('unknown publication keeps its result after explicit review and does not replay creation', async ({
  page,
}) => {
  await setup(page, 'unknown')
  const history = page
    .locator('#zip-extraction-fixture')
    .getByRole('region', { name: 'ZIP 解压记录' })
  await history.getByRole('button').filter({ hasText: 'ZIP 解压结果待核实' }).click()
  const modal = page.getByRole('dialog', { name: 'ZIP 解压记录' }),
    confirm = modal.getByRole('button', { name: '记录解压结果已核对' })
  await expect(confirm).toBeDisabled()
  await modal.getByRole('checkbox').check()
  await confirm.click()
  await expect(confirm).toHaveCount(0)
  await expect(modal).toContainText('ZIP 解压结果待核实')
  await expect(page.locator('#zip-extraction-fixture')).toHaveAttribute('data-executions', '0')
})
test('running extraction reconnects and stops without preparing or replaying', async ({ page }) => {
  await setup(page, 'resume')
  const host = page.locator('#zip-extraction-fixture')
  await host.getByRole('button', { name: '停止解压', exact: true }).click()
  await expect(host.getByText('ZIP 解压仍在进行，请在历史中核对结果。')).toHaveCount(0)
  const calls = JSON.parse((await host.getAttribute('data-calls'))!)
  expect(
    calls.filter((c: any) =>
      ['prepare_zip_extraction', 'execute_zip_extraction'].includes(c.command),
    ),
  ).toHaveLength(0)
})
test('extraction confirmation translates while retaining exact source and output paths', async ({
  page,
}) => {
  await setup(page)
  for (const [language, title, confirm] of [
    ['en', 'Extract ZIP', 'Confirm ZIP extraction'],
    ['ja', 'ZIP を展開', 'ZIP の展開を確認'],
  ]) {
    await page.evaluate(
      async (language) => (await import('/src/i18n/locale.ts')).setLanguagePreference(language),
      language,
    )
    const dialog = page.getByRole('dialog', { name: title, exact: true })
    await expect(dialog).toBeVisible()
    await expect(dialog.getByRole('button', { name: confirm, exact: true })).toBeDisabled()
    await expect(dialog).toContainText('/Users/demo/Archives/Trip.zip')
    await expect(dialog).toContainText('/Users/demo/Extracted/Trip')
  }
  await expect(page.locator('#zip-extraction-fixture')).toHaveAttribute('data-executions', '0')
})
