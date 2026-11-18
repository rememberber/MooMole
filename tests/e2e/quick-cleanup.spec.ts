import { expect, test } from '@playwright/test'

test('cleanup filters stay separated from result groups and the empty state', async ({ page }) => {
  for (const mode of ['software', 'normal', 'essential']) {
    await page.goto('/')
    await page.evaluate(
      async (mode) => (await import('/tests/e2e/fixtures/quick-cleanup.tsx')).mount(mode),
      mode,
    )
    const view = page.locator('#quick-fixture')
    if (mode === 'essential') await view.getByText('审阅清理项目', { exact: true }).click()
    const search = view.getByRole('searchbox')
    const filters = search.locator('..')
    const groups = view.locator('.quick-review > .quick-group')
    for (const [width, height] of [
      [1200, 820],
      [960, 680],
    ]) {
      await page.setViewportSize({ width, height })
      await expect(groups.nth(1)).toBeVisible()
      const [toolbar, first, second] = await Promise.all([
        filters.boundingBox(),
        groups.first().boundingBox(),
        groups.nth(1).boundingBox(),
      ])
      expect(first!.y - (toolbar!.y + toolbar!.height)).toBeGreaterThanOrEqual(12)
      expect(second!.y - (first!.y + first!.height)).toBeGreaterThanOrEqual(12)
    }
    if (mode === 'software') {
      await search.fill('no-match')
      const empty = view.getByText('没有匹配的软件缓存候选', { exact: false })
      await expect(empty).toBeVisible()
      const [toolbar, result] = await Promise.all([filters.boundingBox(), empty.boundingBox()])
      expect(result!.y - (toolbar!.y + toolbar!.height)).toBeGreaterThanOrEqual(12)
    }
  }
})

test('verified rebuildable caches default selected; logs, protection, unsupported and expiry stay guarded', async ({
  page,
}) => {
  await page.clock.install()
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/quick-cleanup.tsx')).mount())
  const quick = page.locator('#quick-fixture')
  const log = quick.getByRole('checkbox', { name: '选择 /Users/demo/Library/Logs/example.log' })
  const cache = quick.getByRole('checkbox', {
    name: '选择 /Users/demo/Library/Caches/com.apple.helpd',
  })
  await expect(cache).toBeChecked()
  await expect(log).not.toBeChecked()
  await expect(
    quick.getByRole('checkbox', { name: '选择 /Users/demo/Library/Caches/example' }),
  ).not.toBeChecked()
  await expect(
    quick.getByRole('checkbox', { name: '选择 /Users/demo/Library/Logs/protected.log' }),
  ).toBeDisabled()
  await expect(
    quick.getByRole('checkbox', { name: '选择 /Users/demo/Library/Caches/Quick Look' }),
  ).toBeDisabled()
  await expect(
    quick.getByRole('checkbox', { name: '选择 /Users/demo/Library/Logs/directory' }),
  ).toBeDisabled()
  await log.check()
  await quick.getByRole('searchbox', { name: '搜索清理项目' }).fill('no-match')
  await expect(quick.getByText('已选 2 项', { exact: false })).toBeVisible()
  await quick.getByRole('button', { name: '移入废纸篓…', exact: true }).click()
  expect(await page.evaluate(() => JSON.parse(sessionStorage.getItem('quick-plan')!))).toEqual({
    scanId: 'scan',
    ids: ['log', 'cache'],
  })
  await quick.getByRole('searchbox').clear()
  await page.clock.fastForward(11_000)
  await expect(log).toBeDisabled()
  await expect(cache).not.toBeChecked()
  await expect(quick.getByRole('button', { name: '移入废纸篓…', exact: true })).toBeDisabled()
})

test('protecting a default selection removes it; large items are listed with bound uninstall target', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/quick-cleanup.tsx')).mount())
  const quick = page.locator('#quick-fixture')
  const cache = quick.getByRole('checkbox', {
    name: '选择 /Users/demo/Library/Caches/com.apple.helpd',
  })
  await expect(cache).toBeChecked()
  await cache.locator('..').getByRole('button', { name: '保护', exact: true }).click()
  await expect(cache).toBeDisabled()
  await expect(cache).not.toBeChecked()
  await quick.getByText('大文件', { exact: true }).click()
  await expect(quick.getByText('movie.mp4', { exact: true })).toBeVisible()
  await quick.getByText('大应用', { exact: true }).click()
  await quick.getByRole('button', { name: '审阅卸载' }).click()
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('quick-uninstall')!)),
  ).toMatchObject({ taskId: 'apps-task', index: 0, path: '/Applications/Large App.app' })
  await quick.getByText('卸载残留', { exact: true }).click()
  await expect(
    quick.getByText('来自 Mole 卸载残留扫描的结构化候选。', { exact: false }),
  ).toBeVisible()
})

test('navigation places quick cleanup immediately after overview', async ({ page }) => {
  await page.goto('/')
  const nav = page.getByRole('navigation', { name: '主导航' })
  const names = await nav.getByRole('button').allTextContents()
  expect(names.slice(0, 2).map((name) => name.trim())).toEqual(['概览', '清理空间'])
  await nav.getByRole('button', { name: '清理空间' }).click()
  await expect(page.getByRole('main', { name: '清理空间内容' })).toBeVisible()
})

test('one rejected cache inspection preserves other independent recommendations', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/quick-cleanup.tsx')).mount('one-failure'),
  )
  const quick = page.locator('#quick-fixture')
  await expect(
    quick.getByRole('checkbox', { name: '选择 /Users/demo/Library/Caches/Quick Look' }),
  ).toBeChecked()
  const rejected = quick.getByRole('checkbox', {
    name: '选择 /Users/demo/Library/Caches/com.apple.helpd',
  })
  await expect(rejected).not.toBeChecked()
  await expect(rejected.locator('..')).toContainText('推荐检查未完成')
})

test('overview cleanup offers checked recommendations immediately and keeps optional review collapsed', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/quick-cleanup.tsx')).mount('essential'),
  )
  const quick = page.locator('#quick-fixture')
  await expect(quick.getByRole('button', { name: '移入废纸篓…', exact: true })).toBeEnabled()
  const details = quick.locator('.quick-review-details')
  await expect(details).not.toHaveAttribute('open')
  await expect(quick.getByText('大文件', { exact: true })).toHaveCount(0)
  await expect(quick.getByText('大应用', { exact: true })).toHaveCount(0)
  await quick.getByRole('button', { name: '移入废纸篓…', exact: true }).click()
  expect(await page.evaluate(() => JSON.parse(sessionStorage.getItem('quick-plan')!))).toEqual({
    scanId: 'scan',
    ids: ['cache'],
  })
  await quick.getByText('审阅清理项目', { exact: true }).click()
  await expect(
    quick.getByRole('checkbox', { name: '选择 /Users/demo/Library/Logs/example.log', exact: true }),
  ).not.toBeChecked()
  await expect(
    quick.getByRole('checkbox', {
      name: '选择 /Users/demo/Library/Caches/com.apple.helpd',
      exact: true,
    }),
  ).toBeChecked()
})
