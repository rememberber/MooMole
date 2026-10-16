import { expect, test } from '@playwright/test'

test('review leads with results and impact, preserves visible uncertainty, and opens details by keyboard', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/permission-plan.tsx')).mount())
  const dialog = page.getByRole('dialog', { name: '审阅计划' })
  await expect(dialog.getByText('5 项需要重新确认', { exact: true })).toBeVisible()
  await expect(dialog.getByText('当前未开放执行', { exact: false })).toBeVisible()
  await expect(dialog.getByRole('status')).toContainText('下一步：先查看需确认的项目')
  await expect(dialog.locator('.plan-item-details[open]')).toHaveCount(0)
  await expect(dialog.locator('.plan-info[open]')).toHaveCount(0)
  const clear = dialog.locator('article').filter({ hasText: '/fixture/clear.log' })
  const restricted = dialog.locator('article').filter({ hasText: '/fixture/restricted.log' })
  const unknown = dialog.locator('article').filter({ hasText: '/fixture/acl.log' })
  await expect(clear.getByRole('heading', { name: 'clear.log', exact: true })).toBeVisible()
  await expect(clear.locator('.full-path')).toBeVisible()
  await expect(clear.getByText('文件信息未见变化', { exact: true })).toBeVisible()
  await expect(clear.locator('.plan-item-impact')).toContainText('影响尚不明确')
  await expect(restricted.getByText('文件访问受限', { exact: true })).toBeVisible()
  await expect(unknown.getByText('权限待确认', { exact: true })).toBeVisible()
  for (const name of ['legacy', 'future', 'incomplete'])
    await expect(
      dialog
        .locator('article')
        .filter({ hasText: `/fixture/${name}.log` })
        .getByText('权限待确认', { exact: true }),
    ).toBeVisible()
  await expect(restricted.getByLabel('本机权限观察')).not.toBeVisible()
  const summary = restricted.getByText('检查详情与用途', { exact: true })
  await summary.focus()
  await page.keyboard.press('Enter')
  await expect(restricted.getByLabel('本机权限观察')).toBeVisible()
  await expect(restricted.getByLabel('用途与影响说明')).toBeVisible()
  await expect(summary).toBeFocused()
  await page.keyboard.press('Space')
  await expect(restricted.getByLabel('本机权限观察')).not.toBeVisible()
  const info = dialog.getByText('计划信息与检查范围', { exact: true })
  await info.click()
  await expect(dialog.getByText('Mole 规则尚未完成本次复查', { exact: false })).toBeVisible()
  await info.click()
  await page.screenshot({ path: 'test-results/review-simple-light.png', fullPage: true })
  await page.keyboard.press('Escape')
  await expect(dialog).toHaveCount(0)
})

test('compact review wraps within narrow windows in both themes and retains filter and expiry notices', async ({
  page,
}) => {
  await page.clock.install()
  await page.setViewportSize({ width: 390, height: 740 })
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/permission-plan.tsx')).mount())
  const dialog = page.getByRole('dialog', { name: '审阅计划' })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(dialog.getByRole('button', { name: '复查 Mole 规则', exact: true })).toBeVisible()
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/review-simple-narrow-${theme}.png`,
      fullPage: true,
    })
  }
  await dialog.getByRole('checkbox', { name: '只看需要确认的项目' }).check()
  await expect(dialog.locator('article')).toHaveCount(5)
  await page.clock.fastForward(901_000)
  await expect(dialog.getByRole('status')).toHaveText('此计划已失效，之前的复查结果不再适用。')
  await expect(dialog.getByRole('button', { name: '复查 Mole 规则', exact: true })).toBeDisabled()
  await expect(dialog.getByText('计划已过期', { exact: true })).toHaveCount(6)
})
