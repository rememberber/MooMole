import { expect, test } from '@playwright/test'
async function mount(page: import('@playwright/test').Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/menu-bar.tsx')).mount(mode),
    mode,
  )
  return page.getByRole('region', { name: '菜单栏持续管理' })
}
test('menu bar and continued running require an explicit save and survive a reload', async ({
  page,
}) => {
  let region = await mount(page)
  const checkbox = region.getByRole('checkbox')
  await expect(checkbox).not.toBeChecked()
  await checkbox.check()
  await expect(region).toContainText('菜单栏未显示')
  expect(JSON.parse((await page.locator('#menu-bar-fixture').getAttribute('data-calls'))!)).toEqual(
    ['read'],
  )
  await region.getByRole('button', { name: '保存菜单栏设置' }).click()
  await expect(region).toContainText('菜单栏已显示，关闭窗口后继续运行')
  region = await mount(page)
  await expect(region.getByRole('checkbox')).toBeChecked()
  expect(JSON.parse((await page.locator('#menu-bar-fixture').getAttribute('data-calls'))!)).toEqual(
    ['read'],
  )
  await region.getByRole('checkbox').uncheck()
  await region.getByRole('button', { name: '保存菜单栏设置' }).click()
  await expect(region).toContainText('菜单栏未显示')
})
test('a saved preference does not claim a native tray was created when native creation failed', async ({
  page,
}) => {
  const region = await mount(page, 'native-failure')
  await region.getByRole('checkbox').check()
  await region.getByRole('button', { name: '保存菜单栏设置' }).click()
  await expect(region.getByRole('checkbox')).toBeChecked()
  await expect(region).toContainText('菜单栏未显示')
  await expect(region.getByRole('alert')).toContainText('owned native unavailable')
})
test('unverifiable state disables preference changes', async ({ page }) => {
  const region = await mount(page, 'invalid')
  await expect(region.getByRole('alert')).toContainText('菜单栏状态无法核对')
  await expect(region.getByRole('checkbox')).toBeDisabled()
  await expect(region.getByRole('button')).toBeDisabled()
})
