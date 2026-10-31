import { test, expect } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { applications } from './fixtures/application-library'
async function start(page: import('@playwright/test').Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/application-update.tsx')).mount(mode),
    mode,
  )
  const rows = page.getByRole('region', { name: 'Mole 应用清单' }).getByRole('article')
  await rows.first().getByText('位置与来源', { exact: true }).click()
  await rows.first().getByRole('button', { name: '从新版应用包更新…' }).click()
  return page.getByRole('dialog', { name: '更新此应用' })
}
test('review exact target and ad-hoc source before a single replacement, then require list refresh', async ({
  page,
}) => {
  const dialog = await start(page)
  await expect(page.locator('#application-update-fixture')).toHaveAttribute('data-execute', '0')
  await expect(dialog.getByRole('button', { name: '确认替换并保留旧版' })).toHaveCount(0)
  await dialog.getByRole('button', { name: '选择新版应用…' }).click()
  await expect(dialog.getByRole('region', { name: '应用更新核对' })).toContainText('1.0 → 2.0')
  await expect(dialog).toContainText('临时签名，无法确认发布者身份')
  await expect(dialog.getByRole('button', { name: '确认替换并保留旧版' })).toBeDisabled()
  await page.setViewportSize({ width: 960, height: 780 })
  expect(await dialog.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true)
  await page.screenshot({ path: 'test-results/application-update-review.png' })
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认替换并保留旧版' }).click()
  await expect(dialog.getByRole('region', { name: '应用更新结果' })).toContainText(
    '已替换，旧版已保留',
  )
  await expect(dialog).toContainText('应用包，不是应用数据备份')
  await expect(dialog.getByRole('button', { name: '确认替换并保留旧版' })).toHaveCount(0)
  const calls = JSON.parse(
    (await page.locator('#application-update-fixture').getAttribute('data-calls'))!,
  )
  expect(
    calls.filter((c: { command: string }) => c.command === 'execute_application_update'),
  ).toHaveLength(1)
  const prepare = calls.find(
    (c: { command: string }) => c.command === 'prepare_application_update',
  ).args
  expect(prepare).toMatchObject({
    taskId: '853bf826-4ae1-4695-bc66-85d49e1957b0',
    index: 0,
    replacement: '/Users/demo/Downloads/Example.app',
  })
  expect(prepare).not.toHaveProperty('path')
  const execute = calls.find(
    (c: { command: string }) => c.command === 'execute_application_update',
  ).args
  expect(execute).toMatchObject({ id: '7f786cf6-756c-49a7-b5a7-397fbb7cd15d', confirmed: true })
  expect(execute).not.toHaveProperty('path')
  const bounds = await dialog.boundingBox()
  expect(bounds!.x).toBeGreaterThanOrEqual(0)
  expect(bounds!.width).toBeLessThanOrEqual(1200)
  await dialog.getByRole('button', { name: '返回应用列表' }).click()
  await expect(
    page.getByRole('article').first().getByRole('button', { name: '从新版应用包更新…' }),
  ).toBeDisabled()
})

