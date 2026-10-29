import { expect, test } from '@playwright/test'

async function result(page: import('@playwright/test').Page, mode: string) {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/cleanup-result.tsx')).mount(mode),
    mode,
  )
  const dialog = page.getByRole('dialog', { name: '确认移入废纸篓' })
  await dialog.getByRole('button', { name: '确认清理', exact: true }).click()
  const finished = page.getByRole('dialog', { name: '清理结果' })
  await expect(finished).toBeVisible()
  return finished.getByRole('region', { name: '清理容量结果' })
}
test('result separates chosen and actually moved estimates from independently observed available space', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 680 })
  const summary = await result(page, 'increase')
  await expect(summary.locator('dl > div').filter({ hasText: '本批所选大小' })).toContainText(
    '12 MB',
  )
  await expect(summary.locator('dl > div').filter({ hasText: '确认移入废纸篓' })).toContainText(
    '8 MB',
  )
  await expect(summary).toContainText('增加 16 MB')
  await expect(summary).toContainText('不能归因于本次清理')
  await summary.getByText('空间观察详情', { exact: true }).click()
  await expect(summary).toContainText('处理前可用 50 GB')
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await summary.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(
      true,
    )
    await page.screenshot({
      path: `test-results/cleanup-result-${theme}.png`,
      animations: 'disabled',
    })
  }
  await expect(page.getByLabel('调用次数')).toHaveText('1')
  for (const [language, region, label, delta] of [
    ['en', 'Cleanup size results', 'Selected size for this run', 'Increased by 16 MB'],
    ['ja', 'クリーンアップ容量の結果', '今回選択したサイズ', '16 MB 増加'],
  ]) {
    await page.evaluate(
      async (language) => (await import('/src/i18n/locale.ts')).setLanguagePreference(language),
      language,
    )
    const localized = page.getByRole('region', { name: region })
    await expect(localized).toContainText(label)
    await expect(localized).toContainText(delta)
  }
})
test('zero, decreasing, missing and changed-volume observations do not claim space was reclaimed', async ({
  page,
}) => {
  for (const [mode, text] of [
    ['zero', '未观察到变化'],
    ['decrease', '减少 16 MB'],
    ['missing', '未取得可比较的观察'],
    ['changed', '前后观察的卷或容量已变化'],
  ]) {
    const summary = await result(page, mode)
    await expect(summary).toContainText(text)
    if (mode === 'missing') await expect(summary).toContainText('1 项大小未知')
    if (mode === 'changed') await expect(summary.getByText('增加 16 MB')).toHaveCount(0)
  }
})
