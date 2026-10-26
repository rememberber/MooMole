import { test, expect, type Page } from '@playwright/test'
async function mount(page: Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/execution-history.tsx')).mount(mode),
    mode,
  )
  await page
    .getByRole('region', { name: '执行记录' })
    .getByRole('button', { name: /已中断/ })
    .click()
  return page.getByRole('dialog', { name: '逐项执行结果' })
}
test('cleanup review keeps uncertain items and receipts through stale reads and history reopening', async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 960, height: 680 })
  const d = await mount(page, 'stale-review')
  await expect(d.getByRole('checkbox', { name: /我已核查原对象/ })).toBeVisible()
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    const bounds = await d.evaluate((el) => ({
      width: el.getBoundingClientRect().width,
      height: el.getBoundingClientRect().height,
      overflow: el.scrollWidth > el.clientWidth + 1,
    }))
    expect(bounds.width).toBeLessThanOrEqual(960)
    expect(bounds.height).toBeLessThanOrEqual(640)
    expect(bounds.overflow).toBe(false)
    await d.screenshot({ path: testInfo.outputPath(`cleanup-review-${theme}.png`) })
  }
  await expect(d.getByRole('button', { name: '记录已核查' })).toBeDisabled()
  await d.getByRole('checkbox', { name: /我已核查原对象/ }).check()
  await d.getByRole('button', { name: '记录已核查' }).dblclick()
  await expect(d).toContainText('已人工核查，原未知结果保持')
  await expect(d).toContainText('结果待核实')
  await expect(d).toContainText('已移入废纸篓')
  await d.getByRole('button', { name: '关闭弹窗' }).click()
  await page.getByRole('button', { name: '刷新记录', exact: true }).click()
  await page
    .getByRole('region', { name: '执行记录' })
    .getByRole('button', { name: /已人工核查/ })
    .click()
  await expect(d).toContainText('已人工核查，原未知结果保持')
  await expect(d.getByRole('button', { name: '记录已核查' })).toHaveCount(0)
  expect(await page.evaluate(() => window.executionReviewFixture.calls)).toEqual([
    {
      id: '6d048c7d-d222-410c-875d-a6e3cef18001',
      snapshotSha256: 'abcd'.repeat(16),
    },
  ])
})
test('cleanup review failure preserves facts and retries only when explicitly requested', async ({
  page,
}) => {
  const d = await mount(page, 'review-error')
  await d.getByRole('checkbox', { name: /我已核查原对象/ }).check()
  await d.getByRole('button', { name: '记录已核查' }).click()
  await expect(d.getByRole('alert')).toContainText('核查保存暂时失败')
  expect(await page.evaluate(() => window.executionReviewFixture.calls.length)).toBe(1)
  await expect(d).toContainText('结果待核实')
  await page.evaluate(() => window.executionReviewFixture.allowReview())
  await d.getByRole('button', { name: '记录已核查' }).click()
  await expect(d).toContainText('已人工核查，原未知结果保持')
  expect(await page.evaluate(() => window.executionReviewFixture.calls.length)).toBe(2)
})
test('a mismatched review response cannot turn an unknown item into a successful move', async ({
  page,
}) => {
  const d = await mount(page, 'review-mismatch')
  await d.getByRole('checkbox', { name: /我已核查原对象/ }).check()
  await d.getByRole('button', { name: '记录已核查' }).click()
  await expect(d.getByRole('alert')).toContainText('核查响应与原记录不一致')
  await expect(d).toContainText('只有意图，完成结果无法确认')
  await expect(d.getByRole('button', { name: '记录已核查' })).toBeEnabled()
})
test('a late acknowledgement does not overwrite another history record', async ({ page }) => {
  const d = await mount(page, 'review-delayed')
  await d.getByRole('checkbox', { name: /我已核查原对象/ }).check()
  await d.getByRole('button', { name: '记录已核查' }).click()
  await d.getByRole('button', { name: '关闭弹窗' }).click()
  await page
    .getByRole('region', { name: '执行记录' })
    .getByRole('button', { name: /已取消/ })
    .click()
  await page.evaluate(() => window.executionReviewFixture.releaseReview())
  await expect(d).toContainText('开始前取消，未尝试操作')
  await expect(d).not.toContainText('已人工核查')
  await expect(d.getByRole('button', { name: '记录已核查' })).toHaveCount(0)
})
declare global {
  interface Window {
    executionReviewFixture: {
      calls: { id: string; snapshotSha256: string }[]
      allowReview: () => void
      releaseReview: () => void
    }
  }
}
