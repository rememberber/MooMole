import { expect, test } from '@playwright/test'
test('application repair enters selection without any registration request', async ({ page }) => {
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/search-repair.tsx')).mount())
  await page.getByRole('button', { name: '选择应用修复…' }).click()
  await expect(page.locator('#search-repair-fixture')).toHaveAttribute('data-route', 'applications')
  await expect(page.locator('#search-repair-fixture')).toHaveAttribute('data-calls', '[]')
})
async function mount(page: import('@playwright/test').Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/search-repair.tsx')).mount(mode),
    mode,
  )
  await page.getByRole('button', { name: '更新目录索引…' }).click()
  const dialog = page.getByRole('dialog', { name: '更新文件搜索索引' })
  await expect(dialog.getByRole('button', { name: '确认更新索引' })).toBeDisabled()
  await dialog.getByRole('button', { name: '选择文件夹…' }).click()
  await expect(dialog).toContainText('/Users/demo/Documents/Example')
  return dialog
}
test('problem repair selects and confirms a folder once, explains verification and supports reopening', async ({
  page,
}) => {
  const dialog = await mount(page)
  await expect(page.locator('#search-repair-fixture')).not.toHaveAttribute(
    'data-calls',
    /execute_search_repair/,
  )
  await dialog.getByRole('button', { name: '确认更新索引' }).click()
  await expect(dialog.getByRole('region', { name: '索引操作结果' })).toContainText('索引命令已完成')
  await expect(dialog).toContainText('命令完成不代表问题已修复')
  const calls = JSON.parse(
    (await page.locator('#search-repair-fixture').getAttribute('data-calls'))!,
  )
  const submitted = calls.filter((c: { command: string }) => c.command === 'execute_search_repair')
  expect(submitted).toHaveLength(1)
  expect(submitted[0].args).toMatchObject({
    id: 'f0d4482c-f575-4861-980b-8db64b314803',
    confirmed: true,
  })
  expect(submitted[0].args).not.toHaveProperty('path')
  await dialog.getByRole('button', { name: '返回维护' }).click()
  await page.getByRole('button', { name: '更新目录索引…' }).click()
  await expect(dialog.getByRole('button', { name: '选择文件夹…' })).toBeVisible()
  await expect(dialog.getByRole('button', { name: '确认更新索引' })).toBeDisabled()
})
test('cancellation targets the same command and preserves uncertainty', async ({ page }) => {
  const dialog = await mount(page, 'pending')
  await dialog.getByRole('button', { name: '确认更新索引' }).click()
  await dialog.getByRole('button', { name: '停止索引命令' }).click()
  await expect(dialog).toContainText('索引结果待核实')
  await expect(dialog).toContainText('已提交的后台索引无法撤销')
  const calls = JSON.parse(
    (await page.locator('#search-repair-fixture').getAttribute('data-calls'))!,
  )
  expect(calls.find((c: { command: string }) => c.command === 'cancel_task').args.taskId).toBe(
    calls.find((c: { command: string }) => c.command === 'execute_search_repair').args.requestId,
  )
})
test('a result for a different folder is rejected without a retry', async ({ page }) => {
  const dialog = await mount(page, 'mismatch')
  await dialog.getByRole('button', { name: '确认更新索引' }).click()
  await expect(dialog.getByRole('alert')).toContainText('索引结果无法核对')
  await expect(dialog.getByRole('button', { name: '确认更新索引' })).toHaveCount(0)
})
