import { test, expect } from '@playwright/test'

for (const action of ['open', 'quit']) {
  test(`application ${action} requires confirmation and refreshes the list after one exact request`, async ({
    page,
  }) => {
    await page.goto('/')
    await page.evaluate(async () => (await import('/tests/e2e/fixtures/app-control.tsx')).mount())
    const rows = page.getByRole('region', { name: 'Mole 应用清单' }).getByRole('article')
    const label = action === 'quit' ? '退出…' : '打开'
    await expect(rows.nth(1).getByRole('button', { name: label, exact: true })).toBeDisabled()
    await rows.first().getByRole('button', { name: label, exact: true }).click()
    const dialog = page.getByRole('dialog')
    await expect(dialog).toContainText('/Applications/Example.app')
    await expect(page.locator('#app-control-fixture')).toHaveAttribute('data-calls', '0')
    await dialog
      .getByRole('button', { name: action === 'quit' ? '确认正常退出' : '确认打开应用' })
      .click()
    await expect(dialog.getByRole('region', { name: '应用操作结果' })).toContainText(
      action === 'quit' ? '已观察到应用退出' : '已观察到应用运行',
    )
    await expect(page.locator('#app-control-fixture')).toHaveAttribute('data-calls', '1')
    const args = JSON.parse((await page.locator('#app-control-fixture').getAttribute('data-args'))!)
    expect(args).toMatchObject({ taskId: 'fixture-app-task', index: 0, action, confirmed: true })
    expect(args).not.toHaveProperty('path')
    await dialog.getByRole('button', { name: '返回应用列表' }).click()
    await expect(rows.first().getByRole('button', { name: label, exact: true })).toBeDisabled()
  })
}
test('a different installation result is rejected without retry', async ({ page }) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/app-control.tsx')).mount('mismatch'),
  )
  await page.getByRole('article').first().getByRole('button', { name: '打开', exact: true }).click()
  await page.getByRole('dialog').getByRole('button', { name: '确认打开应用' }).click()
  await expect(page.getByRole('dialog').getByRole('alert')).toContainText('应用操作结果无法核对')
  await expect(page.locator('#app-control-fixture')).toHaveAttribute('data-calls', '1')
  await expect(page.getByRole('button', { name: '确认打开应用' })).toHaveCount(0)
})
