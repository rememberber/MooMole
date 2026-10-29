import { expect, test } from '@playwright/test'
async function mount(page: import('@playwright/test').Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/storage-monitor.tsx')).mount(mode),
    mode,
  )
  return page.getByRole('region', { name: '持续空间管理' })
}
test('opening trends reads only; opt-in, interval and threshold require explicit save', async ({
  page,
}) => {
  const region = await mount(page)
  await expect(region.getByRole('checkbox', { name: '定期记录空间' })).not.toBeChecked()
  await expect(region).toContainText('2 条真实记录')
  await expect(region.getByRole('img', { name: '同一卷可用空间趋势' })).toBeVisible()
  await expect(region).toContainText('最近观察的可用空间低于提醒阈值')
  await region.getByRole('checkbox').check()
  await region.getByRole('combobox', { name: '空间记录间隔' }).selectOption('360')
  await region.getByRole('combobox', { name: '低空间阈值' }).selectOption(String(5 * 1024 ** 3))
  await expect(page.locator('#storage-monitor-fixture')).not.toHaveAttribute(
    'data-calls',
    /configure|sample/,
  )
  await region.getByRole('button', { name: '保存观察设置' }).click()
  await expect(region).toContainText('定期空间观察已开启')
  await expect(region).not.toContainText('最近观察的可用空间低于提醒阈值')
  const calls = JSON.parse(
    (await page.locator('#storage-monitor-fixture').getAttribute('data-calls'))!,
  )
  expect(calls.find((c: { command: string }) => c.command === 'configure').config).toEqual({
    enabled: true,
    intervalMinutes: 360,
    warnBelowBytes: 5 * 1024 ** 3,
  })
  await region.getByRole('button', { name: '分析空间去向' }).click()
  await expect(page.getByLabel('fixture destination')).toHaveText('analyze')
})
test('manual sampling preserves disabled scheduling and stale capacity is explicitly historical', async ({
  page,
}) => {
  await page.clock.install()
  const region = await mount(page)
  await expect(region).toContainText('定期空间观察已关闭')
  await page.clock.fastForward(3 * 60 * 60 * 1000)
  await expect(region).toContainText('历史观察，当前状态未知')
  await expect(region).toContainText('上次观察的可用空间低于阈值')
  await region.getByRole('button', { name: '记录当前空间' }).click()
  await expect(region).toContainText('3 条真实记录')
  await expect(region).toContainText('定期空间观察已关闭')
  await page.setViewportSize({ width: 960, height: 680 })
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
})
test('invalid capacity cannot become zero free space or a chart', async ({ page }) => {
  const region = await mount(page, 'invalid')
  await expect(region.getByRole('alert')).toContainText('空间观察数据无法核对')
  await expect(region.getByRole('img')).toHaveCount(0)
  await expect(region.getByRole('checkbox')).toBeDisabled()
})
