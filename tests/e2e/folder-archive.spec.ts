import { test, expect, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { installPersonalMock } from './fixtures/personal-files-native'
test('file organizing exposes whole-folder archive without requiring a file scan', async ({
  page,
}) => {
  await installPersonalMock(page)
  await page.goto('/')
  await page.getByRole('navigation').getByRole('button', { name: '文件整理', exact: true }).click()
  await page
    .getByRole('banner', { name: '文件整理工具栏' })
    .getByRole('button', { name: '归档文件夹…' })
    .click()
  const modal = page.getByRole('dialog', { name: '归档整个文件夹' })
  await expect(modal.getByRole('textbox', { name: '来源文件夹', exact: true })).toHaveValue(
    '/Users/demo/Downloads',
  )
  await expect(modal.getByRole('button', { name: '检查整个文件夹' })).toBeVisible()
  await expect(modal.getByRole('button', { name: '确认归档整份文件夹' })).toHaveCount(0)
})
async function setup(page: Page, mode = 'normal') {
  await installTaskMock(page, 'history')
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/folder-archive.tsx')).mount(mode),
    mode,
  )
  if (['resume', 'unknown'].includes(mode))
    return page.getByRole('dialog', { name: '整份文件夹归档记录' })
  await page
    .locator('#folder-archive-fixture')
    .getByRole('button', { name: '归档文件夹…', exact: true })
    .click()
  const modal = page.getByRole('dialog', { name: '归档整个文件夹' })
  await modal
    .getByRole('textbox', { name: '来源文件夹', exact: true })
    .fill('/Users/demo/Documents/Trip')
  await modal
    .getByRole('textbox', { name: '归档目标目录', exact: true })
    .fill('/Users/demo/Archives')
  await modal.getByRole('textbox', { name: 'ZIP 文件名' }).fill('Trip.zip')
  await modal.getByRole('button', { name: '检查整个文件夹', exact: true }).click()
  return modal
}
test('whole-folder archive reviews hidden scope and metadata before one explicit creation, then retains history', async ({
  page,
}) => {
  const modal = await setup(page),
    confirm = modal.getByRole('button', { name: '确认归档整份文件夹' })
  await expect(modal.getByRole('region', { name: '整份文件夹归档核对' })).toContainText('3 个文件')
  await expect(modal).toContainText('1 个隐藏项目')
  await expect(modal).toContainText('ZIP 不加密')
  await expect(confirm).toBeDisabled()
  await modal.getByText('查看前 6 项名称', { exact: true }).click()
  await expect(modal).toContainText('Trip/.notes')
  await page.setViewportSize({ width: 960, height: 780 })
  expect(await modal.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true)
  await page.screenshot({ path: '.cache/folder-archive-review.png', animations: 'disabled' })
  await modal.getByRole('checkbox').check()
  await confirm.click()
  await expect(modal.getByRole('region', { name: '文件夹归档结果' })).toContainText(
    '整份文件夹已归档，原件保留',
  )
  await expect(confirm).toHaveCount(0)
  const calls = JSON.parse(
      (await page.locator('#folder-archive-fixture').getAttribute('data-calls'))!,
    ),
    execution = calls.filter((c: any) => c.command === 'execute_folder_archive')
  expect(execution).toHaveLength(1)
  expect(execution[0].args).toMatchObject({ confirmed: true })
  expect(execution[0].args).not.toHaveProperty('root')
  await modal.getByRole('button', { name: '关闭归档窗口' }).click()
  const history = page
    .locator('#folder-archive-fixture')
    .getByRole('region', { name: '整份文件夹归档记录' })
  await history.getByRole('button').filter({ hasText: '整份文件夹已归档' }).click()
  await expect(page.getByRole('dialog', { name: '整份文件夹归档记录' })).toContainText(
    '/Users/demo/Archives/Trip.zip',
  )
})
for (const mode of ['expired', 'mismatch', 'refused'])
  test(`${mode} whole folder cannot execute`, async ({ page }) => {
    const modal = await setup(page, mode)
    if (mode === 'expired')
      await expect(modal.getByRole('button', { name: '确认归档整份文件夹' })).toBeDisabled()
    else
      await expect(modal.getByRole('alert')).toContainText(
        mode === 'refused' ? 'no partial archive' : '文件夹归档数据无法核对',
      )
    await expect(page.locator('#folder-archive-fixture')).toHaveAttribute('data-executions', '0')
  })
for (const mode of ['checkingCancel', 'cancel'])
  test(`${mode} cancels only the owned folder request`, async ({ page }) => {
    const modal = await setup(page, mode)
    if (mode === 'cancel') {
      await modal.getByRole('checkbox').check()
      await modal.getByRole('button', { name: '确认归档整份文件夹' }).click()
    }
    await modal.getByRole('button', { name: '停止文件夹归档', exact: true }).click()
    if (mode === 'checkingCancel')
      await expect(modal.getByRole('alert')).toContainText('owned folder preparation cancelled')
    else
      await expect(modal.getByRole('region', { name: '文件夹归档结果' })).toContainText(
        '文件夹归档已取消',
      )
    const calls = JSON.parse(
      (await page.locator('#folder-archive-fixture').getAttribute('data-calls'))!,
    )
    expect(calls.find((c: any) => c.command === 'cancel_task').args.taskId).toBe(
      calls.find(
        (c: any) =>
          c.command === (mode === 'cancel' ? 'execute_folder_archive' : 'prepare_folder_archive'),
      ).args.requestId,
    )
  })
test('a changed source digest in the result never offers automatic retry', async ({ page }) => {
  const modal = await setup(page, 'badResult')
  await modal.getByRole('checkbox').check()
  await modal.getByRole('button', { name: '确认归档整份文件夹' }).click()
  await expect(modal.getByRole('alert')).toContainText('文件夹归档数据无法核对')
  await expect(modal.getByRole('button', { name: '确认归档整份文件夹' })).toHaveCount(0)
})
test('unknown publication keeps its result after explicit review and does not replay creation', async ({
  page,
}) => {
  await setup(page, 'unknown')
  const history = page
    .locator('#folder-archive-fixture')
    .getByRole('region', { name: '整份文件夹归档记录' })
  await history.getByRole('button').filter({ hasText: '文件夹归档结果待核实' }).click()
  const modal = page.getByRole('dialog', { name: '整份文件夹归档记录' }),
    confirm = modal.getByRole('button', { name: '记录文件夹归档已核对' })
  await expect(confirm).toBeDisabled()
  await modal.getByRole('checkbox').check()
  await confirm.click()
  await expect(confirm).toHaveCount(0)
  await expect(modal).toContainText('文件夹归档结果待核实')
  await expect(page.locator('#folder-archive-fixture')).toHaveAttribute('data-executions', '0')
})
test('running folder archive reconnects and stops without preparing or replaying', async ({
  page,
}) => {
  await setup(page, 'resume')
  const host = page.locator('#folder-archive-fixture')
  await host.getByRole('button', { name: '停止文件夹归档', exact: true }).click()
  await expect(host.getByText('整份文件夹归档仍在进行，请在历史中核对结果。')).toHaveCount(0)
  const calls = JSON.parse((await host.getAttribute('data-calls'))!)
  expect(
    calls.filter((c: any) =>
      ['prepare_folder_archive', 'execute_folder_archive'].includes(c.command),
    ),
  ).toHaveLength(0)
})