test('the installed-app page exposes a manual package update for the retained ordinary row', async ({
  page,
}) => {
  await installTaskMock(page, 'apps', undefined, applications)
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await page.getByRole('button', { name: '应用列表', exact: true }).click()
  await expect(page.getByRole('button', { name: '应用更新记录', exact: true })).toBeVisible()
  await page.getByRole('button', { name: '读取应用', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const rows = page.getByRole('region', { name: 'Mole 应用清单' }).getByRole('article')
  await expect(rows).toHaveCount(5)
  await rows.nth(1).getByText('位置与来源', { exact: true }).click()
  await expect(rows.nth(1).getByRole('button', { name: '从新版应用包更新…' })).toBeDisabled()
  await rows.first().getByText('位置与来源', { exact: true }).click()
  await rows.first().getByRole('button', { name: '从新版应用包更新…' }).click()
  const dialog = page.getByRole('dialog', { name: '更新此应用' })
  await expect(dialog).toContainText('/Applications/Example.app')
  await expect(dialog.getByRole('button', { name: '选择新版应用…' })).toBeEnabled()
  await expect(dialog.getByRole('button', { name: '确认替换并保留旧版' })).toHaveCount(0)
})
for (const mode of ['expired', 'mismatch'])
  test(`${mode} preparation never enables replacement`, async ({ page }) => {
    const dialog = await start(page, mode)
    await dialog.getByRole('button', { name: '选择新版应用…' }).click()
    if (mode === 'expired') {
      await expect(dialog).toContainText('本次核对已过期')
      await expect(dialog.getByRole('button', { name: '确认替换并保留旧版' })).toBeDisabled()
    } else {
      await expect(dialog.getByRole('alert')).toContainText('应用更新数据无法核对')
      await expect(dialog.getByRole('checkbox')).toHaveCount(0)
    }
    await expect(page.locator('#application-update-fixture')).toHaveAttribute('data-execute', '0')
  })
for (const mode of ['cancel', 'checkingCancel'])
  test(`${mode} targets only its own request and cannot replay`, async ({ page }) => {
    const dialog = await start(page, mode)
    await dialog.getByRole('button', { name: '选择新版应用…' }).click()
    if (mode === 'cancel') {
      await dialog.getByRole('checkbox').check()
      await dialog.getByRole('button', { name: '确认替换并保留旧版' }).click()
    }
    await dialog.getByRole('button', { name: '停止更新' }).click()
    if (mode === 'cancel') {
      await expect(dialog.getByRole('region', { name: '应用更新结果' })).toContainText('已取消')
      await expect(dialog.getByRole('button', { name: '确认替换并保留旧版' })).toHaveCount(0)
    } else await expect(dialog.getByRole('alert')).toContainText('owned preparation cancelled')
    const calls = JSON.parse(
      (await page.locator('#application-update-fixture').getAttribute('data-calls'))!,
    )
    const cancel = calls.find((c: { command: string }) => c.command === 'cancel_task')
    const owned = calls.find(
      (c: { command: string }) =>
        c.command ===
        (mode === 'cancel' ? 'execute_application_update' : 'prepare_application_update'),
    )
    expect(cancel.args.taskId).toBe(owned.args.requestId)
  })
test('an unbound execution result does not offer retry', async ({ page }) => {
  const dialog = await start(page, 'badResult')
  await dialog.getByRole('button', { name: '选择新版应用…' }).click()
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认替换并保留旧版' }).click()
  await expect(dialog.getByRole('alert')).toContainText('应用更新数据无法核对')
  await expect(dialog.getByRole('button', { name: '确认替换并保留旧版' })).toHaveCount(0)
  await expect(page.locator('#application-update-fixture')).toHaveAttribute('data-execute', '1')
})
test('unknown history requires explicit manual review and only reveals recorded locations', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/application-update.tsx')).mount('history'),
  )
  await page.getByRole('button', { name: '应用更新记录', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '应用更新记录' })
  await expect(dialog).toContainText('更新结果待核实')
  await expect(dialog.getByRole('button', { name: '记录已人工核对' })).toBeDisabled()
  await dialog.getByRole('button', { name: '显示旧版或暂存目录' }).click()
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '记录已人工核对' }).click()
  await expect(dialog.getByRole('button', { name: '记录已人工核对' })).toHaveCount(0)
  const calls = JSON.parse(
    (await page.locator('#application-update-fixture').getAttribute('data-calls'))!,
  )
  expect(calls.find((c: { command: string }) => c.command === 'reveal_path').args.path).toBe(
    '/Applications/.moomole-app-update-7f786cf6-756c-49a7-b5a7-397fbb7cd15d',
  )
  expect(
    calls.filter((c: { command: string }) => c.command === 'execute_application_update'),
  ).toHaveLength(0)
})
