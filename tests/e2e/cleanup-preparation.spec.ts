import { expect, test } from '@playwright/test'

test('real-rule preparation summary keeps scope, retention and recovery boundaries clear', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 800 })
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/cleanup-preparation.tsx')).mount(),
  )
  const dialog = page.getByRole('dialog', { name: '审阅计划' })
  await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
  await expect(
    dialog.getByText('1 项日志完成本轮准备检查 · 1 项保留', { exact: true }),
  ).toBeVisible()
  await expect(dialog.getByRole('status')).toContainText('当前未开放执行')
  await expect(dialog.getByText('日志准备已核对', { exact: true })).toHaveCount(1)
  await expect(dialog.getByText('日志暂不处理', { exact: true })).toHaveCount(1)
  await dialog.getByRole('checkbox', { name: '只看需要确认的项目' }).check()
  await expect(dialog.locator('article')).toHaveCount(1)
  await expect(dialog.locator('article')).toContainText('此缓存保留')
  await dialog.getByText('计划信息与检查范围', { exact: true }).click()
  await expect(dialog.getByText('拟采用移入废纸篓', { exact: false })).toBeVisible()
  await expect(dialog.getByText('当前不提供应用内恢复', { exact: false })).toBeVisible()
  await dialog.getByRole('checkbox', { name: '只看需要确认的项目' }).uncheck()
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await page.screenshot({ path: `test-results/cleanup-preparation-${theme}.png`, fullPage: true })
  }
})

test('expired preparation withdraws positive badges and remains read-only', async ({ page }) => {
  await page.clock.install()
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/cleanup-preparation.tsx')).mount(),
  )
  const dialog = page.getByRole('dialog', { name: '审阅计划' })
  await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
  await expect(dialog.getByText('日志准备已核对', { exact: true })).toBeVisible()
  await page.clock.fastForward(31_000)
  await expect(dialog.getByText('日志处理前检查已失效', { exact: true })).toBeVisible()
  await expect(dialog.getByText('日志准备已核对', { exact: true })).toHaveCount(0)
  await expect(dialog.getByRole('status')).toContainText('重新复查 Mole')
})

test('local failure, repeated-rule cancellation and unknown protocol never retain old preparation', async ({
  page,
}) => {
  for (const mode of ['refresh-error', 'cancel', 'invalid']) {
    await page.goto('/')
    await page.evaluate(
      async (mode) => (await import('/tests/e2e/fixtures/cleanup-preparation.tsx')).mount(mode),
      mode,
    )
    const dialog = page.getByRole('dialog', { name: '审阅计划' })
    await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
    if (mode === 'invalid') {
      await expect(dialog.getByText('日志处理前检查结果无法核对', { exact: true })).toBeVisible()
    } else {
      await expect(dialog.getByText('日志准备已核对', { exact: true })).toBeVisible()
      await dialog
        .getByRole('button', {
          name: mode === 'cancel' ? '复查 Mole 规则' : '重新复核',
          exact: true,
        })
        .click()
      if (mode === 'cancel')
        await dialog.getByRole('button', { name: '取消复查', exact: true }).click()
      await expect(dialog.getByRole('alert')).toBeVisible()
    }
    await expect(dialog.getByText('日志准备已核对', { exact: true })).toHaveCount(0)
    await expect(dialog.getByText('当前未开放执行', { exact: false }).first()).toBeVisible()
  }
})
